//! WAVEStorm in the browser: the same app as the desktop build, through eframe's web runner.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id("wavestorm_canvas").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"wavestorm_canvas\">");
            return;
        };
        let result = eframe::WebRunner::new()
            .start(canvas, eframe::WebOptions::default(), Box::new(|_cc| Ok(Box::new(wavestorm_ui::WaveStormApp::new()))))
            .await;
        if let Some(el) = document.get_element_by_id("wavestorm_loading") {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>WAVEStorm failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("wavestorm-web runs in the browser: build it with `trunk build --release` in apps/wavestorm-web");
}
