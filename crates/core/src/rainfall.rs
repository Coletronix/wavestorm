//! Rainfall distributions (dimensionless mass curves) and rainfall events.
//!
//! A [`Storm`] is a cumulative mass curve: the fraction of the total depth fallen by each time.
//! Built-in storms come from published NRCS tables; custom storms can be given as mass-curve
//! points, intensity points, incremental depths, or IDF ("Chicago") coefficients.
//! A [`RainEvent`] picks a storm and supplies the depth, duration handling, back-to-back count
//! and antecedent moisture condition.

use crate::rainfall_data;
use serde::{Deserialize, Serialize};

/// How a storm is stretched or cut when the event duration differs from the storm's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DurationMode {
    /// Use the storm's own duration.
    #[default]
    Default,
    /// Stretch or compress the whole storm to the new duration (changes peak intensity).
    Rescale,
    /// Keep the central part of the storm (preserves shape and peak intensity).
    Trim,
}

impl DurationMode {
    pub const ALL: [DurationMode; 3] = [DurationMode::Default, DurationMode::Rescale, DurationMode::Trim];
    pub fn label(self) -> &'static str {
        match self {
            DurationMode::Default => "Default",
            DurationMode::Rescale => "Rescale",
            DurationMode::Trim => "Trim",
        }
    }
}

/// A dimensionless cumulative rainfall curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Storm {
    pub name: String,
    pub comment: String,
    /// Default storm duration in hours.
    pub duration: f64,
    /// Times in hours from storm start, strictly increasing, first = 0, last = `duration`.
    pub times: Vec<f64>,
    /// Cumulative fractions, non-decreasing, first = 0, last = 1.
    pub fractions: Vec<f64>,
    /// Interpolate with a monotone cubic instead of straight lines.
    pub smooth: bool,
    /// Storms designed for a fixed duration (NRCS types) should be trimmed, not rescaled.
    pub fixed_duration: bool,
    /// Suggested event depth (inches) for storms that carry one.
    #[serde(default)]
    pub default_depth: Option<f64>,
    /// Smoothing tangents (fraction per hour) that replace the automatic monotone ones at the
    /// given times, shaping the curve inside the peak interval without moving any table point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tangents: Vec<(f64, f64)>,
}

impl Storm {
    /// Builds a storm from cumulative values at an even step; values are normalised so the last
    /// one is 1.
    pub fn from_mass_points(name: &str, comment: &str, duration: f64, values: &[f64], smooth: bool) -> Option<Storm> {
        if values.len() < 2 || duration <= 0.0 {
            return None;
        }
        let total = *values.last()?;
        if total <= 0.0 || values.windows(2).any(|w| w[1] < w[0]) {
            return None;
        }
        let step = duration / (values.len() - 1) as f64;
        let times = (0..values.len()).map(|i| i as f64 * step).collect();
        let fractions = values.iter().map(|v| (v - values[0]) / (total - values[0])).collect();
        Some(Storm {
            name: name.into(),
            comment: comment.into(),
            duration,
            times,
            fractions,
            smooth,
            fixed_duration: false,
            default_depth: None,
            tangents: Vec::new(),
        })
    }

    /// Builds a storm from intensity points at an even step (first at 0, last at `duration`);
    /// the mass curve is their trapezoidal integral.
    pub fn from_intensity_points(name: &str, comment: &str, duration: f64, intensity: &[f64]) -> Option<Storm> {
        if intensity.len() < 2 || intensity.iter().any(|v| *v < 0.0 || !v.is_finite()) {
            return None;
        }
        let mut mass = vec![0.0];
        for w in intensity.windows(2) {
            let last = *mass.last()?;
            mass.push(last + 0.5 * (w[0] + w[1]));
        }
        Storm::from_mass_points(name, comment, duration, &mass, false)
    }

