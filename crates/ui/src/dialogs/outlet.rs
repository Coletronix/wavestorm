//! Outlet device editor.

use super::lookup::{ENTRANCES, manning_button};
use super::{Buttons, conv_field, ok_cancel_apply, opt_field};
use crate::theme::Tokens;
use crate::widgets;
use egui::{Color32, Rect, RichText, Stroke, Ui, pos2, vec2};
use stormwaves_core::geometry::{PipeShape, Section};
use stormwaves_core::outlets::{DeviceKind, ExfilMode, OpeningShape, OutletDevice, Plane, Routing};
use stormwaves_core::project::Pond;
use stormwaves_core::units::Units;

pub struct DeviceEditor {
    pub index: Option<usize>,
    pub device: OutletDevice,
}

impl DeviceEditor {
    pub fn new(index: Option<usize>, device: OutletDevice) -> Self {
        DeviceEditor { index, device }
    }
}

/// Device templates for the "Add outlet device" buttons.
pub fn templates() -> Vec<(&'static str, DeviceKind)> {
    vec![
        (
            "Culvert",
            DeviceKind::Culvert {
                shape: PipeShape::Round,
                width: 1.5,
                height: 1.5,
                length: 50.0,
                outlet_invert: f64::NAN,
                n: 0.013,
                ke: 0.5,
                cc: 0.9,
                fill: 0.0,
                entrance: "RCP, square edge headwall".into(),
            },
        ),
        (
            "Orifice/Grate",
            DeviceKind::Orifice {
                shape: OpeningShape::Circular { diameter: 0.5 },
                plane: Plane::Vertical,
                cd: 0.6,
                columns: 1,
                rows: 1,
                row_spacing: 0.0,
                weir_at_low_head: true,
                grate: None,
            },
        ),
        ("Sharp-Crested Weir", DeviceKind::SharpCrestedWeir { length: 4.0, end_contractions: 2, crest_height: 0.0, rise: None }),
        ("Broad-Crested Weir", DeviceKind::BroadCrestedWeir { length: 10.0, breadth: 2.0, side_z: 0.0, rise: None }),
        ("Vee/Trapezoidal Weir", DeviceKind::VeeTrapWeir { notch_angle: 90.0, crest_length: 0.0, cv: 2.50, rise: None }),
        // A crest profile of any shape, symmetrical or not (offset, elevation above the invert).
        ("Asymmetrical / Custom Weir", DeviceKind::CustomWeir { points: vec![(0.0, 1.5), (3.0, 0.0), (6.0, 0.0), (8.0, 0.5), (12.0, 1.0)], coefficient: 3.0 }),
        ("Exfiltration", DeviceKind::Exfiltration { mode: ExfilMode::SurfaceVelocity { velocity: 1.0 / 12.0 / 3600.0 }, phase_in: 0.01, max_elevation: None }),
        ("Special (Rating Table)", DeviceKind::Special { table: vec![(0.0, 0.0), (1.0, 1.0), (2.0, 3.0)], head_based: true, extrapolate: false }),
        ("Constant Flow/Skimmer", DeviceKind::ConstantFlow { flow: 0.5, phase_in: 0.3 }),
        ("Pump", DeviceKind::Pump { rating: vec![(0.0, 2.0), (10.0, 1.5), (20.0, 0.0)], on_elevation: 102.0, off_elevation: 100.5, discharge_elevation: 110.0, max_elevation: None, priority_group: 0 }),
        ("Tube/Siphon", DeviceKind::Siphon { diameter: 1.0, length: 40.0, n: 0.013, ke: 0.5, outlet_elevation: 95.0, prime_elevation: 104.0, break_elevation: 100.5 }),
        ("Dam Breach", DeviceKind::DamBreach { start_time: None, start_elevation: Some(105.0), formation: 1.0, final_bottom: 100.0, final_width: 20.0, side_z: 1.0 }),
        ("Delayed Gate", DeviceKind::DelayedGate { trigger_elevation: 102.0, delay: 2.0, opening: 1.0, width: 2.0, height: 2.0, cd: 0.6 }),
        ("Channel Outlet", DeviceKind::Channel { section: Section::Trapezoidal { bottom: 4.0, z_left: 2.0, z_right: 2.0, depth: 3.0 }, n: 0.03, slope: 0.01 }),
    ]
}

