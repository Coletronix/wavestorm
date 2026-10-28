//! Real-world position of the background: a georeference ties one background pixel to a
//! latitude/longitude and gives the bearing of the image's x axis, so with the calibrated scale
//! every diagram point has a latitude and longitude (and back). Sites are small, so the ground
//! is treated as a plane tangent at the anchor (feet per degree from the WGS 84 ellipsoid);
//! across a mile the error is well under 0.1 %.
//!
//! Also the request URLs and response parsing for the public services used to place a site:
//! address lookup (OpenStreetMap Nominatim) and aerial imagery (USGS The National Map, public
//! domain, and Esri World Imagery for recent captures) with the dates it was captured. Network
//! access itself lives in the UI.

use crate::drawing::Pt;
use crate::project::Underlay;
use serde::{Deserialize, Serialize};

/// Longitude and latitude in decimal degrees (x, y order, as in WKT).
pub type LonLat = [f64; 2];

const FT_PER_M: f64 = 1.0 / 0.3048;

/// Ties a background pixel (x right, y down) to a place on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoRef {
    /// The anchor pixel of the background image.
    pub pixel: [f64; 2],
    /// The anchor's longitude and latitude.
    pub lon_lat: LonLat,
    /// Angle (radians, counter-clockwise) from true east to the image's +x axis: 0 for a
    /// north-up image.
    #[serde(default)]
    pub bearing: f64,
}

/// Feet per degree of latitude and of longitude at latitude `lat` (WGS 84 series).
pub fn feet_per_degree(lat: f64) -> (f64, f64) {
    let p = lat.to_radians();
    let m_lat = 111_132.92 - 559.82 * (2.0 * p).cos() + 1.175 * (4.0 * p).cos() - 0.0023 * (6.0 * p).cos();
    let m_lon = 111_412.84 * p.cos() - 93.5 * (3.0 * p).cos() + 0.118 * (5.0 * p).cos();
    (m_lat * FT_PER_M, m_lon * FT_PER_M)
}

/// Ground offset in feet (east, north) of `p` from `origin`.
pub fn offset_feet(origin: LonLat, p: LonLat) -> [f64; 2] {
    let (fy, fx) = feet_per_degree(origin[1]);
    [(p[0] - origin[0]) * fx, (p[1] - origin[1]) * fy]
}

/// The place `en` feet east and north of `origin`.
pub fn offset_lon_lat(origin: LonLat, en: [f64; 2]) -> LonLat {
    let (fy, fx) = feet_per_degree(origin[1]);
    [origin[0] + en[0] / fx, origin[1] + en[1] / fy]
}

/// Feet per background pixel, when the background is calibrated.
fn feet_per_pixel(u: &Underlay) -> Option<f64> {
    let f = u.feet_per_unit? * u.scale;
    (f.is_finite() && f > 0.0).then_some(f)
}

/// The background pixel under diagram point `d`.
pub fn diagram_to_pixel(u: &Underlay, d: Pt) -> [f64; 2] {
    let s = if u.scale > 0.0 { u.scale } else { 1.0 };
    [(d[0] - u.origin[0]) / s, (d[1] - u.origin[1]) / s]
}

/// The diagram point over background pixel `px`.
pub fn pixel_to_diagram(u: &Underlay, px: [f64; 2]) -> Pt {
    [u.origin[0] + px[0] * u.scale, u.origin[1] + px[1] * u.scale]
}

/// Where a georeferenced, calibrated background puts the diagram in the real world: the parts
/// of an [`Underlay`] the conversions need, without its image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    origin: [f64; 2],
    scale: f64,
    feet_per_pixel: f64,
    geo: GeoRef,
}

impl Placement {
    pub fn new(u: &Underlay) -> Option<Placement> {
        Some(Placement {
            origin: u.origin,
            scale: if u.scale > 0.0 { u.scale } else { 1.0 },
            feet_per_pixel: feet_per_pixel(u)?,
            geo: u.geo?,
        })
    }

