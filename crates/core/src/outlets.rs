//! Pond outlet devices and their stage-discharge relationships.
//!
//! Each device has an invert and a routing: primary, secondary, tertiary, discarded, or another
//! device (a compound outlet such as orifices and a weir feeding a riser's culvert barrel).
//! Flows are in cfs, elevations in feet.

use crate::geometry::{G, MANNING_K, PipeShape, Section};
use crate::storage::PondStorage;
use serde::{Deserialize, Serialize};

/// Where a device's flow goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Routing {
    #[default]
    Primary,
    Secondary,
    Tertiary,
    /// Flow that leaves the model (exfiltration, evaporation).
    Discarded,
    /// Flow passes through another device (index into the pond's device list).
    Device(usize),
}

impl Routing {
    pub fn label(self) -> String {
        match self {
            Routing::Primary => "Primary".into(),
            Routing::Secondary => "Secondary".into(),
            Routing::Tertiary => "Tertiary".into(),
            Routing::Discarded => "Discarded".into(),
            Routing::Device(i) => format!("Device {}", i + 1),
        }
    }
}

/// Orifice opening shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OpeningShape {
    Circular { diameter: f64 },
    Rectangular { width: f64, height: f64 },
}

impl OpeningShape {
    pub fn area(&self) -> f64 {
        match self {
            OpeningShape::Circular { diameter } => std::f64::consts::PI * diameter * diameter / 4.0,
            OpeningShape::Rectangular { width, height } => width * height,
        }
    }
    pub fn perimeter(&self) -> f64 {
        match self {
            OpeningShape::Circular { diameter } => std::f64::consts::PI * diameter,
            OpeningShape::Rectangular { width, height } => 2.0 * (width + height),
        }
    }
    fn rise(&self) -> f64 {
        match self {
            OpeningShape::Circular { diameter } => *diameter,
            OpeningShape::Rectangular { height, .. } => *height,
        }
    }
    fn width_at(&self, z: f64) -> f64 {
        match self {
            OpeningShape::Circular { diameter } => {
                if z <= 0.0 || z >= *diameter {
                    0.0
                } else {
                    let u = 2.0 * z / diameter - 1.0;
                    diameter * (1.0 - u * u).sqrt()
                }
            }
            OpeningShape::Rectangular { width, height } => {
                if z <= 0.0 || z >= *height { 0.0 } else { *width }
            }
        }
    }
}

/// Orientation of an orifice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Plane {
    Horizontal,
    Vertical,
    /// Angle from horizontal, degrees (0 = horizontal, 90 = vertical).
    Angle(f64),
}

/// Exfiltration calculation basis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExfilMode {
    /// A fixed flow (cfs) whenever the water is above the invert.
    ConstantFlow { flow: f64 },
    /// Velocity (ft/s) over the water surface area.
    SurfaceVelocity { velocity: f64 },
    /// Velocity (ft/s) over the wetted area (bottom and sides).
    WettedVelocity { velocity: f64 },
    /// Darcy's law, V = Ks·(WSE − GW)/(e − GW), integrated over the surface (or wetted) area by
    /// elevation e, so lower parts of the pond seep faster than its upper sides.
    Conductivity { ks: f64, groundwater: f64, wetted: bool },
}

/// The kinds of outlet device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeviceKind {
    Culvert {
        shape: PipeShape,
        /// Diameter or span (ft).
        width: f64,
        /// Rise (ft); equal to the width for a round pipe.
        height: f64,
        length: f64,
        outlet_invert: f64,
        n: f64,
        ke: f64,
        cc: f64,
        /// Inside fill depth (ft).
        fill: f64,
        entrance: String,
    },
    Orifice {
        shape: OpeningShape,
        plane: Plane,
        cd: f64,
        columns: u32,
        rows: u32,
        /// Vertical centre-to-centre spacing of rows (ft), for vertical grates.
        row_spacing: f64,
        weir_at_low_head: bool,
        /// Overall grate dimensions (width, length) for perimeter control of horizontal grates.
        grate: Option<(f64, f64)>,
    },
    SharpCrestedWeir { length: f64, end_contractions: u8, crest_height: f64, rise: Option<f64> },
    BroadCrestedWeir { length: f64, breadth: f64, side_z: f64, rise: Option<f64> },
    VeeTrapWeir { notch_angle: f64, crest_length: f64, cv: f64, rise: Option<f64> },
    /// Arbitrary crest profile: (offset, elevation) points; weir flow over the profile.
    CustomWeir { points: Vec<(f64, f64)>, coefficient: f64 },
    Exfiltration { mode: ExfilMode, phase_in: f64, max_elevation: Option<f64> },
    /// User rating table; `head_based` means the first column is head above the invert.
    Special { table: Vec<(f64, f64)>, head_based: bool, extrapolate: bool },
    /// A float-controlled skimmer or flow regulator: constant flow once fully submerged.
    ConstantFlow { flow: f64, phase_in: f64 },
    /// Pump with a rating curve of flow (cfs) against total head (ft) and on/off elevations.
    /// It also stops above `max_elevation`. Pumps sharing a non-zero `priority_group` run one
    /// at a time: the one with the highest satisfied "on" elevation.
    Pump {
        rating: Vec<(f64, f64)>,
        on_elevation: f64,
        off_elevation: f64,
        discharge_elevation: f64,
        #[serde(default)]
        max_elevation: Option<f64>,
        #[serde(default)]
        priority_group: u32,
        /// Discharge pipe length (ft) and diameter (ft). When both are positive the friction
        /// head of the pipe (Hazen-Williams) is added to the lift the pump works against.
        #[serde(default)]
        discharge_length: f64,
        #[serde(default)]
        discharge_diameter: f64,
        /// Hazen-Williams roughness coefficient C of the discharge pipe.
        #[serde(default = "default_hazen_c")]
        hazen_c: f64,
    },
    /// Tube, siphon or float valve: full-pipe flow once primed at `prime_elevation`, until the
    /// water falls to `break_elevation`. The device invert is the intake.
    Siphon { diameter: f64, length: f64, n: f64, ke: f64, outlet_elevation: f64, prime_elevation: f64, break_elevation: f64 },
    /// Progressive dam breach: starts at a time or water level, then the breach bottom drops from
    /// the crest (device invert) to `final_bottom` and widens to `final_width` over `formation`.
    DamBreach { start_time: Option<f64>, start_elevation: Option<f64>, formation: f64, final_bottom: f64, final_width: f64, side_z: f64 },
    /// Automated gate (rectangular vertical opening at the invert) that starts opening `delay`
    /// hours after the water first reaches `trigger_elevation` and is fully open after `opening`.
    /// With `step` > 0 (hours) it opens in increments, once per step, instead of continuously.
    /// With a `vertical_range` (ft) the whole width x height inlet instead moves: its lip drops
    /// from invert + range to the invert as it opens. A `rotary` weir (batch-detention bucket)
    /// also turns from a horizontal to a vertical opening as it is lowered.
    DelayedGate {
        trigger_elevation: f64,
        delay: f64,
        opening: f64,
        width: f64,
        height: f64,
        cd: f64,
        #[serde(default)]
        step: f64,
        #[serde(default)]
        vertical_range: f64,
        #[serde(default)]
        rotary: bool,
    },
    /// The pond discharges straight into a channel: normal-depth flow at depth HW − invert.
    Channel { section: crate::geometry::Section, n: f64, slope: f64 },
}

