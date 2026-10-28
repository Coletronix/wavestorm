//! Hydrologic soil groups from the USDA NRCS soil survey (SSURGO) for an area of interest.
//!
//! The area of interest (AOI) is sent as a WGS 84 polygon to NRCS Soil Data Access, the public
//! web service behind Web Soil Survey, which returns the soil map unit polygons clipped to it
//! with each map unit's hydrologic soil group ("dominant condition", the rating Web Soil
//! Survey shows). The map is kept in the project, so it works offline afterwards.
//!
//! With a georeferenced background the map units are projected onto the diagram and each
//! subcatchment's sub-areas are split by soil group (TR-55 Table 2-2 curve numbers for the
//! row's cover). Dual groups (A/D, B/D, C/D) are D unless the site is drained (NRCS NEH
//! Part 630 Chapter 7), when the first letter applies. Network access lives in the UI.

use crate::cn::{CN_TABLE, CnRow, Hsg, SubArea};
use crate::drawing::{Pt, polygon_area, triangulate};
use crate::geo::{self, LonLat};
use crate::project::{NodeData, Project, Subcatchment, Underlay};
use serde::{Deserialize, Serialize};

/// NRCS Soil Data Access tabular query endpoint (accepts spatial macros; allows browser use).
pub const SDA_URL: &str = "https://SDMDataAccess.sc.egov.usda.gov/Tabular/post.rest";

/// Largest area of interest accepted (acres). Web Soil Survey's own limit is 100,000 acres;
/// a stormwater site is far smaller and a smaller request keeps the project file small.
pub const MAX_AOI_ACRES: f64 = 10_000.0;

/// Soil groups covering less than this fraction of a shape are left out of its split.
pub const MIN_PART: f64 = 0.01;

const ACRE: f64 = 43_560.0;

/// A soil map unit (one SSURGO map unit key).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapUnit {
    pub mukey: String,
    /// Map unit symbol, e.g. "L138B".
    pub symbol: String,
    pub name: String,
    /// Hydrologic soil group as published ("B", "B/D", or empty when not rated).
    pub hydgrp: String,
}

/// One clipped map unit polygon: its outer ring then any holes, in longitude/latitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoilPolygon {
    pub mukey: String,
    pub rings: Vec<Vec<LonLat>>,
}

/// The soil map of a project's area of interest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoilMap {
    /// The area of interest (longitude/latitude, not closed).
    pub aoi: Vec<LonLat>,
    pub units: Vec<MapUnit>,
    pub polygons: Vec<SoilPolygon>,
    /// The site is drained: dual groups take their first letter instead of D.
    #[serde(default)]
    pub drained: bool,
    /// Date the data was retrieved (m/d/yyyy).
    #[serde(default)]
    pub retrieved: String,
    /// Draw the map units on the diagram.
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool {
    true
}

impl SoilMap {
    pub fn unit(&self, mukey: &str) -> Option<&MapUnit> {
        self.units.iter().find(|u| u.mukey == mukey)
    }

    /// The soil group used for a map unit, after the drained setting.
    pub fn group_of(&self, mukey: &str) -> Option<Hsg> {
        self.unit(mukey).and_then(|u| effective_hsg(&u.hydgrp, self.drained))
    }
}

/// The soil group used for a published rating: a dual group is D, or its first letter when the
/// site is drained. Empty or unknown ratings give `None`.
pub fn effective_hsg(hydgrp: &str, drained: bool) -> Option<Hsg> {
    let s = hydgrp.trim().to_ascii_uppercase();
    let letter = |c: Option<char>| match c? {
        'A' => Some(Hsg::A),
        'B' => Some(Hsg::B),
        'C' => Some(Hsg::C),
        'D' => Some(Hsg::D),
        _ => None,
    };
    match s.split_once('/') {
        Some((first, second)) => {
            if drained {
                letter(first.trim().chars().next())
            } else {
                letter(second.trim().chars().next())
            }
        }
        None if s.len() == 1 => letter(s.chars().next()),
        None => None,
    }
}

/// Map colour of a soil group (green A to red D; grey when unrated), on screen and in print.
pub fn group_rgb(g: Option<Hsg>) -> (u8, u8, u8) {
    match g {
        Some(Hsg::A) => (56, 168, 0),
        Some(Hsg::B) => (230, 200, 0),
        Some(Hsg::C) => (245, 130, 20),
        Some(Hsg::D) => (215, 40, 40),
        None => (140, 140, 140),
    }
}

/// Area (sq ft) of a longitude/latitude ring, projected on the plane tangent at `origin`.
pub fn lon_lat_area(ring: &[LonLat], origin: LonLat) -> f64 {
    let pts: Vec<Pt> = ring.iter().map(|p| geo::offset_feet(origin, *p)).collect();
    polygon_area(&pts)
}

/// The vertex mean of a ring (a site's reference point).
pub fn center(ring: &[LonLat]) -> LonLat {
    let n = ring.len().max(1) as f64;
    let s = ring.iter().fold([0.0, 0.0], |s, p| [s[0] + p[0], s[1] + p[1]]);
    [s[0] / n, s[1] / n]
}

