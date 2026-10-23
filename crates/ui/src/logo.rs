//! The WAVEStorm logo. `assets/logo.jpg` is the master artwork on a white background; [`cut_out`]
//! turns the white into transparency when the texture is built. `assets/logo.png` and
//! `assets/logo-dark.png` (for dark backgrounds) are the same cut-out written by
//! `cargo run -p wavestorm-ui --example logo_png` for the README and the web loading screen;
//! re-run it after replacing the JPG.

use egui::{ColorImage, Context, Id, TextureFilter, TextureHandle, TextureOptions, Ui, vec2};
use image::{RgbImage, RgbaImage};

const JPG: &[u8] = include_bytes!("../assets/logo.jpg");

/// The master artwork decoded and cut out of its white background.
pub fn rgba() -> Option<RgbaImage> {
    Some(cut_out(&image::load_from_memory(JPG).ok()?.to_rgb8()))
}

/// The logo texture, built once per theme brightness. On dark themes the dark ink (lettering,
/// outlines and navy bands) is mirrored in lightness so it stays readable.
pub fn texture(ctx: &Context, dark: bool) -> Option<TextureHandle> {
    let id = Id::new(("wavestorm-logo", dark));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let mut img = rgba()?;
    if dark {
        for px in img.pixels_mut() {
            let [r, g, b] = lighten_dark([px[0], px[1], px[2]]);
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    }
    let img = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let t = ctx.load_texture("logo", img, TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear)));
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// Draws the logo `width` points wide.
pub fn show(ui: &mut Ui, width: f32) {
    if let Some(t) = texture(ui.ctx(), ui.visuals().dark_mode) {
        let size = t.size_vec2();
        ui.add(egui::Image::new(&t).fit_to_exact_size(vec2(width, width * size.y / size.x)));
    }
}

/// Colour-to-alpha against white. Opacity is reduced only within two pixels of background white,
/// relative to the darkest ink in a 5 × 5 neighbourhood, so dark strokes keep crisp anti-aliased
/// edges while pale fills stay opaque. The colour of partly transparent pixels is unmixed from
/// the white, and the result is cropped to the artwork plus a 6 px margin.
pub fn cut_out(src: &RgbImage) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let depth: Vec<f32> = src.pixels().map(|p| f32::from(255 - p.0.iter().copied().min().unwrap_or(255)) / 255.0).collect();
    let white: Vec<f32> = depth.iter().map(|&d| if d < 0.06 { 1.0 } else { 0.0 }).collect();
    let ink = max_filter(&depth, w, h, 2);
    let near_white = max_filter(&white, w, h, 2);
    let mut out = RgbaImage::new(src.width(), src.height());
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (i, (px, o)) in src.pixels().zip(out.pixels_mut()).enumerate() {
        let d = depth.get(i).copied().unwrap_or(0.0);
        let alpha = if d < 0.03 {
            0.0
        } else if near_white.get(i).copied().unwrap_or(0.0) > 0.0 {
            ((d - 0.03) / (ink.get(i).copied().unwrap_or(1.0) - 0.03).max(0.27)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if alpha <= 0.0 {
            continue;
        }
        let unmix = |c: u8| ((f32::from(c) - (1.0 - alpha) * 255.0) / alpha).clamp(0.0, 255.0).round() as u8;
        *o = image::Rgba([unmix(px[0]), unmix(px[1]), unmix(px[2]), (alpha * 255.0).round() as u8]);
        if alpha > 0.02 {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x1 < x0 || y1 < y0 {
        return out;
    }
    let pad = 6;
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad + 1).min(w), (y1 + pad + 1).min(h));
    image::imageops::crop_imm(&out, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image()
}

/// Separable square maximum filter of radius `r`.
fn max_filter(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let pass = |v: &[f32], horizontal: bool| -> Vec<f32> {
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let (c, n) = if horizontal { (x, w) } else { (y, h) };
                (c.saturating_sub(r)..(c + r + 1).min(n)).map(|k| if horizontal { y * w + k } else { k * w + x }).filter_map(|j| v.get(j).copied()).fold(0.0, f32::max)
            })
            .collect()
    };
    pass(&pass(v, true), false)
}

/// Mirrors the lightness of dark colours (L' = max(L, 0.96 − 1.45 L)) keeping hue and
/// saturation: near-black becomes near-white, navy becomes pale blue, and colours lighter than
/// L ≈ 0.39 are unchanged.
pub fn lighten_dark(c: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = c.map(|v| f32::from(v) / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let l2 = l.max(0.96 - 1.45 * l);
    if l2 <= l {
        return c;
    }
    let chroma = mx - mn;
    let s = if chroma > 0.0 { chroma / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6) } else { 0.0 };
    let scale = if chroma > 0.0 { s * (1.0 - (2.0 * l2 - 1.0).abs()) / chroma } else { 0.0 };
    [r, g, b].map(|v| ((l2 + (v - l) * scale).clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_background_is_transparent() {
        let img = super::rgba().unwrap();
        assert!(img.width() > 900 && img.height() > 300, "{}x{}", img.width(), img.height());
        // Corners are background; the W stem is solid dark ink.
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert_eq!(img.get_pixel(img.width() - 1, img.height() - 1)[3], 0);
        let w = img.get_pixel(53, 192);
        assert!(w[3] == 255 && w[0] < 40, "{w:?}");
    }

    #[test]
    fn dark_ink_is_lightened() {
        assert!(super::lighten_dark([5, 6, 8])[0] > 220);
        assert_eq!(super::lighten_dark([84, 160, 200]), [84, 160, 200]);
    }
}