    /// Builds a storm from depth increments at an even step (no value at time zero).
    pub fn from_increments(name: &str, comment: &str, duration: f64, increments: &[f64], smooth: bool) -> Option<Storm> {
        if increments.is_empty() || increments.iter().any(|v| *v < 0.0 || !v.is_finite()) {
            return None;
        }
        let mut mass = vec![0.0];
        for v in increments {
            let last = *mass.last()?;
            mass.push(last + v);
        }
        let total = *mass.last()?;
        let mut storm = Storm::from_mass_points(name, comment, duration, &mass, smooth)?;
        storm.default_depth = Some(total);
        Some(storm)
    }

    /// "Chicago" storm from IDF coefficients: depth(t) = t * a / (b + t)^c for a nested event of
    /// duration t centred on `peak_time`.
    pub fn chicago(name: &str, duration: f64, a: f64, b: f64, c: f64, peak_time: Option<f64>, step: f64) -> Option<Storm> {
        if duration <= 0.0 || step <= 0.0 || !(a.is_finite() && b.is_finite() && c.is_finite()) {
            return None;
        }
        let tp = peak_time.unwrap_or(duration / 2.0).clamp(0.0, duration);
        let r = if duration > 0.0 { tp / duration } else { 0.5 };
        let depth = |t: f64| if t <= 0.0 { 0.0 } else { t * a / (b + t).powf(c) };
        let total = depth(duration);
        if total <= 0.0 {
            return None;
        }
        let n = (duration / step).ceil().max(1.0) as usize;
        let mut values = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let t = (i as f64 * step).min(duration);
            // A nested event of duration td occupies r*td before the peak and (1-r)*td after.
            let v = if t <= tp {
                let td = if r > 0.0 { (tp - t) / r } else { 0.0 };
                r * (total - depth(td.min(duration)))
            } else {
                let td = if r < 1.0 { (t - tp) / (1.0 - r) } else { 0.0 };
                r * total + (1.0 - r) * depth(td.min(duration))
            };
            values.push(v.max(0.0));
        }
        for i in 1..values.len() {
            if values[i] < values[i - 1] {
                values[i] = values[i - 1];
            }
        }
        let mut s = Storm::from_mass_points(name, "IDF-based (Chicago) storm", duration, &values, true)?;
        s.default_depth = Some(total);
        Some(s)
    }

    /// Cumulative fraction at `t` hours (0 before the storm, 1 after it).
    pub fn fraction_at(&self, t: f64) -> f64 {
        if !t.is_finite() || t <= 0.0 || self.times.len() < 2 {
            return 0.0;
        }
        if t >= self.duration {
            return 1.0;
        }
        let i = match self.times.partition_point(|x| *x <= t) {
            0 => 0,
            p => p - 1,
        };
        let j = (i + 1).min(self.times.len() - 1);
        let (t0, t1) = (self.times[i], self.times[j]);
        let (f0, f1) = (self.fractions[i], self.fractions[j]);
        let h = t1 - t0;
        if h <= 0.0 {
            return f0;
        }
        let s = (t - t0) / h;
        let value = if self.smooth {
            let (m0, m1) = (self.slope(i), self.slope(j));
            let s2 = s * s;
            let s3 = s2 * s;
            (2.0 * s3 - 3.0 * s2 + 1.0) * f0
                + (s3 - 2.0 * s2 + s) * h * m0
                + (-2.0 * s3 + 3.0 * s2) * f1
                + (s3 - s2) * h * m1
        } else {
            f0 + (f1 - f0) * s
        };
        value.clamp(f0.min(f1), f0.max(f1))
    }

    /// Tangent at knot `k`: a storm's own tangent for that time, else the Fritsch–Carlson
    /// monotone one.
    fn slope(&self, k: usize) -> f64 {
        if let Some(t) = self.times.get(k) {
            if let Some((_, m)) = self.tangents.iter().find(|(at, _)| (at - t).abs() < 1e-6) {
                return *m;
            }
        }
        let n = self.times.len();
        let secant = |a: usize| {
            let h = self.times[a + 1] - self.times[a];
            if h > 0.0 { (self.fractions[a + 1] - self.fractions[a]) / h } else { 0.0 }
        };
        if k == 0 {
            return secant(0);
        }
        if k + 1 >= n {
            return secant(n - 2);
        }
        let (d0, d1) = (secant(k - 1), secant(k));
        if d0 <= 0.0 || d1 <= 0.0 {
            return 0.0;
        }
        // Weighted harmonic mean keeps the cubic monotone.
        let (h0, h1) = (self.times[k] - self.times[k - 1], self.times[k + 1] - self.times[k]);
        let (w0, w1) = (2.0 * h1 + h0, h1 + 2.0 * h0);
        (w0 + w1) / (w0 / d0 + w1 / d1)
    }

    /// Re-expresses the storm over `duration` hours according to `mode`.
    pub fn adjusted(&self, duration: f64, mode: DurationMode) -> Storm {
        if duration <= 0.0 || (duration - self.duration).abs() < 1e-9 || mode == DurationMode::Default {
            return self.clone();
        }
        match mode {
            DurationMode::Rescale | DurationMode::Default => {
                let k = duration / self.duration;
                let tangents = self.tangents.iter().map(|(t, m)| (t * k, m / k)).collect();
                Storm { duration, times: self.times.iter().map(|t| t * k).collect(), tangents, ..self.clone() }
            }
            DurationMode::Trim => self.trimmed(duration),
        }
    }

    /// Central `duration` hours of the storm, renormalised to a total of one.
    fn trimmed(&self, duration: f64) -> Storm {
        let duration = duration.min(self.duration);
        let start = 0.5 * (self.duration - duration);
        let end = start + duration;
        let (f0, f1) = (self.fraction_at(start), self.fraction_at(end));
        let span = (f1 - f0).max(1e-12);
        let n = ((duration / 0.01).round() as usize).clamp(2, 100_000);
        let times: Vec<f64> = (0..=n).map(|i| duration * i as f64 / n as f64).collect();
        let fractions = times.iter().map(|t| ((self.fraction_at(start + t) - f0) / span).clamp(0.0, 1.0)).collect();
        // The samples already carry the smoothed shape; the tangents' times no longer apply.
        Storm { duration, times, fractions, smooth: false, tangents: Vec::new(), ..self.clone() }
    }
}

