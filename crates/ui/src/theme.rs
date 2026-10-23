//! Colour and shape tokens. Widgets read colours from [`Tokens`]; nothing hard-codes a colour.
//!
//! `Classic` reproduces the familiar Windows-classic engineering-software look (grey dialogs,
//! 3-D bevels, navy selection) that users of routing-diagram stormwater tools expect.
//! `Modern` and `ModernDark` are flatter alternatives with the same layout. `Glass` is a dark,
//! iridescent glass theme with its own modernised layout (see `glass.rs` and
//! `WaveStormApp::draw_glass`).

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    Classic,
    Modern,
    ModernDark,
    Glass,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 4] = [ThemeKind::Classic, ThemeKind::Modern, ThemeKind::ModernDark, ThemeKind::Glass];
    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Classic => "Classic",
            ThemeKind::Modern => "Modern Light",
            ThemeKind::ModernDark => "Modern Dark",
            ThemeKind::Glass => "Iridescent Glass",
        }
    }
}

/// Theme tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub kind: ThemeKind,
    /// Dialog and chrome face colour.
    pub face: Color32,
    /// Bevel highlight / shadow / dark shadow.
    pub light: Color32,
    pub shadow: Color32,
    pub dark: Color32,
    /// Text fields and list backgrounds.
    pub field: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    /// Selection (menu highlight, selected rows).
    pub select: Color32,
    pub select_text: Color32,
    /// Window caption gradient.
    pub caption: Color32,
    pub caption_end: Color32,
    pub caption_text: Color32,
    /// Diagram canvas.
    pub canvas: Color32,
    pub grid: Color32,
    /// Routing arrows (primary) and secondary/tertiary arrows.
    pub arrow: Color32,
    pub arrow_secondary: Color32,
    pub arrow_tertiary: Color32,
    /// Node symbols: fill and outline per type.
    pub subcat: (Color32, Color32),
    pub reach: (Color32, Color32),
    pub pond: (Color32, Color32),
    pub link: (Color32, Color32),
    pub node_text: Color32,
    pub label_text: Color32,
    /// Report charts.
    pub chart_bg: Color32,
    pub chart_plot: Color32,
    pub chart_grid: Color32,
    pub chart_title: Color32,
    pub series: [Color32; 6],
    /// Message severities.
    pub note: Color32,
    pub hint: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub bevel: bool,
    pub radius: u8,
    /// Glass rendering: translucent gradient surfaces with iridescent rims (`glass.rs`) and the
    /// modernised layout.
    pub glass: bool,
}

