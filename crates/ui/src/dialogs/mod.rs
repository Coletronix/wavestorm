//! Edit dialogs and settings windows.

pub mod chamber;
pub mod lookup;
pub mod misc;
pub mod node;
pub mod outlet;
pub mod settings;
pub mod storm;

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

/// The dialog button row: OK, Cancel, (Apply) and Help, centred. Help opens help topic `help`.
pub fn ok_cancel_apply(ui: &mut Ui, apply: bool, help: &'static str) -> Buttons {
    let mut r = Buttons::None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let w = if apply { 4.0 * 84.0 } else { 3.0 * 84.0 };
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
        if widgets::button(ui, "Help", 78.0).clicked() {
            widgets::request_help(ui.ctx(), help);
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        r = Buttons::Cancel;
    }
    r
}

/// Numeric field shown in display units; `to`/`from` convert between internal and display.
#[allow(clippy::too_many_arguments)]
pub fn conv_field(
    ui: &mut Ui,
    id: &str,
    label: &str,
    unit: &str,
    value: &mut f64,
    to: impl Fn(f64) -> f64,
    from: impl Fn(f64) -> f64,
    decimals: usize,
    width: f32,
) -> bool {
    let mut shown = if value.is_finite() { to(*value) } else { f64::NAN };
    let changed = match field_tip(label).filter(|_| !widgets::in_tip(ui.ctx())) {
        Some(tip) => {
            widgets::with_tip(ui, tip, |ui| widgets::num_field(ui, id, label, unit, &mut shown, decimals, width))
        }
        None => widgets::num_field(ui, id, label, unit, &mut shown, decimals, width),
    };
    if changed {
        *value = if shown.is_finite() { from(shown) } else { f64::NAN };
    }
    changed
}

/// Pop-up tip for a common field, by its label (shown when tips are on in Settings > General).
fn field_tip(label: &str) -> Option<&'static str> {
    let key = label.trim_end_matches(':').trim();
    Some(match key {
        "Invert" | "Inlet Invert" | "Invert Elevation" | "Gate Invert" => {
            "Elevation of the bottom of the opening or conduit at its inlet."
        }
        "Outlet Invert" => "Elevation of the conduit bottom at its downstream end; with the length it sets the slope.",
        "Length" => "Length along the flow path.",
        "Diameter" => "Inside diameter of a round opening or pipe.",
        "Width" | "Span" => "Inside width of the opening (span for arches and ellipses).",
        "Height" | "Rise" => "Inside height of the opening (rise for arches and ellipses).",
        "Manning's n" | "Manning's Number" | "n" => "Manning's roughness coefficient of the surface or conduit.",
        "Entrance Loss Ke" | "Ke" => {
            "Culvert entrance loss coefficient (FHWA HDS-5 tables): about 0.2 for a beveled edge, 0.5 for a square edge, 0.9 for a projecting thin wall."
        }
        "Contraction Coefficient" | "Cc" => "Contraction of the flow entering the culvert under inlet control.",
        "Coefficient" | "Discharge Coefficient" | "Orifice Coefficient" | "Cd" => {
            "Discharge coefficient: about 0.6 for a sharp-edged orifice."
        }
        "Discharge Multiplier" => "Multiplies the device's flow, e.g. the number of identical devices.",
        "Crest Length" | "Weir Length" => "Length of the weir crest perpendicular to the flow.",
        "Breadth" | "Crest Breadth" => "Breadth of a broad-crested weir in the direction of flow.",
        "Side Slope Z" | "Side-Z" | "Z" => "Horizontal run per unit rise of the side slopes.",
        "Voids" | "Stone Voids" => "Percent of the gross volume available for water (about 40 % for clean stone).",
        "Multiplier" => "Number of identical storage volumes.",
        "Trigger Elevation" => "Water level that starts the timer of a delayed gate.",
        "Time Delay" => "Hours between reaching the trigger level and the gate starting to open.",
        "Opening Time" => "Hours the gate takes to open fully (0 = opens at once).",
        "Conductivity" | "Exfiltration" => "Rate at which stored water seeps into the ground.",
        "Area" => "Plan area of this sub-area or opening.",
        "CN" => "Runoff curve number (TR-55 table 2-2) for this land cover and soil group.",
        "Slope" => "Average slope along the flow path (rise / run).",
        "Starting Elevation" => "Water level at the start of the calculation (blank = bottom of the storage).",
        _ => return None,
    })
}

/// Optional numeric field: blank means `None`.
#[allow(clippy::too_many_arguments)]
pub fn opt_field(
    ui: &mut Ui,
    id: &str,
    label: &str,
    unit: &str,
    value: &mut Option<f64>,
    to: impl Fn(f64) -> f64,
    from: impl Fn(f64) -> f64,
    decimals: usize,
    width: f32,
) -> bool {
    let mut v = value.unwrap_or(f64::NAN);
    let changed = conv_field(ui, id, label, unit, &mut v, to, from, decimals, width);
    if changed {
        *value = if v.is_finite() { Some(v) } else { None };
    }
    changed
}
