//! Shapes drawn on the diagram over the background: subcatchment outlines, sub-areas drawn
//! inside them, and Tc flow paths. Polygon measures and tests, keeping drawn sub-areas inside
//! their subcatchment without overlapping, and updating areas and Tc lengths when drawings
//! change. Coordinates are diagram units; `fpu` is feet per diagram unit (the background
//! calibration, 1 when uncalibrated).

use crate::cn::SubArea;
use crate::project::{Node, NodeData, Project, Subcatchment};
use crate::storage::{CustomKind, StorageVolume, VolumeShape};
use serde::{Deserialize, Serialize};

/// A point in diagram units.
pub type Pt = [f64; 2];

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn cross(a: Pt, b: Pt) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Area of a simple polygon (absolute value of the shoelace sum).
pub fn polygon_area(p: &[Pt]) -> f64 {
    if p.len() < 3 {
        return 0.0;
    }
    let s: f64 = (0..p.len()).map(|i| cross(p[i], p[(i + 1) % p.len()])).sum();
    (s / 2.0).abs()
}

/// Area centroid of a simple polygon (the vertex mean when it has no area).
pub fn centroid(p: &[Pt]) -> Pt {
    let n = p.len();
    if n == 0 {
        return [0.0, 0.0];
    }
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (q, r) = (p[i], p[(i + 1) % n]);
        let c = cross(q, r);
        a += c;
        cx += (q[0] + r[0]) * c;
        cy += (q[1] + r[1]) * c;
    }
    if a.abs() < 1e-12 {
        let s = p.iter().fold([0.0, 0.0], |s, q| [s[0] + q[0], s[1] + q[1]]);
        return [s[0] / n as f64, s[1] / n as f64];
    }
    [cx / (3.0 * a), cy / (3.0 * a)]
}

/// Triangles (vertex indices) covering a simple polygon, concave ones included, by ear
/// clipping. A degenerate remainder is closed with a fan.
pub fn triangulate(p: &[Pt]) -> Vec<[usize; 3]> {
    let n = p.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let signed: f64 = (0..n).map(|i| cross(p[i], p[(i + 1) % n])).sum();
    if signed < 0.0 {
        idx.reverse();
    }
    let in_triangle = |q: Pt, a: Pt, b: Pt, c: Pt| {
        let (d1, d2, d3) = (cross(sub(b, a), sub(q, a)), cross(sub(c, b), sub(q, b)), cross(sub(a, c), sub(q, c)));
        d1 > 1e-12 && d2 > 1e-12 && d3 > 1e-12
    };
    let mut tris = Vec::with_capacity(n - 2);
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find(|&i| {
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            cross(sub(p[b], p[a]), sub(p[c], p[b])) > 0.0
                && !idx.iter().any(|&j| j != a && j != b && j != c && in_triangle(p[j], p[a], p[b], p[c]))
        });
        let Some(i) = ear else { break };
        tris.push([idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]]);
        idx.remove(i);
    }
    for k in 1..idx.len().saturating_sub(1) {
        tris.push([idx[0], idx[k], idx[k + 1]]);
    }
    tris
}

/// Length of a polyline.
pub fn polyline_length(p: &[Pt]) -> f64 {
    p.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// The point of segment `a`–`b` nearest `p`.
fn nearest_on_segment(p: Pt, a: Pt, b: Pt) -> Pt {
    let ab = sub(b, a);
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let f = if len2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    [a[0] + ab[0] * f, a[1] + ab[1] * f]
}

/// The point of polygon `poly`'s boundary nearest `p`.
pub fn nearest_on_boundary(p: Pt, poly: &[Pt]) -> Pt {
    let n = poly.len();
    (0..n)
        .map(|i| nearest_on_segment(p, poly[i], poly[(i + 1) % n]))
        .min_by(|a, b| dist(p, *a).total_cmp(&dist(p, *b)))
        .unwrap_or(p)
}

/// Distance tolerance for a polygon: a millionth of its size.
fn tolerance(poly: &[Pt]) -> f64 {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in poly {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    (dist(lo, hi) * 1e-6).max(1e-9)
}

/// Whether `p` lies on `poly`'s boundary (within `tol`).
pub fn on_boundary(p: Pt, poly: &[Pt], tol: f64) -> bool {
    dist(p, nearest_on_boundary(p, poly)) <= tol
}

/// Whether `p` is inside `poly` (even-odd rule; points on the boundary may go either way).
pub fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

/// Inside or on the boundary.
fn inside_or_on(p: Pt, poly: &[Pt], tol: f64) -> bool {
    on_boundary(p, poly, tol) || point_in_polygon(p, poly)
}

/// Strictly inside (not on the boundary).
fn strictly_inside(p: Pt, poly: &[Pt], tol: f64) -> bool {
    !on_boundary(p, poly, tol) && point_in_polygon(p, poly)
}

/// Whether segments `a`–`b` and `c`–`d` cross at a point inside both (touching and collinear
/// overlaps do not count).
fn proper_cross(a: Pt, b: Pt, c: Pt, d: Pt, tol: f64) -> bool {
    let side = |p: Pt, q: Pt, r: Pt| {
        let v = cross(sub(q, p), sub(r, p));
        let scale = dist(p, q).max(1e-12);
        if v.abs() / scale <= tol { 0.0 } else { v.signum() }
    };
    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
}

fn edges(p: &[Pt]) -> impl Iterator<Item = (Pt, Pt)> + '_ {
    (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()]))
}