    /// Longitude/latitude of diagram point `d`.
    pub fn to_lon_lat(&self, d: Pt) -> LonLat {
        let (g, fpp) = (self.geo, self.feet_per_pixel);
        let px = [(d[0] - self.origin[0]) / self.scale, (d[1] - self.origin[1]) / self.scale];
        // Image y runs down; ground north runs up.
        let v = [(px[0] - g.pixel[0]) * fpp, -(px[1] - g.pixel[1]) * fpp];
        let (s, c) = g.bearing.sin_cos();
        offset_lon_lat(g.lon_lat, [v[0] * c - v[1] * s, v[0] * s + v[1] * c])
    }

    /// Diagram point of longitude/latitude `ll`.
    pub fn from_lon_lat(&self, ll: LonLat) -> Pt {
        let (g, fpp) = (self.geo, self.feet_per_pixel);
        let en = offset_feet(g.lon_lat, ll);
        let (s, c) = g.bearing.sin_cos();
        let v = [en[0] * c + en[1] * s, -en[0] * s + en[1] * c];
        let px = [g.pixel[0] + v[0] / fpp, g.pixel[1] - v[1] / fpp];
        [self.origin[0] + px[0] * self.scale, self.origin[1] + px[1] * self.scale]
    }
}

/// Longitude/latitude of diagram point `d`, when the background is georeferenced and calibrated.
pub fn to_lon_lat(u: &Underlay, d: Pt) -> Option<LonLat> {
    Some(Placement::new(u)?.to_lon_lat(d))
}

/// Diagram point of longitude/latitude `ll`, when the background is georeferenced and calibrated.
pub fn from_lon_lat(u: &Underlay, ll: LonLat) -> Option<Pt> {
    Some(Placement::new(u)?.from_lon_lat(ll))
}

/// A converter from longitude/latitude to diagram points for a georeferenced background.
pub fn projector(u: &Underlay) -> Option<impl Fn(LonLat) -> Pt + '_> {
    let p = Placement::new(u)?;
    Some(move |ll| p.from_lon_lat(ll))
}

/// Georeferences a background from two diagram points `d1`, `d2` with known places `g1`, `g2`.
/// Returns the georeference (anchored at the first point) and the feet per diagram unit
/// implied by the two places, which replaces any earlier scale calibration.
pub fn two_point(u: &Underlay, d1: Pt, g1: LonLat, d2: Pt, g2: LonLat) -> Result<(GeoRef, f64), String> {
    for g in [g1, g2] {
        if !valid_lon_lat(g) {
            return Err("Enter each point's latitude and longitude in decimal degrees.".into());
        }
    }
    let p1 = diagram_to_pixel(u, d1);
    let p2 = diagram_to_pixel(u, d2);
    let v = [p2[0] - p1[0], -(p2[1] - p1[1])];
    let pixels = v[0].hypot(v[1]);
    let mid = [(g1[0] + g2[0]) / 2.0, (g1[1] + g2[1]) / 2.0];
    let a = offset_feet(mid, g1);
    let b = offset_feet(mid, g2);
    let ground = [b[0] - a[0], b[1] - a[1]];
    let feet = ground[0].hypot(ground[1]);
    if pixels < 1.0 {
        return Err("The two points are on top of each other: pick points far apart.".into());
    }
    if feet < 1.0 {
        return Err("The two places are less than a foot apart: check the coordinates.".into());
    }
    let fpp = feet / pixels;
    let fpu = fpp / if u.scale > 0.0 { u.scale } else { 1.0 };
    let bearing = ground[1].atan2(ground[0]) - v[1].atan2(v[0]);
    Ok((GeoRef { pixel: p1, lon_lat: g1, bearing }, fpu))
}

pub fn valid_lon_lat(g: LonLat) -> bool {
    g[0].is_finite() && g[1].is_finite() && g[0].abs() <= 180.0 && g[1].abs() <= 85.0
}

