//! Hydrograph routing: level-pool (storage-indication) routing for ponds and reaches, the
//! storage-indication-plus-translation reach method, and Muskingum-Cunge.

use crate::hydrograph::{Hydrograph, SEC_PER_HR};

/// A tabulated stage → (storage, outflow) relationship for level-pool routing.
#[derive(Debug, Clone, Default)]
pub struct StageTable {
    pub elev: Vec<f64>,
    /// Cubic feet.
    pub storage: Vec<f64>,
    /// Outflow (cfs) when it depends on stage alone.
    pub outflow: Vec<f64>,
}

impl StageTable {
    /// Tabulates `storage(e)` and `outflow(e)` between `bottom` and `top` in `n` steps.
    pub fn build(bottom: f64, top: f64, n: usize, storage: impl Fn(f64) -> f64, outflow: impl Fn(f64) -> f64) -> StageTable {
        let n = n.clamp(2, 200_000);
        let top = if top > bottom { top } else { bottom + 1.0 };
        let mut t = StageTable { elev: Vec::with_capacity(n + 1), storage: Vec::with_capacity(n + 1), outflow: Vec::with_capacity(n + 1) };
        let mut last_s = 0.0f64;
        let mut last_o = 0.0f64;
        for i in 0..=n {
            let e = bottom + (top - bottom) * i as f64 / n as f64;
            // Storage and outflow must be non-decreasing for a stable solution (a full pipe's
            // Manning curve peaks just below the crown, for example).
            let s = storage(e).max(last_s);
            let o = outflow(e);
            let o = if o.is_finite() { o.max(last_o) } else { last_o };
            t.elev.push(e);
            t.storage.push(s);
            t.outflow.push(o);
            last_s = s;
            last_o = o;
        }
        t
    }

    pub fn top(&self) -> f64 {
        self.elev.last().copied().unwrap_or(0.0)
    }

    pub fn bottom(&self) -> f64 {
        self.elev.first().copied().unwrap_or(0.0)
    }

    /// Storage at an elevation; above the table it grows with the top surface area.
    pub fn storage_at(&self, e: f64) -> f64 {
        let n = self.elev.len();
        if n < 2 {
            return 0.0;
        }
        if e >= self.elev[n - 1] {
            let dh = self.elev[n - 1] - self.elev[n - 2];
            let area = if dh > 0.0 { (self.storage[n - 1] - self.storage[n - 2]) / dh } else { 0.0 };
            return self.storage[n - 1] + area * (e - self.elev[n - 1]);
        }
        interp(&self.elev, &self.storage, e)
    }

    /// Elevation holding a given storage (inverse of `storage_at`).
    pub fn elev_at_storage(&self, s: f64) -> f64 {
        let n = self.storage.len();
        if n < 2 {
            return self.bottom();
        }
        if s <= self.storage[0] {
            return self.elev[0];
        }
        if s >= self.storage[n - 1] {
            let dh = self.elev[n - 1] - self.elev[n - 2];
            let area = if dh > 0.0 { (self.storage[n - 1] - self.storage[n - 2]) / dh } else { 0.0 };
            return if area > 0.0 { self.elev[n - 1] + (s - self.storage[n - 1]) / area } else { self.elev[n - 1] };
        }
        let i = self.storage.partition_point(|v| *v < s).clamp(1, n - 1);
        let (s0, s1) = (self.storage[i - 1], self.storage[i]);
        if s1 > s0 { self.elev[i - 1] + (self.elev[i] - self.elev[i - 1]) * (s - s0) / (s1 - s0) } else { self.elev[i] }
    }

    pub fn outflow_at(&self, e: f64) -> f64 {
        let n = self.elev.len();
        if n < 2 {
            return 0.0;
        }
        if e >= self.elev[n - 1] {
            // Extrapolate the last segment.
            let (e0, e1) = (self.elev[n - 2], self.elev[n - 1]);
            let (o0, o1) = (self.outflow[n - 2], self.outflow[n - 1]);
            return if e1 > e0 { o1 + (o1 - o0).max(0.0) * (e - e1) / (e1 - e0) } else { o1 };
        }
        interp(&self.elev, &self.outflow, e)
    }
}

fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return 0.0;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = xs[..n].partition_point(|v| *v <= x).clamp(1, n - 1);
    let (x0, x1) = (xs[i - 1], xs[i]);
    if x1 > x0 { ys[i - 1] + (ys[i] - ys[i - 1]) * (x - x0) / (x1 - x0) } else { ys[i] }
}

/// Elevation, storage and outflow at each time step of a level-pool routing.
#[derive(Debug, Clone, Default)]
pub struct LevelPoolResult {
    pub elevation: Vec<f64>,
    pub storage: Vec<f64>,
    pub outflow: Hydrograph,
    /// Highest elevation reached beyond the table, if any (storage range exceeded).
    pub exceeded_by: Option<f64>,
}

/// Level-pool routing. At each step the continuity equation
/// S₂ = S₁ + (I₁+I₂)/2·Δt − (O₁+O₂)/2·Δt is solved for the new elevation, where the outflow
/// is `outflow(elevation, step, commit)`. With a stage-only outflow this is the
/// storage-indication (modified Puls) method; the step function also allows tailwater and pump
/// states. `commit` is true for the single call made at each step's final elevation, so stateful
/// devices update only then.
pub fn level_pool(
    table: &StageTable,
    inflow: &Hydrograph,
    start_elev: f64,
    outflow: &mut dyn FnMut(f64, usize, bool) -> f64,
) -> LevelPoolResult {
    let n = inflow.len();
    let dt_s = inflow.dt * SEC_PER_HR;
    let mut r = LevelPoolResult {
        elevation: Vec::with_capacity(n),
        storage: Vec::with_capacity(n),
        outflow: Hydrograph::zeros(inflow.t0, inflow.dt, n),
        exceeded_by: None,
    };
    if n == 0 || !(dt_s > 0.0) {
        return r;
    }
    let mut e = start_elev.max(table.bottom());
    let mut s = table.storage_at(e);
    let mut o = outflow(e, 0, true).max(0.0);
    r.elevation.push(e);
    r.storage.push(s);
    r.outflow.q[0] = o;
    for i in 1..n {
        let (i1, i2) = (inflow.q[i - 1].max(0.0), inflow.q[i].max(0.0));
        let base = s + 0.5 * (i1 + i2) * dt_s - 0.5 * o * dt_s;
        // f(e2) = S(e2) + O(e2)·Δt/2 − base is increasing in e2; bracket and bisect.
        let f = |e2: f64, outflow: &mut dyn FnMut(f64, usize, bool) -> f64| table.storage_at(e2) + 0.5 * outflow(e2, i, false).max(0.0) * dt_s - base;
        let lo0 = table.bottom();
        let (mut lo, mut hi) = (lo0, e.max(lo0));
        let mut guard = 0;
        while f(hi, outflow) < 0.0 && guard < 80 {
            hi = hi + (hi - lo0).max(0.5) + 0.5;
            guard += 1;
        }
        if f(lo, outflow) >= 0.0 {
            hi = lo;
        }
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if f(mid, outflow) < 0.0 { lo = mid } else { hi = mid }
            if hi - lo < 1e-7 {
                break;
            }
        }
        e = 0.5 * (lo + hi);
        o = outflow(e, i, true).max(0.0);
        // Never release more than is stored (keeps zero-storage ponds exact).
        let available = (base / (0.5 * dt_s)).max(0.0);
        s = if o > available {
            o = available;
            0.0
        } else {
            table.storage_at(e)
        };
        if e > table.top() {
            let ex = e - table.top();
            r.exceeded_by = Some(r.exceeded_by.map_or(ex, |v: f64| v.max(ex)));
        }
        r.elevation.push(e);
        r.storage.push(s);
        r.outflow.q[i] = o;
    }
    r
}

/// Shifts a hydrograph later by `lag` hours (linear interpolation), keeping its time base.
pub fn translate(h: &Hydrograph, lag: f64) -> Hydrograph {
    if lag <= 0.0 {
        return h.clone();
    }
    Hydrograph { t0: h.t0, dt: h.dt, q: (0..h.len()).map(|i| h.value_at(h.time(i) - lag).max(0.0)).collect() }
}

