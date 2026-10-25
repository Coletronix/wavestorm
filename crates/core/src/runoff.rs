//! Runoff hydrographs: the NRCS (SCS) unit-hydrograph method (TR-20), the Santa Barbara Urban
//! Hydrograph, and the Rational method.

use crate::hydrograph::{Hydrograph, SEC_PER_HR};
use crate::rainfall::Rainfall;
use crate::uh::{CFS_PER_SQMI_INCH_PER_HR, UnitHydrograph};

pub const SQFT_PER_ACRE: f64 = 43_560.0;
pub const SQFT_PER_SQMI: f64 = 27_878_400.0;

/// Potential maximum retention S (inches) for a curve number.
pub fn retention(cn: f64) -> f64 {
    if cn <= 0.0 { f64::INFINITY } else { 1000.0 / cn - 10.0 }
}

/// Cumulative runoff depth (inches) from cumulative precipitation `p` (inches):
/// Q = (P - λS)² / (P + (1-λ)S), zero until P exceeds the initial abstraction λS.
pub fn scs_runoff_depth(p: f64, cn: f64, ia_ratio: f64) -> f64 {
    if cn >= 100.0 {
        return p.max(0.0);
    }
    let s = retention(cn);
    if !s.is_finite() {
        return 0.0;
    }
    let ia = ia_ratio * s;
    if p <= ia {
        return 0.0;
    }
    let pe = p - ia;
    pe * pe / (pe + s)
}

/// One area/curve-number pair contributing to a subcatchment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CnArea {
    pub area_sf: f64,
    pub cn: f64,
}

/// Cumulative excess (inches over the whole area) at time `t`, for the given area split.
fn excess_at(rain: &Rainfall, parts: &[CnArea], ia_ratio: f64, t: f64) -> f64 {
    let total: f64 = parts.iter().map(|p| p.area_sf).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let p = rain.depth_at(t);
    parts.iter().map(|a| a.area_sf / total * scs_runoff_depth(p, a.cn, ia_ratio)).sum()
}

/// Result of a runoff computation on the calculation grid.
#[derive(Debug, Clone, Default)]
pub struct RunoffSeries {
    pub hydrograph: Hydrograph,
    /// Cumulative precipitation (inches) at each grid time.
    pub precip: Vec<f64>,
    /// Cumulative excess (inches) at each grid time.
    pub excess: Vec<f64>,
}

/// NRCS unit-hydrograph runoff (TR-20 method).
///
/// The unit hydrograph has the NRCS unit duration ΔD = 0.133 Tc, so Tp = ΔD/2 + 0.6 Tc. Rainfall
/// excess is convolved on an internal step no longer than ΔD (the main step divided evenly), and
/// the result is sampled at the main step. Runoff volume is conserved exactly.
// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord)]
pub fn scs_uh_runoff(
    rain: &Rainfall,
    parts: &[CnArea],
    ia_ratio: f64,
    tc_hr: f64,
    uh: &UnitHydrograph,
    t0: f64,
    dt: f64,
    n: usize,
) -> RunoffSeries {
    let area_sf: f64 = parts.iter().map(|p| p.area_sf).sum();
    let mut out = RunoffSeries {
        hydrograph: Hydrograph::zeros(t0, dt, n),
        precip: Vec::with_capacity(n),
        excess: Vec::with_capacity(n),
    };
    if n == 0 || !(dt > 0.0) {
        return out;
    }
    for i in 0..n {
        let t = t0 + i as f64 * dt;
        out.precip.push(rain.depth_at(t));
        out.excess.push(excess_at(rain, parts, ia_ratio, t));
    }
    if area_sf <= 0.0 {
        return out;
    }
    let sub = substeps(tc_hr, dt);
    let df = dt / sub as f64;
    let fine_n = (n - 1).saturating_mul(sub) + 1;
    // Rainfall excess before the window begins arrives as one block in the first step.
    let mut prev = excess_at(rain, parts, ia_ratio, t0);
    let mut increments = Vec::with_capacity(fine_n);
    increments.push(prev);
    for i in 1..fine_n {
        let e = excess_at(rain, parts, ia_ratio, t0 + i as f64 * df);
        increments.push((e - prev).max(0.0));
        prev = e;
    }
    let kernel = uh_kernel(uh, tc_hr, df, area_sf);
    let mut fine = vec![0.0; fine_n];
    for (i, inc) in increments.iter().enumerate() {
        if *inc <= 0.0 {
            continue;
        }
        for (k, u) in kernel.iter().enumerate() {
            match fine.get_mut(i + k) {
                Some(slot) => *slot += inc * u,
                None => break,
            }
        }
    }
    for (i, q) in out.hydrograph.q.iter_mut().enumerate() {
        *q = fine.get(i * sub).copied().unwrap_or(0.0);
    }
    out
}

