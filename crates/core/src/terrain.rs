//! Ground elevations for a site: a bare-earth elevation grid from the USGS 3D Elevation Program
//! (3DEP, public domain; lidar-derived at 1 m where it has been flown, which is most of the
//! country), sampled along drawn Tc flow paths for segment slopes, and drawn as a shaded-relief
//! background with contours.
//!
//! The grid is fetched like the map images (`geo::export_extent`): north-up, centred on a
//! place, square cells of `feet_per_pixel` on the ground, so it lines up with any georeferenced
//! background. Elevations are stored in feet (NAVD 88, as 3DEP publishes them in metres).

use crate::drawing::Pt;
use crate::geo::{self, GeoRef, LonLat, Placement};
use crate::project::Project;
use crate::tc::PathLeg;
use serde::{Deserialize, Serialize};

/// 3DEP bare-earth DEM image service (dynamic mosaic of the best available resolution).
pub const DEM_URL: &str =
    "https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage";

/// Credit for the elevations and the backgrounds drawn from them.
pub const CREDIT: &str = "USGS 3D Elevation Program (3DEP) bare-earth DEM";

/// Cell size asked for (ft): 1 m, the resolution of 3DEP's lidar DEMs.
pub const DEM_FEET_PER_PIXEL: f64 = 1.0 / 0.3048;

/// Most cells along either side of a grid (larger sites get coarser cells).
pub const MAX_DEM_PIXELS: u32 = 1500;

/// The smallest slope given to a segment (ft/ft): where the ground is flat or rises along the
/// path, TR-55's equations need a positive slope.
pub const MIN_SLOPE: f64 = 0.0005;

const FT_PER_M: f64 = 1.0 / 0.3048;

/// A bare-earth elevation grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    /// The place at the centre of the grid.
    pub center: LonLat,
    pub width: u32,
    pub height: u32,
    /// Ground size of a cell (ft).
    pub feet_per_pixel: f64,
    /// Elevations (ft, NAVD 88) row by row from the north-west corner; NaN where there are none.
    #[serde(with = "f32_base64")]
    pub elev: Vec<f32>,
    /// When it was downloaded.
    #[serde(default)]
    pub retrieved: String,
}

/// `Vec<f32>` as base64 of little-endian bytes (a grid as JSON numbers would be ~3× larger).
mod f32_base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[f32], s: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        s.serialize_str(&crate::base64::encode(&bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        let text = String::deserialize(d)?;
        let bytes = crate::base64::decode(&text).map_err(serde::de::Error::custom)?;
        Ok(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }
}

/// A DEM request: north-up, `width_px` × `height_px` cells of `feet_per_pixel`.
#[derive(Debug, Clone, PartialEq)]
pub struct DemRequest {
    pub url: String,
    pub width_px: u32,
    pub height_px: u32,
    pub feet_per_pixel: f64,
}

/// The request for the elevations of a `width_ft` × `height_ft` area centred on `center`: 1 m
/// cells, coarser when that would be more than [`MAX_DEM_PIXELS`] along a side. The reply is
/// raw 32-bit floats (band sequential) followed by a validity bit mask ([`parse_dem`]).
pub fn dem_request(center: LonLat, width_ft: f64, height_ft: f64) -> Result<DemRequest, String> {
    geo::check_area(center, width_ft, height_ft)?;
    let fpp = DEM_FEET_PER_PIXEL.max(width_ft.max(height_ft) / MAX_DEM_PIXELS as f64);
    let (w, h) = ((width_ft / fpp).round().max(2.0) as u32, (height_ft / fpp).round().max(2.0) as u32);
    let url = format!(
        "{DEM_URL}?{}&format=bsq&pixelType=F32&interpolation=RSP_BilinearInterpolation&f=image",
        geo::export_extent(center, w, h, fpp)
    );
    Ok(DemRequest { url, width_px: w, height_px: h, feet_per_pixel: fpp })
}

/// Reads a DEM reply: `w × h` little-endian f32 elevations in metres, row by row from the
/// north-west, then (usually) one bit per cell, most significant bit first, set where the cell
/// has data.
pub fn parse_dem(bytes: &[u8], req: &DemRequest, center: LonLat, retrieved: &str) -> Result<Terrain, String> {
    let n = req.width_px as usize * req.height_px as usize;
    let mask_len = n.div_ceil(8);
    let (values, mask) = match bytes.len() {
        l if l == n * 4 + mask_len => bytes.split_at(n * 4),
        l if l == n * 4 => (bytes, &[][..]),
        _ => {
            return Err(if bytes.starts_with(b"{") || bytes.starts_with(b"<") {
                "The elevation service answered with a message instead of elevations.".into()
            } else {
                format!(
                    "The elevation service sent {} bytes, not the {} x {} grid asked for.",
                    bytes.len(),
                    req.width_px,
                    req.height_px
                )
            });
        }
    };
    let elev: Vec<f32> = values
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let valid = mask.is_empty() || mask.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0);
            let m = f32::from_le_bytes(*c);
            // Below the Dead Sea or above Everest is a fill value, not ground.
            if valid && m.is_finite() && (-500.0..9000.0).contains(&m) {
                (f64::from(m) * FT_PER_M) as f32
            } else {
                f32::NAN
            }
        })
        .collect();
    if !elev.iter().any(|z| z.is_finite()) {
        return Err("There are no 3DEP elevations for this place.".into());
    }
    Ok(Terrain {
        center,
        width: req.width_px,
        height: req.height_px,
        feet_per_pixel: req.feet_per_pixel,
        elev,
        retrieved: retrieved.to_string(),
    })
}