/// Points along each edge (its quarter points), to catch edges that leave a concave polygon
/// between vertices that are inside it.
fn edge_samples(p: &[Pt]) -> impl Iterator<Item = Pt> + '_ {
    edges(p).flat_map(|(a, b)| [0.25, 0.5, 0.75].map(|f| [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]))
}

/// Whether polygon `inner` lies within `outer` (touching its boundary is allowed).
pub fn polygon_within(inner: &[Pt], outer: &[Pt]) -> bool {
    let tol = tolerance(outer);
    inner.len() >= 3
        && inner.iter().all(|q| inside_or_on(*q, outer, tol))
        && !edges(inner).any(|(a, b)| edges(outer).any(|(c, d)| proper_cross(a, b, c, d, tol)))
        && edge_samples(inner).all(|q| inside_or_on(q, outer, tol))
}

/// Whether two polygons share interior area (sharing edges or points is allowed).
pub fn polygons_overlap(a: &[Pt], b: &[Pt]) -> bool {
    let tol = tolerance(a).max(tolerance(b));
    if edges(a).any(|(p, q)| edges(b).any(|(r, s)| proper_cross(p, q, r, s, tol))) {
        return true;
    }
    let probes =
        |x: &[Pt]| x.iter().copied().chain(edge_samples(x)).chain(std::iter::once(centroid(x))).collect::<Vec<Pt>>();
    probes(a).into_iter().any(|q| strictly_inside(q, b, tol))
        || probes(b).into_iter().any(|q| strictly_inside(q, a, tol))
}

/// `p`, or the nearest point of `poly`'s boundary when `p` is outside it.
pub fn clamp_inside(p: Pt, poly: &[Pt]) -> Pt {
    if poly.len() < 3 || point_in_polygon(p, poly) { p } else { nearest_on_boundary(p, poly) }
}