/// Santa Barbara Urban Hydrograph: the instantaneous excess hydrograph routed through a linear
/// reservoir with K = Tc, Q₂ = Q₁ + w(I₁ + I₂ - 2Q₁), w = Δt / (2Tc + Δt).
#[allow(clippy::too_many_arguments)]
// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn sbuh_runoff(
    rain: &Rainfall,
    parts: &[CnArea],
    ia_ratio: f64,
    tc_hr: f64,
    t0: f64,
    dt: f64,
    n: usize,
) -> RunoffSeries {
    let area_sf: f64 = parts.iter().map(|p| p.area_sf).sum();
    let mut out = RunoffSeries {
        hydrograph: Hydrograph::zeros(t0, dt, n),
        precip: Vec::with_capacity(n),
        excess: Vec::with_capacity(n),
    };
    if n == 0 || !(dt > 0.0) {
        return out;
    }
    for i in 0..n {
        let t = t0 + i as f64 * dt;
        out.precip.push(rain.depth_at(t));
        out.excess.push(excess_at(rain, parts, ia_ratio, t));
    }
    if area_sf <= 0.0 {
        return out;
    }
    // Instantaneous hydrograph: excess rate over the preceding step times the area.
    let to_cfs = area_sf / 12.0 / (dt * SEC_PER_HR);
    let start = excess_at(rain, parts, ia_ratio, t0);
    let inst: Vec<f64> = (0..n)
        .map(|i| if i == 0 { start * to_cfs } else { (out.excess[i] - out.excess[i - 1]).max(0.0) * to_cfs })
        .collect();
    let tc = if tc_hr.is_finite() { tc_hr.max(0.0) } else { 0.0 };
    let w = dt / (2.0 * tc + dt);
    let q = &mut out.hydrograph.q;
    q[0] = w * inst[0];
    for i in 1..n {
        q[i] = (q[i - 1] + w * (inst[i - 1] + inst[i] - 2.0 * q[i - 1])).max(0.0);
    }
    out
}

/// Units factor for Q = C·i·A with i in in/hr and A in acres (43,560 / 43,200 ≈ 1.008 cfs).
pub const RATIONAL_FACTOR: f64 = SQFT_PER_ACRE / 12.0 / SEC_PER_HR;

/// Rational-method trapezoidal hydrograph: rises over Tc·rise, holds the peak C·i·A until the
/// storm duration ends, then falls over Tc·fall.
// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord)]
pub fn rational_runoff(
    c: f64,
    area_sf: f64,
    intensity_in_hr: f64,
    duration_hr: f64,
    tc_hr: f64,
    rise: f64,
    fall: f64,
    t0: f64,
    dt: f64,
    n: usize,
) -> Hydrograph {
    let mut h = Hydrograph::zeros(t0, dt, n);
    let peak = c.max(0.0) * intensity_in_hr.max(0.0) * area_sf.max(0.0) / SQFT_PER_ACRE * RATIONAL_FACTOR;
    if !(peak > 0.0) || !(duration_hr > 0.0) {
        return h;
    }
    let tr = (tc_hr.max(0.0) * rise.max(0.0)).max(1e-9);
    let tf = (tc_hr.max(0.0) * fall.max(0.0)).max(1e-9);
    for (i, q) in h.q.iter_mut().enumerate() {
        let t = t0 + i as f64 * dt;
        let rising = (t / tr).clamp(0.0, 1.0);
        // After the rain stops, the hydrograph recedes from wherever the rise had reached.
        *q = if t <= duration_hr {
            peak * rising
        } else {
            let at_end = (duration_hr / tr).clamp(0.0, 1.0);
            peak * at_end * (1.0 - (t - duration_hr) / tf).clamp(0.0, 1.0)
        };
    }
    h
}

/// NRCS unit duration as a fraction of Tc (NEH 630, eq. 16-13: ΔD = 0.133 Tc).
pub const UNIT_DURATION_PER_TC: f64 = 0.133;
/// NRCS lag as a fraction of Tc (L = 0.6 Tc).
pub const LAG_PER_TC: f64 = 0.6;

/// Delay of the unit-hydrograph response as a fraction of Tc. Peak times published by the
/// current reference version lag the textbook convolution by about 0.09 Tc (fitted to 18
/// published subcatchments and their combined link peaks; see AGENTS.md). Peak rates and
/// volumes are unaffected.
pub const UH_DELAY_PER_TC: f64 = 0.09;

