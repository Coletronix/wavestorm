//! A settled UI must stop asking for frames. If something requests a repaint every frame (as
//! sending the window title every frame once did), the app renders continuously and, with the
//! swap-chain queue, the screen lags a few frames behind the mouse.

use wavestorm_ui::{WaveStormApp, script};

#[test]
fn app_goes_idle_without_input() {
    for theme in ["theme:classic", "theme:dark", "theme:glass"] {
        for extra in ["", "report:1P", "edit:1P", "settings"] {
            let ctx = egui::Context::default();
            let mut app = WaveStormApp::headless();
            script(&mut app, theme);
            script(&mut app, extra);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
            let mut delay = std::time::Duration::ZERO;
            // Opening a window animates for about 13 frames (0.2 s at 60 Hz); then it must settle.
            for _ in 0..30 {
                let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
                out.textures_delta.clear();
                delay = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap_or_default();
            }
            assert!(delay.as_secs() >= 1, "{theme} {extra}: still repainting every {delay:?}");
        }
    }
}
