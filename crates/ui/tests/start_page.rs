//! With no project open the app shows its start page, stays empty (nothing creates a project
//! behind the user's back), goes idle, and opens projects from there.

use egui::vec2;
use wavestorm_ui::platform::{Incoming, Purpose};
use wavestorm_ui::{WaveStormApp, script};

fn harness() -> egui_kittest::Harness<'static, WaveStormApp> {
    egui_kittest::Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

#[test]
fn nothing_open_shows_the_start_page() {
    for theme in ["theme:classic", "theme:glass"] {
        let mut h = harness();
        script(h.state_mut(), theme);
        h.run_steps(3);
        script(h.state_mut(), "close");
        h.run_steps(5);
        assert!(h.state_mut().docs.is_empty(), "{theme}: closing the last project leaves nothing open");
        // Keys that need a project do nothing.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.key_press(egui::Key::Delete);
        h.run_steps(3);
        assert!(h.state_mut().docs.is_empty(), "{theme}: still nothing open");
        // Opening a project from the start page.
        let json = wavestorm_ui::samples::sample_project().to_json().unwrap().into_bytes();
        h.state_mut().platform.push(Incoming { purpose: Purpose::OpenProject, name: "site.waves".into(), path: None, bytes: json });
        h.run_steps(3);
        assert_eq!(h.state_mut().docs.len(), 1, "{theme}: opened");
        script(h.state_mut(), "close");
        script(h.state_mut(), "new");
        h.run_steps(2);
        assert_eq!(h.state_mut().docs.len(), 1, "{theme}: new project");
    }
}
