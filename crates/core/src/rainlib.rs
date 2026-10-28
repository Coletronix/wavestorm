//! Rainfall library files: rainfall distributions (`.hcr`), unit hydrographs (`.huh`), IDF curve
//! files (`.idf`) and county rainfall lookup tables (`*-Rain.txt`).
//!
//! The first two follow the text formats documented by the reference application's own
//! `Rainfall.txt` / `UH.txt` description files (key=value lines, `//` comments). The IDF text
//! format is an assumption of this project (no sample was available); see [`parse_idf`].
//! Everything here is pure text in, data out, so it works on the desktop and in the browser.

use crate::noaa;
use crate::project::{IdfCurve, interpolate_loglog};
use crate::rainfall::{RainEvent, Storm};
use crate::uh::UnitHydrograph;

// ---------------------------------------------------------------------------------------------
// Small text helpers

/// Evaluates a number or a simple arithmetic expression (`+ - * /`, parentheses), such as
/// `1440*0.3` or `863/60`.
pub fn eval_expr(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Ok(v) = t.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn peek(&self) -> Option<u8> {
            self.s.get(self.i).copied()
        }
        fn ws(&mut self) {
            while self.peek().is_some_and(|c| c == b' ' || c == b'\t') {
                self.i += 1;
            }
        }
        fn expr(&mut self, depth: u32) -> Option<f64> {
            let mut v = self.term(depth)?;
            loop {
                self.ws();
                match self.peek() {
                    Some(b'+') => {
                        self.i += 1;
                        v += self.term(depth)?;
                    }
                    Some(b'-') => {
                        self.i += 1;
                        v -= self.term(depth)?;
                    }
                    _ => return Some(v),
                }
            }
        }
        fn term(&mut self, depth: u32) -> Option<f64> {
            let mut v = self.factor(depth)?;
            loop {
                self.ws();
                match self.peek() {
                    Some(b'*') => {
                        self.i += 1;
                        v *= self.factor(depth)?;
                    }
                    Some(b'/') => {
                        self.i += 1;
                        let d = self.factor(depth)?;
                        if d == 0.0 {
                            return None;
                        }
                        v /= d;
                    }
                    _ => return Some(v),
                }
            }
        }
        fn factor(&mut self, depth: u32) -> Option<f64> {
            if depth > 32 {
                return None;
            }
            self.ws();
            match self.peek()? {
                b'-' => {
                    self.i += 1;
                    self.factor(depth + 1).map(|v| -v)
                }
                b'+' => {
                    self.i += 1;
                    self.factor(depth + 1)
                }
                b'(' => {
                    self.i += 1;
                    let v = self.expr(depth + 1)?;
                    self.ws();
                    (self.peek()? == b')').then(|| {
                        self.i += 1;
                        v
                    })
                }
                _ => {
                    let start = self.i;
                    while let Some(c) = self.peek() {
                        let exp_sign = (c == b'+' || c == b'-')
                            && self.i > start
                            && matches!(self.s.get(self.i - 1), Some(b'e' | b'E'));
                        if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || exp_sign {
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    std::str::from_utf8(self.s.get(start..self.i)?).ok()?.parse::<f64>().ok()
                }
            }
        }
    }
    let mut p = P { s: t.as_bytes(), i: 0 };
    let v = p.expr(0)?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

/// Splits `key=value` lines, dropping blank lines and `//` comments (also trailing ones).
fn key_values(text: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_ascii_lowercase();
            if !k.is_empty() {
                out.push((n + 1, k, v.trim().to_string()));
            }
        }
    }
    out
}

fn numbers(value: &str, line: usize) -> Result<Vec<f64>, String> {
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(|t| eval_expr(t).ok_or_else(|| format!("Line {line}: '{t}' is not a number")))
        .collect()
}

fn scalar(value: &str, line: usize) -> Result<f64, String> {
    eval_expr(value).ok_or_else(|| format!("Line {line}: '{value}' is not a number"))
}

fn truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "on")
}

fn time_factor(units: &str) -> f64 {
    let u = units.trim().to_ascii_lowercase();
    if u.starts_with("min") {
        1.0 / 60.0
    } else if u.starts_with("sec") {
        1.0 / 3600.0
    } else {
        1.0
    }
}

