//! Background underlays: decoding images and rendering PDF pages to textures.

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

/// Largest texture side we create (fits every common GPU).
pub const MAX_SIDE: u32 = 8192;

/// A decoded background raster.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// Number of pages in a PDF (1 for images).
pub fn page_count(bytes: &[u8]) -> usize {
    if !is_pdf(bytes) {
        return 1;
    }
    hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map(|pdf| pdf.pages().len()).unwrap_or(0)
}

/// Decodes an image file or renders one PDF page at about `dpi` dots per inch.
pub fn rasterize(bytes: &[u8], page: usize, dpi: f32) -> Result<Raster, String> {
    if is_pdf(bytes) {
        let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map_err(|e| format!("Cannot read PDF: {e:?}"))?;
        let pages = pdf.pages();
        let p = pages.get(page).ok_or_else(|| format!("PDF has no page {}", page + 1))?;
        let (w, h) = p.render_dimensions();
        // PDF units are points (1/72 in); keep the longest side within the texture limit.
        let mut scale = dpi / 72.0;
        let longest = w.max(h) * scale;
        if longest > MAX_SIDE as f32 {
            scale *= MAX_SIDE as f32 / longest;
        }
        let settings = hayro::RenderSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE, ..Default::default() };
        let cache = hayro::RenderCache::new();
        let pix = hayro::render(p, &cache, &hayro::hayro_interpret::InterpreterSettings::default(), &settings);
        let png = pix.into_png().map_err(|e| format!("Cannot render PDF page: {e:?}"))?;
        return decode_image(&png);
    }
    decode_image(bytes)
}

fn decode_image(bytes: &[u8]) -> Result<Raster, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Cannot read image: {e}"))?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Ok(Raster { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// The visible background as a raster for printing (longest side at most 1600 px, opacity
/// applied against white), or `None` when there is none.
pub fn print_background(project: &wavestorm_core::project::Project) -> Option<wavestorm_core::printing::PrintBackground> {
    let u = project.diagram.underlay.as_ref().filter(|u| u.visible)?;
    // The same raster the diagram texture is made from, so `scale` applies to its pixels.
    let r = rasterize(&u.bytes, u.page, 150.0).ok()?;
    let size = [r.width as f64 * u.scale, r.height as f64 * u.scale];
    let img = image::RgbaImage::from_raw(r.width, r.height, r.rgba)?;
    let img = if r.width.max(r.height) > 1600 { image::DynamicImage::ImageRgba8(img).thumbnail(1600, 1600).to_rgba8() } else { img };
    let alpha = u.opacity.clamp(0.05, 1.0);
    let mut rgb = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for p in img.pixels() {
        let a = alpha * p.0[3] as f32 / 255.0;
        rgb.extend(p.0[..3].iter().map(|c| (255.0 - a * (255.0 - *c as f32)).round() as u8));
    }
    Some(wavestorm_core::printing::PrintBackground {
        rect: [u.origin[0], u.origin[1], u.origin[0] + size[0], u.origin[1] + size[1]],
        width: img.width(),
        height: img.height(),
        rgb: std::sync::Arc::new(rgb),
    })
}

pub fn texture(ctx: &Context, name: &str, r: &Raster) -> TextureHandle {
    let img = ColorImage::from_rgba_unmultiplied([r.width as usize, r.height as usize], &r.rgba);
    ctx.load_texture(name, img, TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_and_rejects_garbage() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let r = rasterize(&png, 0, 96.0).unwrap();
        assert_eq!((r.width, r.height), (3, 2));
        assert!(rasterize(b"not an image", 0, 96.0).is_err());
        assert!(rasterize(b"%PDF-1.4 broken", 0, 96.0).is_err());
        assert_eq!(page_count(b"%PDF-garbage"), 0);
    }
}