/// Where to put a label in `region` outside every polygon of `holes`: the sampled point
/// farthest from all their edges, with that distance (a grid search refined around the best
/// sample). `None` when no sample lands in the region.
pub fn label_spot(region: &[Pt], holes: &[&[Pt]]) -> Option<(Pt, f64)> {
    if region.len() < 3 {
        return None;
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in region {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    // Holes outside the region's bounding box can neither contain a point of it nor be nearer
    // than its own boundary.
    let holes: Vec<&[Pt]> = holes
        .iter()
        .copied()
        .filter(|h| {
            h.iter().any(|q| q[0] >= lo[0])
                && h.iter().any(|q| q[0] <= hi[0])
                && h.iter().any(|q| q[1] >= lo[1])
                && h.iter().any(|q| q[1] <= hi[1])
        })
        .collect();
    let clearance = |p: Pt| -> Option<f64> {
        if !point_in_polygon(p, region) || holes.iter().any(|h| h.len() >= 3 && point_in_polygon(p, h)) {
            return None;
        }
        let d = std::iter::once(region)
            .chain(holes.iter().copied())
            .filter(|h| h.len() >= 2)
            .map(|h| dist(p, nearest_on_boundary(p, h)))
            .fold(f64::INFINITY, f64::min);
        Some(d)
    };
    let search = |lo: Pt, hi: Pt, n: usize, best: Option<(Pt, f64)>| {
        let mut best = best;
        for i in 0..n {
            for j in 0..n {
                let p = [
                    lo[0] + (hi[0] - lo[0]) * (i as f64 + 0.5) / n as f64,
                    lo[1] + (hi[1] - lo[1]) * (j as f64 + 0.5) / n as f64,
                ];
                if let Some(d) = clearance(p)
                    && best.is_none_or(|(_, b)| d > b)
                {
                    best = Some((p, d));
                }
            }
        }
        best
    };
    const N: usize = 24;
    let best = search(lo, hi, N, None)?;
    let cell = [(hi[0] - lo[0]) / N as f64, (hi[1] - lo[1]) / N as f64];
    search([best.0[0] - cell[0], best.0[1] - cell[1]], [best.0[0] + cell[0], best.0[1] + cell[1]], 8, Some(best))
}

/// What area labels should keep clear of when they can: boxes around the node symbols and
/// their names, and the Tc paths (each as a zero-area polygon running out and back, so its
/// boundary is the path).
pub fn label_obstacles(p: &Project) -> Vec<Vec<Pt>> {
    let d = p.diagram.symbol_size;
    let symbols = p.nodes.iter().filter(|n| !matches!(n.data, NodeData::Text(_))).map(|n| {
        let [x, y] = n.pos;
        let bottom = if n.name.is_empty() { d * 0.55 } else { d * 0.9 };
        vec![
            [x - d * 0.65, y - d * 0.55],
            [x + d * 0.65, y - d * 0.55],
            [x + d * 0.65, y + bottom],
            [x - d * 0.65, y + bottom],
        ]
    });
    let paths = p
        .nodes
        .iter()
        .filter_map(|n| n.tc_path.as_ref())
        .filter(|t| t.len() >= 2)
        .map(|t| t.iter().chain(t.iter().rev().skip(1)).copied().collect());
    symbols.chain(paths).collect()
}

/// Where to put a label in `region`, with its clearance: clear of `soft` too when the label
/// `fits` (given the clearance) there, else the roomiest spot clear of `hard` only (whether or
/// not it fits, so callers can shrink the label), else the region's centroid with no room.
pub fn place_label(region: &[Pt], hard: &[&[Pt]], soft: &[&[Pt]], fits: impl Fn(f64) -> bool) -> (Pt, f64) {
    let all = [hard, soft].concat();
    label_spot(region, &all)
        .filter(|(_, d)| fits(*d))
        .or_else(|| label_spot(region, hard))
        .unwrap_or((centroid(region), 0.0))
}

/// The letters of a node's Tc path points (every point when no segment ends are recorded).
pub fn tc_path_labels(n: &Node) -> Vec<Option<String>> {
    let len = n.tc_path.as_ref().map_or(0, Vec::len);
    if n.tc_path_breaks.is_empty() {
        (0..len).map(|i| Some(crate::tc::point_label(i))).collect()
    } else {
        crate::tc::path_labels(len, &n.tc_path_breaks)
    }
}

/// The flow type of each leg of a node's stored Tc path, from the Tc segments measured from it.
pub fn tc_leg_flows(n: &Node) -> Vec<Option<crate::tc::PathFlow>> {
    let legs = n.tc_path.as_ref().map_or(0, |p| p.len().saturating_sub(1));
    let NodeData::Subcat(sc) = &n.data else { return vec![None; legs] };
    (0..legs)
        .map(|i| {
            sc.tc
                .iter()
                .find(|s| s.path.is_some_and(|[a, b]| a <= i && i < b))
                .and_then(|s| crate::tc::path_flow_of(&s.kind))
        })
        .collect()
}

/// Why a drawn sub-area cannot be used, if it cannot.
pub fn subarea_problem(shape: &[Pt], outline: &[Pt], others: &[&[Pt]]) -> Option<&'static str> {
    if shape.len() < 3 || polygon_area(shape) <= 0.0 {
        return Some("A sub-area needs at least three points around some area.");
    }
    if !polygon_within(shape, outline) {
        return Some("A sub-area must lie inside its subcatchment's outline.");
    }
    if others.iter().any(|o| polygons_overlap(shape, o)) {
        return Some("Sub-areas cannot overlap each other.");
    }
    None
}

/// Feet per diagram unit: the background calibration, or 1 when uncalibrated.
pub fn feet_per_unit(p: &Project) -> f64 {
    p.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit).filter(|f| f.is_finite() && *f > 0.0).unwrap_or(1.0)
}

/// Recomputes a drawn subcatchment's areas: each drawn sub-area from its outline, and the
/// remainder row (the flagged one, else the first row not drawn, else a new open-space row) as
/// the outline's area less the drawn ones. Returns false, changing nothing, for subcatchments
/// without an outline or without drawn sub-areas.
pub fn sync_subareas(sc: &mut Subcatchment, outline: Option<&[Pt]>, fpu: f64) -> bool {
    let Some(outline) = outline.filter(|o| o.len() >= 3) else { return false };
    if !sc.areas.iter().any(|a| a.outline.is_some()) {
        return false;
    }
    let f2 = fpu * fpu;
    let mut drawn = 0.0;
    for a in sc.areas.iter_mut() {
        if let Some(o) = &a.outline {
            a.area = polygon_area(o) * f2;
            a.remainder = false;
            drawn += a.area;
        }
    }
    let rest = (polygon_area(outline) * f2 - drawn).max(0.0);
    let idx = sc.areas.iter().position(|a| a.remainder).or_else(|| sc.areas.iter().position(|a| a.outline.is_none()));
    match idx {
        Some(i) => {
            for (j, a) in sc.areas.iter_mut().enumerate() {
                a.remainder = j == i;
            }
            if let Some(a) = sc.areas.get_mut(i) {
                a.area = rest;
            }
        }
        None => {
            let mut r = SubArea::new(rest, 61.0, "Open Space, Good, HSG B");
            r.remainder = true;
            sc.areas.push(r);
        }
    }
    true
}

