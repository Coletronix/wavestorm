//! General settings: preferences change the app's behaviour without touching the user's files.

use wavestorm_ui::windows::Window;
use wavestorm_ui::{WaveStormApp, script};

fn project_reports(app: &mut WaveStormApp) -> usize {
    app.doc().windows.iter().filter(|w| matches!(w, Window::ProjectReport(_))).count()
}

#[test]
fn project_reports_replace_unless_multiple_allowed() {
    let mut app = WaveStormApp::headless();
    script(&mut app, "project-report");
    script(&mut app, "project-report");
    assert_eq!(project_reports(&mut app), 1);

    app.prefs.multiple_project_reports = true;
    script(&mut app, "project-report");
    script(&mut app, "project-report");
    assert_eq!(project_reports(&mut app), 3);
    let serials: Vec<u64> = app
        .doc()
        .windows
        .iter()
        .filter_map(|w| if let Window::ProjectReport(s) = w { Some(s.serial) } else { None })
        .collect();
    assert_eq!(serials, vec![0, 1, 2]);
}

#[test]
fn headless_app_uses_default_prefs() {
    let app = WaveStormApp::headless();
    assert_eq!(app.prefs, wavestorm_ui::prefs::Prefs::default());
    assert_eq!(app.theme, app.prefs.theme);
}
