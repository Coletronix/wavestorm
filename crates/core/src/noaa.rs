//! NOAA Atlas 14 point precipitation-frequency tables (the CSV downloaded from the Precipitation
//! Frequency Data Server), used to look up rainfall event depths for a site.

use crate::rainfall::RainEvent;

/// Depths by duration (rows) and average recurrence interval (columns), in inches.
#[derive(Debug, Clone, PartialEq)]
pub struct PfTable {
    /// Description of the source (volume, project area, coordinates).
    pub source: String,
    /// Average recurrence intervals (years).
    pub aris: Vec<f64>,
    pub rows: Vec<PfRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PfRow {
    /// Duration as written in the table, e.g. "24-hr".
    pub label: String,
    pub hours: f64,
    /// Depth (inches) for each ARI.
    pub depths: Vec<f64>,
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

/// Reads the first "PRECIPITATION FREQUENCY ESTIMATES" block of a PFDS CSV file (depths in
/// inches or millimetres; millimetres are converted).
pub fn parse_pfds_csv(text: &str) -> Result<PfTable, String> {
    let mut lines = text.lines().map(str::trim);
    let header: Vec<&str> = text
        .lines()
        .map(str::trim)
        .take_while(|l| !l.eq_ignore_ascii_case("PRECIPITATION FREQUENCY ESTIMATES"))
        .filter(|l| !l.is_empty())
        .collect();
    let metric = header.first().is_some_and(|l| l.to_ascii_lowercase().contains("millimet"));
    if !header.iter().any(|l| l.to_ascii_lowercase().contains("depth"))
        && header.iter().any(|l| l.to_ascii_lowercase().contains("intensity"))
    {
        return Err("This NOAA table holds intensities; download the precipitation depth table instead.".into());
    }
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
    Ok(PfTable { source, aris, rows })
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
            let depth = *r.depths.get(i)?;
            let ari = *table.aris.get(i)?;
            depth.is_finite().then(|| RainEvent::new(&event_name(ari), storm, (depth * 100.0).round() / 100.0))
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
    }
}