impl DeviceKind {
    pub fn label(&self) -> &'static str {
        match self {
            DeviceKind::Culvert { .. } => "Culvert",
            DeviceKind::Orifice { plane: Plane::Horizontal, .. } => "Horiz. Orifice/Grate",
            DeviceKind::Orifice { .. } => "Vert. Orifice/Grate",
            DeviceKind::SharpCrestedWeir { .. } => "Sharp-Crested Rectangular Weir",
            DeviceKind::BroadCrestedWeir { .. } => "Broad-Crested Rectangular Weir",
            DeviceKind::VeeTrapWeir { crest_length, .. } if *crest_length <= 0.0 => "Vee Notch Weir",
            DeviceKind::VeeTrapWeir { .. } => "Trapezoidal Weir",
            DeviceKind::CustomWeir { .. } => "Custom Weir/Orifice",
            DeviceKind::Exfiltration { .. } => "Exfiltration",
            DeviceKind::Special { .. } => "Special & User-Defined",
            DeviceKind::ConstantFlow { .. } => "Constant Flow",
            DeviceKind::Pump { .. } => "Pump",
            DeviceKind::Siphon { .. } => "Tube/Siphon/Float Valve",
            DeviceKind::DamBreach { .. } => "Dam Breach",
            DeviceKind::DelayedGate { .. } => "Delayed Gate/Valve",
            DeviceKind::Channel { .. } => "Channel Outlet",
        }
    }

    /// Whether the device's output depends on its history (pumps cycle on and off).
    pub fn has_state(&self) -> bool {
        matches!(self, DeviceKind::Pump { .. } | DeviceKind::Siphon { .. } | DeviceKind::DamBreach { .. } | DeviceKind::DelayedGate { .. })
    }
}

/// One outlet device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutletDevice {
    pub description: String,
    pub invert: f64,
    pub routing: Routing,
    /// Number of identical devices (or a fraction to derate one).
    pub multiplier: f64,
    pub kind: DeviceKind,
    /// Sized automatically (round culverts only; see `resize`).
    #[serde(default)]
    pub auto_size: bool,
}

impl OutletDevice {
    pub fn new(description: &str, invert: f64, routing: Routing, kind: DeviceKind) -> Self {
        OutletDevice { description: description.into(), invert, routing, multiplier: 1.0, kind, auto_size: false }
    }
}

/// What controls a device's flow at a given head (for the device-flow analysis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    None,
    Weir,
    Orifice,
    Barrel,
    InletControl,
    Exfiltration,
    Table,
    Constant,
    Pump,
    /// Limited by a downstream device in a compound outlet.
    Passes,
}

impl Control {
    pub fn label(self) -> &'static str {
        match self {
            Control::None => "Controls 0.00 cfs",
            Control::Weir => "Weir Controls",
            Control::Orifice => "Orifice Controls",
            Control::Barrel => "Barrel Controls",
            Control::InletControl => "Inlet Controls",
            Control::Exfiltration => "Exfiltration Controls",
            Control::Table => "Custom Controls",
            Control::Constant => "Constant Controls",
            Control::Pump => "Pump Controls",
            Control::Passes => "Passes",
        }
    }
}

/// A device's potential flow at one headwater.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceFlow {
    pub flow: f64,
    pub control: Control,
    /// Representative velocity (ft/s), when meaningful.
    pub velocity: f64,
}

impl DeviceFlow {
    const ZERO: DeviceFlow = DeviceFlow { flow: 0.0, control: Control::None, velocity: 0.0 };
}

/// Per-device state for devices whose flow depends on history.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeviceState {
    /// Pump running / siphon primed.
    pub active: bool,
    /// When a breach began, or when a gate's trigger level was first reached (hours).
    pub since: Option<f64>,
}

/// Updates device states after a routing step settles at headwater `hw` and time `t`.
pub fn update_states(devices: &[OutletDevice], hw: f64, t: f64, states: &mut [DeviceState]) {
    for (d, st) in devices.iter().zip(states.iter_mut()) {
        match &d.kind {
            DeviceKind::Pump { on_elevation, off_elevation, max_elevation, .. } => {
                if max_elevation.is_some_and(|m| hw > m) {
                    st.active = false;
                } else if hw >= *on_elevation {
                    st.active = true;
                } else if hw <= *off_elevation {
                    st.active = false;
                }
            }
            DeviceKind::Siphon { prime_elevation, break_elevation, .. } => {
                if hw >= *prime_elevation {
                    st.active = true;
                } else if hw <= *break_elevation || hw <= d.invert {
                    st.active = false;
                }
            }
            DeviceKind::DamBreach { start_time, start_elevation, .. } => {
                let go = start_time.is_some_and(|s| t >= s) || start_elevation.is_some_and(|e| hw >= e);
                if go && st.since.is_none() {
                    st.since = Some(t);
                }
            }
            DeviceKind::DelayedGate { trigger_elevation, .. } if hw >= *trigger_elevation && st.since.is_none() => {
                st.since = Some(t);
            }
            _ => {}
        }
    }
    // Priority groups: of the running pumps in a group, keep only the one with the highest
    // "on" elevation.
    let pump = |d: &OutletDevice| match &d.kind {
        DeviceKind::Pump { on_elevation, priority_group, .. } if *priority_group > 0 => Some((*priority_group, *on_elevation)),
        _ => None,
    };
    let mut leader: Vec<(u32, usize, f64)> = Vec::new();
    for (i, (d, st)) in devices.iter().zip(states.iter()).enumerate() {
        if let (Some((g, on)), true) = (pump(d), st.active) {
            match leader.iter_mut().find(|l| l.0 == g) {
                Some(l) if on > l.2 => *l = (g, i, on),
                Some(_) => {}
                None => leader.push((g, i, on)),
            }
        }
    }
    for (i, (d, st)) in devices.iter().zip(states.iter_mut()).enumerate() {
        if let Some((g, _)) = pump(d)
            && leader.iter().any(|l| l.0 == g && l.1 != i)
        {
            st.active = false;
        }
    }
}

/// Inputs that vary through the routing.
#[derive(Debug, Clone, Copy)]
pub struct Conditions<'a> {
    pub hw: f64,
    /// Tailwater elevation for the device's outflow, if any.
    pub tw: Option<f64>,
    pub storage: &'a PondStorage,
    pub state: DeviceState,
    /// Current time (hours).
    pub time: f64,
}