impl Terrain {
    fn at(&self, col: usize, row: usize) -> Option<f64> {
        if col >= self.width as usize || row >= self.height as usize {
            return None;
        }
        let z = *self.elev.get(row * self.width as usize + col)?;
        z.is_finite().then_some(f64::from(z))
    }

    /// Elevation at grid position (`gx`, `gy`) in cells from the north-west cell's centre:
    /// bilinear between the four cells around it, leaving out cells without data. Positions up
    /// to half a cell outside the grid take the edge cells; further out there is none.
    pub fn sample(&self, gx: f64, gy: f64) -> Option<f64> {
        let (w, h) = (self.width as f64, self.height as f64);
        if !(gx >= -0.5 && gy >= -0.5 && gx <= w - 0.5 && gy <= h - 0.5) {
            return None;
        }
        let (gx, gy) = (gx.clamp(0.0, w - 1.0), gy.clamp(0.0, h - 1.0));
        let (x0, y0) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - x0, gy - y0);
        let (c, r) = (x0 as usize, y0 as usize);
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (dc, dr, wt) in
            [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)]
        {
            if wt > 0.0
                && let Some(z) = self.at(c + dc, r + dr)
            {
                sum += z * wt;
                weight += wt;
            }
        }
        if weight > 1e-9 {
            return Some(sum / weight);
        }
        // In a gap: the mean of the cells around the nearest one.
        let (c, r) = (gx.round() as usize, gy.round() as usize);
        let around: Vec<f64> = (0..3)
            .flat_map(|dr| (0..3).map(move |dc| (dc, dr)))
            .filter_map(|(dc, dr)| self.at((c + dc).checked_sub(1)?, (r + dr).checked_sub(1)?))
            .collect();
        (!around.is_empty()).then(|| around.iter().sum::<f64>() / around.len() as f64)
    }

    /// Ground elevation (ft) at a place.
    pub fn elevation_at(&self, ll: LonLat) -> Option<f64> {
        let en = geo::offset_feet(self.center, ll);
        let gx = en[0] / self.feet_per_pixel + self.width as f64 / 2.0 - 0.5;
        let gy = self.height as f64 / 2.0 - en[1] / self.feet_per_pixel - 0.5;
        self.sample(gx, gy)
    }

    /// Lowest and highest elevations (ft).
    pub fn range(&self) -> Option<[f64; 2]> {
        let mut it = self.elev.iter().filter(|z| z.is_finite()).map(|z| f64::from(*z));
        let first = it.next()?;
        Some(it.fold([first, first], |[lo, hi], z| [lo.min(z), hi.max(z)]))
    }

    /// Share of the cells with an elevation.
    pub fn coverage(&self) -> f64 {
        if self.elev.is_empty() {
            return 0.0;
        }
        self.elev.iter().filter(|z| z.is_finite()).count() as f64 / self.elev.len() as f64
    }
}

/// The project's elevations placed on the diagram, for reading the ground under diagram points.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    terrain: Terrain,
    place: Placement,
}

impl Surface {
    /// When the project has elevations and its background is georeferenced and calibrated.
    pub fn new(p: &Project) -> Option<Surface> {
        let place = Placement::new(p.diagram.underlay.as_ref()?)?;
        Some(Surface { terrain: p.terrain.clone()?, place })
    }

