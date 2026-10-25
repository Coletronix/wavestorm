//! WAVEStorm desktop application.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1);
    let mut viewport = eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]).with_min_inner_size([640.0, 420.0]).with_title("WAVEStorm").with_app_id("wavestorm");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../crates/ui/assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // One queued frame instead of eframe's default two: the UI keeps up with the mouse.
        wgpu_options: eframe::WgpuConfiguration { surface: eframe::SurfaceConfig::LOW_LATENCY, ..Default::default() },
        ..Default::default()
    };
    eframe::run_native(
        "WAVEStorm",
        options,
        Box::new(move |_cc| {
            let app = wavestorm_ui::WaveStormApp::new();
            if let Some(p) = &path
                && let Err(e) = app.platform.open_path(p)
            {
                eprintln!("{e}");
            }
            Ok(Box::new(app))
        }),
    )
}