/// Time to peak of the unit hydrograph: Tp = ΔD/2 + L with ΔD = 0.133 Tc and L = 0.6 Tc.
pub fn time_to_peak(tc_hr: f64) -> f64 {
    (UNIT_DURATION_PER_TC / 2.0 + LAG_PER_TC) * tc_hr
}

/// Internal steps per main step so that the convolution step does not exceed ΔD.
fn substeps(tc_hr: f64, dt: f64) -> usize {
    let dd = UNIT_DURATION_PER_TC * tc_hr;
    if dd <= 0.0 { 1 } else { ((dt / dd).ceil() as usize).clamp(1, 20) }
}

/// Response (cfs per inch of excess) at steps 0, 1, 2… after an excess increment that ends at
/// step 0, sampled from the dimensionless curve at τ = (k+1)·dt − delay after the increment began and
/// normalised so the kernel's volume is exactly one inch over the area.
pub fn uh_kernel(uh: &UnitHydrograph, tc_hr: f64, dt: f64, area_sf: f64) -> Vec<f64> {
    let volume_cf = area_sf / 12.0;
    let flow_per_step = volume_cf / (dt * SEC_PER_HR);
    if tc_hr <= 0.0 || uh.base() <= 0.0 {
        // Instant runoff: the whole increment leaves within the step.
        return vec![flow_per_step];
    }
    let tp = time_to_peak(tc_hr);
    let delay = UH_DELAY_PER_TC * tc_hr;
    let steps = (((uh.base() * tp + delay) / dt).ceil() as usize).saturating_add(1).min(1_000_000);
    let mut kernel: Vec<f64> = (0..steps).map(|k| uh.ordinate(((k + 1) as f64 * dt - delay) / tp)).collect();
    let sum: f64 = kernel.iter().sum();
    if sum <= 0.0 {
        return vec![flow_per_step];
    }
    for v in kernel.iter_mut() {
        *v *= flow_per_step / sum;
    }
    kernel
}

