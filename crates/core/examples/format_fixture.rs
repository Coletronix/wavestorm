//! Writes a `.waves` project as this release saves it, for the file-format compatibility tests
//! (`tests/old_formats.rs`): the sample project plus the optional parts a project can have
//! (background image, drawn outline, sub-area and Tc path, notes, display settings).
//!
//! Run once for every new major or minor release, and commit the file it writes:
//!
//! ```sh
//! cargo run -p wavestorm-core --example format_fixture
//! ```
//!
//! Keep it compiling against the core API only, so it also runs on older release tags.

use wavestorm_core::project::{NodeData, Underlay};

#[path = "../../ui/src/samples.rs"]
#[allow(dead_code)]
mod samples;

/// A 2 x 2 pixel PNG.
const PNG: [u8; 77] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0, 114, 182, 13,
    36, 0, 0, 0, 20, 73, 68, 65, 84, 120, 156, 99, 56, 113, 103, 203, 255, 168, 158, 168, 255, 12, 48, 6, 0, 102, 196,
    11, 45, 83, 54, 155, 156, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn main() {
    let mut p = samples::sample_project();
    p.notes = "Format fixture: every optional part of a project.".into();
    p.diagram.show_grid = true;
    p.diagram.snap_to_grid = true;
    p.diagram.grid = 12.0;
    p.units.decimals.flow = Some(3);
    p.diagram.underlay = Some(Underlay {
        file_name: "site.png".into(),
        bytes: PNG.to_vec(),
        page: 0,
        origin: [-50.0, -50.0],
        scale: 400.0,
        rotation: 0.1,
        feet_per_unit: Some(2.5),
        opacity: 0.6,
        visible: true,
        locked: true,
        geo: None,
    });
    // The first subcatchment: drawn over the background, with a drawn sub-area and Tc path.
    if let Some(n) = p.nodes.iter_mut().find(|n| matches!(n.data, NodeData::Subcat(_))) {
        let [x, y] = n.pos;
        n.notes = "Drawn over the background.".into();
        n.locked = true;
        n.outline = Some(vec![[x - 60.0, y - 60.0], [x + 60.0, y - 60.0], [x + 60.0, y + 60.0], [x - 60.0, y + 60.0]]);
        n.tc_path = Some(vec![[x - 50.0, y - 50.0], [x, y - 10.0], [x + 50.0, y + 50.0]]);
        n.tc_path_breaks = vec![0, 2];
        if let NodeData::Subcat(sc) = &mut n.data
            && let Some(a) = sc.areas.first_mut()
        {
            a.outline = Some(vec![[x - 40.0, y - 40.0], [x, y - 40.0], [x, y]]);
        }
    }
    let version = env!("CARGO_PKG_VERSION");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/formats");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("cannot make {}: {e}", dir.display()));
    let path = dir.join(format!("wavestorm-{version}-full.waves"));
    let text = p.to_json().unwrap_or_else(|e| panic!("cannot write the project: {e}"));
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    println!("Wrote {}", path.display());
}