/// Head-dependent broad-crested weir coefficients (US units), Brater & King Table 5-3.
const BC_HEADS: [f64; 18] = [0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0];
const BC_BREADTHS: [f64; 11] = [0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 10.0, 15.0];
#[rustfmt::skip]
#[allow(clippy::approx_constant)] // published coefficients, not π
const BC_COEF: [[f64; 11]; 18] = [
    [2.80, 2.75, 2.69, 2.62, 2.54, 2.48, 2.44, 2.38, 2.34, 2.49, 2.68],
    [2.92, 2.80, 2.72, 2.64, 2.61, 2.60, 2.58, 2.54, 2.50, 2.56, 2.70],
    [3.08, 2.89, 2.75, 2.64, 2.61, 2.60, 2.68, 2.69, 2.70, 2.70, 2.70],
    [3.30, 3.04, 2.85, 2.68, 2.60, 2.60, 2.67, 2.68, 2.68, 2.69, 2.64],
    [3.32, 3.14, 2.98, 2.75, 2.66, 2.64, 2.65, 2.67, 2.68, 2.68, 2.63],
    [3.32, 3.20, 3.08, 2.86, 2.70, 2.65, 2.64, 2.67, 2.66, 2.69, 2.64],
    [3.32, 3.26, 3.20, 2.92, 2.77, 2.68, 2.64, 2.65, 2.65, 2.67, 2.64],
    [3.32, 3.29, 3.28, 3.07, 2.89, 2.75, 2.68, 2.66, 2.65, 2.64, 2.63],
    [3.32, 3.32, 3.31, 3.07, 2.88, 2.74, 2.68, 2.66, 2.65, 2.64, 2.63],
    [3.32, 3.31, 3.30, 3.03, 2.85, 2.76, 2.72, 2.68, 2.65, 2.64, 2.63],
    [3.32, 3.32, 3.31, 3.28, 3.07, 2.89, 2.81, 2.72, 2.67, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.20, 3.05, 2.92, 2.73, 2.66, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.19, 2.97, 2.76, 2.68, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.07, 2.79, 2.70, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 2.88, 2.74, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.07, 2.79, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 2.88, 2.64, 2.63],
    [3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.32, 3.07, 2.64, 2.63],
];

fn lerp_index(xs: &[f64], x: f64) -> (usize, f64) {
    if x <= xs[0] {
        return (0, 0.0);
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return (last - 1, 1.0);
    }
    let i = xs.partition_point(|v| *v <= x).saturating_sub(1).min(last - 1);
    (i, (x - xs[i]) / (xs[i + 1] - xs[i]))
}

/// Broad-crested weir coefficient for a head and crest breadth (bilinear interpolation).
pub fn broad_crested_coefficient(head: f64, breadth: f64) -> f64 {
    let (hi, hf) = lerp_index(&BC_HEADS, head);
    let (bi, bf) = lerp_index(&BC_BREADTHS, breadth);
    let at = |h: usize, b: usize| BC_COEF[h][b];
    let low = at(hi, bi) + (at(hi, bi + 1) - at(hi, bi)) * bf;
    let high = at(hi + 1, bi) + (at(hi + 1, bi + 1) - at(hi + 1, bi)) * bf;
    low + (high - low) * hf
}

/// Coefficient table for reporting a broad-crested weir: heads up to the first constant run.
pub fn broad_crested_table(breadth: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for h in BC_HEADS {
        let c = (broad_crested_coefficient(h, breadth) * 100.0).round() / 100.0;
        out.push((h, c));
        if out.len() > 2 && out.iter().rev().take(2).all(|(_, v)| (*v - c).abs() < 1e-9) && h >= 4.0 {
            out.pop();
            break;
        }
    }
    out
}

/// Villemonte submergence factor for weirs: (1 − (Ht/H)^n)^0.385.
fn submergence(head: f64, tail_head: f64, exponent: f64) -> f64 {
    if tail_head <= 0.0 || head <= 0.0 {
        return 1.0;
    }
    if tail_head >= head {
        return 0.0;
    }
    (1.0 - (tail_head / head).powf(exponent)).max(0.0).powf(0.385)
}

/// Discharge through an opening by integrating strips: q = Cd·w(z)·√(2g(H−z)).
fn strip_orifice(shape: &OpeningShape, cd: f64, head: f64) -> f64 {
    let rise = shape.rise();
    let top = head.min(rise);
    if top <= 0.0 {
        return 0.0;
    }
    let n = 64;
    let dz = top / n as f64;
    (0..n)
        .map(|i| {
            let z = (i as f64 + 0.5) * dz;
            cd * shape.width_at(z) * (2.0 * G * (head - z).max(0.0)).sqrt() * dz
        })
        .sum()
}

/// A vertical opening: centroid formula when submerged, strip integration (weir-like) below,
/// blended so the curve is continuous at the crown.
fn vertical_opening(shape: &OpeningShape, cd: f64, head: f64) -> f64 {
    let rise = shape.rise();
    if head <= 0.0 || rise <= 0.0 {
        return 0.0;
    }
    let area = shape.area();
    if head >= rise {
        return cd * area * (2.0 * G * (head - rise / 2.0)).sqrt();
    }
    let at_crown = cd * area * (2.0 * G * (rise / 2.0)).sqrt();
    let strip_crown = strip_orifice(shape, cd, rise);
    let scale = if strip_crown > 0.0 { at_crown / strip_crown } else { 1.0 };
    strip_orifice(shape, cd, head) * scale
}

/// Culvert discharge (one barrel) following the six flow types of Norman et al.: the lesser of
/// barrel (outlet) control and inlet (orifice) control.
#[allow(clippy::too_many_arguments)]
pub fn culvert_flow(
    shape: PipeShape,
    width: f64,
    height: f64,
    fill: f64,
    length: f64,
    inlet_invert: f64,
    outlet_invert: f64,
    n: f64,
    ke: f64,
    cc: f64,
    hw: f64,
    tw: Option<f64>,
) -> DeviceFlow {
    let head = hw - inlet_invert;
    if head <= 0.0 || width <= 0.0 || height <= 0.0 {
        return DeviceFlow::ZERO;
    }
    if let Some(t) = tw
        && t >= hw
    {
        return DeviceFlow::ZERO;
    }
    let fill = fill.clamp(0.0, height * 0.99);
    let section = Section::Pipe { shape, width, height, fill };
    let rise = height - fill;
    let head_in = hw - (inlet_invert + fill);
    // Barrel (types 1a/1b/2a/2b): depth ≈ 3/4 H, capped at the rise; tailwater may raise it.
    let d_est = (0.75 * head_in).min(rise).max(0.0);
    let mut props = section.props(d_est);
    if shape == PipeShape::Box && d_est >= rise {
        // The roof joins the wetted perimeter gradually as the barrel goes from just full
        // (H = 4/3 rise) to pressure flow, avoiding a jump in the rating curve.
        let open = section.props(rise * (1.0 - 1e-9));
        let blend = ((0.75 * head_in - rise) / (0.25 * rise)).clamp(0.0, 1.0);
        props.perimeter = open.perimeter + width * blend;
    }
    let mut depth = d_est;
    if let Some(t) = tw {
        let tw_depth = t - (outlet_invert + fill);
        if tw_depth > depth {
            depth = tw_depth;
        }
    }
    let fall = inlet_invert - outlet_invert;
    let r = props.hydraulic_radius();
    let barrel = if props.area > 0.0 && r > 0.0 {
        let num = head_in - depth + fall;
        let friction = 2.0 * G * n * n * length.max(0.0) / (MANNING_K * MANNING_K * r.powf(4.0 / 3.0));
        let den = (ke + 1.0) + friction;
        if num > 0.0 && den > 0.0 { props.area * (2.0 * G * num / den).sqrt() } else { 0.0 }
    } else {
        0.0
    };
    // Inlet control (types 1c/2c): orifice with Cd = Cc/(1 + Ke).
    let cd = if ke > -1.0 { cc / (1.0 + ke) } else { cc };
    let inlet = match shape {
        PipeShape::Round if fill <= 0.0 => vertical_opening(&OpeningShape::Circular { diameter: width }, cd, head_in),
        PipeShape::Box => vertical_opening(&OpeningShape::Rectangular { width, height: rise }, cd, head_in),
        _ => {
            // Integrate over the barrel shape.
            let full_area = section.props(rise).area;
            if head_in >= rise {
                let centroid = section_centroid(&section, rise);
                cd * full_area * (2.0 * G * (head_in - centroid)).max(0.0).sqrt()
            } else {
                let at_crown = cd * full_area * (2.0 * G * (rise - section_centroid(&section, rise))).max(0.0).sqrt();
                let strip = |h: f64| section_strip(&section, cd, h, rise);
                let s = strip(rise);
                if s > 0.0 { strip(head_in) * at_crown / s } else { 0.0 }
            }
        }
    };
    let (flow, control) = if inlet <= barrel { (inlet, Control::InletControl) } else { (barrel, Control::Barrel) };
    let area = section.props(rise).area;
    let velocity = match control {
        Control::Barrel if props.area > 0.0 => flow / props.area,
        _ if area > 0.0 => flow / area,
        _ => 0.0,
    };
    // A culvert under inlet control that is fully submerged behaves as an orifice.
    let control = if control == Control::InletControl && head_in >= rise { Control::Orifice } else { control };
    DeviceFlow { flow, control, velocity }
}