/// After a subcatchment's outline changed from area `old` (sq ft): a drawn subcatchment is
/// re-synced; otherwise the sub-areas are scaled in proportion (one open-space row is created
/// when there are none).
pub fn outline_changed(sc: &mut Subcatchment, outline: &[Pt], old: f64, fpu: f64) {
    if sync_subareas(sc, Some(outline), fpu) {
        return;
    }
    let area = polygon_area(outline) * fpu * fpu;
    let current: f64 = sc.areas.iter().map(|a| a.area).sum();
    if sc.areas.is_empty() {
        sc.areas.push(SubArea::new(area, 61.0, "Open Space, Good, HSG B"));
    } else if current > 0.0 {
        let k = area / if old > 0.0 { old } else { current };
        for a in &mut sc.areas {
            a.area *= k;
        }
    }
}

/// Sets the lengths of a subcatchment's Tc segments measured from its drawn path (those with
/// `TcSegment::path`) from the path's geometry.
pub fn sync_tc_lengths(sc: &mut Subcatchment, path: &[Pt], fpu: f64) {
    for s in &mut sc.tc {
        if let Some([a, b]) = s.path
            && b < path.len()
            && a < b
        {
            let len = polyline_length(&path[a..=b]) * fpu;
            s.kind = s.kind.clone().with_length(len);
        }
    }
}

/// Re-measures a node's drawings: drawn sub-areas and the remainder, Tc segments from the
/// drawn path, and an estimated pond's storage from its footprint.
pub fn sync_node(n: &mut Node, fpu: f64) {
    let outline = n.outline.clone();
    let path = n.tc_path.clone();
    match &mut n.data {
        NodeData::Subcat(sc) => {
            sync_subareas(sc, outline.as_deref(), fpu);
            if let Some(p) = &path {
                sync_tc_lengths(sc, p, fpu);
            }
        }
        NodeData::Pond(pond) => {
            if let (Some(o), Some(est)) = (&outline, pond.estimate)
                && let Ok(v) = pond_storage(o, fpu, &est)
                && let Some(slot) = pond.storage.volumes.iter_mut().find(|v| v.description == POND_ESTIMATE)
            {
                *slot = v;
            }
        }
        _ => {}
    }
}

/// Description of the storage volume the pond estimator writes (and later re-estimates when
/// the footprint or the calibration changes).
pub const POND_ESTIMATE: &str = "Pond estimate";

/// Pond estimator inputs: the drawn outline is the top of the pond, the sides slope inward
/// at `z` horizontal to 1 vertical down to a flat bottom `depth` below the top elevation `top`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PondEstimate {
    pub depth: f64,
    pub z: f64,
    pub top: f64,
}

impl Default for PondEstimate {
    fn default() -> Self {
        PondEstimate { depth: 4.0, z: 3.0, top: 104.0 }
    }
}

impl PondEstimate {
    /// Elevation of the flat bottom.
    pub fn bottom(&self) -> f64 {
        self.top - self.depth
    }
}

/// Area `a`, perimeter `p` and corner sum `k` of a polygon (diagram units), such that the
/// polygon offset inward by `r` (mitred corners) has area `a - p·r + k·r²`. `k` is the sum of
/// tan(φ/2) over the turning angles φ (positive at convex corners, negative at re-entrant
/// ones): 4 for a rectangle, giving (W − 2r)(L − 2r).
fn inset_terms(outline: &[Pt]) -> (f64, f64, f64) {
    let mut pts: Vec<Pt> = Vec::with_capacity(outline.len());
    for q in outline {
        if pts.last().is_none_or(|l| dist(*l, *q) > 1e-9) {
            pts.push(*q);
        }
    }
    while pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| dist(*a, *b) <= 1e-9) {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return (0.0, 0.0, 0.0);
    }
    let signed: f64 = (0..n).map(|i| cross(pts[i], pts[(i + 1) % n])).sum();
    let orient = if signed < 0.0 { -1.0 } else { 1.0 };
    let mut perimeter = 0.0;
    let mut k = 0.0;
    for i in 0..n {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        perimeter += dist(b, c);
        let (e1, e2) = (sub(b, a), sub(c, b));
        let phi = orient * cross(e1, e2).atan2(e1[0] * e2[0] + e1[1] * e2[1]);
        // A near-reversal (a spike) would blow up the mitre; it adds next to no area.
        k += (phi / 2.0).clamp(-1.5, 1.5).tan();
    }
    (polygon_area(&pts), perimeter, k)
}

