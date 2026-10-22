//! Colour and shape tokens. Widgets read colours from [`Tokens`]; nothing hard-codes a colour.
//!
//! `Classic` reproduces the familiar Windows-classic engineering-software look (grey dialogs,
//! 3-D bevels, navy selection) that users of routing-diagram stormwater tools expect.
//! `Modern` and `ModernDark` are flatter alternatives with the same layout.

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    Classic,
    Modern,
    ModernDark,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 3] = [ThemeKind::Classic, ThemeKind::Modern, ThemeKind::ModernDark];
    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Classic => "Classic",
            ThemeKind::Modern => "Modern Light",
            ThemeKind::ModernDark => "Modern Dark",
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
            ThemeKind::ModernDark => Tokens {
                face: Color32::from_rgb(46, 48, 54),
                light: Color32::from_rgb(70, 73, 80),
                shadow: Color32::from_rgb(30, 31, 35),
                dark: Color32::from_rgb(20, 21, 24),
                field: Color32::from_rgb(32, 34, 38),
                text: Color32::from_rgb(232, 233, 236),
                text_dim: Color32::from_rgb(170, 174, 182),
                text_disabled: Color32::from_rgb(110, 114, 122),
                select: Color32::from_rgb(53, 132, 228),
                caption: Color32::from_rgb(46, 48, 54),
                caption_end: Color32::from_rgb(46, 48, 54),
                caption_text: Color32::from_rgb(232, 233, 236),
                canvas: Color32::from_rgb(28, 30, 34),
                grid: Color32::from_rgb(48, 52, 60),
                arrow: Color32::from_rgb(150, 170, 255),
                node_text: Color32::from_rgb(20, 20, 20),
                label_text: Color32::from_rgb(225, 228, 235),
                chart_bg: Color32::from_rgb(38, 40, 46),
                chart_plot: Color32::from_rgb(28, 30, 34),
                chart_grid: Color32::from_rgb(70, 74, 82),
                chart_title: Color32::from_rgb(150, 190, 255),
                note: Color32::from_rgb(220, 222, 228),
                hint: Color32::from_rgb(140, 170, 255),
                warning: Color32::from_rgb(255, 140, 120),
                error: Color32::from_rgb(255, 90, 90),
                bevel: false,
                radius: 4,
                ..base
            },
        }
    }

    pub fn get(ctx: &Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new("stormwaves-tokens"))).unwrap_or_else(|| Tokens::for_kind(ThemeKind::Classic))
    }

    /// Installs the tokens and the matching egui visuals.
    pub fn apply(self, ctx: &Context) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("stormwaves-tokens"), self));
        let mut v = if self.kind == ThemeKind::ModernDark { Visuals::dark() } else { Visuals::light() };
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
        ctx.all_styles_mut(|s| {
            s.visuals = v.clone();
            s.spacing.item_spacing = egui::vec2(6.0, 4.0);
            s.spacing.button_padding = egui::vec2(6.0, 2.0);
            s.spacing.interact_size.y = 20.0;
            s.spacing.menu_margin = egui::Margin::same(2);
            s.spacing.window_margin = egui::Margin::same(6);
            s.interaction.selectable_labels = false;
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
}