/// Muskingum-Cunge routing with parameters from a reference flow: celerity c = dQ/dA,
/// K = L/c, X = ½(1 − Q/(T·S·c·L)); the reach is split so K ≈ Δt per sub-reach.
pub fn muskingum_cunge(
    inflow: &Hydrograph,
    length: f64,
    slope: f64,
    q_ref: f64,
    celerity: f64,
    top_width: f64,
) -> (Hydrograph, f64) {
    let dt = inflow.dt * SEC_PER_HR;
    if length <= 0.0 || celerity <= 0.0 || dt <= 0.0 {
        return (inflow.clone(), 0.0);
    }
    let pieces = ((length / (celerity * dt)).ceil() as usize).clamp(1, 1000);
    let dx = length / pieces as f64;
    let k = dx / celerity;
    let x = if top_width > 0.0 && slope > 0.0 {
        (0.5 * (1.0 - q_ref / (top_width * slope * celerity * dx))).clamp(0.0, 0.5)
    } else {
        0.0
    };
    let den = k * (1.0 - x) + 0.5 * dt;
    let c0 = (0.5 * dt - k * x) / den;
    let c1 = (0.5 * dt + k * x) / den;
    let c2 = (k * (1.0 - x) - 0.5 * dt) / den;
    let mut q = inflow.q.clone();
    for _ in 0..pieces {
        let mut out = vec![0.0; q.len()];
        if let Some(first) = q.first() {
            out[0] = *first;
        }
        for i in 1..q.len() {
            out[i] = (c0 * q[i] + c1 * q[i - 1] + c2 * out[i - 1]).max(0.0);
        }
        q = out;
    }
    (Hydrograph { t0: inflow.t0, dt: inflow.dt, q }, x)
}

/// Detention times for a routed hydrograph pair (minutes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DetentionTime {
    pub center_of_mass: Option<f64>,
    pub com_in: Option<f64>,
    pub com_out: Option<f64>,
    /// Plug-flow (first-in, first-out) average detention time.
    pub plug_flow: Option<f64>,
    /// Volume (cu ft) the plug-flow figure was computed for.
    pub plug_volume: f64,
}

/// Center-of-mass and plug-flow detention times. `initial_storage` is discharged first.
pub fn detention_time(inflow: &Hydrograph, outflow: &Hydrograph, initial_storage: f64) -> DetentionTime {
    let com_in = inflow.center_of_mass();
    let com_out = outflow.center_of_mass();
    let center_of_mass = match (com_in, com_out) {
        (Some(a), Some(b)) => Some((b - a) * 60.0),
        _ => None,
    };
    // Cumulative volumes.
    let cum = |h: &Hydrograph| {
        let mut v = vec![0.0];
        for w in h.q.windows(2) {
            let last = v.last().copied().unwrap_or(0.0);
            v.push(last + 0.5 * (w[0] + w[1]) * h.dt * SEC_PER_HR);
        }
        v
    };
    let (vi, vo) = (cum(inflow), cum(outflow));
    let total_in = vi.last().copied().unwrap_or(0.0);
    let total_out = vo.last().copied().unwrap_or(0.0);
    let plug_volume = (total_out - initial_storage).min(total_in).max(0.0);
    let plug_flow = if plug_volume > 0.0 {
        let time_at = |cumv: &[f64], h: &Hydrograph, v: f64| {
            let i = cumv.partition_point(|x| *x < v).clamp(1, cumv.len().max(2) - 1);
            let (v0, v1) = (cumv[i - 1], cumv[i]);
            let f = if v1 > v0 { (v - v0) / (v1 - v0) } else { 0.0 };
            h.time(i - 1) + f * h.dt
        };
        let m = 200;
        let mut sum = 0.0;
        for k in 0..m {
            let v = plug_volume * (k as f64 + 0.5) / m as f64;
            sum += time_at(&vo, outflow, v + initial_storage) - time_at(&vi, inflow, v);
        }
        Some(sum / m as f64 * 60.0)
    } else {
        None
    };
    DetentionTime { center_of_mass, com_in: com_in.map(|t| t * 60.0), com_out: com_out.map(|t| t * 60.0), plug_flow, plug_volume }
}