fn depth_factor(units: &str) -> f64 {
    if units.trim().to_ascii_lowercase().starts_with("mm") { 1.0 / 25.4 } else { 1.0 }
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

// ---------------------------------------------------------------------------------------------
// Rainfall distributions (.hcr)

/// A parsed rainfall file: one storm per `curve=` section (or a single storm).
#[derive(Debug, Clone, PartialEq)]
pub struct HcrFile {
    pub storms: Vec<Storm>,
    /// The file asks not to be listed (it is a single-event variant of a multi-curve file).
    pub hidden: bool,
}

#[derive(Default, Clone)]
struct Curve {
    name: Option<String>,
    comment: Option<String>,
    duration: Option<f64>,
    peaktime: Option<f64>,
    stormdepth: Option<f64>,
    smoothing: Option<bool>,
    depth: Vec<f64>,
    inten: Vec<f64>,
    incr: Vec<f64>,
    timedepth: Vec<f64>,
    timeincr: Vec<f64>,
    coef: Vec<[f64; 5]>,
    coef1: Option<[f64; 3]>,
}

impl Curve {
    fn has_data(&self) -> bool {
        !(self.depth.is_empty()
            && self.inten.is_empty()
            && self.incr.is_empty()
            && self.timedepth.is_empty()
            && self.timeincr.is_empty()
            && self.coef.is_empty())
            || self.coef1.is_some()
    }
}

/// Parses a rainfall file with one or several curves.
pub fn parse_hcr_file(text: &str) -> Result<HcrFile, String> {
    let mut name = String::new();
    let mut hidden = false;
    let (mut tu, mut du) = (1.0, 1.0);
    let mut global = Curve::default();
    let mut curves: Vec<Curve> = Vec::new();
    let mut cur: Option<Curve> = None;
    for (line, key, value) in key_values(text) {
        match key.as_str() {
            "name" => name = value,
            "timeunits" => tu = time_factor(&value),
            "depthunits" => du = depth_factor(&value),
            "hidden" => hidden = truthy(&value),
            "curve" => {
                curves.extend(cur.take());
                cur = Some(Curve { name: Some(value), ..Curve::default() });
            }
            _ => {
                let c = cur.as_mut().unwrap_or(&mut global);
                match key.as_str() {
                    "comment" => c.comment = Some(value),
                    "duration" => c.duration = Some(scalar(&value, line)?),
                    "peaktime" => c.peaktime = Some(scalar(&value, line)?),
                    "stormdepth" => c.stormdepth = Some(scalar(&value, line)?),
                    "smoothing" => c.smoothing = Some(truthy(&value)),
                    "depth" => c.depth.extend(numbers(&value, line)?),
                    "inten" => c.inten.extend(numbers(&value, line)?),
                    "incr" => c.incr.extend(numbers(&value, line)?),
                    "timedepth" => c.timedepth.extend(numbers(&value, line)?),
                    "timeincr" => c.timeincr.extend(numbers(&value, line)?),
                    "coef" => {
                        let v = numbers(&value, line)?;
                        let seg: [f64; 5] = v
                            .try_into()
                            .map_err(|_| format!("Line {line}: coef needs a time limit and four coefficients"))?;
                        c.coef.push(seg);
                    }
                    "coef1" => {
                        let v = numbers(&value, line)?;
                        let abc: [f64; 3] = v
                            .try_into()
                            .map_err(|_| format!("Line {line}: coef1 needs three coefficients (a, b, c)"))?;
                        c.coef1 = Some(abc);
                    }
                    _ => {}
                }
            }
        }
    }
    curves.extend(cur.take());
    let multi = !curves.is_empty();
    if !multi {
        curves.push(global.clone());
    }
    if name.is_empty() {
        name = "Rainfall".into();
    }
    let mut storms = Vec::new();
    for c in &curves {
        let storm_name = match (&c.name, multi) {
            (Some(n), true) => format!("{name} {n}"),
            _ => name.clone(),
        };
        let eff = Curve {
            comment: c.comment.clone().or_else(|| global.comment.clone()),
            duration: c.duration.or(global.duration),
            peaktime: c.peaktime.or(global.peaktime),
            stormdepth: c.stormdepth.or(global.stormdepth),
            smoothing: c.smoothing.or(global.smoothing),
            ..c.clone()
        };
        if multi && !eff.has_data() {
            continue;
        }
        storms.push(build_storm(&storm_name, &eff, tu, du).map_err(|e| format!("{storm_name}: {e}"))?);
    }
    if storms.is_empty() {
        return Err("The file contains no rainfall data".into());
    }
    Ok(HcrFile { storms, hidden })
}

/// Parses a rainfall file and returns its first storm.
pub fn parse_hcr(text: &str) -> Result<Storm, String> {
    parse_hcr_file(text)?.storms.into_iter().next().ok_or_else(|| "The file contains no rainfall data".to_string())
}

fn finish(mut s: Storm, c: &Curve, du: f64) -> Storm {
    s.comment = c.comment.clone().unwrap_or_default();
    if let Some(d) = c.stormdepth.filter(|d| *d > 0.0) {
        s.default_depth = Some(d * du);
    }
    s
}

// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn build_storm(name: &str, c: &Curve, tu: f64, du: f64) -> Result<Storm, String> {
    let smooth = c.smoothing.unwrap_or(true);
    let comment = c.comment.clone().unwrap_or_default();
    // Mass curve from polynomial segments P = C0 + C1 T + C2 T^2 + C3 T^3 (T in file time units).
    if !c.coef.is_empty() {
        let last = c.coef.last().map_or(0.0, |s| s[0]);
        let duration = c.duration.unwrap_or(last);
        if !(duration > 0.0) {
            return Err("coef segments need a positive duration".into());
        }
        let eval = |t: f64| {
            let seg = c.coef.iter().find(|s| t <= s[0] + 1e-9).or(c.coef.last());
            seg.map_or(0.0, |s| s[1] + t * (s[2] + t * (s[3] + t * s[4])))
        };
        let hours = duration * tu;
        let n = ((hours / 0.01).round() as usize).clamp(240, 4000);
        let mut times = Vec::with_capacity(n + 1);
        let mut vals = Vec::with_capacity(n + 1);
        let mut run = 0.0_f64;
        for i in 0..=n {
            let t = duration * i as f64 / n as f64;
            run = run.max(eval(t).max(0.0));
            times.push(t * tu);
            vals.push(run);
        }
        let end = vals.last().copied().unwrap_or(0.0);
        if !(end > 0.0) {
            return Err("the coefficients give no rainfall".into());
        }
        let fractions: Vec<f64> = vals.iter().map(|v| (v / end).clamp(0.0, 1.0)).collect();
        let mut s = Storm {
            name: name.into(),
            comment,
            duration: hours,
            times,
            fractions,
            smooth: false,
            fixed_duration: true,
            default_depth: None,
            tangents: Vec::new(),
        };
        if let Some(f) = s.fractions.last_mut() {
            *f = 1.0;
        }
        return Ok(finish(s, c, du));
    }
    // IDF-type coefficients ("Chicago" storm): depth = t a / (b + t)^c.
    if let Some([a, b, cc]) = c.coef1 {
        let duration = c.duration.ok_or("coef1 needs a duration")?;
        let peak = c.peaktime.map(|p| p.clamp(0.0, duration));
        let step = duration / 240.0;
        let shape = Storm::chicago(name, duration, if a > 0.0 { a } else { 1.0 }, b, cc, peak, step)
            .ok_or("invalid IDF coefficients")?;
        let default_depth = (a > 0.0).then(|| shape.default_depth.unwrap_or(0.0) * du);
        let s = Storm {
            comment,
            duration: duration * tu,
            times: shape.times.iter().map(|t| t * tu).collect(),
            default_depth,
            ..shape
        };
        return Ok(finish(s, c, du));
    }
    if !c.depth.is_empty() {
        let duration = c.duration.map_or(1.0, |d| d * tu);
        let mut s = Storm::from_mass_points(name, &comment, duration, &c.depth, smooth)
            .ok_or("the mass curve must start at zero and increase")?;
        let first = c.depth.first().copied().unwrap_or(0.0);
        let total = c.depth.last().copied().unwrap_or(1.0) - first;
        if (total - 1.0).abs() > 1e-6 {
            s.default_depth = Some(total * du);
        }
        return Ok(finish(s, c, du));
    }
    if !c.inten.is_empty() {
        let duration = c.duration.map_or(1.0, |d| d * tu);
        let n = c.inten.len();
        if n < 2 || c.inten.iter().any(|v| *v < 0.0) {
            return Err("intensity points must be non-negative, at least two".into());
        }
        let mut s =
            Storm::from_intensity_points(name, &comment, duration, &c.inten).ok_or("invalid intensity points")?;
        // The intensity is linear between points, so the mass curve is piecewise quadratic;
        // a cubic with the exact end tangents reproduces it exactly.
        let sum: f64 = c.inten.windows(2).map(|w| 0.5 * (w[0] + w[1])).sum();
        if sum > 0.0 {
            let step = duration / (n - 1) as f64;
            s.smooth = true;
            s.tangents = s.times.iter().zip(&c.inten).map(|(t, i)| (*t, i / (step * sum))).collect();
        }
        return Ok(finish(s, c, du));
    }
    if !c.incr.is_empty() {
        let duration = c.duration.map_or(1.0, |d| d * tu);
        let mut s = Storm::from_increments(name, &comment, duration, &c.incr, smooth)
            .ok_or("incremental depths must be non-negative with a positive total")?;
        s.default_depth = s.default_depth.map(|d| d * du);
        return Ok(finish(s, c, du));
    }
    if !c.timedepth.is_empty() || !c.timeincr.is_empty() {
        let cumulative = !c.timedepth.is_empty();
        let flat = if cumulative { &c.timedepth } else { &c.timeincr };
        if flat.len() % 2 != 0 || flat.len() < 4 {
            return Err("time/depth data needs at least two (time, depth) pairs".into());
        }
        let mut pts: Vec<(f64, f64)> = Vec::new();
        let mut sum = 0.0;
        for p in flat.chunks(2) {
            let (t, d) = (p[0], p[1]);
            let v = if cumulative {
                d
            } else {
                sum += d;
                sum
            };
            if let Some(last) = pts.last_mut() {
                if t < last.0 {
                    return Err("times must not decrease".into());
                }
                if (t - last.0).abs() < 1e-12 {
                    last.1 = v;
                    continue;
                }
            }
            pts.push((t, v));
        }
        let (t0, v0) = pts.first().copied().unwrap_or((0.0, 0.0));
        let total = pts.last().map_or(0.0, |p| p.1) - v0;
        if pts.len() < 2 || !(total > 0.0) || pts.windows(2).any(|w| w[1].1 < w[0].1) {
            return Err("cumulative depths must increase".into());
        }
        let times: Vec<f64> = pts.iter().map(|p| (p.0 - t0) * tu).collect();
        let fractions: Vec<f64> = pts.iter().map(|p| ((p.1 - v0) / total).clamp(0.0, 1.0)).collect();
        let s = Storm {
            name: name.into(),
            comment,
            duration: times.last().copied().unwrap_or(0.0),
            times,
            fractions,
            smooth,
            fixed_duration: false,
            default_depth: Some(total * du),
            tangents: Vec::new(),
        };
        return Ok(finish(s, c, du));
    }
    Err("no rainfall data".into())
}