fn nrcs(name: &str, comment: &str, table: &[f64; 241]) -> Storm {
    Storm {
        name: name.into(),
        comment: comment.into(),
        duration: 24.0,
        times: (0..241).map(|i| i as f64 * 0.1).collect(),
        fractions: table.to_vec(),
        smooth: true,
        fixed_duration: true,
        default_depth: None,
        tangents: Vec::new(),
    }
}

/// Tangents at the peak of the Type II and III curves. The monotone cubic alone spreads the
/// peak 0.1-hour depth too evenly, so short-Tc peaks came out 6-12 % low and early against
/// published results; these tangents (fitted to 18 published short-Tc subcatchments, see
/// AGENTS.md) concentrate it at the knot between the two largest increments. Every 0.1-hour
/// table value is unchanged, so longer-Tc results are not affected.
const TYPE_II_PEAK_TANGENTS: [(f64, f64); 3] = [(11.8, 0.75), (11.9, 1.6), (12.0, 0.214)];
const TYPE_III_PEAK_TANGENTS: [(f64, f64); 1] = [(12.0, 1.3)];
/// Type IA's peak is flat; these mainly move its timing (fitted to 44 published subcatchments,
/// weaker evidence than for Types II and III because a flat peak's time is ill-defined).
const TYPE_IA_PEAK_TANGENTS: [(f64, f64); 3] = [(7.7, 0.145), (7.8, 0.218), (7.9, 0.211)];

