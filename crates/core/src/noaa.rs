//! NOAA Atlas 14 point precipitation-frequency tables (the CSV downloaded from the Precipitation
//! Frequency Data Server), used to look up rainfall event depths for a site.

use crate::rainfall::RainEvent;

/// Depths by duration (rows) and average recurrence interval (columns), in inches (or
/// intensities in inches per hour, see `intensity`).
#[derive(Debug, Clone, PartialEq)]
pub struct PfTable {
    /// Description of the source (volume, project area, coordinates).
    pub source: String,
    /// Average recurrence intervals (years).
    pub aris: Vec<f64>,
    pub rows: Vec<PfRow>,
    /// The values are intensities (in/hr) rather than depths (in).
    pub intensity: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PfRow {
    /// Duration as written in the table, e.g. "24-hr".
    pub label: String,
    pub hours: f64,
    /// Depth (inches) for each ARI (in/hr for an intensity table).
    pub depths: Vec<f64>,
}

impl PfTable {
    /// Depth (inches) of row `r`, column `i`.
    pub fn depth(&self, r: &PfRow, i: usize) -> Option<f64> {
        let v = *r.depths.get(i)?;
        let d = if self.intensity { v * r.hours } else { v };
        (d.is_finite() && d > 0.0).then_some(d)
    }
}

/// The PFDS request for the mean partial-duration estimates at a point (English units).
/// `depth` false asks for intensities. The server answers plain CSV text; it does not allow
/// requests from web pages (no CORS header), so the browser build cannot use it directly.
pub fn pfds_url(lat: f64, lon: f64, depth: bool) -> String {
    format!(
        "https://hdsc.nws.noaa.gov/cgi-bin/new/fe_text_mean.csv?lat={lat:.4}&lon={lon:.4}&data={}&units=english&series=pds",
        if depth { "depth" } else { "intensity" }
    )
}

/// Hours in a duration label such as "5-min", "24-hr" or "2-day".
fn duration_hours(label: &str) -> Option<f64> {
    let (n, unit) = label.trim().trim_end_matches(':').split_once('-')?;
    let n: f64 = n.trim().parse().ok()?;
    match unit.trim().to_ascii_lowercase().as_str() {
        "min" => Some(n / 60.0),
        "hr" => Some(n),
        "day" => Some(n * 24.0),
        _ => None,
    }
}

/// Reads a NOAA depth table for rainfall events: [`parse_pfds`], refusing intensity tables.
pub fn parse_pfds_csv(text: &str) -> Result<PfTable, String> {
    let t = parse_pfds(text)?;
    if t.intensity {
        return Err("This NOAA table holds intensities; download the precipitation depth table instead.".into());
    }
    Ok(t)
}

/// Reads a NOAA Atlas 14 precipitation frequency file: the current PFDS CSV (durations as
/// rows, depth or intensity, inches or millimetres) or the earlier layout (through 2011:
/// frequencies as rows under "MEAN PRECIPITATION FREQUENCY ESTIMATES"). A PFDS error reply
/// ("ErrorMsg = '...'", e.g. a point outside every project area) is returned as the error.
pub fn parse_pfds(text: &str) -> Result<PfTable, String> {
    if let Some(i) = text.find("ErrorMsg") {
        let msg = text.get(i..).unwrap_or_default().split_once('=').map_or("", |(_, m)| m);
        let msg = msg.lines().next().unwrap_or_default().trim().trim_end_matches(';').trim().trim_matches('\'');
        if !msg.is_empty() {
            return Err(format!("NOAA: {msg}"));
        }
    }
    if text.lines().any(|l| l.trim().eq_ignore_ascii_case("MEAN PRECIPITATION FREQUENCY ESTIMATES")) {
        return parse_pfds_2011(text);
    }
    let mut lines = text.lines().map(str::trim);
    let header: Vec<&str> = text
        .lines()
        .map(str::trim)
        .take_while(|l| !l.eq_ignore_ascii_case("PRECIPITATION FREQUENCY ESTIMATES"))
        .filter(|l| !l.is_empty())
        .collect();
    let metric = header.first().is_some_and(|l| l.to_ascii_lowercase().contains("millimet"));
    let intensity = !header.iter().any(|l| l.to_ascii_lowercase().contains("depth"))
        && header.iter().any(|l| l.to_ascii_lowercase().contains("intensity"));
    if !lines.any(|l| l.eq_ignore_ascii_case("PRECIPITATION FREQUENCY ESTIMATES")) {
        return Err(
            "Not a NOAA Atlas 14 precipitation frequency CSV (no PRECIPITATION FREQUENCY ESTIMATES table).".into()
        );
    }
    let aris_line = lines.next().unwrap_or("");
    let aris: Vec<f64> = aris_line
        .split_once(':')
        .map(|(_, r)| r.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .unwrap_or_default();
    if aris.is_empty() {
        return Err("The precipitation frequency table has no recurrence intervals.".into());
    }
    let k = if metric { 1.0 / 25.4 } else { 1.0 };
    let mut rows = Vec::new();
    for l in lines {
        if l.is_empty() {
            break;
        }
        let Some((label, rest)) = l.split_once(':') else { break };
        let Some(hours) = duration_hours(label) else { continue };
        // Rows read "24-hr:, 2.22,2.76,…": skip the empty field after the colon.
        let values = rest.trim().trim_start_matches(',');
        let depths: Vec<f64> =
            values.split(',').map(|v| v.trim().parse::<f64>().map(|d| d * k).unwrap_or(f64::NAN)).collect();
        rows.push(PfRow { label: label.trim().to_string(), hours, depths });
    }
    if rows.is_empty() {
        return Err("The precipitation frequency table has no durations.".into());
    }
    let pick = |key: &str| header.iter().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().to_string());
    let source = [
        header.get(1).map(|s| s.to_string()),
        pick("Project area:"),
        pick("Latitude:").map(|v| format!("Lat {v}")),
        pick("Longitude:").map(|v| format!("Lon {v}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    Ok(PfTable { source, aris, rows, intensity })
}

/// The earlier NOAA layout: "Freq (yr),5-min,10-min,...,24-hr" then one row per frequency.
fn parse_pfds_2011(text: &str) -> Result<PfTable, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let metric = lines.iter().take(3).any(|l| l.to_ascii_lowercase().contains("millimet"));
    let k = if metric { 1.0 / 25.4 } else { 1.0 };
    let start = lines
        .iter()
        .position(|l| l.eq_ignore_ascii_case("MEAN PRECIPITATION FREQUENCY ESTIMATES"))
        .ok_or("No MEAN PRECIPITATION FREQUENCY ESTIMATES table.")?;
    let head = lines.get(start + 1).ok_or("The NOAA table has no column headings.")?;
    let cols: Vec<Option<f64>> = head.split(',').skip(1).map(duration_hours).collect();
    let mut rows: Vec<PfRow> = cols
        .iter()
        .zip(head.split(',').skip(1))
        .filter_map(|(h, label)| h.map(|hours| PfRow { label: label.trim().to_string(), hours, depths: Vec::new() }))
        .collect();
    if rows.is_empty() {
        return Err("The NOAA table has no durations.".into());
    }
    let mut aris = Vec::new();
    for l in lines.iter().skip(start + 2) {
        let mut cells = l.split(',').map(str::trim);
        let Some(ari) = cells.next().and_then(|c| c.parse::<f64>().ok()) else { break };
        aris.push(ari);
        let values: Vec<f64> = cells.map(|v| v.parse::<f64>().map(|d| d * k).unwrap_or(f64::NAN)).collect();
        let mut r = rows.iter_mut();
        for (h, v) in cols.iter().zip(values.iter().chain(std::iter::repeat(&f64::NAN))) {
            if h.is_some()
                && let Some(row) = r.next()
            {
                row.depths.push(*v);
            }
        }
    }
    if aris.is_empty() {
        return Err("The precipitation frequency table has no recurrence intervals.".into());
    }
    let pick = |key: &str| {
        lines.iter().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().trim_start_matches(',').trim().to_string())
    };
    let source = [
        lines.first().map(|s| s.to_string()),
        pick("Station,"),
        pick("Lat (dd),").map(|v| format!("Lat {v}")),
        pick("Lon (dd),").map(|v| format!("Lon {v}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    Ok(PfTable { source, aris, rows, intensity: false })
}

/// Event name for a recurrence interval: "2-Year", "100-Year".
pub fn event_name(ari: f64) -> String {
    if ari.fract() == 0.0 { format!("{}-Year", ari as i64) } else { format!("{ari}-Year") }
}

/// Rainfall events for the chosen duration row and ARI columns, using `storm`.
pub fn events(table: &PfTable, row: usize, aris: &[usize], storm: &str) -> Vec<RainEvent> {
    let Some(r) = table.rows.get(row) else { return Vec::new() };
    aris.iter()
        .filter_map(|&i| {
            let depth = table.depth(r, i)?;
            let ari = *table.aris.get(i)?;
            Some(RainEvent::new(&event_name(ari), storm, (depth * 100.0).round() / 100.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NOAA Atlas 14 PFDS output (public domain) for a point in New Hampshire.
    const NH: &str = "Point precipitation frequency estimates (inches)
NOAA Atlas 14 Volume 10 Version 3
Data type: Precipitation depth
Time series type: Partial duration
Project area: Northeastern States
Location name (ESRI Maps): None
Station Name: None
Latitude: 43.2081 Degree
Longitude: -71.5376 Degree
Elevation (USGS): None None


PRECIPITATION FREQUENCY ESTIMATES
by duration for ARI (years):, 1,2,5,10,25,50,100,200,500,1000
5-min:, 0.283,0.338,0.428,0.503,0.606,0.684,0.765,0.857,0.988,1.10
60-min:, 0.858,1.02,1.30,1.53,1.84,2.07,2.32,2.60,3.00,3.32
24-hr:, 2.22,2.76,3.65,4.39,5.41,6.17,6.98,7.90,9.24,10.4
2-day:, 2.57,3.18,4.17,4.99,6.13,6.97,7.88,8.97,10.6,12.1

LOWER LIMIT OF 90% CONFIDENCE INTERVAL
by duration for ARI (years):, 1,2,5,10,25,50,100,200,500,1000
5-min:, 0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1
";

    #[test]
    fn reads_depth_table() {
        let t = parse_pfds_csv(NH).unwrap();
        assert_eq!(t.aris.len(), 10);
        assert_eq!(t.rows.len(), 4, "stops at the end of the first block");
        assert_eq!(t.rows[0].hours, 5.0 / 60.0);
        assert_eq!((t.rows[2].label.as_str(), t.rows[2].hours, t.rows[2].depths[6]), ("24-hr", 24.0, 6.98));
        assert_eq!(t.rows[3].hours, 48.0);
        assert!(t.source.contains("Northeastern States") && t.source.contains("43.2081"));
        let ev = events(&t, 2, &[1, 3, 6], "Type III 24-hr");
        let got: Vec<(&str, f64)> = ev.iter().map(|e| (e.name.as_str(), e.depth)).collect();
        assert_eq!(got, vec![("2-Year", 2.76), ("10-Year", 4.39), ("100-Year", 6.98)]);
        assert!(ev.iter().all(|e| e.storm == "Type III 24-hr"));
    }

    #[test]
    fn converts_millimetres_and_rejects_other_files() {
        let mm = NH.replace("(inches)", "(millimeters)").replace("2.22,2.76", "254.0,2.76");
        let t = parse_pfds_csv(&mm).unwrap();
        assert!((t.rows[2].depths[0] - 10.0).abs() < 1e-9);
        assert!(parse_pfds_csv("a,b,c\n1,2,3").is_err());
        assert!(parse_pfds_csv(&NH.replace("Precipitation depth", "Precipitation intensity")).is_err());
        // As an IDF source an intensity table is fine: depth = intensity x duration.
        let i = parse_pfds(&NH.replace("Precipitation depth", "Precipitation intensity")).unwrap();
        assert!(i.intensity);
        assert!((i.depth(&i.rows[2], 0).unwrap() - 2.22 * 24.0).abs() < 1e-9);
    }

    /// The earlier PFDS layout (through 2011), as documented with public sample values for
    /// Montgomery, Maryland (frequencies as rows).
    const MD_2011: &str = "NOAA Atlas 14 Precipitation frequency estimates (inches)
Point Estimates
Data series, Partial duration maxima
State, Maryland
Station, Montgomery
Lon (dd), -77.2120
Lat (dd), 39.1913
Elev (feet), 406
MEAN PRECIPITATION FREQUENCY ESTIMATES
Freq (yr),5-min,10-min,15-min,30-min,60-min,120-min,3-hr,6-hr,12-hr,24-hr
1,   0.34, 0.54, 0.68, 0.93, 1.16, 1.37, 1.48, 1.82, 2.23, 2.57
2,   0.41, 0.65, 0.82, 1.13, 1.42, 1.67, 1.79, 2.21, 2.69, 3.10
100, 0.72, 1.15, 1.45, 2.22, 3.06, 3.83, 4.17, 5.25, 6.72, 8.23
";

    #[test]
    fn reads_the_earlier_layout_and_error_replies() {
        let t = parse_pfds(MD_2011).unwrap();
        assert_eq!(t.aris, vec![1.0, 2.0, 100.0]);
        assert_eq!(t.rows.len(), 10);
        assert_eq!((t.rows[5].label.as_str(), t.rows[5].hours), ("120-min", 2.0));
        assert_eq!((t.rows[9].hours, t.rows[9].depths.clone()), (24.0, vec![2.57, 3.10, 8.23]));
        assert!(t.source.contains("Montgomery") && t.source.contains("39.1913"));
        let err = "Date/time (GMT):  Wed Oct 28 02:48:50 2026\npyRunTime:  0.027\nresult = 'none';\nErrorMsg =  'Error 3.0: Selected location is not within a project area';\n";
        assert_eq!(parse_pfds(err).unwrap_err(), "NOAA: Error 3.0: Selected location is not within a project area");
        assert!(
            pfds_url(42.03, -93.62, true).ends_with("lat=42.0300&lon=-93.6200&data=depth&units=english&series=pds")
        );
    }
}
