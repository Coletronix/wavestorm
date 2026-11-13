//! Saving asks first when it would lose data: what the `.hcp` format cannot hold, or what a
//! newer WAVEStorm release put in the project.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use wavestorm_core::project::{Project, Underlay};
use wavestorm_ui::WaveStormApp;
use wavestorm_ui::doc::Doc;

type H = egui_kittest::Harness<'static, WaveStormApp>;

fn harness() -> H {
    egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

fn underlay() -> Underlay {
    Underlay {
        file_name: "site.png".into(),
        bytes: vec![1, 2, 3],
        page: 0,
        origin: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        feet_per_unit: None,
        opacity: 0.5,
        visible: true,
        locked: false,
        geo: None,
    }
}

/// A fresh file path under the test's temporary folder.
fn temp_path(name: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("save_warnings");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path.to_string_lossy().into_owned()
}

/// Opens `doc` as the only project and closes any windows.
fn show(h: &mut H, doc: Doc) {
    h.run_steps(2);
    let app = h.state_mut();
    app.docs.clear();
    app.docs.push(doc);
    h.run_steps(2);
    h.state_mut().doc().windows.clear();
    h.run_steps(2);
}

#[test]
fn saving_to_hcp_lists_what_it_loses_and_asks() {
    let mut h = harness();
    let path = temp_path("site.hcp");
    let mut p = wavestorm_ui::samples::sample_project();
    p.diagram.underlay = Some(underlay());
    let mut doc = Doc::new(p, Some(path.clone()));
    doc.hcp = true;
    show(&mut h, doc);

    h.state_mut().save(false);
    h.run_steps(2);
    h.get_by_label_contains("The background image");
    assert!(!std::path::Path::new(&path).exists(), "nothing written before the user answers");

    // Cancel leaves everything as it was.
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(!std::path::Path::new(&path).exists());

    // Save anyway writes the file, and the same losses are not asked about again.
    h.state_mut().save(false);
    h.run_steps(2);
    h.get_by_label("Save as .hcp Anyway").click();
    h.run_steps(2);
    assert!(std::path::Path::new(&path).exists(), "saved after agreeing");
    std::fs::remove_file(&path).unwrap();
    h.state_mut().save(false);
    h.run_steps(2);
    assert!(h.query_by_label("Save as .hcp Anyway").is_none(), "asked only once");
    assert!(std::path::Path::new(&path).exists());
}

#[test]
fn saving_a_plain_project_to_hcp_does_not_ask() {
    let mut h = harness();
    let path = temp_path("plain.hcp");
    let mut p = Project::default();
    p.add_node(wavestorm_core::project::NodeType::Subcat, [0.0, 0.0]);
    let mut doc = Doc::new(p, Some(path.clone()));
    doc.hcp = true;
    show(&mut h, doc);
    h.state_mut().save(false);
    h.run_steps(2);
    assert!(h.query_by_label("Save as .hcp Anyway").is_none());
    assert!(std::path::Path::new(&path).exists());
}

#[test]
fn a_project_from_a_newer_release_warns_on_open_and_save() {
    let mut h = harness();
    let path = temp_path("newer.waves");
    let text = Project::default()
        .to_json()
        .unwrap()
        .replace(&format!("\"saved_by\": \"{}\"", wavestorm_core::project::VERSION), "\"saved_by\": \"999.0.0\"");
    h.run_steps(2);
    h.state_mut().platform.push(wavestorm_ui::platform::Incoming {
        purpose: wavestorm_ui::platform::Purpose::OpenProject,
        name: "newer.waves".into(),
        path: Some(path.clone()),
        bytes: text.into_bytes(),
    });
    h.run_steps(3);
    h.get_by_label_contains("saved by WAVEStorm 999.0.0");
    h.get_by_label("OK").click();
    h.run_steps(2);

    h.state_mut().save(false);
    h.run_steps(2);
    h.get_by_label("Save Anyway").click();
    h.run_steps(2);
    let saved = std::fs::read_to_string(&path).expect("saved after agreeing");
    assert!(saved.contains(&format!("\"saved_by\": \"{}\"", wavestorm_core::project::VERSION)));
}