    /// Ground elevation (ft) under diagram point `d`.
    pub fn at(&self, d: Pt) -> Option<f64> {
        self.terrain.elevation_at(self.place.to_lon_lat(d))
    }
}

/// Ground elevation (ft) under diagram point `d`, when the project has elevations there.
pub fn elevation_at(p: &Project, d: Pt) -> Option<f64> {
    let place = Placement::new(p.diagram.underlay.as_ref()?)?;
    p.terrain.as_ref()?.elevation_at(place.to_lon_lat(d))
}

/// Sets the slopes of a drawn flow path's legs from the ground elevations `elev` at its points
/// (`elev[i]` at point `i`; leg `i` runs from point `i` to `i + 1`). Consecutive legs of one flow
/// type form a segment whose slope is its fall over its length, the average slope TR-55 asks
/// for; it goes on the segment's first leg (the others keep it, see `tc::path_runs`). Slopes are
/// rounded to 0.0001 and at least [`MIN_SLOPE`]. Returns the segments (first and last point)
/// where the ground is flat or rises, or None (legs unchanged) when a point has no elevation.
pub fn path_slopes(legs: &mut [PathLeg], elev: &[Option<f64>]) -> Option<Vec<[usize; 2]>> {
    let z = |i: usize| elev.get(i).copied().flatten();
    if (0..=legs.len()).any(|i| z(i).is_none()) {
        return None;
    }
    let mut flat = Vec::new();
    let mut i = 0;
    while let Some(first) = legs.get(i).copied() {
        let mut j = i;
        let mut length = 0.0;
        while let Some(leg) = legs.get(j).filter(|l| l.flow == first.flow) {
            if leg.length.is_finite() && leg.length > 0.0 {
                length += leg.length;
            }
            j += 1;
        }
        let fall = z(i).zip(z(j)).map_or(0.0, |(a, b)| a - b);
        let mut slope = if length > 0.0 { (fall / length * 10_000.0).round() / 10_000.0 } else { 0.0 };
        if slope < MIN_SLOPE {
            flat.push([i, j]);
            slope = MIN_SLOPE;
        }
        for (k, leg) in legs.iter_mut().enumerate().take(j).skip(i) {
            leg.slope = (k == i).then_some(slope);
        }
        i = j;
    }
    Some(flat)
}

/// A contour interval (ft) and the interval of the heavier index contours for ground spanning
/// `relief` ft: the smallest standard interval giving at most about 25 contours.
pub fn contour_interval(relief: f64) -> (f64, f64) {
    const STEPS: [(f64, f64); 8] =
        [(0.5, 2.0), (1.0, 5.0), (2.0, 10.0), (5.0, 25.0), (10.0, 50.0), (20.0, 100.0), (50.0, 250.0), (100.0, 500.0)];
    STEPS.iter().copied().find(|(i, _)| relief / i <= 25.0).unwrap_or((100.0, 500.0))
}

/// A shaded-relief picture of a terrain grid with contours, to use as a background.
#[derive(Debug, Clone, PartialEq)]
pub struct Relief {
    pub width: u32,
    pub height: u32,
    /// RGBA pixels, row by row from the top (north).
    pub rgba: Vec<u8>,
    pub feet_per_pixel: f64,
    /// Where the picture is (its centre pixel at the grid's centre).
    pub geo: GeoRef,
    /// Contour interval and index-contour interval (ft).
    pub interval: f64,
    pub index: f64,
}

/// Colours from low to high ground (green lowlands to brown uplands), before shading.
const TINTS: [(f64, [f64; 3]); 4] = [
    (0.0, [96.0, 150.0, 104.0]),
    (0.35, [176.0, 196.0, 128.0]),
    (0.7, [222.0, 200.0, 146.0]),
    (1.0, [184.0, 140.0, 104.0]),
];

fn tint(t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    for w in TINTS.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let f = if b > a { (t - a) / (b - a) } else { 0.0 };
            return [ca[0] + (cb[0] - ca[0]) * f, ca[1] + (cb[1] - ca[1]) * f, ca[2] + (cb[2] - ca[2]) * f];
        }
    }
    TINTS[TINTS.len() - 1].1
}

/// The value at fraction `q` of the sorted values.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let i = ((sorted.len().saturating_sub(1)) as f64 * q).round() as usize;
    sorted.get(i).copied().unwrap_or(0.0)
}