/// The JSON body of the Soil Data Access request for the map units clipped to `aoi`
/// (longitude/latitude). The polygon is repaired (`MakeValid`) so a crossed outline still works.
pub fn sda_request(aoi: &[LonLat]) -> Result<String, String> {
    if aoi.len() < 3 || aoi.iter().any(|p| !geo::valid_lon_lat(*p)) {
        return Err("The area of interest needs at least three points with valid coordinates.".into());
    }
    let acres = lon_lat_area(aoi, center(aoi)) / ACRE;
    if acres > MAX_AOI_ACRES {
        return Err(format!(
            "The area of interest is {acres:.0} acres; the limit is {MAX_AOI_ACRES:.0} acres. Draw a smaller one."
        ));
    }
    let mut ring: Vec<String> = aoi.iter().map(|p| format!("{:.7} {:.7}", p[0], p[1])).collect();
    if let Some(first) = ring.first().cloned() {
        ring.push(first);
    }
    let query = format!(
        "~DeclareGeometry(@aoi)~ \
         select @aoi = geometry::STGeomFromText('POLYGON(({}))', 4326).MakeValid() \
         ~DeclareIdGeomTable(@clip)~ \
         ~GetClippedMapunits(@aoi,polygon,geo,@clip)~ \
         select c.id as mukey, m.musym, m.muname, a.hydgrpdcd, c.geom.STAsText() as wkt \
         from @clip c join mapunit m on m.mukey = c.id left join muaggatt a on a.mukey = c.id",
        ring.join(",")
    );
    Ok(serde_json::json!({ "format": "JSON+COLUMNNAME", "query": query }).to_string())
}