fn section_centroid(section: &Section, rise: f64) -> f64 {
    let n = 100;
    let (mut a, mut m) = (0.0, 0.0);
    let mut last = 0.0;
    for i in 1..=n {
        let z = rise * i as f64 / n as f64;
        let area = section.props(z).area;
        let da = area - last;
        a += da;
        m += da * (z - rise / (2.0 * n as f64));
        last = area;
    }
    if a > 0.0 { m / a } else { rise / 2.0 }
}

fn section_strip(section: &Section, cd: f64, head: f64, rise: f64) -> f64 {
    let top = head.min(rise);
    let n = 64;
    let dz = top / n as f64;
    let mut q = 0.0;
    for i in 0..n {
        let z0 = i as f64 * dz;
        let da = section.props(z0 + dz).area - section.props(z0).area;
        q += cd * da * (2.0 * G * (head - z0 - dz / 2.0).max(0.0)).sqrt();
    }
    q
}

fn default_hazen_c() -> f64 {
    120.0
}

/// Hazen-Williams friction head (ft) in a pipe: `hf = 4.73 L Q^1.852 / (C^1.852 D^4.87)` with
/// Q in cfs and L, D in feet (standard US form of the public Hazen-Williams equation).
pub fn hazen_williams_head(q: f64, length: f64, diameter: f64, c: f64) -> f64 {
    if q <= 0.0 || length <= 0.0 || diameter <= 0.0 || c <= 0.0 {
        return 0.0;
    }
    4.73 * length * q.powf(1.852) / (c.powf(1.852) * diameter.powf(4.87))
}

/// Pump flow at a static `lift`: the rating curve (sorted by head) read at the lift plus the
/// friction head of the discharge pipe, found by bisection because the friction depends on Q.
fn pump_flow(sorted: &[(f64, f64)], lift: f64, length: f64, diameter: f64, c: f64) -> f64 {
    let q0 = table_lookup(sorted, lift, false);
    if length <= 0.0 || diameter <= 0.0 || q0 <= 0.0 {
        return q0;
    }
    // f(q) = curve(lift + hf(q)) - q falls with q; the root lies in [0, q0].
    let f = |q: f64| table_lookup(sorted, lift + hazen_williams_head(q, length, diameter, c), false) - q;
    let (mut lo, mut hi) = (0.0, q0);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Linear interpolation in a (x, y) table, optionally extrapolating past the last pair.
fn table_lookup(table: &[(f64, f64)], x: f64, extrapolate: bool) -> f64 {
    if table.is_empty() {
        return 0.0;
    }
    if x <= table[0].0 {
        return if x < table[0].0 { 0.0 } else { table[0].1 };
    }
    let last = table.len() - 1;
    if x >= table[last].0 {
        if extrapolate && last > 0 {
            let (a, b) = (table[last - 1], table[last]);
            if b.0 > a.0 {
                return (b.1 + (b.1 - a.1) * (x - b.0) / (b.0 - a.0)).max(0.0);
            }
        }
        return table[last].1;
    }
    let i = table.partition_point(|p| p.0 <= x).saturating_sub(1).min(last - 1);
    let (a, b) = (table[i], table[i + 1]);
    if b.0 > a.0 { a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0) } else { a.1 }
}

impl OutletDevice {
    /// Potential flow of this device by itself (before any compound-outlet limits).
    pub fn potential(&self, c: &Conditions) -> DeviceFlow {
        let mult = self.multiplier.max(0.0);
        let mut f = self.raw_potential(c);
        f.flow *= mult;
        if !f.flow.is_finite() || f.flow < 0.0 {
            f.flow = 0.0;
        }
        f
    }