/// Parses a place typed or pasted as "latitude, longitude" in decimal degrees (the order map
/// sites copy), e.g. "42.0312, -93.6178". N/S/E/W suffixes are accepted. Returns lon/lat.
pub fn parse_lat_lon(text: &str) -> Option<LonLat> {
    // Tokens, with a lone hemisphere letter ("42.03° N") joined to the number before it.
    let mut parts: Vec<String> = Vec::new();
    for s in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty()) {
        let s = s.replace('°', "");
        match parts.last_mut() {
            Some(prev) if matches!(s.to_ascii_uppercase().as_str(), "N" | "S" | "E" | "W") => prev.push_str(&s),
            _ if s.is_empty() => {}
            _ => parts.push(s),
        }
    }
    let [a, b] = parts.as_slice() else { return None };
    let num = |s: &str, neg: char| -> Option<f64> {
        let last = s.chars().last()?.to_ascii_uppercase();
        let (body, sign) = match last {
            'N' | 'E' | 'S' | 'W' => (s.get(..s.len() - 1)?, if last == neg { -1.0 } else { 1.0 }),
            _ => (s, 1.0),
        };
        Some(sign * body.parse::<f64>().ok()?)
    };
    let ll = [num(b, 'W')?, num(a, 'S')?];
    valid_lon_lat(ll).then_some(ll)
}

/// "42.031200, -93.617800" (latitude first, six decimals ≈ 4 inches).
pub fn format_lat_lon(g: LonLat) -> String {
    format!("{:.6}, {:.6}", g[1], g[0])
}

/// Percent-encodes a query-string value.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Address lookup request (OpenStreetMap Nominatim; US results first, one match).
pub fn geocode_url(query: &str) -> String {
    format!("https://nominatim.openstreetmap.org/search?format=json&limit=1&countrycodes=us&q={}", url_encode(query))
}

/// The place and its description from a Nominatim response.
pub fn parse_geocode(body: &str) -> Result<(LonLat, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "The address service sent an unreadable reply.")?;
    let first = v.as_array().and_then(|a| a.first()).ok_or("No place matched that address.")?;
    let num = |k: &str| first.get(k).and_then(|x| x.as_str()).and_then(|x| x.parse::<f64>().ok());
    let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else {
        return Err("The address service sent an unreadable reply.".into());
    };
    let name = first.get("display_name").and_then(|x| x.as_str()).unwrap_or_default().to_string();
    Ok(([lon, lat], name))
}

/// Map image sources (United States only). All but [`MapSource::Recent`] are USGS The National
/// Map services (public domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MapSource {
    /// Esri World Imagery: commercial satellite and aerial imagery, in most of the country
    /// captured within the last year or two and 0.3–0.5 m per pixel (NAIP is flown every two or
    /// three years and takes a year or more to be published). Not public domain: shown with
    /// Esri's attribution.
    #[default]
    Recent,
    /// USDA NAIP orthoimagery (about 2 ft per pixel), from the USGS NAIP image service.
    Imagery,
    /// The cached basemap of imagery with roads and place names: good for finding the site,
    /// but its imagery is much coarser than NAIP at site scale.
    ImageryLabels,
    /// US Topo map.
    Topo,
}

