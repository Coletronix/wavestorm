//! Measures the CPU time of one frame (UI + tessellation) per theme, with the pointer moving.
//!
//! `cargo run --release -p wavestorm-ui --example frame_bench`

use std::time::Instant;
use wavestorm_ui::{WaveStormApp, script};

fn main() {
    let probe = std::env::args().nth(1);
    if let Some(cmd) = probe {
        // `frame_bench <script>`: print the repaint delay for 40 idle frames.
        let ctx = egui::Context::default();
        let mut app = WaveStormApp::headless();
        for c in cmd.split(';') {
            script(&mut app, c);
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let mut delays = Vec::new();
        for _ in 0..40 {
            let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
            out.textures_delta.clear();
            let d =
                out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay.as_secs_f32()).unwrap_or(-1.0);
            delays.push(if d > 1e6 { "∞".to_string() } else { format!("{d}") });
        }
        println!("{}", delays.join(" "));
        return;
    }
    for theme in ["theme:classic", "theme:dark", "theme:glass"] {
        for extra in ["", "report:1P"] {
            let ctx = egui::Context::default();
            let mut app = WaveStormApp::headless();
            script(&mut app, theme);
            if !extra.is_empty() {
                script(&mut app, extra);
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
            let frames = 120;
            let (mut ui_t, mut tess_t) = (0.0, 0.0);
            for i in 0..frames + 10 {
                let mut raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                raw.events.push(egui::Event::PointerMoved(egui::pos2(
                    200.0 + (i % 50) as f32 * 15.0,
                    300.0 + (i % 7) as f32 * 20.0,
                )));
                let t0 = Instant::now();
                let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
                out.textures_delta.clear();
                let t1 = Instant::now();
                let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
                let t2 = Instant::now();
                std::hint::black_box(prims);
                if i >= 10 {
                    ui_t += (t1 - t0).as_secs_f64();
                    tess_t += (t2 - t1).as_secs_f64();
                }
            }
            println!(
                "{theme:14} {extra:10} ui {:6.2} ms  tessellate {:6.2} ms",
                ui_t * 1e3 / frames as f64,
                tess_t * 1e3 / frames as f64
            );
            // With no input, a settled UI should stop asking for frames.
            let mut delays = Vec::new();
            for _ in 0..6 {
                let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
                out.textures_delta.clear();
                delays.push(
                    out.viewport_output
                        .get(&egui::ViewportId::ROOT)
                        .map(|v| v.repaint_delay.as_secs_f32())
                        .unwrap_or(-1.0),
                );
            }
            println!("    idle repaint delays (s): {delays:?}");
        }
    }
}