/// Returns Some(true) on OK, Some(false) on Cancel.
pub fn show(ctx: &egui::Context, ed: &mut DeviceEditor, u: &Units, pond: &Pond, pond_number: &str) -> Option<bool> {
    let mut result = Buttons::None;
    let title = format!("Pond {pond_number}P {} Outlet", ed.device.kind.label());
    crate::frame::Win::new(title).id(egui::Id::new("device-editor")).collapsible(false).resizable(false).show(ctx, |ui| {
        let d = &mut ed.device;
        let len = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.len_label(), v, |x| u.len(x), |x| u.len_in(x), 2, 130.0);
        let inch = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.small_len_label(), v, |x| u.small_len(x), |x| u.small_len_in(x), 1, 130.0);
        ui.horizontal(|ui| {
            widgets::text_field(ui, "Description:", &mut d.description, 220.0);
            let mut routes = vec![(Routing::Primary, "Primary".to_string()), (Routing::Secondary, "Secondary".to_string()), (Routing::Tertiary, "Tertiary".to_string()), (Routing::Discarded, "Discarded".to_string())];
            for i in 0..pond.devices.len() {
                if Some(i) != ed.index {
                    routes.push((Routing::Device(i), format!("Device {}", i + 1)));
                }
            }
            widgets::combo_field(ui, "route", "Routing:", &mut d.routing, &routes, 120.0);
            conv_field(ui, "mult", "Discharge Multiplier:", "", &mut d.multiplier, |x| x, |x| x.max(0.0), 2, 130.0);
        });
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| match &mut d.kind {
                DeviceKind::Culvert { shape, width, height, length, outlet_invert, n, ke, cc, fill, entrance } => {
                    if !outlet_invert.is_finite() {
                        *outlet_invert = d.invert - 0.01 * *length;
                    }
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Inlet Invert:", &mut d.invert);
                        let opts: Vec<(PipeShape, String)> = PipeShape::ALL.iter().map(|s| (*s, s.label().to_string())).collect();
                        if widgets::combo_field(ui, "shape", "Shape:", shape, &opts, 130.0) && *shape == PipeShape::Round {
                            *height = *width;
                        }
                    });
                    ui.horizontal(|ui| {
                        len(ui, "out", "Outlet Invert:", outlet_invert);
                        if inch(ui, "dia", if *shape == PipeShape::Round { "Diameter:" } else { "Width:" }, width) && *shape == PipeShape::Round {
                            *height = *width;
                        }
                    });
                    ui.horizontal(|ui| {
                        len(ui, "len", "Length:", length);
                        if *shape != PipeShape::Round {
                            inch(ui, "h", "Height:", height);
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut s = if *length > 0.0 { (d.invert - *outlet_invert) / *length } else { 0.0 };
                        if conv_field(ui, "slope", "Slope:", "ft/ft", &mut s, |x| x, |x| x, 4, 130.0) && s.is_finite() {
                            *outlet_invert = d.invert - s * *length;
                        }
                        inch(ui, "fill", "Inside Fill:", fill);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "n", "Manning's Number:", "", n, |x| x, |x| x, 3, 100.0);
                        if let Some(v) = manning_button(ui, "cn") {
                            *n = v;
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut opts: Vec<(String, String)> = ENTRANCES.iter().map(|(name, k)| (name.to_string(), format!("{name} (Ke={k})"))).collect();
                        opts.insert(0, (String::new(), "(custom)".into()));
                        if widgets::combo_field(ui, "ent", "Entrance:", entrance, &opts, 270.0) {
                            if let Some((_, k)) = ENTRANCES.iter().find(|(name, _)| name == entrance) {
                                *ke = *k;
                            }
                        }
                        conv_field(ui, "ke", "Ke:", "", ke, |x| x, |x| x.max(0.0), 3, 70.0);
                        conv_field(ui, "cc", "Contraction Coef.:", "", cc, |x| x, |x| x.clamp(0.1, 1.0), 3, 120.0);
                    });
                    let sec = Section::Pipe { shape: *shape, width: *width, height: *height, fill: *fill };
                    let p = sec.props(sec.full_depth());
                    ui.label(format!("Area= {:.2} sf   Perimeter= {:.2}'", p.area, p.perimeter));
                }
                DeviceKind::Orifice { shape, plane, cd, columns, rows, row_spacing, weir_at_low_head, grate } => {
                    len(ui, "inv", "Invert Elevation:", &mut d.invert);
                    ui.horizontal(|ui| {
                        if widgets::radio(ui, matches!(plane, Plane::Horizontal), "Horizontal").clicked() {
                            *plane = Plane::Horizontal;
                        }
                        if widgets::radio(ui, matches!(plane, Plane::Vertical), "Vertical").clicked() {
                            *plane = Plane::Vertical;
                        }
                        if widgets::radio(ui, matches!(plane, Plane::Angle(_)), "Custom angle").clicked() {
                            *plane = Plane::Angle(45.0);
                        }
                        if let Plane::Angle(a) = plane {
                            conv_field(ui, "ang", "Angle:", "deg", a, |x| x, |x| x.clamp(0.0, 90.0), 1, 90.0);
                        }
                    });
                    ui.horizontal(|ui| {
                        let circ = matches!(shape, OpeningShape::Circular { .. });
                        if widgets::radio(ui, circ, "Circular").clicked() && !circ {
                            *shape = OpeningShape::Circular { diameter: 0.5 };
                        }
                        if widgets::radio(ui, !circ, "Rectangular").clicked() && circ {
                            *shape = OpeningShape::Rectangular { width: 1.0, height: 0.5 };
                        }
                    });
                    ui.horizontal(|ui| match shape {
                        OpeningShape::Circular { diameter } => {
                            inch(ui, "dia", "Diameter:", diameter);
                        }
                        OpeningShape::Rectangular { width, height } => {
                            inch(ui, "w", "Width:", width);
                            inch(ui, "h", "Height:", height);
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut c = *columns as f64;
                        if conv_field(ui, "cols", "Columns:", "", &mut c, |x| x, |x| x.clamp(1.0, 1000.0), 0, 90.0) {
                            *columns = c.round().clamp(1.0, 1000.0) as u32;
                        }
                        let mut r = *rows as f64;
                        if conv_field(ui, "rows", "Rows:", "", &mut r, |x| x, |x| x.clamp(1.0, 1000.0), 0, 90.0) {
                            *rows = r.round().clamp(1.0, 1000.0) as u32;
                        }
                        if *rows > 1 && !matches!(plane, Plane::Horizontal) {
                            inch(ui, "rs", "Row Spacing:", row_spacing);
                        }
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "cd", "Discharge Coefficient:", "", cd, |x| x, |x| x.clamp(0.0, 1.0), 3, 140.0);
                        widgets::check(ui, weir_at_low_head, "Use weir flow at low head");
                    });
                    if matches!(plane, Plane::Horizontal) {
                        let mut has = grate.is_some();
                        if widgets::check(ui, &mut has, "Set grate dimensions (perimeter weir control)").changed() {
                            *grate = if has { Some((3.0, 3.0)) } else { None };
                        }
                        if let Some((w, l)) = grate {
                            ui.horizontal(|ui| {
                                inch(ui, "gw", "Grate Width:", w);
                                inch(ui, "gl", "Grate Length:", l);
                            });
                        }
                    }
                }
                DeviceKind::SharpCrestedWeir { length, end_contractions, crest_height, rise } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Crest Elevation:", &mut d.invert);
                        len(ui, "len", "Crest Length:", length);
                    });
                    ui.horizontal(|ui| {
                        let mut ec = *end_contractions as f64;
                        if conv_field(ui, "ec", "End Contractions:", "0-2", &mut ec, |x| x, |x| x.clamp(0.0, 2.0), 0, 130.0) {
                            *end_contractions = ec.round().clamp(0.0, 2.0) as u8;
                        }
                        len(ui, "ch", "Crest Height (above channel):", crest_height);
                        opt_field(ui, "rise", "Weir Rise:", u.len_label(), rise, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                    });
                }
                DeviceKind::BroadCrestedWeir { length, breadth, side_z, rise } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Crest Elevation:", &mut d.invert);
                        len(ui, "len", "Crest Length:", length);
                        len(ui, "br", "Crest Breadth:", breadth);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "z", "Side Slope Z:", "h:v", side_z, |x| x, |x| x.max(0.0), 2, 110.0);
                        opt_field(ui, "rise", "Weir Rise:", u.len_label(), rise, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                    });
                    let t = stormwaves_core::outlets::broad_crested_table(*breadth);
                    ui.label(RichText::new(format!("Head: {}", t.iter().map(|p| format!("{:.1}", p.0)).collect::<Vec<_>>().join(" "))).monospace());
                    ui.label(RichText::new(format!("Coef: {}", t.iter().map(|p| format!("{:.2}", p.1)).collect::<Vec<_>>().join(" "))).monospace());
                }
                DeviceKind::VeeTrapWeir { notch_angle, crest_length, cv, rise } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Invert Elevation:", &mut d.invert);
                        conv_field(ui, "ang", "Notch Angle:", "deg", notch_angle, |x| x, |x| x.clamp(0.0, 179.0), 1, 110.0);
                        len(ui, "len", "Crest Length (0 = vee):", crest_length);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "cv", "Vee Coefficient:", "", cv, |x| x, |x| x, 2, 110.0);
                        opt_field(ui, "rise", "Weir Rise:", u.len_label(), rise, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                    });
                }
                DeviceKind::CustomWeir { points, coefficient } => {
                    conv_field(ui, "c", "Weir Coefficient:", "", coefficient, |x| x, |x| x, 2, 120.0);
                    ui.label("Crest profile: offset and elevation of each point");
                    let mut del = None;
                    for (i, p) in points.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            widgets::num_box(ui, ui.id().with(("o", i)), &mut p.0, 2, 80.0, None);
                            widgets::num_box(ui, ui.id().with(("e", i)), &mut p.1, 2, 80.0, None);
                            if widgets::button(ui, "X", 22.0).clicked() {
                                del = Some(i);
                            }
                        });
                    }
                    if let Some(i) = del {
                        points.remove(i);
                    }
                    if widgets::button(ui, "Add Point", 80.0).clicked() {
                        let last = points.last().copied().unwrap_or((0.0, 0.0));
                        points.push((last.0 + 1.0, last.1));
                    }
                }
                DeviceKind::Exfiltration { mode, phase_in, max_elevation } => {
                    len(ui, "inv", "Exfiltration Invert:", &mut d.invert);
                    let per_hr = |v: f64| v * 12.0 * 3600.0;
                    let from_hr = |v: f64| v / 12.0 / 3600.0;
                    ui.horizontal(|ui| {
                        if widgets::radio(ui, matches!(mode, ExfilMode::ConstantFlow { .. }), "Constant flow").clicked() {
                            *mode = ExfilMode::ConstantFlow { flow: 0.1 };
                        }
                        if widgets::radio(ui, matches!(mode, ExfilMode::SurfaceVelocity { .. }), "Surface area").clicked() {
                            *mode = ExfilMode::SurfaceVelocity { velocity: from_hr(1.0) };
                        }
                        if widgets::radio(ui, matches!(mode, ExfilMode::WettedVelocity { .. }), "Wetted area").clicked() {
                            *mode = ExfilMode::WettedVelocity { velocity: from_hr(1.0) };
                        }
                        if widgets::radio(ui, matches!(mode, ExfilMode::Conductivity { .. }), "Conductivity").clicked() {
                            *mode = ExfilMode::Conductivity { ks: from_hr(1.0), groundwater: d.invert - 5.0, wetted: false };
                        }
                    });
                    ui.horizontal(|ui| match mode {
                        ExfilMode::ConstantFlow { flow } => {
                            conv_field(ui, "q", "Flow:", u.flow_label(), flow, |x| u.flow(x), |x| u.flow_in(x), 3, 120.0);
                        }
                        ExfilMode::SurfaceVelocity { velocity } | ExfilMode::WettedVelocity { velocity } => {
                            conv_field(ui, "v", "Exfiltration Rate:", "in/hr", velocity, per_hr, from_hr, 3, 140.0);
                        }
                        ExfilMode::Conductivity { ks, groundwater, wetted } => {
                            conv_field(ui, "ks", "Conductivity Ks:", "in/hr", ks, per_hr, from_hr, 3, 130.0);
                            len(ui, "gw", "Groundwater Elev:", groundwater);
                            widgets::check(ui, wetted, "Wetted area");
                        }
                    });
                    ui.horizontal(|ui| {
                        len(ui, "pi", "Phase-In Depth:", phase_in);
                        opt_field(ui, "max", "Maximum Elevation:", u.len_label(), max_elevation, |x| u.len(x), |x| u.len_in(x), 2, 140.0);
                    });
                }
                DeviceKind::Special { table, head_based, extrapolate } => {
                    len(ui, "inv", "Invert Elevation:", &mut d.invert);
                    ui.horizontal(|ui| {
                        if widgets::radio(ui, !*head_based, "Elevation vs. discharge").clicked() {
                            *head_based = false;
                        }
                        if widgets::radio(ui, *head_based, "Head vs. discharge").clicked() {
                            *head_based = true;
                        }
                        widgets::check(ui, extrapolate, "Extrapolate");
                    });
                    ui.label(RichText::new(format!("{:>12} {:>12}", if *head_based { "Head (ft)" } else { "Elev (ft)" }, "Q (cfs)")).monospace());
                    let mut del = None;
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for (i, p) in table.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                widgets::num_box(ui, ui.id().with(("x", i)), &mut p.0, 2, 90.0, None);
                                widgets::num_box(ui, ui.id().with(("q", i)), &mut p.1, 3, 90.0, None);
                                if widgets::button(ui, "X", 22.0).clicked() {
                                    del = Some(i);
                                }
                            });
                        }
                    });
                    if let Some(i) = del {
                        table.remove(i);
                    }
                    if widgets::button(ui, "Add Row", 80.0).clicked() {
                        let last = table.last().copied().unwrap_or((0.0, 0.0));
                        table.push((last.0 + 0.5, last.1));
                    }
                }
                DeviceKind::ConstantFlow { flow, phase_in } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Invert Elevation:", &mut d.invert);
                        conv_field(ui, "q", "Full Flow:", u.flow_label(), flow, |x| u.flow(x), |x| u.flow_in(x), 3, 120.0);
                        len(ui, "pi", "Phase-In Depth:", phase_in);
                    });
                }
                DeviceKind::Pump { rating, on_elevation, off_elevation, discharge_elevation, max_elevation, priority_group } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Intake Elevation:", &mut d.invert);
                        len(ui, "on", "On Elevation:", on_elevation);
                        len(ui, "off", "Off Elevation:", off_elevation);
                    });
                    ui.horizontal(|ui| {
                        len(ui, "de", "Discharge Elevation:", discharge_elevation);
                        super::opt_field(ui, "max", "Maximum Elevation:", u.len_label(), max_elevation, |x| u.len(x), |x| u.len_in(x), 2, 120.0);
                        let mut g = *priority_group as f64;
                        if conv_field(ui, "grp", "Priority Group:", "0 = none", &mut g, |x| x, |x| x.clamp(0.0, 99.0).round(), 0, 110.0) {
                            *priority_group = if g.is_finite() { g as u32 } else { 0 };
                        }
                    });
                    ui.label(RichText::new(format!("{:>12} {:>12}", "Lift (ft)", "Q (cfs)")).monospace());
                    let mut del = None;
                    for (i, p) in rating.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            widgets::num_box(ui, ui.id().with(("ph", i)), &mut p.0, 2, 90.0, None);
                            widgets::num_box(ui, ui.id().with(("pq", i)), &mut p.1, 3, 90.0, None);
                            if widgets::button(ui, "X", 22.0).clicked() {
                                del = Some(i);
                            }
                        });
                    }
                    if let Some(i) = del {
                        rating.remove(i);
                    }
                    if widgets::button(ui, "Add Row", 80.0).clicked() {
                        let last = rating.last().copied().unwrap_or((0.0, 0.0));
                        rating.push((last.0 + 5.0, last.1));
                    }
                }
                DeviceKind::Siphon { diameter, length, n, ke, outlet_elevation, prime_elevation, break_elevation } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Intake Elevation:", &mut d.invert);
                        len(ui, "out", "Outlet Elevation:", outlet_elevation);
                    });
                    ui.horizontal(|ui| {
                        inch(ui, "dia", "Diameter:", diameter);
                        len(ui, "len", "Length:", length);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "n", "Manning's n:", "", n, |x| x, |x| x, 3, 100.0);
                        conv_field(ui, "ke", "Ke:", "", ke, |x| x, |x| x.max(0.0), 3, 80.0);
                    });
                    ui.horizontal(|ui| {
                        len(ui, "prime", "Start (Prime) Elevation:", prime_elevation);
                        len(ui, "break", "Stop (Break) Elevation:", break_elevation);
                    });
                }
                DeviceKind::DamBreach { start_time, start_elevation, formation, final_bottom, final_width, side_z } => {
                    len(ui, "inv", "Dam Crest Elevation:", &mut d.invert);
                    ui.horizontal(|ui| {
                        opt_field(ui, "st", "Start Time:", "hours", start_time, |x| x, |x| x, 2, 120.0);
                        opt_field(ui, "se", "or Start Elevation:", u.len_label(), start_elevation, |x| u.len(x), |x| u.len_in(x), 2, 150.0);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "form", "Formation Time:", "hours", formation, |x| x, |x| x.max(0.0), 2, 120.0);
                        len(ui, "fb", "Final Bottom Elev:", final_bottom);
                    });
                    ui.horizontal(|ui| {
                        len(ui, "fw", "Final Bottom Width:", final_width);
                        conv_field(ui, "z", "Side Slope Z:", "h:v", side_z, |x| x, |x| x.max(0.0), 2, 110.0);
                    });
                }
                DeviceKind::DelayedGate { trigger_elevation, delay, opening, width, height, cd } => {
                    ui.horizontal(|ui| {
                        len(ui, "inv", "Gate Invert:", &mut d.invert);
                        len(ui, "trig", "Trigger Elevation:", trigger_elevation);
                    });
                    ui.horizontal(|ui| {
                        conv_field(ui, "delay", "Time Delay:", "hours", delay, |x| x, |x| x.max(0.0), 2, 110.0);
                        conv_field(ui, "open", "Opening Time:", "hours", opening, |x| x, |x| x.max(0.0), 2, 110.0);
                    });
                    ui.horizontal(|ui| {
                        inch(ui, "w", "Width:", width);
                        inch(ui, "h", "Height:", height);
                        conv_field(ui, "cd", "Coefficient:", "", cd, |x| x, |x| x.clamp(0.0, 1.0), 3, 100.0);
                    });
                }
                DeviceKind::Channel { section, n, slope } => {
                    len(ui, "inv", "Channel Invert:", &mut d.invert);
                    ui.horizontal(|ui| {
                        conv_field(ui, "n", "Manning's n:", "", n, |x| x, |x| x, 3, 100.0);
                        if let Some(v) = manning_button(ui, "chn") {
                            *n = v;
                        }
                        conv_field(ui, "s", "Slope:", "ft/ft", slope, |x| x, |x| x, 4, 110.0);
                    });
                    super::node::section_editor(ui, section, u, false);
                }
            });
            sketch(ui, &d.kind);
        });
        result = ok_cancel_apply(ui, false, "pond");
    });
    match result {
        Buttons::Ok | Buttons::Apply => Some(true),
        Buttons::Cancel => Some(false),
        Buttons::None => None,
    }
}

