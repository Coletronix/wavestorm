//! Display units. The engine works in US customary units; these helpers convert and format
//! values for data entry and reports in English or metric units, with "large" area and volume
//! units (acres/acre-feet or hectares/megalitres) optionally switched off.

use serde::{Deserialize, Serialize};

pub const FT_PER_M: f64 = 3.280_839_895;
pub const SQFT_PER_ACRE: f64 = 43_560.0;
pub const SQFT_PER_HA: f64 = 107_639.104;
pub const CF_PER_ML: f64 = 35_314.666_7;
pub const CF_PER_M3: f64 = 35.314_666_7;
pub const MM_PER_IN: f64 = 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UnitSystem {
    #[default]
    English,
    Metric,
}

/// Unit settings of a project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Units {
    pub system: UnitSystem,
    /// Acres & acre-feet (hectares & megalitres) instead of sq ft & cu ft (m² & m³).
    pub large: bool,
    /// Extra decimal places for flows and volumes on small sites.
    #[serde(default)]
    pub low_flow: bool,
    /// Custom decimal places for reported values (automatic when not set).
    #[serde(default)]
    pub decimals: Decimals,
}

/// Decimal places chosen by the user for each kind of reported value; `None` = automatic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Decimals {
    pub area: Option<u8>,
    pub volume: Option<u8>,
    pub flow: Option<u8>,
    pub rain: Option<u8>,
    pub elevation: Option<u8>,
    pub length: Option<u8>,
}

impl Default for Units {
    fn default() -> Self {
        Units { system: UnitSystem::English, large: true, low_flow: false, decimals: Decimals::default() }
    }
}

impl Units {
    pub fn metric(&self) -> bool {
        self.system == UnitSystem::Metric
    }

    // ---- labels
    pub fn len_label(&self) -> &'static str {
        if self.metric() { "meters" } else { "feet" }
    }
    pub fn len_suffix(&self) -> &'static str {
        if self.metric() { "m" } else { "'" }
    }
    pub fn area_label(&self) -> &'static str {
        match (self.metric(), self.large) {
            (false, true) => "ac",
            (false, false) => "sf",
            (true, true) => "ha",
            (true, false) => "m²",
        }
    }
    pub fn area_heading(&self) -> &'static str {
        match (self.metric(), self.large) {
            (false, true) => "acres",
            (false, false) => "sq-ft",
            (true, true) => "hectares",
            (true, false) => "sq-meters",
        }
    }
    pub fn volume_label(&self) -> &'static str {
        match (self.metric(), self.large) {
            (false, true) => "af",
            (false, false) => "cf",
            (true, true) => "ML",
            (true, false) => "m³",
        }
    }
    pub fn volume_heading(&self) -> &'static str {
        match (self.metric(), self.large) {
            (false, true) => "acre-feet",
            (false, false) => "cubic-feet",
            (true, true) => "megaliters",
            (true, false) => "cubic-meters",
        }
    }
    pub fn flow_label(&self) -> &'static str {
        if self.metric() { "m³/s" } else { "cfs" }
    }
    pub fn rain_label(&self) -> &'static str {
        if self.metric() { "mm" } else { "\"" }
    }
    pub fn rain_heading(&self) -> &'static str {
        if self.metric() { "mm" } else { "inches" }
    }
    pub fn vel_label(&self) -> &'static str {
        if self.metric() { "m/s" } else { "fps" }
    }
    pub fn small_len_label(&self) -> &'static str {
        if self.metric() { "cm" } else { "inches" }
    }

    // ---- conversions from internal to display
    pub fn len(&self, ft: f64) -> f64 {
        if self.metric() { ft / FT_PER_M } else { ft }
    }
    pub fn area(&self, sf: f64) -> f64 {
        match (self.metric(), self.large) {
            (false, true) => sf / SQFT_PER_ACRE,
            (false, false) => sf,
            (true, true) => sf / SQFT_PER_HA,
            (true, false) => sf / (FT_PER_M * FT_PER_M),
        }
    }
    pub fn volume(&self, cf: f64) -> f64 {
        match (self.metric(), self.large) {
            (false, true) => cf / SQFT_PER_ACRE,
            (false, false) => cf,
            (true, true) => cf / CF_PER_ML,
            (true, false) => cf / CF_PER_M3,
        }
    }
    pub fn flow(&self, cfs: f64) -> f64 {
        if self.metric() { cfs / CF_PER_M3 } else { cfs }
    }
    pub fn rain(&self, inches: f64) -> f64 {
        if self.metric() { inches * MM_PER_IN } else { inches }
    }
    pub fn vel(&self, fps: f64) -> f64 {
        self.len(fps)
    }
    /// Pipe and orifice sizes: inches or centimetres.
    pub fn small_len(&self, ft: f64) -> f64 {
        if self.metric() { ft / FT_PER_M * 100.0 } else { ft * 12.0 }
    }

    // ---- conversions from display to internal
    pub fn len_in(&self, v: f64) -> f64 {
        if self.metric() { v * FT_PER_M } else { v }
    }
    pub fn area_in(&self, v: f64) -> f64 {
        let one = self.area(1.0);
        if one > 0.0 { v / one } else { v }
    }
    pub fn volume_in(&self, v: f64) -> f64 {
        let one = self.volume(1.0);
        if one > 0.0 { v / one } else { v }
    }
    pub fn flow_in(&self, v: f64) -> f64 {
        if self.metric() { v * CF_PER_M3 } else { v }
    }
    pub fn rain_in(&self, v: f64) -> f64 {
        if self.metric() { v / MM_PER_IN } else { v }
    }
    pub fn small_len_in(&self, v: f64) -> f64 {
        if self.metric() { v / 100.0 * FT_PER_M } else { v / 12.0 }
    }

    // ---- decimals (custom values from `decimals` override the automatic ones)
    pub fn area_dp(&self) -> usize {
        self.decimals.area.map_or(if self.large { 3 } else { 0 }, usize::from)
    }
    pub fn volume_dp(&self) -> usize {
        let auto = match (self.large, self.low_flow) {
            (true, false) => 3,
            (true, true) => 4,
            (false, false) => 0,
            (false, true) => 1,
        };
        self.decimals.volume.map_or(auto, usize::from)
    }
    pub fn flow_dp(&self) -> usize {
        self.decimals.flow.map_or(if self.low_flow { 3 } else { 2 }, usize::from)
    }
    pub fn rain_dp(&self) -> usize {
        self.decimals.rain.map_or(if self.metric() { 0 } else { 2 }, usize::from)
    }
    pub fn elev_dp(&self) -> usize {
        self.decimals.elevation.map_or(2, usize::from)
    }
    pub fn len_dp(&self) -> usize {
        self.decimals.length.map_or(0, usize::from)
    }

    // ---- formatted values with units, report style
    pub fn fmt_area(&self, sf: f64) -> String {
        format!("{} {}", group(self.area(sf), self.area_dp()), self.area_label())
    }
    pub fn fmt_volume(&self, cf: f64) -> String {
        format!("{} {}", group(self.volume(cf), self.volume_dp()), self.volume_label())
    }
    pub fn fmt_flow(&self, cfs: f64) -> String {
        format!("{} {}", group(self.flow(cfs), self.flow_dp()), self.flow_label())
    }
    pub fn fmt_rain(&self, inches: f64) -> String {
        let dp = self.rain_dp();
        if self.metric() { format!("{:.dp$} mm", self.rain(inches)) } else { format!("{inches:.dp$}\"") }
    }
    pub fn fmt_elev(&self, ft: f64) -> String {
        format!("{:.dp$}{}", self.len(ft), self.len_suffix(), dp = self.elev_dp())
    }
    pub fn fmt_len(&self, ft: f64) -> String {
        format!("{}{}", group(self.len(ft), self.len_dp()), self.len_suffix())
    }
}