/// Peak rate factor of a unit hydrograph in the same units as the classic qp = PRF·A·Q/Tp.
pub fn peak_flow_per_inch(uh: &UnitHydrograph, tc_hr: f64, area_sf: f64) -> f64 {
    let tp = time_to_peak(tc_hr);
    if tp <= 0.0 { 0.0 } else { CFS_PER_SQMI_INCH_PER_HR / uh.area() * (area_sf / SQFT_PER_SQMI) / tp }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rainfall::{RainEvent, builtin_storms};

    #[test]
    fn runoff_equation_matches_tr55_table_2_1() {
        // TR-55 Table 2-1: P=4.0, CN=80 -> 2.04 in; P=3.0, CN=70 -> 0.71 in; P=1.0, CN=98 -> 0.79.
        assert!((scs_runoff_depth(4.0, 80.0, 0.2) - 2.04).abs() < 0.005);
        assert!((scs_runoff_depth(3.0, 70.0, 0.2) - 0.71).abs() < 0.005);
        assert!((scs_runoff_depth(1.0, 98.0, 0.2) - 0.79).abs() < 0.005);
        assert_eq!(scs_runoff_depth(0.5, 60.0, 0.2), 0.0);
    }

    fn type2(depth: f64) -> Rainfall {
        let storm = builtin_storms().into_iter().find(|s| s.name == "Type II 24-hr").unwrap();
        Rainfall::new(&storm, &RainEvent::new("25-Year", "Type II 24-hr", depth))
    }

    #[test]
    fn golden_reference_runoff_hydrograph() {
        // Reference sample (11S) from a 2003 version: 25 ac, CN 60, Tc 23.2 min, Type II 4.30",
        // dt 0.10 h, span 10-40 h: 18.56 cfs @ 12.15 h, 1.903 af. The current version's peak
        // delay (UH_DELAY_PER_TC) moves this coarse-step hydrograph ~0.04 h later and the
        // interpolated peak up ~2 %, so the old tabular ordinates are no longer matched exactly.
        let rain = type2(4.30);
        let parts = [CnArea { area_sf: 25.0 * SQFT_PER_ACRE, cn: 60.0 }];
        let uh = UnitHydrograph::nrcs();
        let r = scs_uh_runoff(&rain, &parts, 0.2, 23.2 / 60.0, &uh, 0.0, 0.1, 401);
        let win = r.hydrograph.resample(10.0, 0.1, 301);
        let p = win.peak();
        let vol_af = win.volume_cf() / SQFT_PER_ACRE;
        assert!((p.value - 18.56).abs() / 18.56 < 0.03, "peak {}", p.value);
        assert!((p.time - 12.15).abs() < 0.06, "peak time {}", p.time);
        for (t, g) in [(12.6, 6.30), (13.0, 3.49), (16.0, 1.26), (24.4, 0.15)] {
            assert!((win.value_at(t) - g).abs() < 0.10 * g + 0.05, "t={t}: {} vs {g}", win.value_at(t));
        }
        assert!((vol_af - 1.903).abs() < 0.01, "volume {vol_af}");
    }

    #[test]
    fn golden_node_listing_peaks() {
        // Reference sample project node listing from a 2003 version (Type II 24-hr, 4.30",
        // dt 0.10 h, span 10-40 h); see golden_reference_runoff_hydrograph for the tolerance:
        // (area ac, CN, Tc min, peak cfs, volume af).
        let cases = [
            (20.0, 71.0, 91.9, 11.74, 2.672),
            (43.5, 55.0, 31.1, 16.08, 2.371),
            (27.0, 55.0, 24.8, 11.68, 1.472),
            (12.0, 60.0, 25.0, 8.56, 0.914),
            (25.0, 60.0, 46.7, 11.53, 1.903),
            (25.0, 60.0, 23.2, 18.56, 1.903),
            (25.0, 55.0, 27.0, 10.27, 1.363),
            (23.0, 56.0, 27.0, 10.61, 1.348),
        ];
        let rain = type2(4.30);
        let uh = UnitHydrograph::nrcs();
        for (area, cn, tc, peak, vol) in cases {
            let parts = [CnArea { area_sf: area * SQFT_PER_ACRE, cn }];
            let r = scs_uh_runoff(&rain, &parts, 0.2, tc / 60.0, &uh, 0.0, 0.1, 401);
            let win = r.hydrograph.resample(10.0, 0.1, 301);
            let p = win.peak().value;
            let v = win.volume_cf() / SQFT_PER_ACRE;
            assert!((p - peak).abs() / peak < 0.03, "{area} ac CN{cn} Tc{tc}: {p:.2} vs {peak}");
            assert!((v - vol).abs() / vol < 0.005, "{area} ac CN{cn}: {v:.3} af vs {vol}");
        }
    }

    #[test]
    fn zero_tc_gives_instant_runoff() {
        let rain = type2(4.30);
        let parts = [CnArea { area_sf: SQFT_PER_ACRE, cn: 98.0 }];
        let r = scs_uh_runoff(&rain, &parts, 0.2, 0.0, &UnitHydrograph::nrcs(), 0.0, 0.05, 601);
        let v = r.hydrograph.volume_cf() / SQFT_PER_ACRE * 12.0;
        assert!((v - scs_runoff_depth(4.3, 98.0, 0.2)).abs() < 0.01);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        let rain = type2(4.30);
        let uh = UnitHydrograph::nrcs();
        let _ = scs_uh_runoff(&rain, &[], 0.2, 0.5, &uh, 0.0, 0.1, 10);
        let _ = scs_uh_runoff(&rain, &[CnArea { area_sf: 1.0, cn: 0.0 }], 0.2, 0.5, &uh, 0.0, 0.1, 10);
        let _ = scs_uh_runoff(&rain, &[CnArea { area_sf: 1.0, cn: 80.0 }], 0.2, 0.5, &uh, 0.0, 0.0, 10);
        let _ = scs_uh_runoff(&rain, &[CnArea { area_sf: 1.0, cn: 80.0 }], 0.2, f64::NAN, &uh, 0.0, 0.1, 10);
    }

    #[test]
    fn sbuh_conserves_volume() {
        let rain = type2(3.0);
        let parts = [CnArea { area_sf: 10.0 * SQFT_PER_ACRE, cn: 85.0 }];
        let r = sbuh_runoff(&rain, &parts, 0.2, 0.25, 0.0, 0.05, 1201);
        let depth = r.hydrograph.volume_cf() / (10.0 * SQFT_PER_ACRE) * 12.0;
        assert!((depth - scs_runoff_depth(3.0, 85.0, 0.2)).abs() < 0.01, "{depth}");
    }

    #[test]
    fn rational_peak_is_cia() {
        let h = rational_runoff(0.9, 2.0 * SQFT_PER_ACRE, 4.0, 0.5, 0.25, 1.0, 1.0, 0.0, 0.01, 101);
        assert!((h.max().value - 0.9 * 4.0 * 2.0 * RATIONAL_FACTOR).abs() < 1e-9);
        assert_eq!(h.value_at(1.0), 0.0);
        // Duration shorter than Tc: triangular with a reduced peak.
        let short = rational_runoff(0.9, 2.0 * SQFT_PER_ACRE, 4.0, 0.1, 0.25, 1.0, 1.0, 0.0, 0.01, 101);
        assert!(short.max().value < h.max().value * 0.5);
    }
}