/// The deepest flat-bottomed pond `outline` can hold with `z`:1 side slopes (ft), where the
/// sloping sides meet; `None` for vertical sides or a shape without area.
pub fn pond_max_depth(outline: &[Pt], fpu: f64, z: f64) -> Option<f64> {
    let (a, p, k) = inset_terms(outline);
    let (a, p) = (a * fpu * fpu, p * fpu);
    if a <= 0.0 || p <= 0.0 || z <= 0.0 {
        return None;
    }
    // Smallest positive root of k·r² − p·r + a = 0 (the stable form also covers k ≤ 0); with
    // no real root the inset shrinks to its least area where the perimeter vanishes.
    let disc = p * p - 4.0 * k * a;
    let r = if disc >= 0.0 { 2.0 * a / (p + disc.sqrt()) } else { p / (2.0 * k) };
    Some(r / z)
}

/// Stage-area storage for a pond whose top is the drawn `outline`: surface areas of the
/// inward-offset outline at stages about every half foot from the bottom to the top, stored
/// by the average-end-area (prismatic) formula. The offset is exact for convex outlines until
/// an edge vanishes, and an estimate for others.
pub fn pond_storage(outline: &[Pt], fpu: f64, est: &PondEstimate) -> Result<StorageVolume, String> {
    let PondEstimate { depth, z, top } = *est;
    if !(depth.is_finite() && depth > 0.0) {
        return Err("Enter a depth greater than zero.".into());
    }
    if !(z.is_finite() && z >= 0.0) {
        return Err("Enter a side slope of zero (vertical) or more.".into());
    }
    if !top.is_finite() {
        return Err("Enter the top elevation.".into());
    }
    let bottom = est.bottom();
    let (a, p, k) = inset_terms(outline);
    let (a, p) = (a * fpu * fpu, p * fpu);
    if a <= 0.0 {
        return Err("The outline has no area.".into());
    }
    if pond_max_depth(outline, fpu, z).is_some_and(|max| depth > max * (1.0 + 1e-9)) {
        return Err("The side slopes meet before reaching that depth: use a smaller depth or a steeper slope.".into());
    }
    let steps = ((depth / 0.5).ceil() as usize).clamp(2, 40);
    let mut elevations = Vec::with_capacity(steps + 1);
    let mut values = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let h = depth * i as f64 / steps as f64;
        let r = z * (depth - h);
        elevations.push(bottom + h);
        values.push((a - p * r + k * r * r).max(0.0));
    }
    Ok(StorageVolume::new(
        POND_ESTIMATE,
        bottom,
        VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations, values },
    ))
}

/// Re-measures every node's drawings (after the calibration changes).
pub fn sync_all(p: &mut Project) {
    let fpu = feet_per_unit(p);
    for n in &mut p.nodes {
        sync_node(n, fpu);
    }
}

/// Inserts a point into a node's Tc path after point `after`: segments spanning that leg grow
/// to include it and later point numbers shift, so the segments keep their legs.
pub fn insert_path_point(n: &mut Node, after: usize, p: Pt) {
    let Some(path) = &mut n.tc_path else { return };
    if after + 1 > path.len() {
        return;
    }
    path.insert(after + 1, p);
    let shift = |i: usize| if i > after { i + 1 } else { i };
    n.tc_path_breaks = n.tc_path_breaks.iter().map(|b| shift(*b)).collect();
    if let NodeData::Subcat(sc) = &mut n.data {
        for s in &mut sc.tc {
            if let Some([a, b]) = s.path {
                s.path = Some([shift(a), shift(b)]);
            }
        }
    }
}

/// Removes point `v` from a node's Tc path (keeping at least two). A segment that ends there is
/// shortened and the next one takes over the joined leg; a segment left with no leg is removed
/// (its neighbour absorbs it). Returns false when the point cannot be removed.
pub fn remove_path_point(n: &mut Node, v: usize) -> bool {
    let Some(path) = &mut n.tc_path else { return false };
    if path.len() <= 2 || v >= path.len() {
        return false;
    }
    path.remove(v);
    let last = path.len() - 1;
    let map = |i: usize| {
        if i > v {
            i - 1
        } else if i == v {
            v.saturating_sub(1).min(last)
        } else {
            i
        }
    };
    let mut breaks: Vec<usize> = n.tc_path_breaks.iter().map(|b| if *b == v && v == 0 { 0 } else { map(*b) }).collect();
    breaks.dedup();
    n.tc_path_breaks = breaks;
    if let NodeData::Subcat(sc) = &mut n.data {
        for s in &mut sc.tc {
            if let Some([a, b]) = s.path {
                // The first point: the path now starts at the old second point.
                let (a, b) = if v == 0 { (a.saturating_sub(1), b.saturating_sub(1)) } else { (map(a), map(b)) };
                s.path = Some([a, b]);
            }
        }
        sc.tc.retain(|s| s.path.is_none_or(|[a, b]| a < b));
    }
    true
}

