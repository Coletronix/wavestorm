//! Writes `assets/logo.png` and `assets/logo-dark.png`: the logo cut out of its white background
//! (`ui::logo::cut_out`), 640 px wide, for the README and the web loading screen. The dark
//! variant has its dark ink lightened for dark backgrounds, as the app does on dark themes.
//!
//! `cargo run -p wavestorm-ui --example logo_png`

use image::RgbaImage;

/// Resizes premultiplied so transparent pixels don't bleed their colour into the edges.
fn resize(img: &RgbaImage, width: u32) -> RgbaImage {
    let mut pre = img.clone();
    for p in pre.pixels_mut() {
        let a = u16::from(p[3]);
        for c in 0..3 {
            p[c] = ((u16::from(p[c]) * a + 127) / 255) as u8;
        }
    }
    let height = (img.height() * width + img.width() / 2) / img.width();
    let mut out = image::imageops::resize(&pre, width, height, image::imageops::FilterType::Lanczos3);
    for p in out.pixels_mut() {
        let a = u16::from(p[3]);
        for c in 0..3 {
            p[c] = (u16::from(p[c]) * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
        }
    }
    out
}

fn save(img: &RgbaImage, name: &str) {
    let path = format!("{}/assets/{name}", env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = img.save(&path) {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    }
    println!("wrote {path} ({}x{})", img.width(), img.height());
}

fn main() {
    let Some(img) = wavestorm_ui::logo::rgba() else {
        eprintln!("cannot decode assets/logo.jpg");
        std::process::exit(1);
    };
    save(&resize(&img, 640), "logo.png");
    let mut dark = img;
    for p in dark.pixels_mut() {
        let [r, g, b] = wavestorm_ui::logo::lighten_dark([p[0], p[1], p[2]]);
        (p[0], p[1], p[2]) = (r, g, b);
    }
    save(&resize(&dark, 640), "logo-dark.png");
}
