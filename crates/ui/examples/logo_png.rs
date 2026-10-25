//! Writes `assets/logo.png` and `assets/logo-dark.png`: the logo cut out of its white background
//! (`ui::logo::cut_out`), 640 px wide, for the README and the web loading screen. The dark
//! variant has its dark ink lightened for dark backgrounds, as the app does on dark themes.
//! Also writes `assets/icon.png`, the 512 px square application icon.
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

/// The application icon: the logo centred on a white rounded square `size` px wide, for the window,
/// the Linux desktop entry, the AppImage and the macOS bundle.
fn icon(img: &RgbaImage, size: u32) -> RgbaImage {
    let logo = resize(img, size * 15 / 16);
    let radius = size as f32 * 0.18;
    let edge = size as f32;
    let mut out = RgbaImage::from_fn(size, size, |x, y| {
        // Distance outside the rounded square, for an anti-aliased edge.
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let dx = (radius - px).max(px - (edge - radius)).max(0.0);
        let dy = (radius - py).max(py - (edge - radius)).max(0.0);
        let a = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
        image::Rgba([255, 255, 255, (a * 255.0).round() as u8])
    });
    let x = i64::from((size - logo.width()) / 2);
    let y = i64::from((size - logo.height()) / 2);
    image::imageops::overlay(&mut out, &logo, x, y);
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
    save(&icon(&img, 512), "icon.png");
    let mut dark = img;
    for p in dark.pixels_mut() {
        let [r, g, b] = wavestorm_ui::logo::lighten_dark([p[0], p[1], p[2]]);
        (p[0], p[1], p[2]) = (r, g, b);
    }
    save(&resize(&dark, 640), "logo-dark.png");
}