/// Formats with thousands separators: 1234.5 → "1,234.5".
pub fn group(v: f64, dp: usize) -> String {
    if !v.is_finite() {
        return "-".into();
    }
    let s = format!("{:.*}", dp, v.abs());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let mut out = String::new();
    for (k, c) in int.chars().enumerate() {
        if k > 0 && (int.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    let negative = v < 0.0 && out.chars().any(|c| c.is_ascii_digit() && c != '0');
    if negative { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping() {
        assert_eq!(group(1234567.891, 2), "1,234,567.89");
        assert_eq!(group(831.0, 0), "831");
        assert_eq!(group(-0.001, 2), "0.00");
        assert_eq!(group(f64::NAN, 2), "-");
    }

    #[test]
    fn round_trips() {
        for system in [UnitSystem::English, UnitSystem::Metric] {
            for large in [true, false] {
                let u = Units { system, large, ..Units::default() };
                assert!((u.area_in(u.area(1000.0)) - 1000.0).abs() < 1e-9);
                assert!((u.volume_in(u.volume(1000.0)) - 1000.0).abs() < 1e-9);
                assert!((u.len_in(u.len(3.0)) - 3.0).abs() < 1e-12);
                assert!((u.small_len_in(u.small_len(2.0)) - 2.0).abs() < 1e-12);
            }
        }
        let u = Units::default();
        assert_eq!(u.fmt_area(25.0 * SQFT_PER_ACRE), "25.000 ac");
        assert_eq!(u.fmt_volume(1.903 * SQFT_PER_ACRE), "1.903 af");
    }

    #[test]
    fn custom_decimals_override_automatic_ones() {
        let mut u = Units::default();
        assert_eq!(
            (u.fmt_flow(12.3456), u.fmt_elev(100.0), u.fmt_rain(4.3)),
            ("12.35 cfs".into(), "100.00'".into(), "4.30\"".into())
        );
        u.decimals =
            Decimals { flow: Some(1), elevation: Some(3), rain: Some(1), volume: Some(0), ..Decimals::default() };
        assert_eq!(u.fmt_flow(12.3456), "12.3 cfs");
        assert_eq!(u.fmt_elev(100.0), "100.000'");
        assert_eq!(u.fmt_rain(4.3), "4.3\"");
        assert_eq!(u.fmt_volume(1.903 * SQFT_PER_ACRE), "2 af");
        // Older project files without the field read as automatic.
        let old: Units = serde_json::from_str(r#"{"system":"English","large":true}"#).unwrap();
        assert_eq!(old.decimals, Decimals::default());
    }
}
