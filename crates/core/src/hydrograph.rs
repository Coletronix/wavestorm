//! Hydrographs: flow sampled at a constant time step.
//!
//! Flows are in cubic feet per second and times in hours throughout the engine; unit
//! conversion happens only at the edges (input dialogs and reports).

use serde::{Deserialize, Serialize};

/// Seconds per hour, for converting `cfs * hours` into cubic feet.
pub const SEC_PER_HR: f64 = 3600.0;

/// An interpolated peak: the vertex of the parabola through the three highest samples, the
/// same convention TR-20 uses for reporting peak flows.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Peak {
    pub value: f64,
    pub time: f64,
}

/// A time series at a fixed step, starting at `t0` (hours).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Hydrograph {
    pub t0: f64,
    pub dt: f64,
    pub q: Vec<f64>,
}

impl Hydrograph {
    pub fn zeros(t0: f64, dt: f64, n: usize) -> Self {
        Hydrograph { t0, dt, q: vec![0.0; n] }
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    pub fn time(&self, i: usize) -> f64 {
        self.t0 + i as f64 * self.dt
    }

    pub fn end_time(&self) -> f64 {
        self.time(self.q.len().saturating_sub(1))
    }

    /// Flow at an arbitrary time by linear interpolation; zero outside the series.
    pub fn value_at(&self, t: f64) -> f64 {
        interpolate_series(&self.q, self.t0, self.dt, t)
    }

    /// Index of the largest sample (the first one on ties).
    pub fn max_index(&self) -> Option<usize> {
        max_index(&self.q)
    }

    /// Largest discrete sample and its time.
    pub fn max(&self) -> Peak {
        match self.max_index() {
            Some(i) => Peak { value: self.q[i], time: self.time(i) },
            None => Peak::default(),
        }
    }

    /// Interpolated peak flow (parabola through the highest sample and its neighbours).
    pub fn peak(&self) -> Peak {
        interpolated_peak(&self.q, self.t0, self.dt)
    }

    /// Total volume in cubic feet (trapezoidal integration).
    pub fn volume_cf(&self) -> f64 {
        trapezoid(&self.q) * self.dt * SEC_PER_HR
    }

    /// Time of the centroid of the hydrograph, in hours, if it has any volume.
    pub fn center_of_mass(&self) -> Option<f64> {
        let mut m = 0.0;
        let mut mt = 0.0;
        for (i, &q) in self.q.iter().enumerate() {
            m += q;
            mt += q * self.time(i);
        }
        (m > 0.0).then(|| mt / m)
    }

    /// Adds `other`, resampling it to this hydrograph's time base when they differ.
    pub fn add(&mut self, other: &Hydrograph) {
        let same_base = (self.t0 - other.t0).abs() < 1e-9 && (self.dt - other.dt).abs() < 1e-12;
        for (i, q) in self.q.iter_mut().enumerate() {
            let add = if same_base {
                other.q.get(i).copied().unwrap_or(0.0)
            } else {
                other.value_at(self.t0 + i as f64 * self.dt)
            };
            *q += add;
        }
    }

    pub fn scaled(&self, factor: f64) -> Hydrograph {
        Hydrograph { t0: self.t0, dt: self.dt, q: self.q.iter().map(|v| v * factor).collect() }
    }

    /// Resamples onto a new time base (linear interpolation, zero outside the source).
    pub fn resample(&self, t0: f64, dt: f64, n: usize) -> Hydrograph {
        Hydrograph { t0, dt, q: (0..n).map(|i| self.value_at(t0 + i as f64 * dt)).collect() }
    }

    /// True when any flow is non-zero.
    pub fn has_flow(&self) -> bool {
        self.q.iter().any(|v| *v > 0.0)
    }
}

pub fn trapezoid(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let inner: f64 = v[1..v.len() - 1].iter().sum();
    inner + 0.5 * (v[0] + v[v.len() - 1])
}

pub fn max_index(v: &[f64]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        match best {
            Some(b) if v[b] >= *x => {}
            _ => best = Some(i),
        }
    }
    best
}

/// Peak of a parabola fitted to the largest sample of a series and its two neighbours.
pub fn interpolated_peak(v: &[f64], t0: f64, dt: f64) -> Peak {
    let Some(i) = max_index(v) else { return Peak::default() };
    let y1 = v[i];
    let t = t0 + i as f64 * dt;
    if i == 0 || i + 1 >= v.len() {
        return Peak { value: y1, time: t };
    }
    let (y0, y2) = (v[i - 1], v[i + 1]);
    let denom = y0 - 2.0 * y1 + y2;
    if denom.abs() < 1e-12 {
        return Peak { value: y1, time: t };
    }
    let offset = (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5);
    Peak { value: y1 - 0.25 * (y0 - y2) * offset, time: t + offset * dt }
}

pub fn interpolate_series(v: &[f64], t0: f64, dt: f64, t: f64) -> f64 {
    if v.is_empty() || dt <= 0.0 || !t.is_finite() {
        return 0.0;
    }
    let x = (t - t0) / dt;
    if x < 0.0 || x > (v.len() - 1) as f64 {
        return 0.0;
    }
    let i = x.floor() as usize;
    let f = x - i as f64;
    match v.get(i + 1) {
        Some(next) => v[i] + (next - v[i]) * f,
        None => v[i],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parabolic_peak_recovers_vertex() {
        // y = 10 - (t - 1.3)^2 sampled at 0.5 h steps.
        let v: Vec<f64> = (0..6).map(|i| 10.0 - (i as f64 * 0.5 - 1.3f64).powi(2)).collect();
        let p = interpolated_peak(&v, 0.0, 0.5);
        assert!((p.value - 10.0).abs() < 1e-9);
        assert!((p.time - 1.3).abs() < 1e-9);
    }

    #[test]
    fn volume_and_center_of_mass() {
        let h = Hydrograph { t0: 0.0, dt: 1.0, q: vec![0.0, 1.0, 0.0] };
        assert!((h.volume_cf() - 3600.0).abs() < 1e-9);
        assert_eq!(h.center_of_mass(), Some(1.0));
    }

    #[test]
    fn add_resamples_different_bases() {
        let mut a = Hydrograph::zeros(0.0, 0.5, 5);
        let b = Hydrograph { t0: 0.0, dt: 1.0, q: vec![0.0, 2.0, 0.0] };
        a.add(&b);
        assert_eq!(a.q, vec![0.0, 1.0, 2.0, 1.0, 0.0]);
    }

    #[test]
    fn empty_series_is_harmless() {
        let h = Hydrograph::default();
        assert_eq!(h.peak(), Peak::default());
        assert_eq!(h.volume_cf(), 0.0);
        assert_eq!(h.value_at(3.0), 0.0);
    }
}