impl MapSource {
    pub const ALL: [MapSource; 4] = [MapSource::Recent, MapSource::Imagery, MapSource::ImageryLabels, MapSource::Topo];
    pub fn label(self) -> &'static str {
        match self {
            MapSource::Recent => "Recent aerial imagery (Esri World Imagery)",
            MapSource::Imagery => "Aerial imagery (USDA NAIP)",
            MapSource::ImageryLabels => "Roads and names over coarse imagery",
            MapSource::Topo => "Topographic map",
        }
    }
    /// The export endpoint (all take the same bbox/size/format parameters).
    fn endpoint(self) -> &'static str {
        match self {
            MapSource::Recent => {
                "https://services.arcgisonline.com/arcgis/rest/services/World_Imagery/MapServer/export"
            }
            MapSource::Imagery => {
                "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer/exportImage"
            }
            MapSource::ImageryLabels => {
                "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryTopo/MapServer/export"
            }
            MapSource::Topo => "https://basemap.nationalmap.gov/arcgis/rest/services/USGSTopo/MapServer/export",
        }
    }
    /// Credit printed with the image.
    pub fn credit(self) -> &'static str {
        match self {
            MapSource::Recent => "Esri World Imagery: Esri, Vantor, Earthstar Geographics, and the GIS User Community",
            MapSource::Imagery => "USDA NAIP via USGS The National Map",
            MapSource::ImageryLabels => "USGS The National Map: Orthoimagery and US Topo",
            MapSource::Topo => "USGS The National Map: US Topo",
        }
    }

    /// The request that tells when the imagery at `center` was captured, for the sources that
    /// say ([`parse_imagery_date`] reads the reply).
    pub fn date_url(self, center: LonLat) -> Option<String> {
        let [lon, lat] = center;
        match self {
            // The service's own footprint layer for the zoom levels a site image is drawn from.
            MapSource::Recent => Some(format!(
                "https://services.arcgisonline.com/arcgis/rest/services/World_Imagery/MapServer/identify?\
                 geometry={lon},{lat}&geometryType=esriGeometryPoint&sr=4326&layers=all:0&tolerance=0&\
                 mapExtent={},{},{},{}&imageDisplay=1000,1000,96&returnGeometry=false&f=json",
                lon - 0.002,
                lat - 0.002,
                lon + 0.002,
                lat + 0.002
            )),
            // Category 1 = the primary (full-resolution) NAIP rasters.
            MapSource::Imagery => Some(format!(
                "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer/query?\
                 geometry={lon},{lat}&geometryType=esriGeometryPoint&inSR=4326&\
                 spatialRel=esriSpatialRelIntersects&where=Category%3D1&\
                 outFields=Year,State,acquisition_date&returnGeometry=false&f=json"
            )),
            MapSource::ImageryLabels | MapSource::Topo => None,
        }
    }
}

/// "2025-09-30" from milliseconds since 1970 (UTC).
fn iso_date_from_ms(ms: f64) -> Option<String> {
    if !ms.is_finite() {
        return None;
    }
    // Days to civil date (H. Hinnant's algorithm).
    let z = (ms / 86_400_000.0).floor() as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// When and by whom the imagery at the site was captured, from the reply to
/// [`MapSource::date_url`]: e.g. "captured 2025-09-30 (Vantor, 0.46 m)" or "NAIP 2023 (IA),
/// captured 2023-09-02".
pub fn parse_imagery_date(source: MapSource, body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match source {
        MapSource::Recent => {
            let a = v.get("results")?.as_array()?.first()?.get("attributes")?;
            let s = |k: &str| a.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty() && *x != "Null");
            // "DATE (YYYYMMDD)": 20250930.
            let raw = s("DATE (YYYYMMDD)")?;
            let date = match (raw.get(0..4), raw.get(4..6), raw.get(6..8)) {
                (Some(y), Some(m), Some(d)) if raw.len() == 8 => format!("{y}-{m}-{d}"),
                _ => raw.to_string(),
            };
            let mut detail: Vec<String> = Vec::new();
            if let Some(src) = s("SOURCE") {
                detail.push(src.to_string());
            }
            if let Some(res) = s("RESOLUTION (M)") {
                detail.push(format!("{res} m"));
            }
            Some(if detail.is_empty() {
                format!("captured {date}")
            } else {
                format!("captured {date} ({})", detail.join(", "))
            })
        }
        MapSource::Imagery => {
            // The newest raster under the site.
            let best = v
                .get("features")?
                .as_array()?
                .iter()
                .filter_map(|f| f.get("attributes"))
                .max_by_key(|a| a.get("Year").and_then(|y| y.as_i64()).unwrap_or(0))?;
            let year = best.get("Year").and_then(|y| y.as_i64())?;
            let state = best.get("State").and_then(|s| s.as_str()).map(|s| format!(" ({s})")).unwrap_or_default();
            let date = best
                .get("acquisition_date")
                .and_then(|d| d.as_f64())
                .and_then(iso_date_from_ms)
                .map(|d| format!(", captured {d}"))
                .unwrap_or_default();
            Some(format!("NAIP {year}{state}{date}"))
        }
        MapSource::ImageryLabels | MapSource::Topo => None,
    }
}