/// The built-in storm library.
pub fn builtin_storms() -> Vec<Storm> {
    let mut v = vec![
        nrcs("Type I 24-hr", "NRCS Type I, smoothed 0.1-hr mass curve (WinTR-55 v2)", &rainfall_data::TYPE_I),
        Storm {
            tangents: TYPE_IA_PEAK_TANGENTS.to_vec(),
            ..nrcs("Type IA 24-hr", "NRCS Type IA, smoothed 0.1-hr mass curve (WinTR-55 v2)", &rainfall_data::TYPE_IA)
        },
        Storm {
            tangents: TYPE_II_PEAK_TANGENTS.to_vec(),
            ..nrcs("Type II 24-hr", "NRCS Type II, smoothed 0.1-hr mass curve (WinTR-55 v2)", &rainfall_data::TYPE_II)
        },
        Storm {
            tangents: TYPE_III_PEAK_TANGENTS.to_vec(),
            ..nrcs("Type III 24-hr", "NRCS Type III, smoothed 0.1-hr mass curve (WinTR-55 v2)", &rainfall_data::TYPE_III)
        },
    ];
    for (base, hours) in [("Type II", 6.0), ("Type II", 12.0), ("Type III", 6.0), ("Type III", 12.0)] {
        if let Some(src) = v.iter().find(|s| s.name.starts_with(base) && s.name.ends_with("24-hr") && s.name.len() == base.len() + 6) {
            let mut s = src.trimmed(hours);
            s.name = format!("{base} {}-hr", hours as u32);
            s.comment = format!("Central {} hours of the NRCS {base} 24-hour storm", hours as u32);
            s.fixed_duration = true;
            v.push(s);
        }
    }
    for (name, table, comment) in [
        ("Type I 24-hr Tabular", &rainfall_data::TYPE_I, "Unsmoothed NRCS Type I table, for direct TR-20 comparisons"),
        ("Type IA 24-hr Tabular", &rainfall_data::TYPE_IA, "Unsmoothed NRCS Type IA table, for direct TR-20 comparisons"),
        ("Type II 24-hr Tabular", &rainfall_data::TYPE_II, "Unsmoothed NRCS Type II table, for direct TR-20 comparisons"),
        ("Type III 24-hr Tabular", &rainfall_data::TYPE_III, "Unsmoothed NRCS Type III table, for direct TR-20 comparisons"),
    ] {
        let mut s = nrcs(name, comment, table);
        s.smooth = false;
        v.push(s);
    }
    v.push(Storm {
        name: "Constant Intensity".into(),
        comment: "Uniform rainfall over any duration".into(),
        duration: 1.0,
        times: vec![0.0, 1.0],
        fractions: vec![0.0, 1.0],
        smooth: false,
        fixed_duration: false,
        default_depth: None,
        tangents: Vec::new(),
    });
    v
}

/// Antecedent moisture (runoff) condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Amc {
    I,
    #[default]
    II,
    III,
}

impl Amc {
    pub fn number(self) -> u8 {
        match self {
            Amc::I => 1,
            Amc::II => 2,
            Amc::III => 3,
        }
    }
    pub fn from_number(n: u8) -> Amc {
        match n {
            0 | 1 => Amc::I,
            2 => Amc::II,
            _ => Amc::III,
        }
    }
    /// Converts an average-condition (AMC II) curve number (Chow, Maidment & Mays, 1988).
    pub fn adjust_cn(self, cn: f64) -> f64 {
        match self {
            Amc::II => cn,
            Amc::I => 4.2 * cn / (10.0 - 0.058 * cn),
            Amc::III => 23.0 * cn / (10.0 + 0.13 * cn),
        }
        .clamp(0.0, 100.0)
    }
}

/// A named rainfall event (2-Year, 25-Year, Water Quality, ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RainEvent {
    pub name: String,
    pub storm: String,
    /// Depth in inches.
    pub depth: f64,
    pub duration_mode: DurationMode,
    /// Duration in hours (used when `duration_mode` is not `Default`).
    pub duration: f64,
    pub back_to_back: u32,
    pub amc: Amc,
    /// Rational method intensity (in/hr) and duration (hours), when not taken from an IDF curve.
    #[serde(default)]
    pub rational_intensity: Option<f64>,
    #[serde(default)]
    pub rational_duration: Option<f64>,
    /// Frequency factor applied to the rational C (Cf).
    #[serde(default = "one")]
    pub frequency_factor: f64,
}

fn one() -> f64 {
    1.0
}

impl RainEvent {
    pub fn new(name: &str, storm: &str, depth: f64) -> Self {
        RainEvent {
            name: name.into(),
            storm: storm.into(),
            depth,
            duration_mode: DurationMode::Default,
            duration: 24.0,
            back_to_back: 1,
            amc: Amc::II,
            rational_intensity: None,
            rational_duration: None,
            frequency_factor: 1.0,
        }
    }