/// Reads a Soil Data Access reply into a soil map of `aoi`. Service errors (an XML exception
/// report) and an empty result (no survey there) are errors.
pub fn parse_sda(body: &str, aoi: &[LonLat], drained: bool, retrieved: &str) -> Result<SoilMap, String> {
    if let Some(i) = body.find("<ServiceException>") {
        let msg = body.get(i + 18..).unwrap_or_default();
        let msg = msg.split("</ServiceException>").next().unwrap_or_default().trim().replace("&#39;", "'");
        return Err(format!("Soil Data Access refused the request: {msg}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "Soil Data Access sent an unreadable reply.".to_string())?;
    let rows = v.get("Table").and_then(|t| t.as_array()).cloned().unwrap_or_default();
    let header: Vec<String> = rows
        .first()
        .and_then(|r| r.as_array())
        .map(|r| r.iter().map(|c| c.as_str().unwrap_or_default().to_ascii_lowercase()).collect())
        .unwrap_or_default();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (Some(ck), Some(cs), Some(cn), Some(cg), Some(cw)) =
        (col("mukey"), col("musym"), col("muname"), col("hydgrpdcd"), col("wkt"))
    else {
        return Err("The soil survey has no map units in this area (it may be outside the surveyed United States, \
                    or the area of interest is too small)."
            .into());
    };
    let mut map = SoilMap {
        aoi: aoi.to_vec(),
        units: Vec::new(),
        polygons: Vec::new(),
        drained,
        retrieved: retrieved.into(),
        visible: true,
    };
    for row in rows.iter().skip(1) {
        let Some(r) = row.as_array() else { continue };
        let text = |i: usize| match r.get(i) {
            Some(serde_json::Value::String(s)) => s.trim().to_string(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let mukey = text(ck);
        if mukey.is_empty() {
            continue;
        }
        if map.unit(&mukey).is_none() {
            map.units.push(MapUnit { mukey: mukey.clone(), symbol: text(cs), name: text(cn), hydgrp: text(cg) });
        }
        for rings in parse_wkt(&text(cw)) {
            if rings.first().is_some_and(|r| r.len() >= 3) {
                map.polygons.push(SoilPolygon { mukey: mukey.clone(), rings });
            }
        }
    }
    if map.polygons.is_empty() {
        return Err("The soil survey returned no map unit polygons for this area.".into());
    }
    map.units.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(map)
}

/// The polygons (each a list of rings, outer first) in WKT text: POLYGON, MULTIPOLYGON and
/// the polygons inside a GEOMETRYCOLLECTION. Other geometry (slivers clipped to lines or
/// points) and malformed text are skipped. Closing points are dropped.
pub fn parse_wkt(text: &str) -> Vec<Vec<Vec<LonLat>>> {
    let mut w = Wkt { s: text.as_bytes(), i: 0 };
    let mut out = Vec::new();
    w.geometry(&mut out);
    out
}

struct Wkt<'a> {
    s: &'a [u8],
    i: usize,
}

impl Wkt<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }
    fn word(&mut self) -> String {
        self.skip_ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        String::from_utf8_lossy(self.s.get(start..self.i).unwrap_or_default()).to_ascii_uppercase()
    }
    fn eat(&mut self, c: u8) -> bool {
        self.skip_ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn number(&mut self) -> Option<f64> {
        self.skip_ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.i += 1;
        }
        std::str::from_utf8(self.s.get(start..self.i)?).ok()?.parse().ok()
    }
    /// Skips a balanced parenthesised group (or nothing).
    fn skip_group(&mut self) {
        self.skip_ws();
        if self.s.get(self.i) != Some(&b'(') {
            return;
        }
        let mut depth = 0;
        while let Some(c) = self.s.get(self.i) {
            self.i += 1;
            match c {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
    fn geometry(&mut self, out: &mut Vec<Vec<Vec<LonLat>>>) {
        let kind = self.word();
        // "EMPTY" or a Z/M qualifier.
        let save = self.i;
        let next = self.word();
        if next == "EMPTY" {
            return;
        }
        if next.is_empty() || !matches!(next.as_str(), "Z" | "M" | "ZM") {
            self.i = save;
        }
        match kind.as_str() {
            "POLYGON" => {
                if let Some(p) = self.rings() {
                    out.push(p);
                }
            }
            "MULTIPOLYGON" => {
                if !self.eat(b'(') {
                    return;
                }
                loop {
                    match self.rings() {
                        Some(p) => out.push(p),
                        None => return,
                    }
                    if !self.eat(b',') {
                        break;
                    }
                }
                self.eat(b')');
            }
            "GEOMETRYCOLLECTION" => {
                if !self.eat(b'(') {
                    return;
                }
                loop {
                    self.geometry(out);
                    if !self.eat(b',') {
                        break;
                    }
                }
                self.eat(b')');
            }
            _ => self.skip_group(),
        }
    }
    fn rings(&mut self) -> Option<Vec<Vec<LonLat>>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut rings = Vec::new();
        loop {
            rings.push(self.ring()?);
            if !self.eat(b',') {
                break;
            }
        }
        self.eat(b')').then_some(rings)
    }
    fn ring(&mut self) -> Option<Vec<LonLat>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut pts: Vec<LonLat> = Vec::new();
        loop {
            let x = self.number()?;
            let y = self.number()?;
            // Ignore Z/M values.
            while self.s.get(self.i).is_some_and(|c| *c == b' ') && self.peek_number() {
                self.number()?;
            }
            pts.push([x, y]);
            if !self.eat(b',') {
                break;
            }
        }
        if !self.eat(b')') {
            return None;
        }
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        Some(pts)
    }
    fn peek_number(&self) -> bool {
        let mut j = self.i;
        while self.s.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
            j += 1;
        }
        self.s.get(j).is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'.')
    }
}

/// Area of a convex polygon clipped by a convex polygon (Sutherland–Hodgman), both
/// counter-clockwise or both clockwise.
fn convex_clip_area(subject: &[Pt], clip: &[Pt], ccw: bool) -> f64 {
    let mut poly = subject.to_vec();
    let n = clip.len();
    for k in 0..n {
        if poly.is_empty() {
            return 0.0;
        }
        let (a, b) = (clip[k], clip[(k + 1) % n]);
        let side = |p: Pt| {
            let s = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            if ccw { s } else { -s }
        };
        let input = std::mem::take(&mut poly);
        for i in 0..input.len() {
            let (p, q) = (input[i], input[(i + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                poly.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                poly.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
    }
    polygon_area(&poly)
}

/// A simple polygon prepared for overlap areas: its triangles (counter-clockwise) and bounds.
#[derive(Debug, Clone)]
pub struct Prepared {
    tris: Vec<[Pt; 3]>,
    bounds: [f64; 4],
}

fn bounds_of(p: &[Pt]) -> [f64; 4] {
    p.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, q| {
        [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])]
    })
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

impl Prepared {
    pub fn new(p: &[Pt]) -> Self {
        let tris = triangulate(p)
            .into_iter()
            .filter_map(|[i, j, k]| {
                let (a, b, c) = (*p.get(i)?, *p.get(j)?, *p.get(k)?);
                let turn = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                (turn.abs() > 0.0).then_some(if turn > 0.0 { [a, b, c] } else { [a, c, b] })
            })
            .collect();
        Prepared { tris, bounds: bounds_of(p) }
    }

    /// Area shared with another prepared polygon.
    pub fn overlap(&self, other: &Prepared) -> f64 {
        if !overlaps(&self.bounds, &other.bounds) {
            return 0.0;
        }
        let tb: Vec<[f64; 4]> = other.tris.iter().map(|t| bounds_of(t)).collect();
        let mut sum = 0.0;
        for t in &self.tris {
            let b = bounds_of(t);
            for (u, ub) in other.tris.iter().zip(&tb) {
                if overlaps(&b, ub) {
                    sum += convex_clip_area(t, u, true);
                }
            }
        }
        sum
    }
}

/// Area shared by two simple polygons (either may be concave).
pub fn intersection_area(a: &[Pt], b: &[Pt]) -> f64 {
    Prepared::new(a).overlap(&Prepared::new(b))
}

/// A soil polygon projected onto the diagram.
#[derive(Debug, Clone)]
pub struct DiagramSoil {
    pub mukey: String,
    pub group: Option<Hsg>,
    /// Outer ring then holes, in diagram units.
    pub rings: Vec<Vec<Pt>>,
    prepared: Vec<Prepared>,
}

/// The soil map projected onto the diagram, when the background is georeferenced.
pub fn on_diagram(map: &SoilMap, u: &Underlay) -> Option<Vec<DiagramSoil>> {
    let to = geo::projector(u)?;
    Some(
        map.polygons
            .iter()
            .map(|p| {
                let rings: Vec<Vec<Pt>> = p.rings.iter().map(|r| r.iter().map(|q| to(*q)).collect()).collect();
                let prepared = rings.iter().map(|r| Prepared::new(r)).collect();
                DiagramSoil { mukey: p.mukey.clone(), group: map.group_of(&p.mukey), rings, prepared }
            })
            .collect(),
    )
}

/// The project's soil map on the diagram (needs a georeferenced, calibrated background).
pub fn project_soils(p: &Project) -> Option<Vec<DiagramSoil>> {
    on_diagram(p.soils.as_ref()?, p.diagram.underlay.as_ref()?)
}

/// Area (diagram units²) of `shape` on each soil group A–D, and on unrated or unmapped ground
/// (index 4).
pub fn group_areas(shape: &[Pt], soils: &[DiagramSoil]) -> [f64; 5] {
    let mut out = [0.0; 5];
    if shape.len() < 3 {
        return out;
    }
    let s = Prepared::new(shape);
    let total = polygon_area(shape);
    let mut mapped = 0.0;
    for soil in soils {
        let mut a = 0.0;
        for (i, r) in soil.prepared.iter().enumerate() {
            let x = s.overlap(r);
            if i == 0 { a += x } else { a -= x }
        }
        let a = a.max(0.0);
        mapped += a;
        if let Some(slot) = out.get_mut(soil.group.map_or(4, |g| g as usize)) {
            *slot += a;
        }
    }
    // Ground the map does not cover (outside the AOI) counts as unrated.
    out[4] += (total - mapped).max(0.0);
    out
}

/// The TR-55 table row for a sub-area description ("Woods, Good, HSG B" → "Woods, Good").
pub fn cover_row(description: &str) -> Option<&'static CnRow> {
    let base = strip_hsg(description);
    CN_TABLE.iter().find(|r| strip_hsg(&r.description(Hsg::B)).eq_ignore_ascii_case(base))
}

/// A description without its trailing ", HSG x".
pub fn strip_hsg(description: &str) -> &str {
    let upper = description.to_ascii_uppercase();
    match upper.rfind(", HSG ") {
        Some(i) => description.get(..i).unwrap_or(description).trim_end(),
        None => description.trim_end(),
    }
}

/// The curve number for `g`, or the nearest group the table rates (TR-55 gives no HSG A curve
/// numbers for some arid rangeland covers).
fn cn_or_nearest(row: &CnRow, g: Hsg) -> Option<(Hsg, u8)> {
    let i = g as usize;
    let mut order: Vec<Hsg> = Hsg::ALL.to_vec();
    order.sort_by_key(|h| (*h as usize).abs_diff(i));
    order.into_iter().find_map(|h| row.cn_for(h).map(|cn| (h, cn)))
}

/// What applying soils did to one subcatchment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubcatSoils {
    pub label: String,
    /// Rows before and after.
    pub rows: (usize, usize),
    /// Fraction of the outline on rated mapped soil.
    pub coverage: f64,
    /// Rows left unchanged because their cover is not a TR-55 table cover.
    pub custom: Vec<String>,
}

/// Rows that differ only in their soil group (as a split leaves them) belong together.
fn same_row(a: &SubArea, b: &SubArea) -> bool {
    strip_hsg(&a.description).eq_ignore_ascii_case(strip_hsg(&b.description))
        && a.outline == b.outline
        && a.remainder == b.remainder
        && a.c == b.c
        && a.land_use == b.land_use
}

/// Splits a subcatchment's sub-areas by soil group. Rows split earlier are merged back first,
/// so it can be repeated after drawings or the soil map change. Each row's ground is its own
/// drawing, the outline less the drawn sub-areas for the remainder (and any typed rows of a
/// drawn subcatchment), or the whole outline for a subcatchment without drawn sub-areas (the
/// typed covers are assumed spread evenly over it). Unrated and unmapped ground is shared out
/// among the rated groups; groups under [`MIN_PART`] are left out. A row whose cover has the same
/// curve number on every group present becomes one row on the dominant group.
pub fn split_subcat(sc: &mut Subcatchment, outline: &[Pt], soils: &[DiagramSoil], fpu: f64) -> SubcatSoils {
    let mut report = SubcatSoils { rows: (sc.areas.len(), 0), ..Default::default() };
    // Merge rows split earlier.
    let mut merged: Vec<SubArea> = Vec::new();
    for a in sc.areas.drain(..) {
        match merged.iter_mut().find(|m| same_row(m, &a)) {
            Some(m) => m.area += a.area,
            None => merged.push(a),
        }
    }
    let whole = group_areas(outline, soils);
    let rated: f64 = whole.iter().take(4).sum();
    let total = polygon_area(outline);
    report.coverage = if total > 0.0 { (rated / total).min(1.0) } else { 0.0 };
    let has_drawn = merged.iter().any(|a| a.outline.is_some());
    let mut rest = whole;
    for a in merged.iter().filter_map(|a| a.outline.as_deref()) {
        for (r, d) in rest.iter_mut().zip(group_areas(a, soils)) {
            *r = (*r - d).max(0.0);
        }
    }
    for row in merged {
        let parts = match (&row.outline, has_drawn) {
            (Some(o), _) => group_areas(o, soils),
            (None, true) => rest,
            (None, false) => whole,
        };
        let sum: f64 = parts.iter().take(4).sum();
        let cover = cover_row(&row.description);
        let (Some(cover), true) = (cover, sum > 0.0) else {
            if cover.is_none() && sum > 0.0 {
                report.custom.push(row.description.clone());
            }
            sc.areas.push(row);
            continue;
        };
        let mut groups: Vec<(Hsg, f64)> =
            Hsg::ALL.iter().zip(parts).map(|(g, a)| (*g, a / sum)).filter(|(_, f)| *f >= MIN_PART).collect();
        let kept: f64 = groups.iter().map(|g| g.1).sum();
        groups.iter_mut().for_each(|g| g.1 /= kept);
        let cns: Vec<Option<(Hsg, u8)>> = groups.iter().map(|(g, _)| cn_or_nearest(cover, *g)).collect();
        let one_cn = cns.windows(2).all(|w| w[0].map(|x| x.1) == w[1].map(|x| x.1));
        if one_cn {
            groups.sort_by(|a, b| b.1.total_cmp(&a.1));
            groups.truncate(1);
            if let Some(g) = groups.first_mut() {
                g.1 = 1.0;
            }
        }
        for (g, f) in groups {
            let Some((_, cn)) = cn_or_nearest(cover, g) else { continue };
            let mut part = row.clone();
            part.area = row.area * f;
            part.cn = cn as f64;
            part.hsg = Some(g);
            part.description = cover.description(g);
            sc.areas.push(part);
        }
    }
    crate::drawing::sync_subareas(sc, Some(outline), fpu);
    report.rows.1 = sc.areas.len();
    report
}

/// Splits every outlined subcatchment's sub-areas by soil group. Returns one entry per
/// subcatchment changed, and the labels of those without an outline (skipped).
pub fn apply(p: &mut Project) -> Result<(Vec<SubcatSoils>, Vec<String>), String> {
    let soils = project_soils(p).ok_or("Georeference the background before applying soils.")?;
    let fpu = crate::drawing::feet_per_unit(p);
    let (mut done, mut skipped) = (Vec::new(), Vec::new());
    for n in &mut p.nodes {
        let label = n.label();
        let outline = n.outline.clone();
        let NodeData::Subcat(sc) = &mut n.data else { continue };
        match outline.filter(|o| o.len() >= 3) {
            Some(o) => {
                let mut r = split_subcat(sc, &o, &soils, fpu);
                r.label = label;
                done.push(r);
            }
            None => skipped.push(label),
        }
    }
    Ok((done, skipped))
}

/// The default area of interest: the outlines of all subcatchments (their convex hull) grown
/// by 10 % (at least 50 ft), in diagram units. `None` without outlines.
pub fn default_aoi(p: &Project) -> Option<Vec<Pt>> {
    let pts: Vec<Pt> = p
        .nodes
        .iter()
        .filter(|n| matches!(n.data, NodeData::Subcat(_)))
        .filter_map(|n| n.outline.as_ref())
        .flatten()
        .copied()
        .collect();
    let hull = convex_hull(&pts);
    if hull.len() < 3 {
        return None;
    }
    let c = crate::drawing::centroid(&hull);
    let fpu = crate::drawing::feet_per_unit(p);
    let radius = hull.iter().map(|q| (q[0] - c[0]).hypot(q[1] - c[1])).fold(0.0, f64::max);
    if radius <= 0.0 {
        return None;
    }
    let grow = (radius * 0.1).max(50.0 / fpu);
    Some(
        hull.iter()
            .map(|q| {
                let d = (q[0] - c[0]).hypot(q[1] - c[1]);
                let k = (d + grow) / d.max(1e-9);
                [c[0] + (q[0] - c[0]) * k, c[1] + (q[1] - c[1]) * k]
            })
            .collect(),
    )
}

/// Convex hull (Andrew's monotone chain), counter-clockwise in y-up terms.
pub fn convex_hull(points: &[Pt]) -> Vec<Pt> {
    let mut p: Vec<Pt> = points.iter().copied().filter(|q| q[0].is_finite() && q[1].is_finite()).collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let turn = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut hull: Vec<Pt> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Pt>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2 && turn(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

/// The printed Hydrologic Soil Group report.
pub fn report_lines(p: &Project) -> Vec<String> {
    let u = &p.units;
    let mut out = Vec::new();
    let Some(map) = &p.soils else {
        out.push("No soil survey data in this project.".into());
        out.push("Use Diagram > Soils > Get Soils for the Area of Interest to retrieve it.".into());
        return out;
    };
    out.push("Source: USDA NRCS Soil Survey Geographic Database (SSURGO), via Soil Data Access".into());
    out.push(format!(
        "        (the data behind Web Soil Survey), retrieved {}.",
        if map.retrieved.is_empty() { "(date unknown)" } else { &map.retrieved }
    ));
    out.push("Hydrologic soil group: dominant condition rating of each map unit.".into());
    out.push(if map.drained {
        "Dual groups (A/D, B/D, C/D): the site is drained, so the first letter is used.".into()
    } else {
        "Dual groups (A/D, B/D, C/D): the site is not drained, so group D is used.".into()
    });
    let c = center(&map.aoi);
    let aoi_area = lon_lat_area(&map.aoi, c);
    out.push(String::new());
    out.push(format!("Area of interest: centre {}, {}", geo::format_lat_lon(c), u.fmt_area(aoi_area)));
    out.push(String::new());
    out.push("Map units in the area of interest".into());
    out.push(format!(
        "{:<8} {:<44} {:>6} {:>5} {:>12} {:>7}",
        "Symbol",
        "Map unit name",
        "Rated",
        "Used",
        format!("Area ({})", u.area_label()),
        "% AOI"
    ));
    let mut groups = [0.0; 5];
    for unit in &map.units {
        let area: f64 = map
            .polygons
            .iter()
            .filter(|q| q.mukey == unit.mukey)
            .map(|q| {
                let mut a = 0.0;
                for (i, r) in q.rings.iter().enumerate() {
                    let x = lon_lat_area(r, c);
                    if i == 0 { a += x } else { a -= x }
                }
                a.max(0.0)
            })
            .sum();
        let used = effective_hsg(&unit.hydgrp, map.drained);
        if let Some(slot) = groups.get_mut(used.map_or(4, |g| g as usize)) {
            *slot += area;
        }
        let mut name = unit.name.clone();
        if name.chars().count() > 44 {
            // ASCII dots: the PDF fonts have no ellipsis character.
            name = name.chars().take(41).collect::<String>() + "...";
        }
        out.push(format!(
            "{:<8} {:<44} {:>6} {:>5} {:>12} {:>7.1}",
            unit.symbol,
            name,
            if unit.hydgrp.is_empty() { "-" } else { &unit.hydgrp },
            used.map_or("-".to_string(), |g| g.letter().to_string()),
            crate::units::group(u.area(area), u.area_dp()),
            if aoi_area > 0.0 { area / aoi_area * 100.0 } else { 0.0 }
        ));
    }
    out.push(String::new());
    out.push("Area of interest by soil group".into());
    for (i, a) in groups.iter().enumerate() {
        let name = Hsg::ALL.get(i).map_or("Unrated".to_string(), |g| format!("HSG {}", g.letter()));
        out.push(format!(
            "  {:<8} {:>12} {}  {:>6.1}%",
            name,
            crate::units::group(u.area(*a), u.area_dp()),
            u.area_label(),
            if aoi_area > 0.0 { a / aoi_area * 100.0 } else { 0.0 }
        ));
    }
    out.push(String::new());
    out.push("Subcatchments by soil group (from their drawn outlines)".into());
    let Some(soils) = project_soils(p) else {
        out.push("  The background is not georeferenced, so subcatchments cannot be placed on the soil map.".into());
        return out;
    };
    let fpu = crate::drawing::feet_per_unit(p);
    let f2 = fpu * fpu;
    out.push(format!(
        "  {:<10} {:>12} {:>7} {:>7} {:>7} {:>7} {:>8}",
        "Subcat",
        format!("Area ({})", u.area_label()),
        "% A",
        "% B",
        "% C",
        "% D",
        "% Unrtd"
    ));
    let mut any = false;
    for n in &p.nodes {
        let (NodeData::Subcat(_), Some(o)) = (&n.data, n.outline.as_ref().filter(|o| o.len() >= 3)) else {
            continue;
        };
        any = true;
        let g = group_areas(o, &soils);
        let total: f64 = g.iter().sum();
        let pct = |x: f64| if total > 0.0 { x / total * 100.0 } else { 0.0 };
        out.push(format!(
            "  {:<10} {:>12} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>8.1}",
            n.label(),
            crate::units::group(u.area(polygon_area(o) * f2), u.area_dp()),
            pct(g[0]),
            pct(g[1]),
            pct(g[2]),
            pct(g[3]),
            pct(g[4])
        ));
    }
    if !any {
        out.push("  No subcatchment has a drawn outline.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::GeoRef;
    use crate::project::{Node, NodeType};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Pt> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn dual_groups_are_d_unless_drained() {
        assert_eq!(effective_hsg("B", false), Some(Hsg::B));
        assert_eq!(effective_hsg("B/D", false), Some(Hsg::D));
        assert_eq!(effective_hsg("B/D", true), Some(Hsg::B));
        assert_eq!(effective_hsg("c/d", true), Some(Hsg::C));
        assert_eq!(effective_hsg("", false), None);
        assert_eq!(effective_hsg("Water", false), None);
    }

    #[test]
    fn wkt_polygons_multipolygons_and_collections() {
        let p = parse_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 4 2, 4 4, 2 2))");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
        assert_eq!(p[0][0], vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let m = parse_wkt("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 0)), ((5 5, 6 5, 6 6, 5 5)))");
        assert_eq!(m.len(), 2);
        let g = parse_wkt(
            "GEOMETRYCOLLECTION (LINESTRING (0 0, 1 1), POLYGON ((-93.6 42.0, -93.5 42.0, -93.5 42.1, -93.6 42.0)), POINT (3 3))",
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0][0][0], [-93.6, 42.0]);
        assert_eq!(parse_wkt("POLYGON Z ((0 0 5, 1 0 5, 1 1 5, 0 0 5))")[0][0].len(), 3);
        assert!(parse_wkt("POLYGON EMPTY").is_empty());
        assert!(parse_wkt("POLYGON ((0 0, 1").is_empty());
        assert!(parse_wkt("").is_empty());
    }

    #[test]
    fn overlap_of_concave_polygons() {
        let square = rect(0.0, 0.0, 10.0, 10.0);
        assert!((intersection_area(&square, &rect(5.0, 5.0, 20.0, 20.0)) - 25.0).abs() < 1e-9);
        assert_eq!(intersection_area(&square, &rect(20.0, 20.0, 30.0, 30.0)), 0.0);
        // An L shape (area 75) against the square that holds it, and against its notch.
        let l = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [5.0, 5.0], [5.0, 10.0], [0.0, 10.0]];
        assert!((intersection_area(&l, &square) - 75.0).abs() < 1e-9);
        assert!(intersection_area(&l, &rect(5.0, 5.0, 10.0, 10.0)).abs() < 1e-9);
        // Clockwise input works too.
        let mut cw = square.clone();
        cw.reverse();
        assert!((intersection_area(&cw, &l) - 75.0).abs() < 1e-9);
    }

    /// A project with a north-up background at 1 ft per diagram unit anchored at
    /// (-93.62, 42.03), and a soil map whose west half (x < 100) is B/D and east half C.
    fn site() -> Project {
        let mut p = Project::default();
        p.diagram.underlay = Some(Underlay {
            file_name: String::new(),
            bytes: Vec::new(),
            page: 0,
            origin: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            feet_per_unit: Some(1.0),
            opacity: 1.0,
            visible: true,
            locked: false,
            geo: Some(GeoRef { pixel: [0.0, 0.0], lon_lat: [-93.62, 42.03], bearing: 0.0 }),
        });
        let u = p.diagram.underlay.clone().unwrap();
        let ll = |pts: Vec<Pt>| pts.into_iter().map(|q| geo::to_lon_lat(&u, q).unwrap()).collect::<Vec<_>>();
        p.soils = Some(SoilMap {
            aoi: ll(rect(-50.0, -50.0, 250.0, 250.0)),
            units: vec![
                MapUnit { mukey: "1".into(), symbol: "L55".into(), name: "Nicollet loam".into(), hydgrp: "B/D".into() },
                MapUnit {
                    mukey: "2".into(),
                    symbol: "L107".into(),
                    name: "Webster clay loam".into(),
                    hydgrp: "C".into(),
                },
            ],
            polygons: vec![
                SoilPolygon { mukey: "1".into(), rings: vec![ll(rect(-50.0, -50.0, 100.0, 250.0))] },
                SoilPolygon { mukey: "2".into(), rings: vec![ll(rect(100.0, -50.0, 250.0, 250.0))] },
            ],
            drained: false,
            retrieved: "10/27/2026".into(),
            visible: true,
        });
        p
    }

    fn subcat(p: &mut Project, outline: Vec<Pt>, areas: Vec<SubArea>) -> crate::project::NodeId {
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let n: &mut Node = p.node_mut(id).unwrap();
        n.outline = Some(outline);
        let NodeData::Subcat(sc) = &mut n.data else { panic!() };
        sc.areas = areas;
        id
    }

    fn rows(p: &Project, id: crate::project::NodeId) -> Vec<(String, f64, f64, bool, bool)> {
        let NodeData::Subcat(sc) = &p.node(id).unwrap().data else { panic!() };
        sc.areas.iter().map(|a| (a.description.clone(), a.area, a.cn, a.outline.is_some(), a.remainder)).collect()
    }

    #[test]
    fn typed_rows_split_by_the_outline_soils() {
        let mut p = site();
        // 200 x 100 ft: 60 % on B/D (= D, not drained), 40 % on C.
        let id = subcat(
            &mut p,
            rect(-20.0, 0.0, 180.0, 100.0),
            vec![
                SubArea::new(15_000.0, 55.0, "Woods, Good, HSG B"),
                SubArea::new(5_000.0, 98.0, "Paved parking, HSG B"),
                SubArea::new(1_000.0, 70.0, "My own cover"),
            ],
        );
        let (done, skipped) = apply(&mut p).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(done.len(), 1);
        assert!((done[0].coverage - 1.0).abs() < 1e-6);
        assert_eq!(done[0].custom, vec!["My own cover".to_string()]);
        let r = rows(&p, id);
        assert_eq!(r.len(), 4, "{r:?}");
        assert_eq!(r[0].0, "Woods, Good, HSG C");
        assert!((r[0].1 - 6_000.0).abs() < 1.0 && r[0].2 == 70.0, "{r:?}");
        assert_eq!(r[1].0, "Woods, Good, HSG D");
        assert!((r[1].1 - 9_000.0).abs() < 1.0 && r[1].2 == 77.0, "{r:?}");
        // Same CN on every group: one row, on the dominant group.
        assert_eq!(r[2].0, "Paved parking, HSG D");
        assert!((r[2].1 - 5_000.0).abs() < 1e-6);
        assert_eq!(r[3].0, "My own cover");
        // Applying again (after merging the split rows back) gives the same rows; drained
        // soils turn B/D into B.
        apply(&mut p).unwrap();
        assert_eq!(rows(&p, id), r);
        p.soils.as_mut().unwrap().drained = true;
        apply(&mut p).unwrap();
        let r = rows(&p, id);
        assert_eq!(r[0].0, "Woods, Good, HSG B");
        assert_eq!(r[0].2, 55.0);
        assert!((r[0].1 - 9_000.0).abs() < 1.0, "{r:?}");
    }

    #[test]
    fn drawn_sub_areas_and_remainder_split_by_their_own_ground() {
        let mut p = site();
        // Outline 200 x 100 from x = 0; a drawn pavement-free woods patch fully on B/D (x < 100),
        // and the remainder (open space) partly on each.
        let mut woods = SubArea::new(0.0, 55.0, "Woods, Good, HSG B");
        woods.outline = Some(rect(0.0, 0.0, 50.0, 100.0));
        let mut open = SubArea::new(0.0, 61.0, "Open Space, Good, HSG B");
        open.remainder = true;
        let id = subcat(&mut p, rect(0.0, 0.0, 200.0, 100.0), vec![open, woods]);
        crate::drawing::sync_node(p.node_mut(id).unwrap(), 1.0);
        apply(&mut p).unwrap();
        let r = rows(&p, id);
        // Remainder: 15,000 sq ft, 5,000 on D and 10,000 on C; woods 5,000 on D.
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!((r[0].0.as_str(), r[0].4), ("Open Space, Good, HSG C", true));
        assert!((r[0].1 - 10_000.0).abs() < 1.0, "{r:?}");
        assert_eq!((r[1].0.as_str(), r[1].4), ("Open Space, Good, HSG D", true));
        assert!((r[1].1 - 5_000.0).abs() < 1.0, "{r:?}");
        assert_eq!((r[2].0.as_str(), r[2].3), ("Woods, Good, HSG D", true));
        assert!((r[2].1 - 5_000.0).abs() < 1.0, "{r:?}");
        // Redrawing at a new scale keeps the split proportions.
        p.diagram.underlay.as_mut().unwrap().feet_per_unit = Some(2.0);
        crate::drawing::sync_all(&mut p);
        let r = rows(&p, id);
        assert!((r[0].1 - 40_000.0).abs() < 1.0 && (r[1].1 - 20_000.0).abs() < 1.0, "{r:?}");
    }

    #[test]
    fn unmapped_ground_is_reported_and_shared() {
        let mut p = site();
        // Half of this outline lies east of the soil map (x > 250).
        let id = subcat(&mut p, rect(150.0, 0.0, 350.0, 100.0), vec![SubArea::new(20_000.0, 61.0, "Open Space, Good")]);
        let (done, _) = apply(&mut p).unwrap();
        assert!((done[0].coverage - 0.5).abs() < 1e-6, "{done:?}");
        let r = rows(&p, id);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "Open Space, Good, HSG C");
        assert!((r[0].1 - 20_000.0).abs() < 1e-6);
    }

    #[test]
    fn apply_needs_a_georeference_and_skips_unoutlined() {
        let mut p = site();
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let (done, skipped) = apply(&mut p).unwrap();
        assert!(done.is_empty());
        assert_eq!(skipped, vec![p.node(id).unwrap().label()]);
        p.diagram.underlay.as_mut().unwrap().geo = None;
        assert!(apply(&mut p).is_err());
    }

    #[test]
    fn request_and_reply() {
        let aoi = [[-93.62, 42.03], [-93.615, 42.03], [-93.615, 42.034], [-93.62, 42.034]];
        let body = sda_request(&aoi).unwrap();
        assert!(body.contains("POLYGON((-93.6200000 42.0300000,") && body.contains("42.0300000))"));
        assert!(body.contains("hydgrpdcd") && body.contains("JSON+COLUMNNAME"));
        assert!(sda_request(&aoi[..2]).is_err());
        let huge = [[-94.0, 42.0], [-93.0, 42.0], [-93.0, 43.0], [-94.0, 43.0]];
        assert!(sda_request(&huge).unwrap_err().contains("limit"));

        // A trimmed real reply (USDA NRCS SSURGO, public domain).
        let reply = r#"{"Table":[["mukey","musym","muname","hydgrpdcd","wkt"],
            ["2800480","L55","Nicollet loam, 1 to 3 percent slopes","B/D","POLYGON ((-93.62 42.03, -93.6175 42.03, -93.6175 42.034, -93.62 42.034, -93.62 42.03))"],
            ["2765537","L138B","Clarion loam, Bemis moraine, 2 to 6 percent slopes","B","POLYGON ((-93.6175 42.03, -93.615 42.03, -93.615 42.034, -93.6175 42.034, -93.6175 42.03))"],
            ["2765537","L138B","Clarion loam, Bemis moraine, 2 to 6 percent slopes","B","GEOMETRYCOLLECTION (LINESTRING (-93.615 42.03, -93.615 42.031))"]]}"#;
        let map = parse_sda(reply, &aoi, false, "10/27/2026").unwrap();
        assert_eq!(map.units.len(), 2);
        assert_eq!(map.polygons.len(), 2);
        assert_eq!(map.units[0].symbol, "L138B");
        assert_eq!(map.group_of("2800480"), Some(Hsg::D));
        assert!(parse_sda("{}", &aoi, false, "").unwrap_err().contains("no map units"));
        let err = "<?xml version='1.0'?><ServiceExceptionReport><ServiceException>\nInvalid query: Invalid column name &#39;x&#39;.</ServiceException></ServiceExceptionReport>";
        assert_eq!(
            parse_sda(err, &aoi, false, "").unwrap_err(),
            "Soil Data Access refused the request: Invalid query: Invalid column name 'x'."
        );
        assert!(parse_sda("garbage", &aoi, false, "").is_err());
    }

    #[test]
    fn default_aoi_surrounds_the_outlines() {
        let mut p = site();
        assert!(default_aoi(&p).is_none());
        subcat(&mut p, rect(0.0, 0.0, 100.0, 100.0), Vec::new());
        subcat(&mut p, rect(100.0, 0.0, 300.0, 50.0), Vec::new());
        let aoi = default_aoi(&p).unwrap();
        for q in [[0.0, 0.0], [300.0, 50.0], [0.0, 100.0], [300.0, 0.0]] {
            assert!(crate::drawing::point_in_polygon(q, &aoi), "{q:?} {aoi:?}");
        }
        assert_eq!(convex_hull(&[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [1.0, 0.5], [1.0, -1.0]]).len(), 4);
    }

    #[test]
    fn report_lists_units_and_subcatchments() {
        let mut p = site();
        subcat(&mut p, rect(-20.0, 0.0, 180.0, 100.0), vec![SubArea::new(20_000.0, 61.0, "Open Space, Good")]);
        let lines = report_lines(&p).join("\n");
        assert!(lines.contains("SSURGO") && lines.contains("10/27/2026"), "{lines}");
        assert!(lines.contains("group D is used"));
        assert!(lines.contains("L55") && lines.contains("B/D"));
        // The subcatchment is 60 % D and 40 % C.
        let row = lines.lines().find(|l| l.trim_start().starts_with(&p.nodes[0].label())).unwrap();
        assert!(row.contains("60.0") && row.contains("40.0"), "{row}");
        // The printed report: a soil map page and the tables.
        let results = crate::calc::calculate(&p);
        let opts = crate::printing::PrintOptions {
            diagram: false,
            node_listing: false,
            area_listing: false,
            summaries: false,
            hsg_report: true,
            ..Default::default()
        };
        let pdf = String::from_utf8_lossy(&crate::printing::report_pdf(&p, &results, &opts)).into_owned();
        assert_eq!(pdf.matches("/Type /Page ").count(), 2);
        assert!(pdf.contains("Hydrologic Soil Groups") && pdf.contains("Hydrologic Soil Group Report"));
        p.soils = None;
        assert!(report_lines(&p)[0].contains("No soil survey data"));
    }
}