    fn raw_potential(&self, c: &Conditions) -> DeviceFlow {
        let head = c.hw - self.invert;
        let tail_head = c.tw.map(|t| t - self.invert).unwrap_or(0.0);
        match &self.kind {
            DeviceKind::Culvert { shape, width, height, length, outlet_invert, n, ke, cc, fill, .. } => {
                culvert_flow(*shape, *width, *height, *fill, *length, self.invert, *outlet_invert, *n, *ke, *cc, c.hw, c.tw)
            }
            DeviceKind::Orifice { shape, plane, cd, columns, rows, row_spacing, weir_at_low_head, grate } => {
                orifice_flow(shape, *plane, *cd, *columns, *rows, *row_spacing, *weir_at_low_head, *grate, head, tail_head)
            }
            DeviceKind::SharpCrestedWeir { length, end_contractions, crest_height, rise } => {
                if head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let c_w = if *crest_height > 0.0 { 3.27 + 0.40 * head / crest_height } else { 3.27 };
                let weir = |h: f64| c_w * (length - 0.1 * *end_contractions as f64 * h).max(0.0) * h.powf(1.5);
                let q = weir_with_rise(weir, head, *rise, *length) * submergence(head, tail_head, 1.5);
                weir_result(q, head, *length)
            }
            DeviceKind::BroadCrestedWeir { length, breadth, side_z, rise } => {
                if head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let weir = |h: f64| broad_crested_coefficient(h, *breadth) * (length + side_z * h) * h.powf(1.5);
                let q = weir_with_rise(weir, head, *rise, *length) * submergence(head, tail_head, 1.5);
                weir_result(q, head, length + side_z * head)
            }
            DeviceKind::VeeTrapWeir { notch_angle, crest_length, cv, rise } => {
                if head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let tan = (notch_angle.to_radians() / 2.0).tan().max(0.0);
                let weir = |h: f64| 3.27 * crest_length * h.powf(1.5) + cv * tan * h.powf(2.5);
                let top = crest_length + 2.0 * tan * head;
                let q = weir_with_rise(weir, head, *rise, top) * submergence(head, tail_head, 2.5);
                weir_result(q, head, top)
            }
            DeviceKind::CustomWeir { points, coefficient } => {
                let q = custom_weir(points, *coefficient, c.hw) * submergence(head.max(0.0), tail_head, 1.5);
                DeviceFlow { flow: q, control: if q > 0.0 { Control::Weir } else { Control::None }, velocity: 0.0 }
            }
            DeviceKind::Exfiltration { mode, phase_in, max_elevation } => exfiltration(*mode, *phase_in, *max_elevation, self.invert, c),
            DeviceKind::Special { table, head_based, extrapolate } => {
                let x = if *head_based { head } else { c.hw };
                let q = if head <= 0.0 && *head_based { 0.0 } else { table_lookup(table, x, *extrapolate) };
                DeviceFlow { flow: q, control: if q > 0.0 { Control::Table } else { Control::None }, velocity: 0.0 }
            }
            DeviceKind::ConstantFlow { flow, phase_in } => {
                if head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let f = if *phase_in > 0.0 { (head / phase_in).min(1.0) } else { 1.0 };
                DeviceFlow { flow: flow * f, control: Control::Constant, velocity: 0.0 }
            }
            DeviceKind::Siphon { diameter, length, n, ke, outlet_elevation, .. } => {
                if !c.state.active || head <= 0.0 || *diameter <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let down = outlet_elevation.max(c.tw.unwrap_or(f64::NEG_INFINITY));
                let dh = c.hw - down;
                if dh <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let area = std::f64::consts::PI * diameter * diameter / 4.0;
                let r = diameter / 4.0;
                let friction = 2.0 * G * n * n * length.max(0.0) / (MANNING_K * MANNING_K * r.powf(4.0 / 3.0));
                let v = (2.0 * G * dh / (1.0 + ke.max(0.0) + friction)).sqrt();
                DeviceFlow { flow: area * v, control: Control::Barrel, velocity: v }
            }
            DeviceKind::DamBreach { formation, final_bottom, final_width, side_z, .. } => {
                let Some(start) = c.state.since else { return DeviceFlow::ZERO };
                let f = if *formation > 0.0 { ((c.time - start) / formation).clamp(0.0, 1.0) } else { 1.0 };
                let bottom = self.invert + (final_bottom - self.invert) * f;
                let width = final_width.max(0.0) * f;
                let h = c.hw - bottom;
                if h <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                // Broad-crested trapezoidal breach (USBR): Q = 3.1 b H^1.5 + 2.45 z H^2.5.
                let q = (3.1 * width * h.powf(1.5) + 2.45 * side_z.max(0.0) * h.powf(2.5) * f) * submergence(h, c.tw.map(|t| t - bottom).unwrap_or(0.0), 1.5);
                weir_result(q, h, width + side_z * h)
            }
            DeviceKind::DelayedGate { delay, opening, width, height, cd, step, vertical_range, rotary, .. } => {
                // Fraction open: 0 until triggered and the delay has passed, then rising over the opening time.
                let f = match c.state.since {
                    None => 0.0,
                    Some(start) => {
                        let elapsed = c.time - start - delay;
                        if *opening > 0.0 {
                            // Stepped opening: the gate moves at the start of each step.
                            let e = if *step > 0.0 && elapsed >= 0.0 { ((elapsed / step).floor() + 1.0) * step } else { elapsed };
                            (e / opening).clamp(0.0, 1.0)
                        } else if elapsed >= 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                if *vertical_range > 0.0 || *rotary {
                    // A moving inlet of full size whose lip drops from invert + range (closed
                    // position) to the invert as it opens; water above the lip spills in at any
                    // time. A rotary weir (bucket) also turns from facing up to facing sideways.
                    let lip = self.invert + vertical_range.max(0.0) * (1.0 - f);
                    let plane = if *rotary { Plane::Angle(90.0 * f) } else { Plane::Vertical };
                    let tail = c.tw.map_or(0.0, |t| t - lip);
                    return orifice_flow(&OpeningShape::Rectangular { width: *width, height: *height }, plane, *cd, 1, 1, 0.0, true, None, c.hw - lip, tail);
                }
                if f <= 0.0 || head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let shape = OpeningShape::Rectangular { width: *width, height: height * f };
                let eff = if tail_head > 0.0 { (head - tail_head).max(0.0) + (height * f) / 2.0 } else { head };
                let q = vertical_opening(&shape, *cd, eff.min(head));
                DeviceFlow { flow: q, control: if head >= height * f { Control::Orifice } else { Control::Weir }, velocity: q / (width * height * f).max(1e-9) }
            }
            DeviceKind::Channel { section, n, slope } => {
                if head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let depth = if tail_head > head { 0.0 } else { head };
                let q = section.manning_q(depth, *n, *slope);
                let a = section.props(depth).area;
                DeviceFlow { flow: q, control: Control::Barrel, velocity: if a > 0.0 { q / a } else { 0.0 } }
            }
            DeviceKind::Pump { rating, discharge_elevation, discharge_length, discharge_diameter, hazen_c, .. } => {
                if !c.state.active || head <= 0.0 {
                    return DeviceFlow::ZERO;
                }
                let lift = (discharge_elevation.max(c.tw.unwrap_or(f64::NEG_INFINITY)) - c.hw).max(0.0);
                // Rating table is (head, flow): flow falls as the lift rises.
                let mut sorted = rating.clone();
                sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
                let q = pump_flow(&sorted, lift, *discharge_length, *discharge_diameter, *hazen_c);
                DeviceFlow { flow: q, control: Control::Pump, velocity: 0.0 }
            }
        }
    }
}

fn weir_result(q: f64, head: f64, width: f64) -> DeviceFlow {
    let v = if head > 0.0 && width > 0.0 { q / (head * width) } else { 0.0 };
    DeviceFlow { flow: q, control: if q > 0.0 { Control::Weir } else { Control::None }, velocity: v }
}

/// Weir flow; above the rise (a closed top) the opening switches to orifice flow.
fn weir_with_rise(weir: impl Fn(f64) -> f64, head: f64, rise: Option<f64>, _width: f64) -> f64 {
    match rise {
        Some(r) if r > 0.0 && head > r => {
            // Continuous transition: orifice flow from the area under the rise, scaled to match.
            let q_r = weir(r);
            q_r * ((head - r / 2.0) / (r / 2.0)).sqrt()
        }
        _ => weir(head),
    }
}

/// Weir flow over an arbitrary crest profile: Q = C ∫ (3/2)·w(z)·√(H−z) dz.
fn custom_weir(points: &[(f64, f64)], coef: f64, hw: f64) -> f64 {
    let mut q = 0.0;
    for w in points.windows(2) {
        let ((x0, z0), (x1, z1)) = (w[0], w[1]);
        let dx = (x1 - x0).abs();
        if dx <= 0.0 {
            continue;
        }
        // Integrate across the segment in slices.
        let n = 20;
        for i in 0..n {
            let f = (i as f64 + 0.5) / n as f64;
            let z = z0 + (z1 - z0) * f;
            let h = hw - z;
            if h > 0.0 {
                q += coef * h.powf(1.5) * dx / n as f64;
            }
        }
    }
    q
}

#[allow(clippy::too_many_arguments)]
fn orifice_flow(
    shape: &OpeningShape,
    plane: Plane,
    cd: f64,
    columns: u32,
    rows: u32,
    row_spacing: f64,
    weir_at_low_head: bool,
    grate: Option<(f64, f64)>,
    head: f64,
    tail_head: f64,
) -> DeviceFlow {
    if head <= 0.0 {
        return DeviceFlow::ZERO;
    }
    let count = (columns.max(1) * rows.max(1)) as f64;
    let area = shape.area();
    let angle = match plane {
        Plane::Horizontal => 0.0,
        Plane::Vertical => 90.0,
        Plane::Angle(a) => a.clamp(0.0, 90.0),
    };
    let (q, control) = if angle < 1e-6 {
        // Horizontal: orifice on the full head (net of tailwater), weir over the perimeter at low head.
        let eff_head = (head - tail_head.max(0.0)).max(0.0);
        let orifice = cd * area * count * (2.0 * G * eff_head).sqrt();
        let perimeter = match grate {
            Some((w, l)) if w > 0.0 && l > 0.0 => 2.0 * (w + l),
            _ => shape.perimeter() * count,
        };
        let weir = 3.27 * perimeter * head.powf(1.5) * submergence(head, tail_head, 1.5);
        if weir_at_low_head && weir < orifice { (weir, Control::Weir) } else { (orifice, Control::Orifice) }
    } else {
        // Vertical (or inclined): each row has its own head.
        let rise = shape.rise() * angle.to_radians().sin();
        let mut total = 0.0;
        for r in 0..rows.max(1) {
            let lift = r as f64 * row_spacing.max(0.0);
            let h = head - lift;
            if h <= 0.0 {
                continue;
            }
            let q = if weir_at_low_head {
                vertical_opening(shape, cd, h * if rise > 0.0 { shape.rise() / rise } else { 1.0 })
            } else {
                cd * area * (2.0 * G * (h - rise / 2.0).max(0.0)).sqrt()
            };
            let net = if tail_head > lift {
                // Submerged downstream: drive by the head difference.
                let dh = (head - tail_head).max(0.0);
                cd * area * (2.0 * G * dh).sqrt()
            } else {
                q
            };
            total += net.min(q.max(net)) * columns.max(1) as f64;
        }
        let control = if head >= rise { Control::Orifice } else { Control::Weir };
        (total, control)
    };
    let v = if area > 0.0 && count > 0.0 { q / (area * count) } else { 0.0 };
    DeviceFlow { flow: q, control, velocity: v }
}

/// Area weighted by the Darcy gradient, ∫ (HW − GW) / (e − GW) dA(e): each part of the pond
/// floor or sides seeps under the full head to groundwater across its own distance above
/// groundwater. Surface area counts the whole footprint from the storage bottom; wetted area
/// counts from the device invert. Areas at or below groundwater use a gradient of 1.
fn darcy_area(c: &Conditions, invert: f64, top: f64, groundwater: f64, wetted: bool) -> f64 {
    let area = |e: f64| if wetted { c.storage.wetted_area(e) } else { c.storage.surface_area(e) }.unwrap_or(0.0);
    let lo = if wetted { invert } else { c.storage.bottom().unwrap_or(invert) };
    if top <= lo {
        return 0.0;
    }
    let head = c.hw - groundwater;
    let gradient = |e: f64| {
        let dist = e - groundwater;
        if dist > 0.0 && head > 0.0 { (head / dist).max(0.0) } else { 1.0 }
    };
    // The footprint at the bottom (surface mode), then each band of side area at its height.
    let mut total = if wetted { 0.0 } else { area(lo) * gradient(lo) };
    const STEPS: usize = 32;
    let h = (top - lo) / STEPS as f64;
    let mut below = area(lo);
    for i in 0..STEPS {
        let e1 = lo + (i + 1) as f64 * h;
        let above = area(e1);
        total += (above - below).max(0.0) * gradient(e1 - 0.5 * h);
        below = above;
    }
    total
}

fn exfiltration(mode: ExfilMode, phase_in: f64, max_elevation: Option<f64>, invert: f64, c: &Conditions) -> DeviceFlow {
    let head = c.hw - invert;
    if head <= 0.0 {
        return DeviceFlow::ZERO;
    }
    let phase = if phase_in > 0.0 { (head / phase_in).min(1.0) } else { 1.0 };
    let top = max_elevation.map(|m| c.hw.min(m)).unwrap_or(c.hw);
    let area_between = |wetted: bool| -> f64 {
        let f = |e: f64| if wetted { c.storage.wetted_area(e) } else { c.storage.surface_area(e) };
        let above = f(top).unwrap_or(0.0);
        let below = if wetted { f(invert).unwrap_or(0.0) } else { 0.0 };
        (above - below).max(0.0)
    };
    let flow = match mode {
        ExfilMode::ConstantFlow { flow } => flow,
        ExfilMode::SurfaceVelocity { velocity } => velocity * area_between(false),
        ExfilMode::WettedVelocity { velocity } => velocity * area_between(true),
        ExfilMode::Conductivity { ks, groundwater, wetted } => ks * darcy_area(c, invert, top, groundwater, wetted),
    };
    DeviceFlow { flow: flow.max(0.0) * phase, control: Control::Exfiltration, velocity: 0.0 }
}

/// Flow of every device in a pond at one condition, with compound-outlet limits applied, and
/// the totals by outflow.
#[derive(Debug, Clone, Default)]
pub struct OutletFlows {
    /// Actual flow through each device.
    pub devices: Vec<f64>,
    /// Potential (unrestricted) flow of each device.
    pub potential: Vec<f64>,
    pub controls: Vec<Option<Control>>,
    pub velocities: Vec<f64>,
    pub primary: f64,
    pub secondary: f64,
    pub tertiary: f64,
    pub discarded: f64,
}

impl OutletFlows {
    pub fn total(&self) -> f64 {
        self.primary + self.secondary + self.tertiary + self.discarded
    }
}

/// Final destination of a device (following device-to-device routing), or `None` on a cycle.
pub fn final_routing(devices: &[OutletDevice], mut i: usize) -> Option<Routing> {
    for _ in 0..=devices.len() {
        match devices.get(i)?.routing {
            Routing::Device(j) => i = j,
            other => return Some(other),
        }
    }
    None
}

/// Evaluates all devices. `tw_for` gives the tailwater for each final outflow.
pub fn evaluate(devices: &[OutletDevice], hw: f64, tw_for: &dyn Fn(Routing) -> Option<f64>, storage: &PondStorage, states: &[DeviceState], time: f64) -> OutletFlows {
    let n = devices.len();
    let mut out = OutletFlows {
        devices: vec![0.0; n],
        potential: vec![0.0; n],
        controls: vec![None; n],
        velocities: vec![0.0; n],
        ..Default::default()
    };
    let feeds = |k: usize| devices.iter().enumerate().filter(move |(_, d)| d.routing == Routing::Device(k)).map(|(j, _)| j);
    // Potentials.
    for (i, d) in devices.iter().enumerate() {
        let route = final_routing(devices, i).unwrap_or(Routing::Discarded);
        let cond = Conditions { hw, tw: tw_for(route), storage, state: states.get(i).copied().unwrap_or_default(), time };
        let f = d.potential(&cond);
        out.potential[i] = f.flow;
        out.controls[i] = Some(f.control);
        out.velocities[i] = f.velocity;
    }
    // Resolve compound outlets bottom-up: a device fed by others passes at most its potential.
    fn resolve(k: usize, devices: &[OutletDevice], out: &mut OutletFlows, depth: usize, feeds: &dyn Fn(usize) -> Vec<usize>) -> f64 {
        if depth > devices.len() {
            return 0.0;
        }
        let inputs = feeds(k);
        if inputs.is_empty() {
            out.devices[k] = out.potential[k];
            return out.devices[k];
        }
        let supply: f64 = inputs.iter().map(|&j| resolve(j, devices, out, depth + 1, feeds)).sum();
        let pass = supply.min(out.potential[k]);
        if supply > pass && supply > 0.0 {
            // Downstream device limits: scale the feeders back proportionally.
            let f = pass / supply;
            for &j in &inputs {
                scale_tree(j, f, devices, out, 0);
            }
        } else if pass < out.potential[k] {
            out.controls[k] = Some(Control::Passes);
        }
        out.devices[k] = pass;
        pass
    }
    fn scale_tree(j: usize, f: f64, devices: &[OutletDevice], out: &mut OutletFlows, depth: usize) {
        if depth > devices.len() {
            return;
        }
        out.devices[j] *= f;
        for (i, d) in devices.iter().enumerate() {
            if d.routing == Routing::Device(j) {
                scale_tree(i, f, devices, out, depth + 1);
            }
        }
    }
    let feeds_vec = |k: usize| feeds(k).collect::<Vec<_>>();
    for i in 0..n {
        if !matches!(devices[i].routing, Routing::Device(_)) {
            let q = resolve(i, devices, &mut out, 0, &feeds_vec);
            match devices[i].routing {
                Routing::Primary => out.primary += q,
                Routing::Secondary => out.secondary += q,
                Routing::Tertiary => out.tertiary += q,
                Routing::Discarded => out.discarded += q,
                Routing::Device(_) => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::PondStorage;

    fn pump(on: f64, off: f64, max: Option<f64>, group: u32) -> OutletDevice {
        let kind = DeviceKind::Pump { rating: vec![(0.0, 1.0), (20.0, 0.5)], on_elevation: on, off_elevation: off, discharge_elevation: 110.0, max_elevation: max, priority_group: group, discharge_length: 0.0, discharge_diameter: 0.0, hazen_c: 120.0 };
        OutletDevice::new("Pump", 99.0, Routing::Primary, kind)
    }

    #[test]
    fn pump_discharge_pipe_friction_reduces_flow() {
        let curve = [(0.0, 1.0), (20.0, 0.5)];
        let free = pump_flow(&curve, 10.0, 0.0, 0.0, 120.0);
        assert!((free - 0.75).abs() < 1e-9);
        let piped = pump_flow(&curve, 10.0, 1000.0, 0.5, 120.0);
        assert!(piped < free && piped > 0.5, "{piped}");
        let hf = hazen_williams_head(piped, 1000.0, 0.5, 120.0);
        assert!((table_lookup(&curve, 10.0 + hf, false) - piped).abs() < 1e-6);
    }

    #[test]
    fn old_pump_json_gets_default_discharge_pipe() {
        let k: DeviceKind = serde_json::from_str(r#"{"Pump":{"rating":[[0.0,1.0]],"on_elevation":1.0,"off_elevation":0.5,"discharge_elevation":5.0}}"#).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(k, DeviceKind::Pump { hazen_c, discharge_length, .. } if hazen_c == 120.0 && discharge_length == 0.0));
    }

    #[test]
    fn pump_max_elevation_and_priority_group() {
        let devices = [pump(101.0, 100.0, None, 1), pump(102.0, 100.5, Some(104.0), 1), pump(101.5, 100.0, None, 0)];
        let mut st = [DeviceState::default(); 3];
        let active = |st: &[DeviceState; 3]| st.map(|s| s.active);
        update_states(&devices, 101.2, 0.0, &mut st);
        assert_eq!(active(&st), [true, false, false]);
        // Both group-1 pumps are "on"; only the one with the higher on-elevation runs. The
        // ungrouped pump runs on its own.
        update_states(&devices, 102.5, 0.1, &mut st);
        assert_eq!(active(&st), [false, true, true]);
        // Above the second pump's maximum elevation it stops and the first takes over.
        update_states(&devices, 104.5, 0.2, &mut st);
        assert_eq!(active(&st), [true, false, true]);
        // Falling back below the maximum, the leader runs again (hysteresis keeps it on).
        update_states(&devices, 103.0, 0.3, &mut st);
        assert_eq!(active(&st), [false, true, true]);
        update_states(&devices, 99.5, 0.4, &mut st);
        assert_eq!(active(&st), [false, false, false]);
    }

    fn reference_devices() -> Vec<OutletDevice> {
        vec![
            OutletDevice::new(
                "Culvert",
                50.0,
                Routing::Primary,
                DeviceKind::Culvert {
                    shape: PipeShape::Round,
                    width: 2.0,
                    height: 2.0,
                    length: 12.0,
                    outlet_invert: 49.6,
                    n: 0.013,
                    ke: 0.2,
                    cc: 0.9,
                    fill: 0.0,
                    entrance: "RCP, groove end projecting".into(),
                },
            ),
            OutletDevice::new(
                "Orifice plate at culvert inlet",
                50.5,
                Routing::Device(0),
                DeviceKind::Orifice {
                    shape: OpeningShape::Circular { diameter: 16.0 / 12.0 },
                    plane: Plane::Vertical,
                    cd: 0.6,
                    columns: 1,
                    rows: 1,
                    row_spacing: 0.0,
                    weir_at_low_head: true,
                    grate: None,
                },
            ),
            OutletDevice::new(
                "BC Rectangular Overflow Weir",
                53.6,
                Routing::Secondary,
                DeviceKind::BroadCrestedWeir { length: 5.0, breadth: 2.5, side_z: 0.0, rise: None },
            ),
        ]
    }

    #[test]
    fn reference_device_analysis() {
        // HW=54.49': culvert passes 12.25 of 35.30 potential; orifice controls 12.25 @ 8.8 fps;
        // weir controls 10.90 @ 2.5 fps (secondary).
        let devs = reference_devices();
        let storage = PondStorage::default();
        let tw = |r: Routing| if r == Routing::Primary { Some(49.0) } else { None };
        let f = evaluate(&devs, 54.486, &tw, &storage, &[], 0.0);
        assert!((f.potential[0] - 35.30).abs() < 0.05, "culvert potential {}", f.potential[0]);
        assert!((f.devices[1] - 12.25).abs() < 0.03, "orifice {}", f.devices[1]);
        assert!((f.velocities[1] - 8.8).abs() < 0.05);
        assert!((f.primary - f.devices[1]).abs() < 1e-9);
        assert!((f.secondary - 10.90).abs() < 0.05, "weir {}", f.secondary);
    }

    #[test]
    fn broad_crested_table_matches_reference() {
        let t = broad_crested_table(2.5);
        let coefs: Vec<f64> = t.iter().map(|p| p.1).collect();
        assert_eq!(coefs, vec![2.48, 2.60, 2.60, 2.60, 2.64, 2.65, 2.68, 2.75, 2.74, 2.76, 2.89, 3.05, 3.19, 3.32]);
    }

    #[test]
    fn culvert_respects_tailwater() {
        let f = culvert_flow(PipeShape::Round, 2.0, 2.0, 0.0, 50.0, 100.0, 99.0, 0.013, 0.5, 0.9, 103.0, Some(103.5));
        assert_eq!(f.flow, 0.0);
        let free = culvert_flow(PipeShape::Round, 2.0, 2.0, 0.0, 50.0, 100.0, 99.0, 0.013, 0.5, 0.9, 103.0, None);
        let sub = culvert_flow(PipeShape::Round, 2.0, 2.0, 0.0, 50.0, 100.0, 99.0, 0.013, 0.5, 0.9, 103.0, Some(102.5));
        assert!(sub.flow < free.flow && sub.flow > 0.0);
    }

    #[test]
    fn stage_discharge_is_monotone_for_every_shape() {
        for shape in PipeShape::ALL {
            let mut last = 0.0;
            for i in 0..200 {
                let hw = 100.0 + i as f64 * 0.05;
                let f = culvert_flow(shape, 3.0, 2.0, 0.0, 40.0, 100.0, 99.6, 0.013, 0.5, 0.9, hw, None).flow;
                assert!(f + 1e-6 >= last, "{shape:?} drops at {hw}: {f} < {last}");
                last = f;
            }
        }
        let orifice = OpeningShape::Circular { diameter: 1.0 };
        let mut last = 0.0;
        for i in 0..100 {
            let q = vertical_opening(&orifice, 0.6, i as f64 * 0.03);
            assert!(q + 1e-9 >= last);
            last = q;
        }
    }

    #[test]
    fn exfiltration_and_constant_devices() {
        use crate::storage::{StorageVolume, VolumeShape};
        let storage = PondStorage {
            volumes: vec![StorageVolume::new("pond", 100.0, VolumeShape::Prismatoid { width: 10.0, length: 10.0, height: 5.0, z: 0.0 })],
        };
        let d = OutletDevice::new(
            "Exfiltration",
            100.0,
            Routing::Discarded,
            DeviceKind::Exfiltration { mode: ExfilMode::SurfaceVelocity { velocity: 1.0 / 12.0 / 3600.0 }, phase_in: 0.0, max_elevation: None },
        );
        let c = Conditions { hw: 101.0, tw: None, storage: &storage, state: DeviceState::default(), time: 0.0 };
        // 100 sf at 1 in/hr = 100/12/3600 cfs.
        assert!((d.potential(&c).flow - 100.0 / 12.0 / 3600.0).abs() < 1e-12);
        let k = OutletDevice::new("Skimmer", 100.0, Routing::Primary, DeviceKind::ConstantFlow { flow: 4.2, phase_in: 0.3 });
        assert!((k.potential(&Conditions { hw: 100.15, ..c }).flow - 2.1).abs() < 1e-9);
    }

    #[test]
    fn device_routing_cycle_is_harmless() {
        let mut devs = reference_devices();
        devs[0].routing = Routing::Device(1);
        devs[1].routing = Routing::Device(0);
        assert_eq!(final_routing(&devs, 0), None);
        let _ = evaluate(&devs, 54.0, &|_| None, &PondStorage::default(), &[], 0.0);
    }

    #[test]
    fn stateful_devices() {
        let storage = PondStorage::default();
        let siphon = OutletDevice::new("Siphon", 100.0, Routing::Primary, DeviceKind::Siphon { diameter: 1.0, length: 50.0, n: 0.013, ke: 0.5, outlet_elevation: 95.0, prime_elevation: 104.0, break_elevation: 100.5 });
        let breach = OutletDevice::new("Breach", 110.0, Routing::Secondary, DeviceKind::DamBreach { start_time: None, start_elevation: Some(109.0), formation: 1.0, final_bottom: 100.0, final_width: 20.0, side_z: 1.0 });
        let gate = OutletDevice::new("Gate", 100.0, Routing::Primary, DeviceKind::DelayedGate { trigger_elevation: 103.0, delay: 1.0, opening: 2.0, width: 2.0, height: 2.0, cd: 0.6, step: 0.0, vertical_range: 0.0, rotary: false });
        let devs = vec![siphon, breach, gate];
        let mut st = vec![DeviceState::default(); 3];
        // Below priming: nothing flows.
        update_states(&devs, 103.0, 0.0, &mut st);
        let f = evaluate(&devs, 103.0, &|_| None, &storage, &st, 0.0);
        assert_eq!(f.devices[0], 0.0);
        // Primed above 104, keeps running down to the break level (hysteresis).
        update_states(&devs, 104.5, 1.0, &mut st);
        update_states(&devs, 102.0, 1.1, &mut st);
        assert!(evaluate(&devs, 102.0, &|_| None, &storage, &st, 1.1).devices[0] > 0.0);
        update_states(&devs, 100.4, 1.2, &mut st);
        assert_eq!(evaluate(&devs, 100.4, &|_| None, &storage, &st, 1.2).devices[0], 0.0);
        // Gate triggered at t=0 (103 reached on the first update), opens from t=1 to t=3.
        assert_eq!(evaluate(&devs, 103.0, &|_| None, &storage, &st, 0.5).devices[2], 0.0);
        let half = evaluate(&devs, 103.0, &|_| None, &storage, &st, 2.0).devices[2];
        let full = evaluate(&devs, 103.0, &|_| None, &storage, &st, 3.0).devices[2];
        assert!(half > 0.0 && full > half);
        // Opening in 1-hour steps: constant within a step, jumping at the next one.
        let mut stepped = devs.clone();
        if let DeviceKind::DelayedGate { step, .. } = &mut stepped[2].kind {
            *step = 1.0;
        }
        let q = |t: f64| evaluate(&stepped, 103.0, &|_| None, &storage, &st, t).devices[2];
        assert!((q(1.2) - q(1.9)).abs() < 1e-12 && q(2.1) > q(1.9) && (q(1.9) - half).abs() < 1e-9);
        // Breach begins when the water reaches 109 and grows over an hour.
        update_states(&devs, 109.5, 5.0, &mut st);
        let early = evaluate(&devs, 109.5, &|_| None, &storage, &st, 5.1).devices[1];
        let later = evaluate(&devs, 109.5, &|_| None, &storage, &st, 6.0).devices[1];
        assert!(later > early && early > 0.0);
    }

    #[test]
    fn moving_gate_and_rotary_weir() {
        let storage = PondStorage::default();
        let kind = |range: f64, rotary: bool| DeviceKind::DelayedGate { trigger_elevation: 101.0, delay: 0.0, opening: 2.0, width: 2.0, height: 2.0, cd: 0.6, step: 0.0, vertical_range: range, rotary };
        let devs = vec![OutletDevice::new("Gate", 100.0, Routing::Primary, kind(2.0, false)), OutletDevice::new("Bucket", 100.0, Routing::Primary, kind(2.0, true))];
        let mut st = vec![DeviceState::default(); 2];
        let q = |st: &[DeviceState], hw: f64, t: f64| evaluate(&devs, hw, &|_| None, &storage, st, t).devices;
        // Before the trigger the lip stands at 102: nothing below it, spill-in above it.
        assert_eq!(q(&st, 101.5, 0.0), vec![0.0, 0.0]);
        assert!(q(&st, 102.5, 0.0).iter().all(|v| *v > 0.0));
        // Triggered at t=0; halfway the lip is at 101 and the flow at 103 grows as it lowers.
        update_states(&devs, 101.0, 0.0, &mut st);
        let (start, half, full) = (q(&st, 103.0, 0.0), q(&st, 103.0, 1.0), q(&st, 103.0, 2.0));
        for i in 0..2 {
            assert!(start[i] < half[i] && half[i] < full[i], "{i}: {start:?} {half:?} {full:?}");
        }
        // Fully open, the rotary weir is a vertical opening at the invert like the moving gate.
        assert!((full[0] - full[1]).abs() < 1e-9);
    }

    #[test]
    fn channel_outlet_is_normal_depth() {
        let sec = Section::Rectangular { width: 4.0, depth: 3.0 };
        let d = OutletDevice::new("Channel", 50.0, Routing::Primary, DeviceKind::Channel { section: sec.clone(), n: 0.03, slope: 0.01 });
        let c = Conditions { hw: 51.0, tw: None, storage: &PondStorage::default(), state: DeviceState::default(), time: 0.0 };
        assert!((d.potential(&c).flow - sec.manning_q(1.0, 0.03, 0.01)).abs() < 1e-9);
    }
}