/// Writes a storm as a rainfall file (mass curve points, or time/depth pairs for uneven times).
pub fn write_hcr(storm: &Storm) -> String {
    let clean = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = String::from("// WAVEStorm rainfall table\n\n");
    out.push_str(&format!("name={}\n", clean(&storm.name)));
    if !storm.comment.is_empty() {
        out.push_str(&format!("comment={}\n", clean(&storm.comment)));
    }
    out.push_str("timeunits=hours\ndepthunits=inches\n");
    out.push_str(&format!("duration={}\n", fmt(storm.duration)));
    if let Some(d) = storm.default_depth {
        out.push_str(&format!("stormdepth={}\n", fmt(d)));
    }
    if !storm.smooth {
        out.push_str("smoothing=false\n");
    }
    out.push('\n');
    let n = storm.times.len();
    let even = n >= 2
        && storm
            .times
            .iter()
            .enumerate()
            .all(|(i, t)| (t - storm.duration * i as f64 / (n - 1) as f64).abs() < 1e-9 * storm.duration.max(1.0));
    let peak = storm.tangents.iter().map(|t| t.1).fold(0.0_f64, f64::max);
    if even && storm.smooth && storm.tangents.len() == n && peak > 0.0 {
        // Storms read from intensity points carry exact tangents: write them back as intensities.
        for row in storm.tangents.chunks(10) {
            let cells: Vec<String> = row.iter().map(|t| fmt(t.1 / peak)).collect();
            out.push_str(&format!("inten= {}\n", cells.join(" ")));
        }
    } else if even {
        for row in storm.fractions.chunks(10) {
            let cells: Vec<String> = row.iter().map(|v| fmt(*v)).collect();
            out.push_str(&format!("depth= {}\n", cells.join(" ")));
        }
    } else {
        for (t, f) in storm.times.iter().zip(&storm.fractions) {
            out.push_str(&format!("timedepth= {} {}\n", fmt(*t), fmt(*f)));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Unit hydrographs (.huh)

/// Parses a unit hydrograph file: `inten=` ordinates at even spacing, first and last zero. The
/// time axis is made dimensionless by placing the (first) peak at t/Tp = 1.
pub fn parse_huh(text: &str) -> Result<UnitHydrograph, String> {
    let mut name = String::from("Unit Hydrograph");
    let mut comment = String::new();
    let mut q: Vec<f64> = Vec::new();
    for (line, key, value) in key_values(text) {
        match key.as_str() {
            "name" => name = value,
            "comment" => comment = value,
            "inten" => q.extend(numbers(&value, line)?),
            _ => {}
        }
    }
    if q.len() < 3 {
        return Err("A unit hydrograph needs at least three ordinates".into());
    }
    if q.iter().any(|v| *v < 0.0) {
        return Err("Unit hydrograph ordinates must not be negative".into());
    }
    let (mut peak_i, mut peak) = (0, 0.0);
    for (i, v) in q.iter().enumerate() {
        if *v > peak * (1.0 + 1e-12) {
            peak_i = i;
            peak = *v;
        }
    }
    if peak <= 0.0 || peak_i == 0 {
        return Err("The unit hydrograph must rise to a peak after time zero".into());
    }
    let t: Vec<f64> = (0..q.len()).map(|i| i as f64 / peak_i as f64).collect();
    let q = q.iter().map(|v| v / peak).collect();
    Ok(UnitHydrograph { name, comment, t, q })
}

/// Writes a unit hydrograph at an even t/Tp spacing that places the peak on an ordinate.
pub fn write_huh(uh: &UnitHydrograph) -> String {
    let clean = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = String::from("// WAVEStorm unit hydrograph table\n\n");
    out.push_str(&format!("name={}\n", clean(&uh.name)));
    if !uh.comment.is_empty() {
        out.push_str(&format!("comment={}\n", clean(&uh.comment)));
    }
    let base = uh.t.last().copied().unwrap_or(0.0);
    let step = 0.05;
    let n = (base / step).round() as usize;
    let mut vals: Vec<f64> = (0..=n).map(|i| uh.ordinate(i as f64 * step)).collect();
    if let Some(first) = vals.first_mut() {
        *first = 0.0;
    }
    if let Some(last) = vals.last_mut() {
        *last = 0.0;
    }
    for row in vals.chunks(10) {
        let cells: Vec<String> = row.iter().map(|v| format!("{v:.4}")).collect();
        out.push_str(&format!("inten= {}\n", cells.join(" ")));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// IDF curve files (.hci; also NOAA Atlas 14 tables and WAVEStorm's earlier .idf layout)

/// How one IDF event gives intensity against duration.
#[derive(Debug, Clone, PartialEq)]
pub enum IdfShape {
    /// i = a / (b + t)^c, t in minutes, i in inches per hour.
    Coef { a: f64, b: f64, c: f64 },
    /// (duration minutes, intensity in/hr) points, interpolated log-log.
    Table(Vec<(f64, f64)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdfEvent {
    pub name: String,
    pub shape: IdfShape,
    /// Frequency factor applied to runoff coefficients for this event (Rational method).
    pub factor: f64,
}

impl IdfEvent {
    /// Intensity (in/hr) at a duration in minutes.
    pub fn intensity(&self, minutes: f64) -> Option<f64> {
        match &self.shape {
            IdfShape::Coef { a, b, c } => {
                let d = (b + minutes).powf(*c);
                (d > 0.0 && d.is_finite()).then(|| a / d)
            }
            IdfShape::Table(pts) => interpolate_loglog(pts, minutes),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdfFile {
    pub name: String,
    pub comment: String,
    pub events: Vec<IdfEvent>,
}

impl IdfFile {
    /// The file form of a project IDF curve (for saving it).
    pub fn from_curve(c: &IdfCurve) -> IdfFile {
        let mut events: Vec<IdfEvent> = c
            .events
            .iter()
            .map(|(n, a, b, cc)| IdfEvent {
                name: n.clone(),
                shape: IdfShape::Coef { a: *a, b: *b, c: *cc },
                factor: 1.0,
            })
            .collect();
        events.extend(c.tables.iter().map(|(n, p)| IdfEvent {
            name: n.clone(),
            shape: IdfShape::Table(p.clone()),
            factor: 1.0,
        }));
        IdfFile { name: c.name.clone(), comment: String::new(), events }
    }

    /// Converts to the project's IDF curve (tables and coefficient sets).
    pub fn to_curve(&self) -> IdfCurve {
        let mut c = IdfCurve { name: self.name.clone(), events: Vec::new(), tables: Vec::new() };
        for e in &self.events {
            match &e.shape {
                IdfShape::Coef { a, b, c: cc } => c.events.push((e.name.clone(), *a, *b, *cc)),
                IdfShape::Table(p) => c.tables.push((e.name.clone(), p.clone())),
            }
        }
        c
    }

    /// Rainfall events for a storm of `hours` duration: depth = intensity(duration) x duration,
    /// one event per IDF event, named with `suffix` appended.
    // `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn events_for(&self, storm: &str, hours: f64, suffix: &str) -> Vec<RainEvent> {
        self.events
            .iter()
            .filter_map(|e| {
                let i = e.intensity(hours * 60.0)?;
                let depth = (i * hours * 100.0).round() / 100.0;
                if !(depth > 0.0) {
                    return None;
                }
                let mut ev = RainEvent::new(&format!("{}{suffix}", e.name), storm, depth);
                ev.duration = hours;
                ev.frequency_factor = e.factor;
                Some(ev)
            })
            .collect()
    }
}

/// An IDF file from a NOAA Atlas 14 table: one event per return period, intensity = depth /
/// duration at each tabulated duration.
pub fn idf_from_noaa(table: &noaa::PfTable) -> Result<IdfFile, String> {
    let events = table
        .aris
        .iter()
        .enumerate()
        .map(|(i, ari)| {
            let pts: Vec<(f64, f64)> =
                table.rows.iter().filter_map(|r| table.depth(r, i).map(|d| (r.hours * 60.0, d / r.hours))).collect();
            IdfEvent { name: noaa::event_name(*ari), shape: IdfShape::Table(pts), factor: 1.0 }
        })
        .filter(|e| matches!(&e.shape, IdfShape::Table(p) if p.len() >= 2))
        .collect::<Vec<_>>();
    if events.is_empty() {
        return Err("No events present in the NOAA table".into());
    }
    Ok(IdfFile { name: "NOAA Atlas 14".into(), comment: table.source.clone(), events })
}

/// Parses an IDF definition. Three inputs are understood:
///
/// * The `.hci` IDF file layout published with the reference application's IDF documentation
///   (public "IDF.txt" and sample files, checked against eleven of them): `NAME=`, `COMMENT=`,
///   `DEPTHUNITS=` (INCHES default, MM), `TIMEUNITS=` (the intensity's time base: HOURS
///   default, MINUTES, SECONDS), `DURATUNITS=` (MINUTES default), one `DURATION=` line of
///   durations, then per event `EVENT=`, an optional `Cf=` frequency factor and one of
///   `INTEN=` (one intensity per duration; `2x` = twice the first INTEN line), `DEPTH=` (one
///   depth per duration), `coef1= a b c` (I = a/(b+t)^c), `coef2= a b c` (I = a/(b+t^c)) or
///   `coef3= a b c d` (I = a + bx + cx² + dx³, x = ln t). Values may be simple expressions
///   (`24*60`, `2.5/3`); keys are case-insensitive; `//` starts a comment. Intensities are
///   interpolated log-log between durations and held at the first/last duration outside them;
///   coefficient events are evaluated over the DURATION range (first to last value) and held
///   beyond it, so they are sampled into a dense table.
/// * A NOAA Atlas 14 precipitation frequency CSV (current or pre-2012 layout; depth or
///   intensity), which the same program reads when saved with an `.hci` name.
/// * WAVEStorm's own earlier `.idf` layout (`curve=`, `factor=`, `coef=`, `idf=` pairs).
pub fn parse_idf(text: &str) -> Result<IdfFile, String> {
    let upper = text.to_ascii_uppercase();
    if upper.contains("PRECIPITATION FREQUENCY ESTIMATES") || upper.contains("ERRORMSG") {
        return idf_from_noaa(&noaa::parse_pfds(text)?);
    }
    let keys = key_values(text);
    let has = |k: &str| keys.iter().any(|(_, key, _)| key == k);
    if !has("duration") && (has("curve") || has("idf") || has("coef")) {
        return parse_legacy_idf(text);
    }
    parse_hci(&keys)
}

/// One event of an `.hci` file as written.
enum HciData {
    Inten(Vec<f64>),
    Multiple(f64),
    Depth(Vec<f64>),
    Coef1([f64; 3]),
    Coef2([f64; 3]),
    Coef3([f64; 4]),
}

/// Number of samples a coefficient event is tabulated at over its duration range (log
/// spaced: log-log interpolation between them is within about 0.01 % of the equations).
const COEF_SAMPLES: usize = 64;

fn parse_hci(keys: &[(usize, String, String)]) -> Result<IdfFile, String> {
    let (mut name, mut comment) = (String::from("IDF"), String::new());
    let (mut depth_k, mut per_hour, mut dur_min) = (1.0_f64, 1.0_f64, 1.0_f64);
    let mut durations: Option<(usize, Vec<f64>)> = None;
    let mut events: Vec<(usize, String, f64, Option<HciData>)> = Vec::new();
    let unit_word = |v: &str| v.trim().to_ascii_lowercase();
    for (line, key, value) in keys {
        let line = *line;
        let set = |events: &mut Vec<(usize, String, f64, Option<HciData>)>, d: HciData| -> Result<(), String> {
            let Some(e) = events.last_mut() else { return Err(format!("Line {line}: data before any EVENT= line")) };
            if e.3.is_some() {
                return Err(format!("Line {line}: event {} already has its data", e.1));
            }
            e.3 = Some(d);
            Ok(())
        };
        match key.as_str() {
            "name" => name = value.trim().to_string(),
            "comment" => comment = value.trim().to_string(),
            "depthunits" => depth_k = depth_factor(value),
            // Intensity is depth per this time unit: convert to per hour.
            "timeunits" => {
                let u = unit_word(value);
                per_hour = if u.starts_with("min") {
                    60.0
                } else if u.starts_with("sec") {
                    3600.0
                } else {
                    1.0
                };
            }
            // Durations are in this unit: convert to minutes.
            "duratunits" => {
                let u = unit_word(value);
                dur_min = if u.starts_with("hour") || u == "hr" || u == "hrs" {
                    60.0
                } else if u.starts_with("sec") {
                    1.0 / 60.0
                } else {
                    1.0
                };
            }
            "duration" => durations = Some((line, numbers(value, line)?)),
            // A doubled "==" (seen in published files) and trailing spaces are tolerated.
            "event" => events.push((line, value.trim_start_matches('=').trim().to_string(), 1.0, None)),
            "cf" => {
                if let Some(e) = events.last_mut() {
                    e.2 = scalar(value, line)?;
                }
            }
            "inten" => {
                let v = value.trim();
                let d = match v.strip_suffix(['x', 'X']).and_then(|m| eval_expr(m.trim())) {
                    Some(m) => HciData::Multiple(m),
                    None => HciData::Inten(numbers(v, line)?),
                };
                set(&mut events, d)?;
            }
            "depth" => set(&mut events, HciData::Depth(numbers(value, line)?))?,
            "coef1" | "coef2" => {
                let abc: [f64; 3] = numbers(value, line)?
                    .try_into()
                    .map_err(|_| format!("Line {line}: {key} needs three coefficients (a b c)"))?;
                set(&mut events, if key == "coef1" { HciData::Coef1(abc) } else { HciData::Coef2(abc) })?;
            }
            "coef3" => {
                let abcd: [f64; 4] = numbers(value, line)?
                    .try_into()
                    .map_err(|_| format!("Line {line}: coef3 needs four coefficients (a b c d)"))?;
                set(&mut events, HciData::Coef3(abcd))?;
            }
            _ => {}
        }
    }
    let Some((dline, raw)) = durations else { return Err("The IDF file has no DURATION= line".into()) };
    if raw.is_empty() || raw.iter().any(|d| !(d.is_finite() && *d > 0.0)) {
        return Err(format!("Line {dline}: durations must be positive numbers"));
    }
    let minutes: Vec<f64> = raw.iter().map(|d| d * dur_min).collect();
    // Intensity in the file's units → inches per hour.
    let k = depth_k * per_hour;
    let first_inten = events.iter().find_map(|e| match &e.3 {
        Some(HciData::Inten(v)) => Some(v.clone()),
        _ => None,
    });
    let mut out = Vec::new();
    for (line, ev, cf, data) in events {
        let per_duration = |v: &[f64], what: &str| -> Result<(), String> {
            if v.len() == raw.len() {
                Ok(())
            } else {
                Err(format!("Line {line}: event {ev} has {} {what} values for {} durations", v.len(), raw.len()))
            }
        };
        let pts: Vec<(f64, f64)> = match data {
            None => return Err(format!("Line {line}: event {ev} has no INTEN, DEPTH or coefficient line")),
            Some(HciData::Inten(v)) => {
                per_duration(&v, "intensity")?;
                minutes.iter().zip(&v).map(|(t, i)| (*t, i * k)).collect()
            }
            Some(HciData::Multiple(m)) => {
                let base =
                    first_inten.as_ref().ok_or(format!("Line {line}: INTEN={m}x needs an earlier INTEN line"))?;
                minutes.iter().zip(base).map(|(t, i)| (*t, i * m * k)).collect()
            }
            // Depth over the duration (in the time units the intensity uses).
            Some(HciData::Depth(v)) => {
                per_duration(&v, "depth")?;
                minutes.iter().zip(&v).map(|(t, d)| (*t, d * depth_k / (t / 60.0))).collect()
            }
            Some(c @ (HciData::Coef1(_) | HciData::Coef2(_) | HciData::Coef3(_))) => {
                let (lo, hi) = (raw.first().copied().unwrap_or(1.0), raw.last().copied().unwrap_or(1.0));
                let (lo, hi) = (lo.min(hi), lo.max(hi));
                let n = if hi > lo { COEF_SAMPLES } else { 1 };
                (0..n)
                    .map(|j| {
                        let f = if n > 1 { j as f64 / (n - 1) as f64 } else { 0.0 };
                        let t = lo * (hi / lo).powf(f);
                        let i = match &c {
                            HciData::Coef1([a, b, cc]) => a / (b + t).powf(*cc),
                            HciData::Coef2([a, b, cc]) => a / (b + t.powf(*cc)),
                            HciData::Coef3([a, b, cc, d]) => {
                                let x = t.ln();
                                a + b * x + cc * x * x + d * x * x * x
                            }
                            _ => f64::NAN,
                        };
                        (t * dur_min, i * k)
                    })
                    .collect()
            }
        };
        let mut pts = pts;
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if pts.iter().any(|p| !(p.1.is_finite() && p.1 > 0.0)) {
            return Err(format!("Event {ev}: intensities must be positive (check the values or coefficients)"));
        }
        out.push(IdfEvent { name: ev, shape: IdfShape::Table(pts), factor: if cf > 0.0 { cf } else { 1.0 } });
    }
    if out.is_empty() {
        return Err("No events present in the IDF file".into());
    }
    Ok(IdfFile { name, comment, events: out })
}

/// WAVEStorm's earlier `.idf` layout (before `.hci` support), still read so those files open:
/// `name=`, `comment=`, `timeunits=`, `depthunits=`, then `curve=` events with `factor=`,
/// `coef= a b c` or `idf= t1 i1 t2 i2 ...` pairs.
fn parse_legacy_idf(text: &str) -> Result<IdfFile, String> {
    let mut name = String::from("IDF");
    let mut comment = String::new();
    let (mut tu, mut du) = (1.0_f64, 1.0_f64);
    struct Ev {
        name: String,
        factor: f64,
        coef: Option<[f64; 3]>,
        pts: Vec<f64>,
    }
    let mut evs: Vec<Ev> = Vec::new();
    for (line, key, value) in key_values(text) {
        match key.as_str() {
            "name" => name = value,
            "comment" => comment = value,
            "timeunits" => {
                tu = if value.trim().to_ascii_lowercase().starts_with("hour") || value.trim().eq_ignore_ascii_case("hr")
                {
                    60.0
                } else if value.trim().to_ascii_lowercase().starts_with("sec") {
                    1.0 / 60.0
                } else {
                    1.0
                }
            }
            "depthunits" => du = 1.0 / depth_factor(&value),
            "curve" | "event" => evs.push(Ev { name: value, factor: 1.0, coef: None, pts: Vec::new() }),
            "factor" | "freqfactor" | "frequencyfactor" => {
                if let Some(e) = evs.last_mut() {
                    e.factor = scalar(&value, line)?;
                }
            }
            "coef" | "coef1" => {
                let abc: [f64; 3] = numbers(&value, line)?
                    .try_into()
                    .map_err(|_| format!("Line {line}: coef needs three coefficients (a, b, c)"))?;
                if evs.is_empty() {
                    evs.push(Ev { name: name.clone(), factor: 1.0, coef: None, pts: Vec::new() });
                }
                if let Some(e) = evs.last_mut() {
                    e.coef = Some(abc);
                }
            }
            "idf" => {
                let v = numbers(&value, line)?;
                if evs.is_empty() {
                    evs.push(Ev { name: name.clone(), factor: 1.0, coef: None, pts: Vec::new() });
                }
                if let Some(e) = evs.last_mut() {
                    e.pts.extend(v);
                }
            }
            _ => {}
        }
    }
    // File units: t in `tu` minutes per unit, intensity in `du`-scaled inches per hour.
    let mut events = Vec::new();
    for e in evs {
        let shape = if let Some([a, b, c]) = e.coef {
            // i(t_min) = a'/(b' + t_min)^c' with t = t_min / tu.
            IdfShape::Coef { a: a / du * tu.powf(c), b: b * tu, c }
        } else if e.pts.len() >= 4 && e.pts.len() % 2 == 0 {
            let mut pts: Vec<(f64, f64)> = e.pts.chunks(2).map(|p| (p[0] * tu, p[1] / du)).collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            if pts.iter().any(|p| !(p.0 > 0.0 && p.1 > 0.0)) {
                return Err(format!("Event {}: durations and intensities must be positive", e.name));
            }
            IdfShape::Table(pts)
        } else {
            return Err(format!("Event {} has no coefficients or at least two idf points", e.name));
        };
        events.push(IdfEvent { name: e.name, shape, factor: if e.factor > 0.0 { e.factor } else { 1.0 } });
    }
    if events.is_empty() {
        return Err("No events present in the IDF file".into());
    }
    Ok(IdfFile { name, comment, events })
}

/// Writes an `.hci` IDF file (the layout [`parse_idf`] reads): inches per hour against
/// minutes. All events share one DURATION line (the union of their tabulated durations, or
/// 5 minutes to 24 hours when every event is a coefficient set); table events are written as
/// INTEN values at those durations, coefficient events as `coef1`.
pub fn write_idf(idf: &IdfFile) -> String {
    let clean = |s: &str| s.replace(['\r', '\n'], " ");
    let mut durations: Vec<f64> = idf
        .events
        .iter()
        .flat_map(|e| match &e.shape {
            IdfShape::Table(p) => p.iter().map(|q| q.0).collect(),
            IdfShape::Coef { .. } => Vec::new(),
        })
        .filter(|t| t.is_finite() && *t > 0.0)
        .collect();
    durations.sort_by(f64::total_cmp);
    durations.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if durations.is_empty() {
        durations = vec![5.0, 24.0 * 60.0];
    }
    // "90" reads better as "1.5*60"; whole hours as "2*60".
    let dur = |t: f64| if t >= 60.0 && (t % 60.0).abs() < 1e-9 { format!("{}*60", fmt(t / 60.0)) } else { fmt(t) };
    let mut out = String::from("// IDF curve file written by WAVEStorm\n\n");
    out.push_str(&format!("name={}\n", clean(&idf.name)));
    if !idf.comment.is_empty() {
        out.push_str(&format!("comment={}\n", clean(&idf.comment)));
    }
    out.push_str("\ndepthunits=inches\ntimeunits=hours\nduratunits=minutes\n\n");
    out.push_str(&format!("duration={}\n", durations.iter().map(|t| dur(*t)).collect::<Vec<_>>().join(" ")));
    for e in &idf.events {
        out.push_str(&format!("\nevent={}\n", clean(&e.name)));
        if (e.factor - 1.0).abs() > 1e-12 {
            out.push_str(&format!("Cf={}\n", fmt(e.factor)));
        }
        match &e.shape {
            IdfShape::Coef { a, b, c } => out.push_str(&format!("coef1= {} {} {}\n", fmt(*a), fmt(*b), fmt(*c))),
            IdfShape::Table(_) => {
                let cells: Vec<String> =
                    durations.iter().map(|t| e.intensity(*t).map_or_else(|| "0".into(), fmt)).collect();
                out.push_str(&format!("inten={}\n", cells.join(" ")));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// County rainfall lookup tables (*-Rain.txt)

#[derive(Debug, Clone, PartialEq)]
pub struct RainRow {
    pub id: String,
    pub state: String,
    pub county: String,
    /// Storm distribution code as written in the table ("II", "MSE3", "NOAA_C"...); may be empty.
    pub storm: String,
    /// Depths (inches) by column; zero means "no data".
    pub depths: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RainTable {
    pub name: String,
    /// Depth column headings such as "10-Year" or "100-Year-2050".
    pub columns: Vec<String>,
    pub rows: Vec<RainRow>,
}

/// Splits CSV text into records, honouring double quotes.
fn csv_records(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut rec: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let end_record = |rec: &mut Vec<String>, field: &mut String, records: &mut Vec<Vec<String>>| {
        rec.push(field.trim().to_string());
        field.clear();
        if rec.iter().any(|f| !f.is_empty()) {
            records.push(std::mem::take(rec));
        } else {
            rec.clear();
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                rec.push(field.trim().to_string());
                field.clear();
            }
            '\r' if !quoted => {}
            '\n' if !quoted => end_record(&mut rec, &mut field, &mut records),
            _ => field.push(ch),
        }
    }
    end_record(&mut rec, &mut field, &mut records);
    records
}

/// Parses a lookup table: header `ID,State,County,Storm,<depth columns>` then one row per county.
pub fn parse_rain_table(name: &str, csv: &str) -> Result<RainTable, String> {
    let mut records = csv_records(csv).into_iter();
    let header = records.next().ok_or("The table is empty")?;
    let lower: Vec<String> = header.iter().map(|h| h.to_ascii_lowercase()).collect();
    if lower.len() < 5 || lower[1] != "state" || lower[2] != "county" || lower[3] != "storm" {
        return Err("Not a rainfall lookup table (expected ID,State,County,Storm,... columns)".into());
    }
    let columns: Vec<String> = header.iter().skip(4).cloned().collect();
    let mut rows = Vec::new();
    for r in records {
        if r.len() < 5 {
            continue;
        }
        let depths: Vec<f64> = r.iter().skip(4).map(|v| v.parse::<f64>().unwrap_or(0.0)).collect();
        rows.push(RainRow { id: r[0].clone(), state: r[1].clone(), county: r[2].clone(), storm: r[3].clone(), depths });
    }
    if rows.is_empty() {
        return Err("The table has no rows".into());
    }
    Ok(RainTable { name: name.to_string(), columns, rows })
}

impl RainRow {
    /// The storm codes offered for this row (some tables list several, comma separated).
    pub fn storm_codes(&self) -> Vec<&str> {
        self.storm.split(',').map(str::trim).filter(|c| !c.is_empty()).collect()
    }
}

impl RainTable {
    /// States present, in file order.
    pub fn states(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for r in &self.rows {
            if !v.contains(&r.state) {
                v.push(r.state.clone());
            }
        }
        v
    }

    /// Rows for a state (case-insensitive).
    pub fn counties(&self, state: &str) -> Vec<&RainRow> {
        self.rows.iter().filter(|r| r.state.eq_ignore_ascii_case(state)).collect()
    }

    /// Row for a state and county name.
    pub fn lookup(&self, state: &str, county: &str) -> Option<&RainRow> {
        self.rows.iter().find(|r| r.state.eq_ignore_ascii_case(state) && r.county.eq_ignore_ascii_case(county))
    }

    /// Events for the columns that hold data in `row`, using the given storm name. Event names
    /// are the column headings ("10-Year").
    pub fn events(&self, row: &RainRow, storm: &str) -> Vec<RainEvent> {
        self.columns
            .iter()
            .zip(&row.depths)
            .filter(|(_, d)| **d > 0.0)
            .map(|(c, d)| RainEvent::new(c, storm, *d))
            .collect()
    }
}

fn is_duration_token(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    let l = l.trim_end_matches('s');
    ["-hr", "-min", "-day", "hr", "min", "day"].iter().any(|u| l.ends_with(u))
        && l.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn storm_key(name: &str) -> String {
    name.split_whitespace()
        .filter(|t| !is_duration_token(t))
        .collect::<String>()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Finds the storm in `available` that a lookup table code (such as "II", "MSE3", "NOAA_C",
/// "N10_D", "CA-4", "2_ND") refers to.
pub fn storm_for_code(code: &str, available: &[String]) -> Option<String> {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    if c.is_empty() {
        return None;
    }
    if matches!(c.as_str(), "I" | "IA" | "II" | "III") {
        let want = format!("Type {c} 24-hr");
        return available.iter().find(|n| n.eq_ignore_ascii_case(&want)).cloned();
    }
    let mut targets = vec![c.clone()];
    let rest = |p: &str| c.strip_prefix(p).map(str::to_string);
    if let Some(r) = rest("N10") {
        targets.push(format!("NOAA10{r}"));
    }
    if c.len() == 3 {
        if let Some(r) = rest("NO") {
            targets.push(format!("NOAA{r}"));
        }
        if let Some(r) = rest("NR") {
            targets.push(format!("NRCC{r}"));
        }
        if let Some(r) = rest("NV") {
            targets.push(format!("NEVADA{r}"));
        }
    }
    if let Some(r) = rest("NV2") {
        targets.push(format!("NEVADA2{r}"));
    }
    if c == "NPAC" {
        targets.push("NOPACIFIC".into());
    }
    if let Some(d) = c.strip_suffix("ND").filter(|d| !d.is_empty()) {
        targets.push(format!("ND{d}"));
    }
    for t in &targets {
        if let Some(n) = available.iter().find(|n| storm_key(n) == *t) {
            return Some(n.clone());
        }
    }
    for t in &targets {
        if let Some(n) = available.iter().find(|n| storm_key(n).starts_with(t.as_str())) {
            return Some(n.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// A loaded collection of library files

/// Everything loaded from a rainfall library folder or imported files.
#[derive(Debug, Clone, Default)]
pub struct RainLibrary {
    pub storms: Vec<Storm>,
    pub uhs: Vec<UnitHydrograph>,
    pub idfs: Vec<IdfFile>,
    pub tables: Vec<RainTable>,
    /// Files that could not be read, with the reason.
    pub problems: Vec<String>,
}

impl RainLibrary {
    pub fn is_empty(&self) -> bool {
        self.storms.is_empty() && self.uhs.is_empty() && self.idfs.is_empty() && self.tables.is_empty()
    }

    /// Adds one file by its name (extension decides the kind). Unknown kinds are ignored.
    /// Returns true when the file was recognised and read.
    pub fn add_file(&mut self, file_name: &str, bytes: &[u8]) -> bool {
        let text = String::from_utf8_lossy(bytes);
        let lower = file_name.to_ascii_lowercase();
        let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
        let mut note = |e: String| self.problems.push(format!("{file_name}: {e}"));
        let mut result = Ok(());
        if lower.ends_with(".hcr") {
            match parse_hcr_file(&text) {
                Ok(f) if !f.hidden => {
                    for s in f.storms {
                        if !self.storms.iter().any(|x| x.name == s.name) {
                            self.storms.push(s);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => result = Err(e),
            }
        } else if lower.ends_with(".huh") {
            match parse_huh(&text) {
                Ok(u) => {
                    if !self.uhs.iter().any(|x| x.name == u.name) {
                        self.uhs.push(u);
                    }
                }
                Err(e) => result = Err(e),
            }
        } else if lower.ends_with(".hci") || lower.ends_with(".idf") {
            match parse_idf(&text) {
                Ok(mut i) => {
                    if i.name == "IDF" || i.name == "NOAA Atlas 14" {
                        i.name = stem.to_string();
                    }
                    self.idfs.push(i);
                }
                Err(e) => result = Err(e),
            }
        } else if lower.ends_with("-rain.txt") {
            match parse_rain_table(stem, &text) {
                Ok(t) => self.tables.push(t),
                Err(e) => result = Err(e),
            }
        } else {
            return false;
        }
        if let Err(e) = result {
            note(e);
            return false;
        }
        true
    }

    pub fn storm_names(&self) -> Vec<String> {
        self.storms.iter().map(|s| s.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rainfall::builtin_storms;

    const TYPE_II: &str = "// test fixture: SCS Type II coefficients (public NRCS TR-55)
name=Type II 24-hr
timeunits=hours
duration=24
comment=SCS coefficients
coef= 4.00      0.0000    0.0100   0.00050    0
coef= 8.00      0.0080    0.0060   0.00100    0
coef= 9.00      0.2640    -.0580   0.00500    0
coef= 9.50      -.1410    0.0320   0.00000    0
coef=10.00      0.5810    -.1200   0.00800    0
coef=10.50      0.9810    -.2000   0.01200    0
coef=11.00      1.8630    -.3680   0.02000    0
coef=11.50      5.2510    -.9840   0.04800    0
coef=11.75    155.4870  -27.1120   1.18400    0
coef=11.903   446.7068  -76.68133  3.293333   0
coef=12.00  -1081.0290  180.0810  -7.49500    0
coef=12.50    -18.1650    2.9370   -.11400    0
coef=13.00     -4.4150    0.7370   -.02600    0
coef=13.50     -2.3870    0.4250   -.01400    0
coef=14.00     -1.6580    0.3170   -.01000    0
coef=16.00      -.3840    0.1350   -.00350    0
coef=20.00      0.1920    0.0630   -.00125    0
coef=24.00      0.5920    0.0230   -.00025    0
";

    fn monotone_ends_at_one(s: &Storm) {
        assert!(s.times.len() >= 2 && s.times.len() == s.fractions.len(), "{}", s.name);
        assert!(s.times.windows(2).all(|w| w[1] > w[0]), "{} times", s.name);
        assert!(s.fractions.windows(2).all(|w| w[1] >= w[0] - 1e-12), "{} mass", s.name);
        assert_eq!(s.fractions.first().copied(), Some(0.0), "{}", s.name);
        assert!((s.fractions.last().copied().unwrap_or(0.0) - 1.0).abs() < 1e-9, "{}", s.name);
        assert!((s.times.last().copied().unwrap_or(0.0) - s.duration).abs() < 1e-9, "{}", s.name);
    }

    #[test]
    fn expressions() {
        assert_eq!(eval_expr("1440*0.3"), Some(432.0));
        assert!((eval_expr("863/60").unwrap() - 14.3833333).abs() < 1e-6);
        assert_eq!(eval_expr("-.0580"), Some(-0.058));
        assert_eq!(eval_expr("(1+2)*3-1"), Some(8.0));
        assert_eq!(eval_expr("1/0"), None);
        assert_eq!(eval_expr("abc"), None);
        assert_eq!(eval_expr("2*"), None);
    }

    #[test]
    fn coefficient_storm_matches_builtin_type_ii() {
        let s = parse_hcr(TYPE_II).unwrap();
        monotone_ends_at_one(&s);
        assert_eq!(s.duration, 24.0);
        let b = builtin_storms().into_iter().find(|b| b.name == "Type II 24-hr").unwrap();
        for h in 0..=24 {
            let (x, y) = (s.fraction_at(h as f64), b.fraction_at(h as f64));
            assert!((x - y).abs() < 0.01, "hour {h}: {x} vs {y}");
        }
        for t in [11.0, 11.5, 11.9, 12.0, 12.1, 12.5] {
            assert!((s.fraction_at(t) - b.fraction_at(t)).abs() < 0.01, "t {t}");
        }
    }

    #[test]
    fn mass_intensity_and_incremental_forms() {
        let mass = "name=B\nduration=1\nstormdepth=2.5\ndepth= 0 .02 .08 .2\ndepth= .41 .625 .805 .915 .985 .995 1.0\n";
        let s = parse_hcr(mass).unwrap();
        monotone_ends_at_one(&s);
        assert_eq!(s.default_depth, Some(2.5));
        assert!(s.smooth);
        let raw = parse_hcr(&format!("smoothing=false\n{mass}")).unwrap();
        assert!(!raw.smooth);

        let inten = "name=A\nduration=1\nstormdepth=2.5\ninten=0 .2 .6 1.2 2.1 2.15 1.8 1.1 .7 .1 .05\n";
        let a = parse_hcr(inten).unwrap();
        monotone_ends_at_one(&a);
        // Trapezoid sum of the points: 0.2+... ; fraction at the end of step 2 matches the quadratic.
        let sum: f64 =
            [0.0, 0.2, 0.6, 1.2, 2.1, 2.15, 1.8, 1.1, 0.7, 0.1, 0.05].windows(2).map(|w| 0.5 * (w[0] + w[1])).sum();
        let mid = 0.05;
        // Linear intensity 0 -> .2 over 0.1 h: mass(t) = 0.1*0.2*(t/0.1)^2/2 in step units
        let expect = (0.1 * 0.2 * (mid / 0.1_f64).powi(2) / 2.0) / (0.1 * sum);
        assert!((a.fraction_at(mid) - expect).abs() < 1e-6, "{} vs {}", a.fraction_at(mid), expect);

        let incr = "name=E\nsmoothing=false\nduration=1\nincr= .05 .15 .3 .525 .5375\nincr= .45 .275 .175 .025 .0125\n";
        let e = parse_hcr(incr).unwrap();
        monotone_ends_at_one(&e);
        assert!((e.default_depth.unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn uneven_times_and_units() {
        let g = "name=G\ntimedepth= 2 0\ntimedepth= 6 .25\ntimedepth= 9 .6\ntimedepth= 26 2.7\n";
        let s = parse_hcr(g).unwrap();
        monotone_ends_at_one(&s);
        assert_eq!(s.duration, 24.0, "times are offset to start at zero");
        assert_eq!(s.default_depth, Some(2.7));
        let f = "name=F\ntimeincr= 0 0\ntimeincr= 4 .25\ntimeincr= 7 .35\n";
        let s = parse_hcr(f).unwrap();
        assert!((s.fractions[1] - 0.25 / 0.6).abs() < 1e-9);
        let mm = "name=M\ntimeunits=minutes\ndepthunits=mm\nduration=60\nstormdepth=25.4\ndepth=0 .5 1\n";
        let s = parse_hcr(mm).unwrap();
        assert_eq!(s.duration, 1.0);
        assert!((s.default_depth.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn chicago_coefficients_and_curves() {
        let text = "name=D 24-hr\nduration=24\npeaktime=12\ncurve=10-yr\ncoef1= 0.7035 0.079 0.42\ncurve=25-yr\ncoef1= 1.4123 0.065 0.59\n";
        let f = parse_hcr_file(text).unwrap();
        assert_eq!(f.storms.len(), 2);
        assert_eq!(f.storms[1].name, "D 24-hr 25-yr");
        for s in &f.storms {
            monotone_ends_at_one(s);
            assert!((s.fraction_at(12.0) - 0.5).abs() < 0.01);
        }
        let depth = 24.0 * 0.7035 / (0.079_f64 + 24.0).powf(0.42);
        assert!((f.storms[0].default_depth.unwrap() - depth).abs() < 1e-9);
        let minutes = "name=R\ntimeunits=minutes\nduration=1440\npeaktime=1440*0.3\ncurve=2-yr\nstormdepth=37.2\ndepthunits=mm\ncoef1= 863/60 5 0.8\n";
        let s = parse_hcr(minutes).unwrap();
        assert_eq!(s.duration, 24.0);
        assert!(
            (s.fraction_at(24.0 * 0.3) - 0.3).abs() < 0.01,
            "peak at 30 % of the storm: 30 % of the depth has fallen"
        );
        assert!((s.default_depth.unwrap() - 37.2 / 25.4).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse_hcr("name=x\n").is_err());
        assert!(parse_hcr("name=x\nduration=1\ndepth= 0 .5 .4 1\n").is_err());
        assert!(parse_hcr("name=x\nduration=1\ndepth= 0 abc 1\n").is_err());
        assert!(parse_hcr("name=x\ncoef= 1 2 3\n").is_err());
        assert!(parse_huh("name=x\ninten=0 1\n").is_err());
        assert!(parse_idf("name=x\n").is_err());
        assert!(parse_rain_table("t", "a,b\n1,2\n").is_err());
    }

    #[test]
    fn hcr_round_trip() {
        let b = builtin_storms().into_iter().find(|b| b.name == "Type III 24-hr").unwrap();
        let back = parse_hcr(&write_hcr(&b)).unwrap();
        assert_eq!(back.name, b.name);
        assert!((back.duration - 24.0).abs() < 1e-9);
        for h in 0..=24 {
            assert!((back.fraction_at(h as f64) - b.fraction_at(h as f64)).abs() < 2e-3);
        }
        let g = parse_hcr("name=G\ntimedepth= 0 0\ntimedepth= 4 .25\ntimedepth= 24 2.7\n").unwrap();
        let back = parse_hcr(&write_hcr(&g)).unwrap();
        assert_eq!(back.times, g.times);
        assert!(back.fractions.iter().zip(&g.fractions).all(|(a, b)| (a - b).abs() < 1e-6));
        assert_eq!(back.default_depth, g.default_depth);
    }

    const UH: &str = "// fixture\nname=SCS\ncomment=Standard, Peak Factor = 484\ninten=.000 .030 .100 .190 .310 .470 .660 .820 .930 .990\ninten=1.00 .990 .930 .860 .780 .680 .560 .460 .390 .330\ninten=.280 .241 .207 .174 .147 .126 .107 .091 .077 .066\ninten=.055 .047 .040 .034 .029 .025 .021 .018 .015 .013\ninten=.011 .009 .008 .007 .006 .005 .004 .003 .002 .001\ninten=.000\n";

    #[test]
    fn unit_hydrograph_files() {
        let u = parse_huh(UH).unwrap();
        assert_eq!(u.name, "SCS");
        assert_eq!(u.t.len(), 51);
        assert!((u.t[10] - 1.0).abs() < 1e-12 && (u.q[10] - 1.0).abs() < 1e-12);
        assert!((u.peak_factor() - 484.0).abs() < 3.0, "{}", u.peak_factor());
        let back = parse_huh(&write_huh(&u)).unwrap();
        assert!((back.peak_factor() - u.peak_factor()).abs() < 3.0);
        let g = crate::uh::UnitHydrograph::gamma(300.0).unwrap();
        let back = parse_huh(&write_huh(&g)).unwrap();
        assert!((back.peak_factor() - 300.0).abs() < 6.0, "{}", back.peak_factor());
    }

    /// An `.hci` file in the published layout (our own values): tab and space separators,
    /// expressions, a doubled "==", trailing spaces, Cf, a multiple, a DEPTH event and the
    /// three coefficient forms.
    const HCI: &str = "// IDF test file\nName=Test County\ncomment=Made-up values\n\ndepthunits=INCHES\ntimeunits=hours\nduratunits=minutes\n\nduration=5\t15 1*60 6*60 24*60\n\nevent=2-Year \ninten=5.0 3.5 1.6 0.5 0.15\n\nevent==10-Year\nCf=1.1\ninten=1.5x\n\nevent=Depths\ndepth=0.5 1.0 2.0 3.0 4.8\n";

    #[test]
    fn hci_tables_units_and_quirks() {
        let f = parse_idf(HCI).unwrap();
        assert_eq!((f.name.as_str(), f.comment.as_str()), ("Test County", "Made-up values"));
        let names: Vec<&str> = f.events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["2-Year", "10-Year", "Depths"]);
        let e = &f.events[0];
        assert!((e.intensity(60.0).unwrap() - 1.6).abs() < 1e-12);
        // Log-log between 60 and 360 minutes; held beyond the first and last durations.
        let mid = e.intensity(120.0).unwrap();
        let expect = (1.6f64.ln() + (2.0f64.ln() / 6.0f64.ln()) * (0.5f64 / 1.6).ln()).exp();
        assert!((mid - expect).abs() < 1e-12, "{mid} {expect}");
        assert_eq!(e.intensity(2.0), Some(5.0));
        assert_eq!(e.intensity(3000.0), Some(0.15));
        // 1.5x the first INTEN line, with its frequency coefficient.
        assert!((f.events[1].intensity(15.0).unwrap() - 5.25).abs() < 1e-12);
        assert_eq!(f.events[1].factor, 1.1);
        // Depth over duration: 2.0 in in 60 min = 2.0 in/hr; 4.8 in in 24 h = 0.2 in/hr.
        assert!((f.events[2].intensity(60.0).unwrap() - 2.0).abs() < 1e-12);
        assert!((f.events[2].intensity(1440.0).unwrap() - 0.2).abs() < 1e-12);
        // Millimetres per hour with durations in hours.
        let mm =
            parse_idf("name=x\ndepthunits=mm\nduratunits=hours\nduration=5/60 1 24\nevent=a\ninten=127 25.4 2.54\n")
                .unwrap();
        assert!((mm.events[0].intensity(60.0).unwrap() - 1.0).abs() < 1e-12);
        assert!((mm.events[0].intensity(5.0).unwrap() - 5.0).abs() < 1e-12);
        // Intensity per minute: 0.05 in/min = 3 in/hr.
        let per_min = parse_idf("timeunits=minutes\nduration=10 60\nevent=a\ninten=0.05 0.02\n").unwrap();
        assert!((per_min.events[0].intensity(10.0).unwrap() - 3.0).abs() < 1e-12);
    }

    #[test]
    fn hci_coefficients_are_held_within_their_range() {
        let text = "name=c\nduratunits=minutes\nduration=10 24*60\nevent=one\ncoef1= 69 8.0 0.811\nevent=two\ncoef2= 10.63 8.1 0.45\nevent=three\ncoef3= 11.0983 -2.4724 0.00711 0.01886\n";
        let f = parse_idf(text).unwrap();
        let eqs: [&dyn Fn(f64) -> f64; 3] =
            [&|t| 69.0 / (8.0 + t).powf(0.811), &|t| 10.63 / (8.1 + t.powf(0.45)), &|t| {
                let x = f64::ln(t);
                11.0983 - 2.4724 * x + 0.00711 * x * x + 0.01886 * x * x * x
            }];
        for (e, eq) in f.events.iter().zip(eqs) {
            for t in [10.0, 13.0, 37.5, 60.0, 200.0, 1440.0] {
                let got = e.intensity(t).unwrap();
                assert!((got / eq(t) - 1.0).abs() < 2e-4, "{} at {t}: {got} vs {}", e.name, eq(t));
            }
            // Outside the DURATION range the limit's intensity is used.
            assert!((e.intensity(5.0).unwrap() - eq(10.0)).abs() < 1e-9);
            assert!((e.intensity(2880.0).unwrap() - eq(1440.0)).abs() < 1e-9);
        }
        // Hour-based durations and millimetres: I(1 h) = 16 / 1^0.532 mm/hr.
        let h =
            parse_idf("depthunits=mm\nduratunits=hours\nduration= 5/60 24\nevent=a\ncoef1= 16.0  0  0.532\n").unwrap();
        assert!((h.events[0].intensity(60.0).unwrap() - 16.0 / 25.4).abs() < 1e-6);
    }

    #[test]
    fn hci_errors_name_the_problem() {
        let e = parse_idf("duration=5 10 15\nevent=a\ninten=1 2\n").unwrap_err();
        assert!(e.contains("event a has 2 intensity values for 3 durations"), "{e}");
        assert!(parse_idf("event=a\ninten=1 2\n").unwrap_err().contains("no DURATION"));
        assert!(parse_idf("duration=5 10\nevent=a\n").unwrap_err().contains("no INTEN"));
        assert!(parse_idf("duration=5 10\ninten=1 2\n").unwrap_err().contains("before any EVENT"));
        assert!(parse_idf("duration=5 10\nevent=a\ninten=1 -2\n").unwrap_err().contains("positive"));
        assert!(parse_idf("duration=5 10\nevent=a\ninten=2x\n").unwrap_err().contains("earlier INTEN"));
        assert!(parse_idf("duration=5 10\nevent=a\ncoef1=1 2\n").unwrap_err().contains("three coefficients"));
    }

    #[test]
    fn hci_round_trip() {
        let f = parse_idf(HCI).unwrap();
        let text = write_idf(&f);
        assert!(text.contains("duration=5 15 1*60 6*60 24*60"), "{text}");
        assert!(text.contains("Cf=1.1"));
        let back = parse_idf(&text).unwrap();
        assert_eq!(back.name, "Test County");
        for (a, b) in f.events.iter().zip(&back.events) {
            assert_eq!((a.name.as_str(), a.factor), (b.name.as_str(), b.factor));
            for t in [5.0, 30.0, 100.0, 1440.0] {
                assert!((a.intensity(t).unwrap() - b.intensity(t).unwrap()).abs() < 1e-6);
            }
        }
        // A project curve of coefficient sets is written as coef1 lines.
        let c = IdfCurve { name: "k".into(), events: vec![("2-Year".into(), 60.0, 10.0, 0.8)], tables: Vec::new() };
        let text = write_idf(&IdfFile::from_curve(&c));
        assert!(text.contains("coef1= 60 10 0.8") && text.contains("duration=5 24*60"), "{text}");
        let back = parse_idf(&text).unwrap();
        assert!((back.events[0].intensity(30.0).unwrap() / (60.0 / 40.0f64.powf(0.8)) - 1.0).abs() < 2e-4);
    }

    /// The public sample `.hci` files (downloaded into the gitignored research folder, never
    /// committed) all read, with their intensities at 60 minutes as published.
    #[test]
    fn published_hci_samples_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference_images/research/idf");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut n = 0;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("hci") || e.eq_ignore_ascii_case("csv")) {
                let text = std::fs::read_to_string(&p).unwrap();
                let f = parse_idf(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                assert!(!f.events.is_empty(), "{}", p.display());
                for e in &f.events {
                    let i = e.intensity(60.0).unwrap();
                    assert!(i > 0.05 && i < 200.0, "{} {}: {i}", p.display(), e.name);
                }
                n += 1;
            }
        }
        assert!(n > 0 || !dir.exists());
    }

    #[test]
    fn legacy_idf_layout_still_reads() {
        let text = "name=Test\ntimeunits=minutes\ncurve=10-yr\nfactor=1.1\ncoef= 100 10 0.8\ncurve=2-yr\nidf= 5 5.0  10 4.0\nidf= 60 1.5  120 1.0\n";
        let f = parse_idf(text).unwrap();
        assert_eq!(f.events.len(), 2);
        assert!((f.events[0].intensity(30.0).unwrap() - 100.0 / 40.0_f64.powf(0.8)).abs() < 1e-9);
        assert_eq!(f.events[0].factor, 1.1);
        assert!((f.events[1].intensity(60.0).unwrap() - 1.5).abs() < 1e-9);
        let mid = f.events[1].intensity(85.0).unwrap();
        assert!(mid < 1.5 && mid > 1.0);
        let c = f.to_curve();
        assert!((c.intensity("10-yr", 30.0).unwrap() - f.events[0].intensity(30.0).unwrap()).abs() < 1e-12);
        assert!((c.intensity("2-yr", 60.0).unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(IdfFile::from_curve(&c).events.len(), 2);
        let back = parse_idf(&write_idf(&f)).unwrap();
        assert_eq!(back.events.len(), 2);
        // Written as coef1 and read back as a dense table over 5 min to 24 h.
        assert!((back.events[0].intensity(30.0).unwrap() / f.events[0].intensity(30.0).unwrap() - 1.0).abs() < 2e-4);
        let ev = f.events_for("Type II 24-hr", 24.0, "");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].frequency_factor, 1.1);
        assert!((ev[1].depth - (f.events[1].intensity(1440.0).unwrap() * 24.0 * 100.0).round() / 100.0).abs() < 1e-9);
        // Hour-based coefficients convert to minutes.
        let h = parse_idf("curve=a\ntimeunits=hours\ncoef= 2 0.5 0.7\n").unwrap();
        assert!((h.events[0].intensity(120.0).unwrap() - 2.0 / 2.5_f64.powf(0.7)).abs() < 1e-9);
    }

    #[test]
    fn idf_from_noaa_csv() {
        let csv = "Point precipitation frequency estimates (inches)\nData type: Precipitation depth\nPRECIPITATION FREQUENCY ESTIMATES\nby duration for ARI (years):, 1,2\n60-min:, 1.0,1.2\n2-hr:, 1.4,1.8\n24-hr:, 2.4,3.0\n";
        let f = parse_idf(csv).unwrap();
        assert_eq!(f.events.len(), 2);
        assert_eq!(f.events[1].name, "2-Year");
        let ev = f.events_for("Type II 24-hr", 24.0, "");
        assert_eq!((ev[0].depth, ev[1].depth), (2.4, 3.0));
    }

    const TABLE: &str = "ID,State,County,Storm,1-Year,2-Year,10-Year\r\n\"3705\",\"AK\",\"Aleutians East, Cold Bay AP AK_IA\",\"IA\",\"1.78\",\"2.16\",\"3.14\"\r\n7,AL,Autauga,III,3.7,4.5,0\r\n8,AL,Baldwin,,0,0,0\r\n";

    #[test]
    #[allow(clippy::approx_constant)] // a 3.14" rainfall depth, not π
    fn county_tables() {
        let t = parse_rain_table("NRCS2-Rain", TABLE).unwrap();
        assert_eq!(t.states(), vec!["AK", "AL"]);
        let r = t.lookup("ak", "Aleutians East, Cold Bay AP AK_IA").unwrap();
        assert_eq!(r.storm, "IA");
        let ev = t.events(r, "Type IA 24-hr");
        assert_eq!(
            ev.iter().map(|e| (e.name.as_str(), e.depth)).collect::<Vec<_>>(),
            vec![("1-Year", 1.78), ("2-Year", 2.16), ("10-Year", 3.14)]
        );
        let al = t.counties("AL");
        assert_eq!(al.len(), 2);
        assert_eq!(t.events(al[0], "Type III 24-hr").len(), 2, "zero depths are skipped");
        assert!(t.events(al[1], "x").is_empty());
    }

    #[test]
    fn storm_codes_resolve() {
        let names: Vec<String> = [
            "Type II 24-hr",
            "Type IA 24-hr",
            "NOAA 24-hr C",
            "NOAA10 24-hr D",
            "MSE 24-hr 3",
            "CA 24-hr 4",
            "Nevada2 24-hr North",
            "Nevada 24-hr South",
            "ND 24-hr 2",
            "PR_VI 24-hr 3",
            "NRCC 24-hr A",
            "TX 24-hr B",
            "SW 24-hr 3",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let want = [
            ("II", "Type II 24-hr"),
            ("IA", "Type IA 24-hr"),
            ("NOAA_C", "NOAA 24-hr C"),
            ("N10_D", "NOAA10 24-hr D"),
            ("MSE3", "MSE 24-hr 3"),
            ("CA-4", "CA 24-hr 4"),
            ("NV2_N", "Nevada2 24-hr North"),
            ("2_ND", "ND 24-hr 2"),
            ("PR_VI_3", "PR_VI 24-hr 3"),
            ("NR_A", "NRCC 24-hr A"),
            ("NO_C", "NOAA 24-hr C"),
            ("NV_S", "Nevada 24-hr South"),
            ("TX_B", "TX 24-hr B"),
            ("SW_3", "SW 24-hr 3"),
        ];
        for (code, name) in want {
            assert_eq!(storm_for_code(code, &names).as_deref(), Some(name), "{code}");
        }
        assert_eq!(storm_for_code("III", &names), None);
        assert_eq!(storm_for_code("", &names), None);
    }

    #[test]
    fn library_dispatch() {
        let mut lib = RainLibrary::default();
        assert!(lib.add_file("Type II 24-hr.hcr", TYPE_II.as_bytes()));
        assert!(lib.add_file("SCS.huh", UH.as_bytes()));
        assert!(lib.add_file("Mine.idf", b"curve=a\ncoef= 1 1 1\n"));
        assert!(lib.add_file("Test-Rain.txt", TABLE.as_bytes()));
        assert!(!lib.add_file("bad.hcr", b"name=x\n"));
        assert!(!lib.add_file("readme.md", b"x"));
        assert!(lib.add_file("Hidden.hcr", b"name=H\nhidden=true\nduration=1\ndepth=0 1\n"));
        assert_eq!((lib.storms.len(), lib.uhs.len(), lib.idfs.len(), lib.tables.len()), (1, 1, 1, 1));
        assert_eq!(lib.idfs[0].name, "Mine");
        assert_eq!(lib.problems.len(), 1);
    }

    /// Runs every file of the user-supplied library (skipped when the folder is absent).
    #[test]
    fn sample_library_parses() {
        let dir = std::env::var("WAVESTORM_RAIN_LIB")
            .unwrap_or_else(|_| "../../reference_images/expert_reference/Rainfall".into());
        let Ok(read) = std::fs::read_dir(&dir) else {
            eprintln!("rainfall library folder absent; skipping");
            return;
        };
        let mut lib = RainLibrary::default();
        let (mut hcr, mut huh) = (0, 0);
        let mut multi = 0;
        for entry in read.flatten() {
            let name = entry.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let lower = name.to_ascii_lowercase();
            let Ok(bytes) = std::fs::read(entry.path()) else { continue };
            if lower.ends_with(".hcr") {
                hcr += 1;
                let text = String::from_utf8_lossy(&bytes);
                let f = parse_hcr_file(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
                multi += usize::from(f.storms.len() > 1);
                for s in &f.storms {
                    monotone_ends_at_one(s);
                    // Round trip through the writer keeps the shape.
                    let back = parse_hcr(&write_hcr(s)).unwrap_or_else(|e| panic!("{name} round trip: {e}"));
                    for k in 0..=10 {
                        let t = s.duration * k as f64 / 10.0;
                        assert!((back.fraction_at(t) - s.fraction_at(t)).abs() < 3e-3, "{name} {} at {t}", s.name);
                    }
                }
            } else if lower.ends_with(".huh") {
                huh += 1;
                let u = parse_huh(&String::from_utf8_lossy(&bytes)).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(u.peak_factor() > 50.0 && u.peak_factor() < 1000.0, "{name} PF {}", u.peak_factor());
                assert!(u.t.windows(2).all(|w| w[1] > w[0]));
            }
            lib.add_file(&name, &bytes);
        }
        assert!(hcr > 100 && huh >= 20, "hcr {hcr} huh {huh}");
        assert!(multi > 10);
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert!(lib.tables.len() >= 5, "tables {}", lib.tables.len());
        // Every storm code used by the current table resolves to a library storm.
        let mut names = lib.storm_names();
        names.extend(builtin_storms().into_iter().map(|s| s.name));
        for t in &lib.tables {
            let mut missing = std::collections::BTreeSet::new();
            for r in &t.rows {
                for code in r.storm_codes() {
                    if storm_for_code(code, &names).is_none() {
                        missing.insert(code.to_string());
                    }
                }
            }
            if t.name == "NRCS2-Rain" || t.name == "NJ-2023-Rain" {
                assert!(missing.is_empty(), "{}: unresolved codes {missing:?}", t.name);
            } else {
                eprintln!("{}: unresolved codes {missing:?}", t.name);
            }
        }
    }
}