    /// Label used in report headings, e.g. `Type II 24-hr 25-Year Rainfall=4.30"`.
    pub fn heading(&self) -> String {
        let mut s = format!("{} {} Rainfall={:.2}\"", self.storm, self.name, self.depth);
        if self.back_to_back > 1 {
            s.push_str(&format!(" x{}", self.back_to_back));
        }
        s
    }
}

/// A storm resolved for one event: cumulative precipitation in inches at any time.
#[derive(Debug, Clone)]
pub struct Rainfall {
    pub storm: Storm,
    pub depth: f64,
    pub repeats: u32,
}

impl Rainfall {
    pub fn new(storm: &Storm, event: &RainEvent) -> Rainfall {
        let storm = match event.duration_mode {
            DurationMode::Default => storm.clone(),
            mode => storm.adjusted(event.duration, mode),
        };
        Rainfall { storm, depth: event.depth.max(0.0), repeats: event.back_to_back.max(1) }
    }

    pub fn duration(&self) -> f64 {
        self.storm.duration * self.repeats as f64
    }

    pub fn total_depth(&self) -> f64 {
        self.depth * self.repeats as f64
    }

    /// Cumulative precipitation (inches) at `t` hours.
    pub fn depth_at(&self, t: f64) -> f64 {
        if t <= 0.0 || self.storm.duration <= 0.0 {
            return 0.0;
        }
        let d = self.storm.duration;
        let k = (t / d).floor();
        if k >= self.repeats as f64 {
            return self.total_depth();
        }
        self.depth * (k + self.storm.fraction_at(t - k * d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storm(name: &str) -> Storm {
        builtin_storms().into_iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn type_ii_matches_published_points() {
        let s = storm("Type II 24-hr");
        // Values read back at the table knots (smoothing passes through them).
        assert!((s.fraction_at(12.0) - 0.663).abs() < 1e-6);
        assert!((s.fraction_at(11.8) - 0.43079).abs() < 1e-6);
        assert!((s.fraction_at(24.0) - 1.0).abs() < 1e-12);
        // Monotone between knots.
        let mut last = 0.0;
        for i in 0..=2400 {
            let f = s.fraction_at(i as f64 * 0.01);
            assert!(f + 1e-12 >= last, "decrease at {i}");
            last = f;
        }
    }

    #[test]
    fn trimmed_storm_keeps_peak_intensity() {
        let s = storm("Type II 24-hr");
        let six = storm("Type II 6-hr");
        assert!((six.duration - 6.0).abs() < 1e-12);
        let frac24 = s.fraction_at(12.1) - s.fraction_at(11.9);
        let frac6 = six.fraction_at(3.1) - six.fraction_at(2.9);
        let scale = s.fraction_at(15.0) - s.fraction_at(9.0);
        assert!((frac6 * scale - frac24).abs() < 1e-3);
    }

    #[test]
    fn back_to_back_doubles_depth() {
        let s = storm("Type II 24-hr");
        let mut e = RainEvent::new("10-Year", "Type II 24-hr", 3.0);
        e.back_to_back = 2;
        let r = Rainfall::new(&s, &e);
        assert!((r.depth_at(24.0) - 3.0).abs() < 1e-9);
        assert!((r.depth_at(60.0) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn chicago_storm_is_monotone_and_complete() {
        let s = Storm::chicago("Sample", 24.0, 1.5, 0.2, 0.75, None, 0.1).unwrap();
        assert!((s.fraction_at(24.0) - 1.0).abs() < 1e-9);
        assert!((s.fraction_at(12.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bad_inputs_are_rejected() {
        assert!(Storm::from_mass_points("x", "", 1.0, &[0.0, 0.5, 0.2], false).is_none());
        assert!(Storm::from_mass_points("x", "", 0.0, &[0.0, 1.0], false).is_none());
        assert!(Storm::from_intensity_points("x", "", 1.0, &[f64::NAN, 1.0]).is_none());
    }
}