/// Largest image the map services return (the NAIP service's limit is 4000).
pub const MAX_MAP_PIXELS: u32 = 4000;

/// A north-up map image request covering `width_ft` × `height_ft` centred on `center`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapRequest {
    pub url: String,
    pub width_px: u32,
    pub height_px: u32,
    /// Georeference of the returned image (its centre pixel at `center`).
    pub geo: GeoRef,
    pub feet_per_pixel: f64,
}

/// Builds a map image request. The image is about 0.75 ft per pixel (imagery is about 2 ft),
/// at least 1024 and at most [`MAX_MAP_PIXELS`] pixels wide.
pub fn map_request(source: MapSource, center: LonLat, width_ft: f64, height_ft: f64) -> Result<MapRequest, String> {
    check_area(center, width_ft, height_ft)?;
    let longest = width_ft.max(height_ft);
    let px = (longest / 0.75).clamp(1024.0, MAX_MAP_PIXELS as f64);
    let fpp = longest / px;
    let (w, h) = ((width_ft / fpp).round().max(1.0) as u32, (height_ft / fpp).round().max(1.0) as u32);
    let url = format!("{}?{}&format=jpg&f=image", source.endpoint(), export_extent(center, w, h, fpp));
    Ok(MapRequest { url, width_px: w, height_px: h, geo: center_geo(center, w, h), feet_per_pixel: fpp })
}

/// Checks a requested site area: a valid place, 50 ft to 50,000 ft on each side.
pub fn check_area(center: LonLat, width_ft: f64, height_ft: f64) -> Result<(), String> {
    if !valid_lon_lat(center) {
        return Err("The site location is not a valid latitude and longitude.".into());
    }
    if !(width_ft >= 50.0 && height_ft >= 50.0 && width_ft <= 50_000.0 && height_ft <= 50_000.0) {
        return Err("The area must be between 50 ft and 50,000 ft on each side.".into());
    }
    Ok(())
}

/// The georeference of a north-up image `w` × `h` pixels centred on `center`.
pub fn center_geo(center: LonLat, w: u32, h: u32) -> GeoRef {
    GeoRef { pixel: [w as f64 / 2.0, h as f64 / 2.0], lon_lat: center, bearing: 0.0 }
}