/// Pond sizing estimate: (peak outflow, storage needed) pairs from routing `inflow` through
/// hypothetical ponds whose discharge is proportional to storage (S = K·O) for a wide range of K.
pub fn pond_sizing(inflow: &Hydrograph) -> Vec<(f64, f64)> {
    let dt = inflow.dt * SEC_PER_HR;
    let peak_in = inflow.peak().value;
    if !(dt > 0.0) || !(peak_in > 0.0) {
        return Vec::new();
    }
    let mut out = vec![(peak_in, 0.0)];
    // K from a fraction of the step to many days, log-spaced.
    for i in 0..=120 {
        let k = dt * 0.05 * 10f64.powf(i as f64 / 20.0);
        let (mut s, mut o, mut smax, mut omax) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for w in inflow.q.windows(2) {
            // Exact step for a linear reservoir with trapezoidal inflow.
            let s2 = (s + 0.5 * (w[0] + w[1]) * dt - 0.5 * o * dt) / (1.0 + 0.5 * dt / k);
            s = s2.max(0.0);
            o = s / k;
            smax = smax.max(s);
            omax = omax.max(o);
        }
        out.push((omax, smax));
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Section;

    #[test]
    fn linear_reservoir_conserves_volume() {
        // S = K·O with K = 1 h: outflow volume equals inflow volume over a long span.
        let k = 3600.0;
        let table = StageTable::build(0.0, 100.0, 2000, |e| e * 1000.0, |e| e * 1000.0 / k);
        let mut inflow = Hydrograph::zeros(0.0, 0.1, 400);
        for i in 10..20 {
            inflow.q[i] = 10.0;
        }
        let r = level_pool(&table, &inflow, 0.0, &mut |e, _, _| table.outflow_at(e));
        assert!((r.outflow.volume_cf() - inflow.volume_cf()).abs() / inflow.volume_cf() < 1e-3);
        assert!(r.outflow.peak().value < 10.0);
    }

    #[test]
    fn zero_storage_pond_passes_inflow() {
        let table = StageTable::build(0.0, 10.0, 1000, |_| 0.0, |e| 5.0 * e);
        let inflow = Hydrograph { t0: 0.0, dt: 0.1, q: vec![0.0, 2.0, 6.0, 3.0, 0.0] };
        let r = level_pool(&table, &inflow, 0.0, &mut |e, _, _| table.outflow_at(e));
        for (a, b) in r.outflow.q.iter().zip(&inflow.q) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
    }

    #[test]
    fn reference_reach_stor_ind_trans() {
        // Parabolic 10' x 2' channel, n=0.040, L=5,500', S=0.0047 '/'. Inflow tabulated at 0.5 h
        // in the reference report; outflow and storage compared where the curve is smooth.
        let tab_in = [
            (10.0, 0.0), (11.5, 0.0), (12.0, 3.18), (12.5, 24.89), (13.0, 18.55), (13.5, 17.03), (14.0, 15.96), (14.5, 14.93),
            (15.0, 14.17), (15.5, 13.48), (16.0, 12.97), (16.5, 12.53), (17.0, 12.19), (17.5, 11.86), (18.0, 11.52), (18.5, 11.15),
            (19.0, 10.78), (19.5, 10.35), (20.0, 9.73), (20.5, 9.12), (21.0, 8.09), (21.5, 6.97), (22.0, 6.19), (22.5, 5.60),
            (23.0, 5.25), (23.5, 5.04), (24.0, 4.90), (24.5, 3.74), (25.0, 2.17), (25.5, 1.32), (26.0, 0.84), (27.0, 0.39),
            (28.0, 0.21), (30.0, 0.09), (35.0, 0.02), (40.0, 0.01),
        ];
        let coarse = |t: f64| {
            let i = tab_in.partition_point(|p| p.0 <= t).clamp(1, tab_in.len() - 1);
            let (a, b) = (tab_in[i - 1], tab_in[i]);
            a.1 + (b.1 - a.1) * ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
        };
        let inflow = Hydrograph { t0: 10.0, dt: 0.1, q: (0..301).map(|i| coarse(10.0 + i as f64 * 0.1)).collect() };
        let section = Section::Parabolic { width: 10.0, depth: 2.0 };
        let (n, s, len) = (0.040, 0.0047, 5500.0);
        let table = StageTable::build(48.0, 52.0, 4000, |e| section.props(e - 48.0).area * len, |e| section.manning_q(e - 48.0, n, s));
        let r = level_pool(&table, &inflow, 48.0, &mut |e, _, _| table.outflow_at(e));
        // Storage indication part: storage 34,880 cf at 16.0 h, 22,368 at 22.0 h.
        let at = |t: f64| ((t - 10.0) / 0.1).round() as usize;
        assert!((r.storage[at(16.0)] - 34_880.0).abs() / 34_880.0 < 0.03, "{}", r.storage[at(16.0)]);
        assert!((r.storage[at(22.0)] - 22_368.0).abs() / 22_368.0 < 0.05, "{}", r.storage[at(22.0)]);
        // Translation by L / Vmax.
        let ip = r.outflow.max_index().unwrap();
        let e_pk = r.elevation[ip];
        let vmax = section.velocity(e_pk - 48.0, n, s);
        assert!((vmax - 2.3).abs() < 0.1, "vmax {vmax}");
        let lag = len / vmax / 3600.0;
        let out = translate(&r.outflow, lag);
        for (t, g) in [(15.0, 16.15), (17.0, 13.18), (20.0, 10.91), (25.0, 5.00)] {
            assert!((out.value_at(t) - g).abs() / g < 0.05, "t={t}: {} vs {g}", out.value_at(t));
        }
    }

    #[test]
    fn detention_times() {
        let inflow = Hydrograph { t0: 0.0, dt: 1.0, q: vec![0.0, 10.0, 0.0, 0.0, 0.0] };
        let outflow = Hydrograph { t0: 0.0, dt: 1.0, q: vec![0.0, 0.0, 0.0, 10.0, 0.0] };
        let d = detention_time(&inflow, &outflow, 0.0);
        assert!((d.center_of_mass.unwrap() - 120.0).abs() < 1e-9);
        assert!((d.plug_flow.unwrap() - 120.0).abs() < 1.0);
    }

    #[test]
    fn muskingum_cunge_attenuates_and_delays() {
        let mut inflow = Hydrograph::zeros(0.0, 0.1, 300);
        for i in 0..40 {
            inflow.q[20 + i] = 50.0 * (std::f64::consts::PI * i as f64 / 40.0).sin();
        }
        let (out, x) = muskingum_cunge(&inflow, 10_000.0, 0.002, 50.0, 4.0, 20.0);
        assert!(x >= 0.0 && x <= 0.5);
        assert!(out.peak().value < inflow.peak().value);
        assert!(out.peak().time > inflow.peak().time);
        assert!((out.volume_cf() - inflow.volume_cf()).abs() / inflow.volume_cf() < 0.01);
    }

    #[test]
    fn sizing_curve_is_monotone() {
        let mut inflow = Hydrograph::zeros(0.0, 0.1, 300);
        for i in 0..60 {
            inflow.q[20 + i] = 30.0 * (std::f64::consts::PI * i as f64 / 60.0).sin();
        }
        let curve = pond_sizing(&inflow);
        assert!(curve.len() > 50);
        // Lower allowed outflow needs more storage, and never more than the inflow volume.
        for w in curve.windows(2) {
            assert!(w[1].0 <= w[0].0 && w[1].1 + 1e-6 >= w[0].1);
        }
        assert!(curve.last().unwrap().1 <= inflow.volume_cf() * 1.001);
        assert!(pond_sizing(&Hydrograph::zeros(0.0, 0.1, 10)).is_empty());
    }
}
