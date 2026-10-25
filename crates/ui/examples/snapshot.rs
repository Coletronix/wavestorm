//! Renders the application offscreen and saves a PNG (no window).
//!
//! `cargo run -p wavestorm-ui --example snapshot -- --out ui.png --size 1280x800 --scale 1 --open-report 1P`

use wavestorm_ui::WaveStormApp;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = arg(&args, "--out").unwrap_or_else(|| "snapshot.png".into());
    let (w, h) = arg(&args, "--size")
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))))
        .unwrap_or((1280.0, 800.0));
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let script: Vec<String> = arg(&args, "--do").map(|s| s.split(';').map(String::from).collect()).unwrap_or_default();
    let app = WaveStormApp::headless();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), app);
    harness.run_steps(4);
    for cmd in script {
        wavestorm_ui::script(harness.state_mut(), &cmd);
        harness.run_steps(4);
    }
    let image = harness.render().expect("render");
    image.save(&out).expect("save png");
    println!("wrote {out}");
}