/// The extent and size query parameters of an ArcGIS export request for a north-up image `w` ×
/// `h` pixels of `fpp` ground feet centred on `center`, in Web Mercator, where ground lengths
/// are stretched by 1/cos(latitude); the pixels are square on the ground across a site.
pub fn export_extent(center: LonLat, w: u32, h: u32, fpp: f64) -> String {
    const R: f64 = 6_378_137.0;
    let (lon, lat) = (center[0].to_radians(), center[1].to_radians());
    let (x, y) = (R * lon, R * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln());
    let k = 1.0 / lat.cos();
    let (hw, hh) = (w as f64 * fpp / FT_PER_M / 2.0 * k, h as f64 * fpp / FT_PER_M / 2.0 * k);
    format!("bbox={:.3},{:.3},{:.3},{:.3}&bboxSR=3857&imageSR=3857&size={w},{h}", x - hw, y - hh, x + hw, y + hh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn underlay() -> Underlay {
        Underlay {
            file_name: "x.png".into(),
            bytes: Vec::new(),
            page: 0,
            origin: [100.0, 50.0],
            scale: 0.5,
            rotation: 0.0,
            feet_per_unit: Some(4.0),
            opacity: 1.0,
            visible: true,
            locked: false,
            geo: Some(GeoRef { pixel: [10.0, 20.0], lon_lat: [-93.62, 42.03], bearing: 0.3 }),
        }
    }

    #[test]
    fn degree_lengths_match_published_values() {
        // At 45° a degree of latitude is 111,132 m and of longitude 78,847 m (USGS tables).
        let (lat, lon) = feet_per_degree(45.0);
        assert!((lat * 0.3048 - 111_132.0).abs() < 2.0, "{lat}");
        assert!((lon * 0.3048 - 78_847.0).abs() < 2.0, "{lon}");
    }

    #[test]
    fn diagram_and_lon_lat_round_trip() {
        let u = underlay();
        for d in [[100.0, 50.0], [400.0, -80.0], [-250.0, 900.0]] {
            let ll = to_lon_lat(&u, d).unwrap();
            let back = from_lon_lat(&u, ll).unwrap();
            assert!((back[0] - d[0]).abs() < 1e-6 && (back[1] - d[1]).abs() < 1e-6, "{d:?} {back:?}");
        }
        // The anchor pixel is the anchor place.
        let anchor = pixel_to_diagram(&u, [10.0, 20.0]);
        let ll = to_lon_lat(&u, anchor).unwrap();
        assert!((ll[0] + 93.62).abs() < 1e-12 && (ll[1] - 42.03).abs() < 1e-12);
        // Distances are preserved: 100 units = 400 ft on the ground.
        let a = to_lon_lat(&u, [0.0, 0.0]).unwrap();
        let b = to_lon_lat(&u, [60.0, 80.0]).unwrap();
        let en = offset_feet(a, b);
        assert!((en[0].hypot(en[1]) - 400.0).abs() < 0.01);
        let mut plain = underlay();
        plain.geo = None;
        assert!(to_lon_lat(&plain, [0.0, 0.0]).is_none());
    }

    #[test]
    fn two_points_recover_a_georeference() {
        let truth = underlay();
        let d1 = [130.0, 75.0];
        let d2 = [480.0, 300.0];
        let (g1, g2) = (to_lon_lat(&truth, d1).unwrap(), to_lon_lat(&truth, d2).unwrap());
        let mut u = underlay();
        u.geo = None;
        u.feet_per_unit = None;
        let (geo, fpu) = two_point(&u, d1, g1, d2, g2).unwrap();
        assert!((fpu - 4.0).abs() < 1e-4, "{fpu}");
        assert!((geo.bearing - 0.3).abs() < 1e-5, "{}", geo.bearing);
        u.geo = Some(geo);
        u.feet_per_unit = Some(fpu);
        let p = from_lon_lat(&u, to_lon_lat(&truth, [-300.0, 20.0]).unwrap()).unwrap();
        assert!((p[0] + 300.0).abs() < 0.01 && (p[1] - 20.0).abs() < 0.01, "{p:?}");
        assert!(two_point(&u, d1, g1, d1, g2).is_err());
        assert!(two_point(&u, d1, g1, d2, g1).is_err());
        assert!(two_point(&u, d1, [f64::NAN, 0.0], d2, g2).is_err());
    }

    #[test]
    fn parses_pasted_coordinates() {
        assert_eq!(parse_lat_lon("42.0312, -93.6178"), Some([-93.6178, 42.0312]));
        assert_eq!(parse_lat_lon(" 42.0312 -93.6178 "), Some([-93.6178, 42.0312]));
        assert_eq!(parse_lat_lon("42.0312N, 93.6178W"), Some([-93.6178, 42.0312]));
        assert_eq!(parse_lat_lon("42.0312° N, 93.6178° W"), Some([-93.6178, 42.0312]));
        assert_eq!(parse_lat_lon("123 Main St"), None);
        assert_eq!(parse_lat_lon("95, 10"), None);
        assert_eq!(parse_lat_lon(""), None);
        assert_eq!(format_lat_lon([-93.6178, 42.0312]), "42.031200, -93.617800");
    }

    #[test]
    fn geocode_reply() {
        let body = r#"[{"lat":"42.0267","lon":"-93.6465","display_name":"Ames, Story County, Iowa"}]"#;
        let (ll, name) = parse_geocode(body).unwrap();
        assert_eq!(ll, [-93.6465, 42.0267]);
        assert!(name.starts_with("Ames"));
        assert!(parse_geocode("[]").is_err());
        assert!(parse_geocode("<html>").is_err());
        assert_eq!(url_encode("1 Main St, Ames"), "1+Main+St%2C+Ames");
    }

    #[test]
    fn imagery_dates() {
        // Shapes of the services' replies (2026-10), trimmed.
        let esri = r#"{"results":[{"layerId":0,"attributes":{"DATE (YYYYMMDD)":"20250930",
            "RESOLUTION (M)":"0.46","SOURCE":"Vantor","SRC_DATE2":"9/30/2025"}}]}"#;
        assert_eq!(
            parse_imagery_date(MapSource::Recent, esri).as_deref(),
            Some("captured 2025-09-30 (Vantor, 0.46 m)")
        );
        let naip = r#"{"features":[{"attributes":{"Year":2021,"State":"IA","acquisition_date":1630000000000}},
            {"attributes":{"Year":2023,"State":"IA","acquisition_date":1693612800000}}]}"#;
        assert_eq!(
            parse_imagery_date(MapSource::Imagery, naip).as_deref(),
            Some("NAIP 2023 (IA), captured 2023-09-02")
        );
        assert_eq!(parse_imagery_date(MapSource::Recent, r#"{"results":[]}"#), None);
        assert_eq!(parse_imagery_date(MapSource::Imagery, "<html>"), None);
        assert!(MapSource::Recent.date_url([-93.6, 42.0]).unwrap().contains("identify?geometry=-93.6,42"));
        assert!(MapSource::Topo.date_url([-93.6, 42.0]).is_none());
        assert_eq!(iso_date_from_ms(0.0).as_deref(), Some("1970-01-01"));
        assert_eq!(iso_date_from_ms(951_782_400_000.0).as_deref(), Some("2000-02-29"));
    }

    #[test]
    fn placement_matches_the_underlay_conversions() {
        let u = underlay();
        let p = Placement::new(&u).unwrap();
        let d = [321.0, -45.0];
        assert_eq!(p.to_lon_lat(d), to_lon_lat(&u, d).unwrap());
        let mut plain = underlay();
        plain.feet_per_unit = None;
        assert!(Placement::new(&plain).is_none());
    }

    #[test]
    fn map_request_covers_the_area() {
        let r = map_request(MapSource::Imagery, [-93.62, 42.03], 1500.0, 1000.0).unwrap();
        assert_eq!((r.width_px, r.height_px), (2000, 1333));
        assert!((r.feet_per_pixel - 0.75).abs() < 1e-9);
        assert!(r.url.contains("USGSNAIPImagery/ImageServer/exportImage?") && r.url.contains("size=2000,1333"));
        // The bbox width in Web Mercator metres is the ground width stretched by 1/cos(lat).
        let bbox: Vec<f64> = r
            .url
            .split("bbox=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        let ground_m = (bbox[2] - bbox[0]) * 42.03f64.to_radians().cos();
        assert!((ground_m / 0.3048 - 1500.0).abs() < 1.0, "{ground_m}");
        let recent = map_request(MapSource::Recent, [-93.62, 42.03], 1500.0, 1000.0).unwrap();
        assert!(recent.url.contains("World_Imagery/MapServer/export?") && recent.url.contains("&f=image"));
        let big = map_request(MapSource::Topo, [-93.62, 42.03], 20_000.0, 20_000.0).unwrap();
        assert_eq!(big.width_px, MAX_MAP_PIXELS);
        assert!(map_request(MapSource::Imagery, [-93.62, 42.03], 10.0, 1000.0).is_err());
        assert!(map_request(MapSource::Imagery, [-200.0, 42.03], 1000.0, 1000.0).is_err());
    }
}