/// Draws a terrain grid as shaded relief (lit from the north-west, with the relief exaggerated
/// on gentle ground so that swales and ridges show) over elevation tints, with contours: thin
/// every `interval`, heavier every `index`. The picture has up to three pixels per cell (so
/// contours are smooth) and at most `max_px` pixels along a side. None when the grid has no data.
pub fn render_relief(t: &Terrain, max_px: u32) -> Option<Relief> {
    let mut sorted: Vec<f64> = t.elev.iter().filter(|z| z.is_finite()).map(|z| f64::from(*z)).collect();
    if sorted.is_empty() || t.width == 0 || t.height == 0 {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let (lo, hi) = (percentile(&sorted, 0.02), percentile(&sorted, 0.98));
    let span = (hi - lo).max(0.1);
    let (interval, index) = contour_interval(percentile(&sorted, 1.0) - percentile(&sorted, 0.0));

    // Shading per cell from its slope (central differences, one-sided at edges and gaps).
    let (gw, gh, fpp) = (t.width as usize, t.height as usize, t.feet_per_pixel);
    let grad = |c: usize, r: usize| -> Option<[f64; 2]> {
        let z = t.at(c, r)?;
        let pick = |a: Option<f64>, b: Option<f64>, d: f64| match (a, b) {
            (Some(a), Some(b)) => (a - b) / (2.0 * d),
            (Some(a), None) => (a - z) / d,
            (None, Some(b)) => (z - b) / d,
            (None, None) => 0.0,
        };
        let east = pick(t.at(c + 1, r), c.checked_sub(1).and_then(|c| t.at(c, r)), fpp);
        // Rows run south.
        let north = pick(r.checked_sub(1).and_then(|r| t.at(c, r)), t.at(c, r + 1), fpp);
        Some([east, north])
    };
    let grads: Vec<Option<[f64; 2]>> =
        (0..gh).flat_map(|r| (0..gw).map(move |c| (c, r))).map(|(c, r)| grad(c, r)).collect();
    let mut steep: Vec<f64> = grads.iter().flatten().map(|g| g[0].hypot(g[1])).collect();
    steep.sort_by(f64::total_cmp);
    // Exaggerate so that the steeper tenth of the ground leans about 20°.
    let p90 = percentile(&steep, 0.9);
    let exaggeration = if p90 > 0.0 { (0.36 / p90).clamp(1.0, 10.0) } else { 10.0 };
    let light = {
        let (az, alt) = (315f64.to_radians(), 45f64.to_radians());
        [alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin()]
    };
    let shade: Vec<f32> = grads
        .iter()
        .map(|g| match g {
            Some([e, n]) => {
                let v = [-e * exaggeration, -n * exaggeration, 1.0];
                let len = (v[0] * v[0] + v[1] * v[1] + 1.0).sqrt();
                ((v[0] * light[0] + v[1] * light[1] + v[2] * light[2]) / len).max(0.0) as f32
            }
            None => f32::NAN,
        })
        .collect();
    let shade_grid = Terrain { elev: shade, retrieved: String::new(), ..*t };

    let (wf, hf) = (t.width as f64 * fpp, t.height as f64 * fpp);
    let out_fpp = (fpp / 3.0).max(wf.max(hf) / max_px.max(16) as f64);
    let (w, h) = ((wf / out_fpp).round().max(1.0) as u32, (hf / out_fpp).round().max(1.0) as u32);
    let out_fpp = wf / w as f64;
    let (wu, hu) = (w as usize, h as usize);
    let to_grid = |x: usize, y: usize| ((x as f64 + 0.5) * out_fpp / fpp - 0.5, (y as f64 + 0.5) * out_fpp / fpp - 0.5);
    let zs: Vec<f64> = (0..hu)
        .flat_map(|y| (0..wu).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (gx, gy) = to_grid(x, y);
            t.sample(gx, gy).unwrap_or(f64::NAN)
        })
        .collect();
    let level = |z: f64| (z / interval).floor();
    let mut rgba = Vec::with_capacity(wu * hu * 4);
    for y in 0..hu {
        for x in 0..wu {
            let z = zs.get(y * wu + x).copied().unwrap_or(f64::NAN);
            if !z.is_finite() {
                rgba.extend_from_slice(&[214, 214, 214, 255]);
                continue;
            }
            let (gx, gy) = to_grid(x, y);
            let s = shade_grid.sample(gx, gy).unwrap_or(light[2]);
            // Flat ground (shade = light[2]) keeps its tint; slopes lighten or darken it.
            let k = (0.25 + 0.75 * s / light[2]).clamp(0.2, 1.2);
            let base = tint((z - lo) / span);
            let mut c = [base[0] * k, base[1] * k, base[2] * k];
            // Contour: a level boundary between this pixel and the next one east or south; index
            // contours also take the boundary to the west or north, so they are two pixels wide.
            let mut line: Option<f64> = None;
            let east = zs.get(y * wu + x + 1).filter(|_| x + 1 < wu);
            let south = zs.get((y + 1) * wu + x);
            let west = x.checked_sub(1).and_then(|w| zs.get(y * wu + w));
            let north = y.checked_sub(1).and_then(|n| zs.get(n * wu + x));
            for (n, minor) in [(east, true), (south, true), (west, false), (north, false)] {
                if let Some(&zn) = n
                    && zn.is_finite()
                    && level(zn) != level(z)
                {
                    let crossed = level(z).max(level(zn)) * interval;
                    let is_index = ((crossed / index).round() * index - crossed).abs() < 1e-6;
                    if is_index {
                        line = Some(0.85);
                    } else if minor {
                        line = Some(line.unwrap_or(0.0).max(0.5));
                    }
                }
            }
            if let Some(a) = line {
                let ink = [70.0, 44.0, 24.0];
                c = [c[0] + (ink[0] - c[0]) * a, c[1] + (ink[1] - c[1]) * a, c[2] + (ink[2] - c[2]) * a];
            }
            rgba.extend(c.iter().map(|v| v.round().clamp(0.0, 255.0) as u8));
            rgba.push(255);
        }
    }
    Some(Relief {
        width: w,
        height: h,
        rgba,
        feet_per_pixel: out_fpp,
        geo: geo::center_geo(t.center, w, h),
        interval,
        index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tc::PathFlow;

    const CENTER: LonLat = [-93.62, 42.03];

    /// A plane falling `fall` ft per ft to the east from 900 ft at the centre.
    fn plane(w: u32, h: u32, fpp: f64, fall: f64) -> Terrain {
        let elev = (0..h)
            .flat_map(|_| (0..w).map(move |c| (900.0 - fall * (c as f64 + 0.5 - w as f64 / 2.0) * fpp) as f32))
            .collect();
        Terrain { center: CENTER, width: w, height: h, feet_per_pixel: fpp, elev, retrieved: String::new() }
    }

    fn reply(values_m: &[f32], valid: &[bool]) -> Vec<u8> {
        let mut out: Vec<u8> = values_m.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut mask = vec![0u8; valid.len().div_ceil(8)];
        for (i, v) in valid.iter().enumerate() {
            if *v {
                mask[i / 8] |= 0x80 >> (i % 8);
            }
        }
        out.extend(mask);
        out
    }

    #[test]
    fn dem_request_asks_for_one_metre_cells() {
        let r = dem_request(CENTER, 1500.0, 1000.0).unwrap();
        assert_eq!((r.width_px, r.height_px), (457, 305));
        assert!(
            r.url.starts_with(DEM_URL) && r.url.contains("format=bsq&pixelType=F32") && r.url.contains("size=457,305")
        );
        let big = dem_request(CENTER, 30_000.0, 15_000.0).unwrap();
        assert_eq!(big.width_px, MAX_DEM_PIXELS);
        assert!((big.feet_per_pixel - 20.0).abs() < 1e-9);
        assert!(dem_request(CENTER, 10.0, 100.0).is_err());
    }

    #[test]
    fn parses_floats_and_the_validity_mask() {
        // The service's layout (checked 2026-10 with a 7 x 3 request: 84 bytes + 3 mask bytes).
        let req = DemRequest { url: String::new(), width_px: 3, height_px: 3, feet_per_pixel: 3.28 };
        let m = [100.0, 100.5, 101.0, 99.0, -3.4e38, 100.0, 98.0, 98.5, 99.0];
        let mut valid = [true; 9];
        valid[2] = false;
        let t = parse_dem(&reply(&m, &valid), &req, CENTER, "today").unwrap();
        assert!((f64::from(t.elev[0]) - 100.0 / 0.3048).abs() < 1e-3);
        assert!(t.elev[2].is_nan(), "masked cell");
        assert!(t.elev[4].is_nan(), "fill value");
        assert!((t.coverage() - 7.0 / 9.0).abs() < 1e-12);
        // Without the mask, too.
        let bare: Vec<u8> = m.iter().flat_map(|v| v.to_le_bytes()).collect();
        assert!(parse_dem(&bare, &req, CENTER, "").unwrap().elev[2].is_finite());
        assert!(parse_dem(b"{\"error\":{}}", &req, CENTER, "").is_err());
        assert!(parse_dem(&reply(&[f32::NAN; 9], &[false; 9]), &req, CENTER, "").is_err());
    }

    #[test]
    fn samples_between_cells_and_skips_gaps() {
        let mut t = plane(10, 10, 3.0, 0.02);
        // At the centre the plane is 900 ft; 30 ft east it is 0.6 ft lower.
        assert!((t.elevation_at(CENTER).unwrap() - 900.0).abs() < 1e-3);
        let east = geo::offset_lon_lat(CENTER, [9.0, 0.0]);
        assert!((t.elevation_at(east).unwrap() - 899.82).abs() < 1e-3);
        assert!(t.elevation_at(geo::offset_lon_lat(CENTER, [100.0, 0.0])).is_none());
        // A gap: the neighbours carry the value.
        t.elev[55] = f32::NAN;
        assert!(t.sample(5.0, 5.0).is_some());
        assert_eq!(t.range().map(|r| r[0] < r[1]), Some(true));
    }

    #[test]
    fn grid_round_trips_through_json() {
        let mut t = plane(4, 3, 3.28, 0.01);
        t.elev[1] = f32::NAN;
        let back: Terrain = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.elev.len(), 12);
        assert!(back.elev[1].is_nan());
        assert_eq!(back.elev[5], t.elev[5]);
    }

    #[test]
    fn path_slopes_follow_the_ground_per_segment() {
        let mut legs = vec![
            PathLeg::new(100.0, PathFlow::Sheet),
            PathLeg::new(150.0, PathFlow::Shallow),
            PathLeg::new(250.0, PathFlow::Shallow),
            PathLeg::new(200.0, PathFlow::Channel),
        ];
        legs[2].slope = Some(0.5);
        let elev = [Some(910.0), Some(908.0), Some(905.0), Some(900.0), Some(900.2)];
        let flat = path_slopes(&mut legs, &elev).unwrap();
        let slopes: Vec<Option<f64>> = legs.iter().map(|l| l.slope).collect();
        // Sheet 2/100; shallow (8 ft over 400 ft) on its first leg only; the channel rises.
        assert_eq!(slopes, vec![Some(0.02), Some(0.02), None, Some(MIN_SLOPE)]);
        assert_eq!(flat, vec![[3, 4]]);
        // The legs then make three segments with those slopes.
        let runs = crate::tc::path_runs(&legs);
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[1].from, runs[1].to, runs[1].slope), (1, 3, Some(0.02)));
        // A point off the grid: nothing changes.
        let before = legs.clone();
        assert!(path_slopes(&mut legs, &[Some(1.0), None, Some(1.0), Some(1.0), Some(1.0)]).is_none());
        assert_eq!(legs, before);
    }

    #[test]
    fn contour_intervals() {
        assert_eq!(contour_interval(4.0), (0.5, 2.0));
        assert_eq!(contour_interval(20.0), (1.0, 5.0));
        assert_eq!(contour_interval(45.0), (2.0, 10.0));
        assert_eq!(contour_interval(400.0), (20.0, 100.0));
        assert_eq!(contour_interval(1e6), (100.0, 500.0));
    }

    #[test]
    fn relief_has_contours_where_levels_change() {
        // 30 ft across with a 0.1 fall: 3 ft of relief, 0.5 ft contours (about six lines).
        let t = plane(10, 6, 3.0, 0.1);
        let r = render_relief(&t, 4000).unwrap();
        assert_eq!((r.width, r.height), (30, 18));
        assert_eq!(r.rgba.len(), 30 * 18 * 4);
        assert_eq!(r.interval, 0.5);
        assert!((r.feet_per_pixel - 1.0).abs() < 1e-9);
        assert_eq!(r.geo.pixel, [15.0, 9.0]);
        // Contour pixels are darker than their row neighbours; count them along a row.
        let row: Vec<u32> =
            (0..30).map(|x| r.rgba[(9 * 30 + x) * 4..][..3].iter().map(|v| u32::from(*v)).sum()).collect();
        let dark = (1..29).filter(|&x| row[x] + 60 < row[x - 1].min(row[x + 1]).max(row[x - 1])).count();
        assert!((4..=7).contains(&dark), "{dark} {row:?}");
        // Capped size.
        let small = render_relief(&t, 16).unwrap();
        assert!(small.width <= 16 && small.height <= 16);
        let empty = Terrain { elev: vec![f32::NAN; 60], ..t };
        assert!(render_relief(&empty, 100).is_none());
    }
}
