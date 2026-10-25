//! Dimensionless unit hydrographs (NEH Part 630, Chapter 16).
//!
//! A unit hydrograph shape is a curve of q/qp against t/Tp. Its peak rate factor (PRF, 484 for
//! the standard NRCS curve) follows from the area under the curve:
//! `PRF = 645.33 / ∫ (q/qp) d(t/Tp)` (NEH 630 eq. 16-2).

use serde::{Deserialize, Serialize};

/// Converts square miles × inches/hour into cubic feet per second.
pub const CFS_PER_SQMI_INCH_PER_HR: f64 = 645.333_333;

/// NEH 630 Table 16-1: the standard NRCS dimensionless unit hydrograph (PRF 484).
const NRCS_T: [f64; 33] = [
    0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0, 2.2, 2.4,
    2.6, 2.8, 3.0, 3.2, 3.4, 3.6, 3.8, 4.0, 4.5, 5.0,
];
const NRCS_Q: [f64; 33] = [
    0.000, 0.030, 0.100, 0.190, 0.310, 0.470, 0.660, 0.820, 0.930, 0.990, 1.000, 0.990, 0.930, 0.860, 0.780, 0.680,
    0.560, 0.460, 0.390, 0.330, 0.280, 0.207, 0.147, 0.107, 0.077, 0.055, 0.040, 0.029, 0.021, 0.015, 0.011, 0.005,
    0.000,
];

/// A dimensionless unit hydrograph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitHydrograph {
    pub name: String,
    pub comment: String,
    /// t/Tp, increasing from 0.
    pub t: Vec<f64>,
    /// q/qp, zero at both ends.
    pub q: Vec<f64>,
}

impl UnitHydrograph {
    pub fn nrcs() -> Self {
        UnitHydrograph {
            name: "SCS".into(),
            comment: "Standard NRCS dimensionless UH (NEH 630 Table 16-1), Peak Factor = 484".into(),
            t: NRCS_T.to_vec(),
            q: NRCS_Q.to_vec(),
        }
    }

    /// Gamma-equation unit hydrograph with the given peak rate factor (NEH 630 eq. 16-1).
    pub fn gamma(prf: f64) -> Option<Self> {
        let m = gamma_shape_for_prf(prf)?;
        // Extend until the ordinate is negligible.
        let mut t = Vec::new();
        let mut q = Vec::new();
        let step = 0.05;
        let mut x: f64 = 0.0;
        loop {
            let v = if x <= 0.0 { 0.0 } else { (x * (1.0 - x).exp()).powf(m) };
            t.push(x);
            q.push(v);
            if (x > 1.0 && v < 1e-4) || x > 60.0 {
                break;
            }
            x += step;
        }
        if let Some(last) = q.last_mut() {
            *last = 0.0;
        }
        Some(UnitHydrograph {
            name: format!("Gamma-{}", prf.round() as i64),
            comment: format!("Gamma-equation dimensionless UH, m={m:.3}, Peak Factor = {}", prf.round() as i64),
            t,
            q,
        })
    }

    /// q/qp at t/Tp = x (linear interpolation, zero outside).
    pub fn ordinate(&self, x: f64) -> f64 {
        if !x.is_finite() || x <= 0.0 || self.t.len() < 2 {
            return 0.0;
        }
        let last = self.t.len() - 1;
        if x >= self.t[last] {
            return 0.0;
        }
        let i = self.t.partition_point(|v| *v <= x).saturating_sub(1).min(last - 1);
        let (x0, x1) = (self.t[i], self.t[i + 1]);
        let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
        self.q[i] + (self.q[i + 1] - self.q[i]) * f
    }

    /// Area under the curve, ∫ q/qp d(t/Tp).
    pub fn area(&self) -> f64 {
        self.t.windows(2).zip(self.q.windows(2)).map(|(t, q)| 0.5 * (q[0] + q[1]) * (t[1] - t[0])).sum()
    }

    /// Mass curve: fraction of the unit volume delivered by t/Tp = x.
    pub fn mass(&self, x: f64) -> f64 {
        if x <= 0.0 || self.t.len() < 2 {
            return 0.0;
        }
        let total = self.area();
        if total <= 0.0 {
            return 0.0;
        }
        let mut acc = 0.0;
        for i in 0..self.t.len() - 1 {
            let (x0, x1) = (self.t[i], self.t[i + 1]);
            if x >= x1 {
                acc += 0.5 * (self.q[i] + self.q[i + 1]) * (x1 - x0);
            } else {
                let qx = self.ordinate(x);
                acc += 0.5 * (self.q[i] + qx) * (x - x0);
                break;
            }
        }
        (acc / total).min(1.0)
    }

    /// Peak rate factor implied by the curve's area.
    pub fn peak_factor(&self) -> f64 {
        let a = self.area();
        if a > 0.0 { CFS_PER_SQMI_INCH_PER_HR / a } else { 0.0 }
    }

    /// Time base in units of Tp.
    pub fn base(&self) -> f64 {
        self.t.last().copied().unwrap_or(0.0)
    }
}

/// The built-in UH library: NRCS standard plus gamma curves for common peak factors.
pub fn builtin_uhs() -> Vec<UnitHydrograph> {
    let mut v = vec![UnitHydrograph::nrcs()];
    for prf in [100.0, 150.0, 200.0, 250.0, 284.0, 300.0, 325.0, 350.0, 400.0, 450.0, 484.0, 500.0, 550.0, 575.0, 600.0]
    {
        if let Some(u) = UnitHydrograph::gamma(prf) {
            v.push(u);
        }
    }
    v
}

fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation (g=7, n=9), accurate to ~1e-15 for x > 0.
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// PRF of the gamma UH with shape m: 645.33 · m^(m+1) / (e^m · Γ(m+1)).
pub fn gamma_prf(m: f64) -> f64 {
    if m <= 0.0 {
        return 0.0;
    }
    let ln_area = m - (m + 1.0) * m.ln() + ln_gamma(m + 1.0);
    CFS_PER_SQMI_INCH_PER_HR / ln_area.exp()
}

/// Solves for the gamma shape factor giving `prf` (bisection; PRF increases with m).
pub fn gamma_shape_for_prf(prf: f64) -> Option<f64> {
    if !(50.0..=1500.0).contains(&prf) {
        return None;
    }
    let (mut lo, mut hi) = (0.05, 60.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if gamma_prf(mid) < prf { lo = mid } else { hi = mid }
    }
    Some(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nrcs_peak_factor_is_484() {
        let pf = UnitHydrograph::nrcs().peak_factor();
        assert!((pf - 484.0).abs() < 2.0, "{pf}");
    }

    #[test]
    fn gamma_matches_neh_table_16_5() {
        // NEH 630 Table 16-5: m=1 -> 238, 2 -> 349, 3 -> 433, 3.7 -> 484, 5 -> 566.
        for (m, prf) in [(1.0, 238.0), (2.0, 349.0), (3.0, 433.0), (3.7, 484.0), (5.0, 566.0)] {
            assert!((gamma_prf(m) - prf).abs() < 1.5, "m={m}: {}", gamma_prf(m));
        }
        let u = UnitHydrograph::gamma(300.0).unwrap();
        assert!((u.peak_factor() - 300.0).abs() < 3.0);
    }

    #[test]
    fn mass_curve_ends_at_one() {
        let u = UnitHydrograph::nrcs();
        assert!((u.mass(10.0) - 1.0).abs() < 1e-12);
        assert!((u.mass(1.0) - 0.375).abs() < 0.01);
    }
}
