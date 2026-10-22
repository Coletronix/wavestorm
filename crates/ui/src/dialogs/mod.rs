//! Edit dialogs and settings windows.

pub mod lookup;
pub mod misc;
pub mod node;
pub mod outlet;
pub mod settings;

use crate::widgets;
use egui::Ui;

/// OK / Cancel / Apply row at the bottom of a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Buttons {
    None,
    Ok,
    Cancel,
    Apply,
}

pub fn ok_cancel_apply(ui: &mut Ui, apply: bool) -> Buttons {
    let mut r = Buttons::None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let w = if apply { 3.0 * 84.0 } else { 2.0 * 84.0 };
        ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
        if widgets::button(ui, "OK", 78.0).clicked() {
            r = Buttons::Ok;
        }
        if widgets::button(ui, "Cancel", 78.0).clicked() {
            r = Buttons::Cancel;
        }
        if apply && widgets::button(ui, "Apply", 78.0).clicked() {
            r = Buttons::Apply;
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        r = Buttons::Cancel;
    }
    r
}

/// Numeric field shown in display units; `to`/`from` convert between internal and display.
#[allow(clippy::too_many_arguments)]
pub fn conv_field(ui: &mut Ui, id: &str, label: &str, unit: &str, value: &mut f64, to: impl Fn(f64) -> f64, from: impl Fn(f64) -> f64, decimals: usize, width: f32) -> bool {
    let mut shown = if value.is_finite() { to(*value) } else { f64::NAN };
    let changed = widgets::num_field(ui, id, label, unit, &mut shown, decimals, width);
    if changed {
        *value = if shown.is_finite() { from(shown) } else { f64::NAN };
    }
    changed
}

/// Optional numeric field: blank means `None`.
#[allow(clippy::too_many_arguments)]
pub fn opt_field(ui: &mut Ui, id: &str, label: &str, unit: &str, value: &mut Option<f64>, to: impl Fn(f64) -> f64, from: impl Fn(f64) -> f64, decimals: usize, width: f32) -> bool {
    let mut v = value.unwrap_or(f64::NAN);
    let changed = conv_field(ui, id, label, unit, &mut v, to, from, decimals, width);
    if changed {
        *value = if v.is_finite() { Some(v) } else { None };
    }
    changed
}