/// Applies `f` to every drawing in the project (subcatchment outlines and their sub-areas, Tc
/// paths) and moves the symbols of subcatchments that have an outline with them, so drawings
/// stay registered to a background that is moved or scaled by the same `f`.
pub fn transform(p: &mut Project, f: &dyn Fn(Pt) -> Pt) {
    for n in &mut p.nodes {
        let has_outline = n.outline.is_some();
        if let Some(o) = &mut n.outline {
            o.iter_mut().for_each(|q| *q = f(*q));
        }
        if let Some(t) = &mut n.tc_path {
            t.iter_mut().for_each(|q| *q = f(*q));
        }
        if let NodeData::Subcat(sc) = &mut n.data {
            for a in &mut sc.areas {
                if let Some(o) = &mut a.outline {
                    o.iter_mut().for_each(|q| *q = f(*q));
                }
            }
        }
        if has_outline {
            n.pos = f(n.pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::NodeType;
    use crate::tc::{PathFlow, PathLeg, TcSegment, segments_from_path};

    #[test]
    fn label_spot_avoids_holes() {
        let square = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let (p, d) = label_spot(&square, &[]).unwrap();
        assert!((p[0] - 50.0).abs() < 3.0 && (p[1] - 50.0).abs() < 3.0 && d > 47.0, "{p:?} {d}");
        // The left 60% is a drawn sub-area: the label goes in the strip to its right.
        let left: [Pt; 4] = [[0.0, 0.0], [60.0, 0.0], [60.0, 100.0], [0.0, 100.0]];
        let (p, d) = label_spot(&square, &[&left]).unwrap();
        assert!(p[0] > 60.0 && (d - 20.0).abs() < 2.0, "{p:?} {d}");
        assert!(!point_in_polygon(p, &left));
        assert!(label_spot(&square, &[&square]).is_none_or(|(_, d)| d < 1e-6));
        assert!(label_spot(&square[..2], &[]).is_none());
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Pt> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn polygon_measures() {
        let r = rect(0.0, 0.0, 10.0, 4.0);
        assert_eq!(polygon_area(&r), 40.0);
        assert_eq!(centroid(&r), [5.0, 2.0]);
        assert!(point_in_polygon([1.0, 1.0], &r) && !point_in_polygon([11.0, 1.0], &r));
        assert_eq!(clamp_inside([15.0, 2.0], &r), [10.0, 2.0]);
        assert_eq!(clamp_inside([3.0, 3.0], &r), [3.0, 3.0]);
        // Triangles of a concave polygon cover exactly its area.
        let l = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [4.0, 4.0], [4.0, 10.0], [0.0, 10.0]];
        for poly in [l.clone(), l.iter().rev().copied().collect()] {
            let tris = triangulate(&poly);
            assert_eq!(tris.len(), 4);
            let sum: f64 = tris.iter().map(|t| polygon_area(&[poly[t[0]], poly[t[1]], poly[t[2]]])).sum();
            assert!((sum - polygon_area(&poly)).abs() < 1e-9, "{sum}");
        }
    }

    #[test]
    fn containment_and_overlap() {
        // An L-shaped (concave) outline.
        let outline = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [4.0, 4.0], [4.0, 10.0], [0.0, 10.0]];
        assert!(polygon_within(&rect(1.0, 1.0, 3.0, 3.0), &outline));
        // Touching the outline is fine.
        assert!(polygon_within(&rect(0.0, 0.0, 4.0, 4.0), &outline));
        // Every corner is inside, but the long edge cuts across the notch.
        assert!(!polygon_within(&[[2.0, 2.0], [8.0, 2.0], [2.0, 8.0]], &outline));
        assert!(!polygon_within(&rect(5.0, 5.0, 6.0, 6.0), &outline), "in the notch");
        // Neighbours sharing an edge do not overlap; nested or crossing shapes do.
        assert!(!polygons_overlap(&rect(0.0, 0.0, 2.0, 2.0), &rect(2.0, 0.0, 4.0, 2.0)));
        assert!(polygons_overlap(&rect(0.0, 0.0, 2.0, 2.0), &rect(1.0, 1.0, 3.0, 3.0)));
        assert!(polygons_overlap(&rect(0.0, 0.0, 4.0, 4.0), &rect(1.0, 1.0, 2.0, 2.0)));
        assert!(polygons_overlap(&rect(0.0, 0.0, 2.0, 2.0), &rect(0.0, 0.0, 2.0, 2.0)), "identical");
        let others: [&[Pt]; 1] = [&rect(1.0, 1.0, 2.0, 2.0)];
        assert_eq!(
            subarea_problem(&rect(1.5, 1.5, 3.0, 3.0), &outline, &others),
            Some("Sub-areas cannot overlap each other.")
        );
        assert_eq!(subarea_problem(&rect(2.0, 1.0, 3.0, 2.0), &outline, &others), None);
    }

    #[test]
    fn drawn_subareas_and_remainder() {
        let outline = rect(0.0, 0.0, 100.0, 100.0); // 10,000 units
        let mut sc = Subcatchment {
            areas: vec![SubArea::new(5000.0, 61.0, "Open space")],
            tc: Vec::new(),
            uh: None,
            large_units: true,
        };
        assert!(!sync_subareas(&mut sc, Some(&outline), 2.0), "nothing drawn yet");
        let mut roof = SubArea::new(0.0, 98.0, "Roofs");
        roof.outline = Some(rect(0.0, 0.0, 10.0, 20.0)); // 200 units
        sc.areas.push(roof);
        assert!(sync_subareas(&mut sc, Some(&outline), 2.0));
        // 2 ft per unit: 800 sq ft drawn, the open-space row becomes the remainder.
        assert_eq!(sc.areas[1].area, 800.0);
        assert!(sc.areas[0].remainder);
        assert_eq!(sc.areas[0].area, 40_000.0 - 800.0);
        assert_eq!(sc.areas.iter().map(|a| a.area).sum::<f64>(), 40_000.0, "the rows add up to the outline");
        // With only drawn rows, a remainder row is added.
        let mut only = Subcatchment { areas: vec![sc.areas[1].clone()], tc: Vec::new(), uh: None, large_units: true };
        sync_subareas(&mut only, Some(&outline), 1.0);
        assert_eq!(only.areas.len(), 2);
        assert!(only.areas[1].remainder && only.areas[1].area == 9_800.0);
        // An undrawn subcatchment scales its rows with the outline.
        let mut plain = Subcatchment {
            areas: vec![SubArea::new(300.0, 70.0, "a"), SubArea::new(100.0, 98.0, "b")],
            tc: Vec::new(),
            uh: None,
            large_units: true,
        };
        outline_changed(&mut plain, &rect(0.0, 0.0, 40.0, 20.0), 400.0, 1.0);
        assert_eq!((plain.areas[0].area, plain.areas[1].area), (600.0, 200.0));
    }

    #[test]
    fn path_edits_keep_segments() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let n = p.node_mut(id).unwrap();
        // A to B sheet (0-1), B to C shallow (1-3, two legs).
        n.tc_path = Some(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [100.0, 150.0]]);
        let legs = [
            PathLeg::new(100.0, PathFlow::Sheet),
            PathLeg::new(50.0, PathFlow::Shallow),
            PathLeg::new(100.0, PathFlow::Shallow),
        ];
        n.tc_path_breaks = vec![0, 1, 3];
        if let NodeData::Subcat(sc) = &mut n.data {
            sc.tc = segments_from_path(&legs, &[]);
        }
        let lens = |n: &Node| match &n.data {
            NodeData::Subcat(sc) => sc.tc.iter().map(|s: &TcSegment| (s.kind.length(), s.path)).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        // Moving a point re-measures the segments that use it.
        n.tc_path.as_mut().unwrap()[3] = [100.0, 250.0];
        sync_node(n, 1.0);
        assert_eq!(lens(n), [(100.0, Some([0, 1])), (250.0, Some([1, 3]))]);
        // Inserting a point inside the shallow segment keeps it one segment.
        insert_path_point(n, 2, [100.0, 100.0]);
        assert_eq!(n.tc_path_breaks, [0, 1, 4]);
        assert_eq!(lens(n)[1].1, Some([1, 4]));
        // Removing the A–B break merges: the sheet segment is gone only if it has no leg left.
        assert!(remove_path_point(n, 1));
        sync_node(n, 1.0);
        let l = lens(n);
        assert_eq!(l.len(), 1, "the sheet segment lost its only leg: {l:?}");
        assert_eq!(l[0].1, Some([0, 3]));
        assert!((l[0].0 - (100f64.hypot(50.0) + 50.0 + 150.0)).abs() < 1e-9);
        // Removing the first point shortens the path from its start.
        assert!(remove_path_point(n, 0));
        assert_eq!(lens(n)[0].1, Some([0, 2]));
        // A path keeps at least two points.
        assert!(remove_path_point(n, 0));
        assert!(!remove_path_point(n, 0));
    }

    #[test]
    fn transform_moves_drawings_with_symbol() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Subcat, [5.0, 5.0]);
        let n = p.node_mut(id).unwrap();
        n.outline = Some(rect(0.0, 0.0, 10.0, 10.0));
        transform(&mut p, &|q| [q[0] * 2.0 + 1.0, q[1] * 2.0]);
        let n = p.node(id).unwrap();
        assert_eq!(n.pos, [11.0, 10.0]);
        assert_eq!(n.outline.as_ref().unwrap()[2], [21.0, 20.0]);
    }

    #[test]
    fn pond_estimate_matches_prismatoid() {
        // A 100 x 60 ft top, 4 ft deep at 3:1: the bottom is 76 x 36 ft, as a prismatoid.
        let est = PondEstimate { depth: 4.0, z: 3.0, top: 104.0 };
        assert_eq!(est.bottom(), 100.0);
        let top = rect(0.0, 0.0, 50.0, 30.0); // 2 ft per unit
        let v = pond_storage(&top, 2.0, &est).unwrap();
        let VolumeShape::Custom { kind, elevations, values } = &v.shape else { panic!() };
        assert_eq!(*kind, CustomKind::AreaPrismatic);
        assert_eq!((elevations[0], *elevations.last().unwrap()), (100.0, 104.0));
        assert!((values[0] - 76.0 * 36.0).abs() < 1e-6 && (values.last().unwrap() - 6000.0).abs() < 1e-6);
        let exact =
            StorageVolume::new("", 100.0, VolumeShape::Prismatoid { width: 36.0, length: 76.0, height: 4.0, z: 3.0 });
        let (vol, want) = (v.gross(104.0), exact.gross(104.0));
        // Average end areas over half-foot stages overstate a quadratic area slightly.
        assert!((vol - want).abs() / want < 1e-3, "{vol} vs {want}");
        // Clockwise points give the same storage.
        let mut cw = top.clone();
        cw.reverse();
        assert_eq!(pond_storage(&cw, 2.0, &est).unwrap(), v);
        // The sides of a 60 ft wide pond meet 30 ft in, 10 ft down at 3:1.
        assert!((pond_max_depth(&top, 2.0, 3.0).unwrap() - 10.0).abs() < 1e-9);
        assert!(pond_storage(&top, 2.0, &PondEstimate { depth: 11.0, ..est }).is_err());
        assert!(pond_storage(&top, 2.0, &PondEstimate { depth: 0.0, ..est }).is_err());
        assert!(pond_storage(&top[..2], 2.0, &est).is_err());
        // Vertical sides: the area is the same at every stage.
        let box_ = pond_storage(&top, 2.0, &PondEstimate { z: 0.0, ..est }).unwrap();
        assert!((box_.gross(104.0) - 6000.0 * 4.0).abs() < 1e-6);
    }

    #[test]
    fn pond_estimate_handles_concave_outlines() {
        // An L of two 10-wide arms (area 300): inset by 1 the arms are 8 wide, 18 and 10 long.
        let l = vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]];
        let v = pond_storage(&l, 1.0, &PondEstimate { depth: 1.0, z: 1.0, top: 1.0 }).unwrap();
        let VolumeShape::Custom { values, .. } = &v.shape else { panic!() };
        assert!((values[0] - (18.0 * 8.0 + 8.0 * 10.0)).abs() < 1e-9, "{values:?}");
        assert!((pond_max_depth(&l, 1.0, 1.0).unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn estimated_pond_follows_its_footprint() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Pond, [0.0, 0.0]);
        let est = PondEstimate::default();
        let n = p.node_mut(id).unwrap();
        n.outline = Some(rect(0.0, 0.0, 100.0, 60.0));
        let NodeData::Pond(pond) = &mut n.data else { panic!() };
        pond.estimate = Some(est);
        pond.storage.volumes.push(StorageVolume::new(
            "Other",
            100.0,
            VolumeShape::Prismatoid { width: 1.0, length: 1.0, height: 1.0, z: 0.0 },
        ));
        pond.storage.volumes.push(pond_storage(&rect(0.0, 0.0, 100.0, 60.0), 1.0, &est).unwrap());
        // Calibrating at 2 ft per unit quadruples the top area; other volumes stay.
        p.diagram.underlay = Some(crate::project::Underlay {
            file_name: String::new(),
            bytes: Vec::new(),
            page: 0,
            origin: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            feet_per_unit: Some(2.0),
            opacity: 1.0,
            visible: true,
            locked: false,
        });
        sync_all(&mut p);
        let NodeData::Pond(pond) = &p.node(id).unwrap().data else { panic!() };
        assert_eq!(pond.storage.volumes[0].description, "Other");
        assert_eq!(pond.storage.volumes[1].surface_area(104.0), Some(24_000.0));
    }
}