/// Translucent white, for glass borders and sheens.
pub fn white(alpha: f32) -> Color32 {
    Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl Tokens {
    pub fn for_kind(kind: ThemeKind) -> Tokens {
        let base = Tokens {
            kind,
            face: Color32::from_rgb(212, 208, 200),
            light: Color32::from_rgb(255, 255, 255),
            shadow: Color32::from_rgb(128, 128, 128),
            dark: Color32::from_rgb(64, 64, 64),
            field: Color32::WHITE,
            text: Color32::BLACK,
            text_dim: Color32::from_rgb(64, 64, 64),
            text_disabled: Color32::from_rgb(141, 137, 128),
            select: Color32::from_rgb(10, 36, 106),
            select_text: Color32::WHITE,
            caption: Color32::from_rgb(10, 36, 106),
            caption_end: Color32::from_rgb(166, 202, 240),
            caption_text: Color32::WHITE,
            canvas: Color32::WHITE,
            grid: Color32::from_rgb(222, 226, 236),
            arrow: Color32::from_rgb(40, 40, 140),
            arrow_secondary: Color32::from_rgb(214, 40, 40),
            arrow_tertiary: Color32::from_rgb(20, 140, 60),
            subcat: (Color32::from_rgb(196, 236, 188), Color32::from_rgb(44, 150, 44)),
            reach: (Color32::from_rgb(252, 214, 182), Color32::from_rgb(196, 110, 40)),
            pond: (Color32::from_rgb(200, 228, 252), Color32::from_rgb(40, 90, 230)),
            link: (Color32::from_rgb(252, 212, 222), Color32::from_rgb(214, 50, 90)),
            node_text: Color32::from_rgb(20, 20, 20),
            label_text: Color32::from_rgb(20, 20, 20),
            chart_bg: Color32::WHITE,
            chart_plot: Color32::from_rgb(255, 255, 255),
            chart_grid: Color32::from_rgb(170, 170, 170),
            chart_title: Color32::from_rgb(0, 0, 200),
            series: [
                Color32::from_rgb(30, 200, 60),
                Color32::from_rgb(150, 150, 150),
                Color32::from_rgb(30, 110, 250),
                Color32::from_rgb(240, 50, 50),
                Color32::from_rgb(220, 140, 20),
                Color32::from_rgb(150, 40, 180),
            ],
            note: Color32::from_rgb(0, 0, 0),
            hint: Color32::from_rgb(0, 0, 160),
            warning: Color32::from_rgb(170, 0, 0),
            error: Color32::from_rgb(220, 0, 0),
            bevel: true,
            radius: 0,
            glass: false,
        };
        match kind {
            ThemeKind::Classic => base,
            ThemeKind::Modern => Tokens {
                face: Color32::from_rgb(240, 241, 244),
                light: Color32::from_rgb(252, 252, 253),
                shadow: Color32::from_rgb(200, 203, 210),
                dark: Color32::from_rgb(170, 174, 184),
                text_dim: Color32::from_rgb(90, 94, 104),
                text_disabled: Color32::from_rgb(160, 164, 172),
                select: Color32::from_rgb(53, 132, 228),
                caption: Color32::from_rgb(240, 241, 244),
                caption_end: Color32::from_rgb(240, 241, 244),
                caption_text: Color32::from_rgb(30, 30, 34),
                chart_bg: Color32::from_rgb(250, 250, 252),
                chart_title: Color32::from_rgb(30, 60, 140),
                bevel: false,
                radius: 4,
                ..base
            },
            // Borders (`shadow`), disabled text and the secondary/tertiary routing colours are
            // lighter than a straight inversion of the light theme so they keep at least 3:1
            // contrast on the dark faces.
            ThemeKind::ModernDark => Tokens {
                face: Color32::from_rgb(46, 48, 54),
                light: Color32::from_rgb(70, 73, 80),
                shadow: Color32::from_rgb(92, 96, 106),
                dark: Color32::from_rgb(20, 21, 24),
                field: Color32::from_rgb(32, 34, 38),
                text: Color32::from_rgb(232, 233, 236),
                text_dim: Color32::from_rgb(170, 174, 182),
                text_disabled: Color32::from_rgb(134, 138, 148),
                select: Color32::from_rgb(53, 132, 228),
                caption: Color32::from_rgb(46, 48, 54),
                caption_end: Color32::from_rgb(46, 48, 54),
                caption_text: Color32::from_rgb(232, 233, 236),
                canvas: Color32::from_rgb(28, 30, 34),
                grid: Color32::from_rgb(48, 52, 60),
                arrow: Color32::from_rgb(150, 170, 255),
                arrow_secondary: Color32::from_rgb(255, 110, 110),
                arrow_tertiary: Color32::from_rgb(90, 214, 128),
                node_text: Color32::from_rgb(20, 20, 20),
                label_text: Color32::from_rgb(225, 228, 235),
                chart_bg: Color32::from_rgb(38, 40, 46),
                chart_plot: Color32::from_rgb(28, 30, 34),
                chart_grid: Color32::from_rgb(78, 82, 92),
                chart_title: Color32::from_rgb(150, 190, 255),
                series: [
                    Color32::from_rgb(60, 210, 90),
                    Color32::from_rgb(170, 172, 178),
                    Color32::from_rgb(80, 150, 255),
                    Color32::from_rgb(255, 95, 95),
                    Color32::from_rgb(240, 165, 50),
                    Color32::from_rgb(196, 110, 230),
                ],
                note: Color32::from_rgb(220, 222, 228),
                hint: Color32::from_rgb(140, 170, 255),
                warning: Color32::from_rgb(255, 140, 120),
                error: Color32::from_rgb(255, 118, 118),
                bevel: false,
                radius: 4,
                ..base
            },
            // Palette from the dark-glass references in `reference_images/research/glass/`:
            // slate-navy surfaces, slate text, cyan/violet/rose accents, white hairlines.
            ThemeKind::Glass => Tokens {
                face: Color32::from_rgb(17, 22, 38),
                light: Color32::from_rgb(32, 40, 66),
                shadow: white(0.13),
                dark: Color32::from_rgb(4, 6, 12),
                field: Color32::from_rgb(9, 12, 24),
                text: Color32::from_rgb(226, 232, 240),
                text_dim: Color32::from_rgb(148, 163, 184),
                text_disabled: Color32::from_rgb(108, 120, 144),
                select: Color32::from_rgb(110, 86, 240),
                select_text: Color32::WHITE,
                caption: Color32::from_rgb(17, 22, 38),
                caption_end: Color32::from_rgb(17, 22, 38),
                caption_text: Color32::from_rgb(226, 232, 240),
                canvas: Color32::from_rgb(7, 10, 22),
                grid: Color32::from_rgb(44, 56, 92),
                arrow: Color32::from_rgb(94, 231, 255),
                arrow_secondary: Color32::from_rgb(255, 121, 198),
                arrow_tertiary: Color32::from_rgb(163, 230, 53),
                subcat: (rgba(52, 211, 153, 0.16), Color32::from_rgb(52, 211, 153)),
                reach: (rgba(251, 146, 60, 0.16), Color32::from_rgb(251, 146, 60)),
                pond: (rgba(56, 189, 248, 0.16), Color32::from_rgb(56, 189, 248)),
                link: (rgba(244, 114, 182, 0.16), Color32::from_rgb(244, 114, 182)),
                node_text: Color32::from_rgb(241, 245, 249),
                label_text: Color32::from_rgb(203, 213, 225),
                chart_bg: Color32::from_rgb(13, 18, 34),
                chart_plot: Color32::from_rgb(9, 13, 26),
                chart_grid: Color32::from_rgb(48, 60, 96),
                chart_title: Color32::from_rgb(165, 180, 252),
                series: [
                    Color32::from_rgb(52, 211, 153),
                    Color32::from_rgb(148, 163, 184),
                    Color32::from_rgb(56, 189, 248),
                    Color32::from_rgb(251, 113, 133),
                    Color32::from_rgb(251, 191, 36),
                    Color32::from_rgb(192, 132, 252),
                ],
                note: Color32::from_rgb(226, 232, 240),
                hint: Color32::from_rgb(125, 211, 252),
                warning: Color32::from_rgb(251, 191, 36),
                error: Color32::from_rgb(248, 113, 113),
                bevel: false,
                radius: 8,
                glass: true,
                ..base
            },
        }
    }

    /// Dark themes draw light ink on dark faces.
    pub fn is_dark(&self) -> bool {
        matches!(self.kind, ThemeKind::ModernDark | ThemeKind::Glass)
    }

    pub fn get(ctx: &Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new("wavestorm-tokens"))).unwrap_or_else(|| Tokens::for_kind(ThemeKind::Classic))
    }

    /// Installs the tokens and the matching egui visuals.
    pub fn apply(self, ctx: &Context) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("wavestorm-tokens"), self));
        let mut v = if self.is_dark() { Visuals::dark() } else { Visuals::light() };
        let r = CornerRadius::same(self.radius);
        v.panel_fill = self.face;
        v.window_fill = self.face;
        v.extreme_bg_color = self.field;
        v.faint_bg_color = self.face;
        v.code_bg_color = self.field;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.select;
        v.selection.stroke = Stroke::new(1.0, self.select_text);
        v.hyperlink_color = self.select;
        v.window_corner_radius = r;
        v.menu_corner_radius = r;
        v.window_shadow = egui::epaint::Shadow { offset: [2, 2], blur: if self.bevel { 0 } else { 8 }, spread: 0, color: Color32::from_black_alpha(60) };
        v.popup_shadow = v.window_shadow;
        v.window_stroke = Stroke::new(1.0, self.dark);
        v.striped = false;
        for (w, fill) in [
            (&mut v.widgets.noninteractive, self.face),
            (&mut v.widgets.inactive, self.face),
            (&mut v.widgets.hovered, self.face),
            (&mut v.widgets.active, self.face),
            (&mut v.widgets.open, self.face),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = r;
            w.fg_stroke = Stroke::new(1.0, self.text);
            w.bg_stroke = Stroke::new(1.0, self.shadow);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.shadow);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.dark);
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.expansion = 0.0;
        v.widgets.active.bg_fill = self.shadow.gamma_multiply(0.6);
        if !self.bevel {
            v.widgets.hovered.weak_bg_fill = self.light;
            v.widgets.hovered.bg_fill = self.light;
        }
        if self.glass {
            self.glass_visuals(&mut v);
        }
        ctx.all_styles_mut(|s| {
            s.visuals = v.clone();
            s.spacing.item_spacing = egui::vec2(6.0, 4.0);
            s.spacing.button_padding = egui::vec2(6.0, 2.0);
            s.spacing.interact_size.y = 20.0;
            s.spacing.menu_margin = egui::Margin::same(2);
            s.spacing.window_margin = egui::Margin::same(6);
            s.interaction.selectable_labels = false;
            if self.glass {
                s.spacing.button_padding = egui::vec2(8.0, 3.0);
                s.spacing.menu_margin = egui::Margin::same(6);
                s.spacing.scroll = egui::style::ScrollStyle::floating();
            }
            for (style, size) in [
                (egui::TextStyle::Body, 13.0),
                (egui::TextStyle::Button, 13.0),
                (egui::TextStyle::Small, 11.0),
                (egui::TextStyle::Monospace, 12.5),
                (egui::TextStyle::Heading, 16.0),
            ] {
                if let Some(f) = s.text_styles.get_mut(&style) {
                    f.size = size;
                }
            }
        });
    }

    /// egui visuals for the glass theme: rounded, softly shadowed popups and translucent
    /// widget fills with violet hover/press states.
    fn glass_visuals(&self, v: &mut Visuals) {
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_shadow = egui::epaint::Shadow { offset: [0, 14], blur: 36, spread: 0, color: Color32::from_black_alpha(150) };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 22, spread: 0, color: Color32::from_black_alpha(140) };
        v.window_stroke = Stroke::new(1.0, white(0.12));
        v.hyperlink_color = Color32::from_rgb(125, 211, 252);
        v.selection.bg_fill = rgba(124, 92, 255, 0.55);
        let accent = Color32::from_rgb(167, 139, 250);
        let states = [
            (&mut v.widgets.inactive, white(0.06), white(0.12)),
            (&mut v.widgets.hovered, white(0.11), accent.gamma_multiply(0.7)),
            (&mut v.widgets.active, rgba(124, 92, 255, 0.35), accent),
            (&mut v.widgets.open, white(0.09), white(0.16)),
        ];
        for (w, fill, stroke) in states {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::new(1.0, stroke);
            w.corner_radius = CornerRadius::same(self.radius);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, white(0.08));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn dark_themes_keep_readable_contrast() {
        for kind in [ThemeKind::ModernDark, ThemeKind::Glass] {
            let t = Tokens::for_kind(kind);
            assert!(contrast(t.text, t.face) >= 7.0, "{kind:?} text");
            assert!(contrast(t.text_dim, t.face) >= 4.5, "{kind:?} dim text");
            assert!(contrast(t.text_disabled, t.face) >= 3.0, "{kind:?} disabled text");
            for c in [t.arrow, t.arrow_secondary, t.arrow_tertiary] {
                assert!(contrast(c, t.canvas) >= 3.0, "{kind:?} arrow {c:?}");
            }
            for c in t.series {
                assert!(contrast(c, t.chart_bg) >= 3.0, "{kind:?} series {c:?}");
            }
            for c in [t.hint, t.warning, t.error] {
                assert!(contrast(c, t.face) >= 4.5, "{kind:?} message {c:?}");
            }
        }
        let d = Tokens::for_kind(ThemeKind::ModernDark);
        assert!(contrast(d.shadow, d.face) >= 1.9, "dark borders visible");
    }
}
