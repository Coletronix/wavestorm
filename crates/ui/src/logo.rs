//! The StormWAVES logo. `assets/logo.svg` is the vector master; `assets/logo.png` is that file
//! rendered 1024 px wide (`rsvg-convert -w 1024 logo.svg -o logo.png`), which egui draws as a texture.

use egui::{ColorImage, Context, Id, TextureFilter, TextureHandle, TextureOptions, Ui, vec2};

const PNG: &[u8] = include_bytes!("../assets/logo.png");

/// The logo texture, decoded once per theme brightness. On dark themes the black lettering is
/// drawn light so it stays readable; the blue letters keep their colour.
pub fn texture(ctx: &Context, dark: bool) -> Option<TextureHandle> {
    let id = Id::new(("stormwaves-logo", dark));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let mut rgba = image::load_from_memory(PNG).ok()?.to_rgba8();
    if dark {
        for px in rgba.pixels_mut() {
            if px[0] < 64 && px[1] < 64 && px[2] < 64 {
                px[0] = 0xe8;
                px[1] = 0xe8;
                px[2] = 0xe8;
            }
        }
    }
    let img = ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
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

#[cfg(test)]
mod tests {
    #[test]
    fn logo_decodes() {
        let img = image::load_from_memory(super::PNG).unwrap();
        assert_eq!(img.width(), 1024);
    }
}