/// Live picture of the opening (water-blue on grey), drawn from the entered dimensions.
fn sketch(ui: &mut Ui, kind: &DeviceKind) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(170.0, 150.0), egui::Sense::hover());
    let p = ui.painter();
    let water = Color32::from_rgb(0, 236, 240);
    let ink = Stroke::new(1.5, t.text);
    match kind {
        DeviceKind::Culvert { shape, width, height, fill, .. } => {
            let sec = Section::Pipe { shape: *shape, width: *width, height: *height, fill: 0.0 };
            let s = (rect.width() - 20.0) / width.max(*height).max(0.01) as f32;
            let c = rect.center();
            let n = 48;
            let mut left = Vec::new();
            let mut right = Vec::new();
            for i in 0..=n {
                let z = *height * i as f64 / n as f64;
                let w = match shape {
                    PipeShape::Box => *width,
                    _ => {
                        let a = sec.props((z + 1e-6).min(*height)).area - sec.props((z - 1e-6).max(0.0)).area;
                        a / 2e-6
                    }
                };
                let y = c.y + (*height as f32 / 2.0 - z as f32) * s;
                left.push(pos2(c.x - w as f32 / 2.0 * s, y));
                right.push(pos2(c.x + w as f32 / 2.0 * s, y));
            }
            right.reverse();
            let pts: Vec<egui::Pos2> = left.into_iter().chain(right).collect();
            p.add(egui::Shape::convex_polygon(pts, water, ink));
            if *fill > 0.0 {
                let y = c.y + (*height as f32 / 2.0 - *fill as f32) * s;
                p.rect_filled(Rect::from_min_max(pos2(c.x - *width as f32 / 2.0 * s, y), pos2(c.x + *width as f32 / 2.0 * s, c.y + *height as f32 / 2.0 * s)), 0.0, Color32::from_rgb(130, 130, 130));
            }
        }
        DeviceKind::Orifice { shape, columns, rows, grate, .. } => {
            p.rect_filled(rect.shrink(4.0), 0.0, Color32::from_rgb(110, 110, 110));
            let (cols, rws) = ((*columns).clamp(1, 12) as f32, (*rows).clamp(1, 12) as f32);
            let cell = vec2((rect.width() - 16.0) / cols, (rect.height() - 16.0) / rws);
            for i in 0..cols as usize {
                for j in 0..rws as usize {
                    let cc = rect.min + vec2(8.0, 8.0) + vec2((i as f32 + 0.5) * cell.x, (j as f32 + 0.5) * cell.y);
                    match shape {
                        OpeningShape::Circular { .. } => {
                            p.circle_filled(cc, cell.x.min(cell.y) * 0.35, water);
                        }
                        OpeningShape::Rectangular { width, height } => {
                            let asp = (*width / height.max(1e-6)).clamp(0.2, 5.0) as f32;
                            let size = if asp >= 1.0 { vec2(cell.x * 0.7, cell.x * 0.7 / asp) } else { vec2(cell.y * 0.7 * asp, cell.y * 0.7) };
                            p.rect_filled(Rect::from_center_size(cc, size.min(cell * 0.8)), 0.0, water);
                        }
                    }
                }
            }
            if grate.is_some() {
                p.rect_stroke(rect.shrink(4.0), 0.0, Stroke::new(2.0, t.dark), egui::StrokeKind::Inside);
            }
        }
        DeviceKind::BroadCrestedWeir { .. } | DeviceKind::SharpCrestedWeir { .. } | DeviceKind::VeeTrapWeir { .. } | DeviceKind::CustomWeir { .. } => {
            let base = rect.max.y - 20.0;
            let pts: Vec<egui::Pos2> = match kind {
                DeviceKind::VeeTrapWeir { notch_angle, crest_length, .. } => {
                    let half = (notch_angle.to_radians() / 2.0).tan() as f32;
                    let cl = (*crest_length as f32).min(4.0) * 10.0;
                    let h = 70.0;
                    vec![pos2(rect.center().x - cl / 2.0 - h * half, base - h), pos2(rect.center().x - cl / 2.0, base), pos2(rect.center().x + cl / 2.0, base), pos2(rect.center().x + cl / 2.0 + h * half, base - h)]
                }
                _ => vec![pos2(rect.min.x + 25.0, base - 70.0), pos2(rect.min.x + 25.0, base), pos2(rect.max.x - 25.0, base), pos2(rect.max.x - 25.0, base - 70.0)],
            };
            p.add(egui::Shape::convex_polygon(pts.clone(), water, Stroke::NONE));
            p.add(egui::Shape::line(pts, ink));
        }
        _ => {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, kind.label(), egui::FontId::proportional(13.0), t.text_dim);
        }
    }
}
