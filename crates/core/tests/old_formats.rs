//! Projects saved by every earlier release keep opening, with all their contents.
//!
//! `tests/formats/` holds `.waves` files written by the released code itself (from each release
//! tag, never edited by hand): `wavestorm-X.Y.Z.waves` is the built-in sample project and
//! `wavestorm-X.Y.Z-full.waves` is written by `examples/format_fixture.rs` (the sample plus a
//! background image, drawings, notes and display settings). Never change or delete these files;
//! if one stops opening, the change that broke it must read the old form too.

use std::path::{Path, PathBuf};
use wavestorm_core::project::{NodeData, Project, VERSION};

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/formats");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "waves"))
        .collect();
    files.sort();
    files
}

fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[test]
fn every_earlier_release_opens_and_calculates() {
    let files = fixtures();
    assert!(files.len() >= 12, "fixtures missing: {files:?}");
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let (p, newer) = Project::open_json(&text).unwrap_or_else(|e| panic!("{}: {e}", name(f)));
        assert_eq!(newer, None, "{}: an earlier release must not warn", name(f));
        assert!(!p.nodes.is_empty(), "{}: nodes lost", name(f));
        assert!(!p.settings.events.is_empty(), "{}: rainfall events lost", name(f));

        // It calculates: runoff reaches every node with something upstream.
        let r = wavestorm_core::calc::calculate(&p);
        let flowing = p.nodes.iter().filter(|n| r.node(n.id).is_some_and(|x| x.outflow.has_flow())).count();
        assert!(flowing >= 3, "{}: only {flowing} nodes calculate", name(f));

        // Saving it with this release and opening it again changes nothing.
        let again = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(again, p, "{}: not stable through a save", name(f));
    }
}

#[test]
fn full_fixtures_keep_every_optional_part() {
    let full: Vec<PathBuf> = fixtures().into_iter().filter(|f| name(f).ends_with("-full.waves")).collect();
    assert!(!full.is_empty());
    for f in &full {
        let p = Project::from_json(&std::fs::read_to_string(f).unwrap()).unwrap();
        let n = name(f);
        let u = p.diagram.underlay.as_ref().unwrap_or_else(|| panic!("{n}: background lost"));
        assert_eq!(u.bytes.len(), 77, "{n}: background image bytes");
        assert!(u.bytes.starts_with(b"\x89PNG"), "{n}: background image bytes");
        assert_eq!(u.feet_per_unit, Some(2.5), "{n}: background calibration");
        assert!(p.diagram.show_grid && p.diagram.snap_to_grid, "{n}: diagram settings");
        assert_eq!(p.units.decimals.flow, Some(3), "{n}: unit settings");
        assert!(!p.notes.is_empty(), "{n}: project notes");
        let s = p.nodes.iter().find(|n| n.outline.is_some()).unwrap_or_else(|| panic!("{n}: outline lost"));
        assert_eq!(s.outline.as_ref().map(Vec::len), Some(4), "{n}: outline");
        assert_eq!(s.tc_path.as_ref().map(Vec::len), Some(3), "{n}: Tc path");
        assert_eq!(s.tc_path_breaks, [0, 2], "{n}: Tc path letters");
        assert!(s.locked && !s.notes.is_empty(), "{n}: node lock and notes");
        let NodeData::Subcat(sc) = &s.data else { panic!("{n}: not a subcatchment") };
        assert!(sc.areas.iter().any(|a| a.outline.is_some()), "{n}: drawn sub-area lost");
    }
}

/// Every major or minor release adds its own full fixture, so the next one is tested against it.
#[test]
fn this_release_has_a_fixture() {
    let mut parts = VERSION.split('.');
    let prefix = format!("wavestorm-{}.{}.", parts.next().unwrap_or("0"), parts.next().unwrap_or("0"));
    assert!(
        fixtures().iter().any(|f| name(f).starts_with(&prefix) && name(f).ends_with("-full.waves")),
        "No format fixture for release {VERSION}. Run \
         `cargo run -p wavestorm-core --example format_fixture` and commit the file it writes."
    );
}
