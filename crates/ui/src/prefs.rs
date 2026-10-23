//! Application preferences (Settings > General): kept per user, not per project.

use crate::platform::store;
use crate::theme::ThemeKind;
use serde::{Deserialize, Serialize};

const KEY: &str = "prefs";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeKind,
    /// Minutes between timed backups of changed projects (`<project>.bak`); 0 turns them off.
    pub backup_minutes: f64,
    /// Allow several Project Reports windows at once (otherwise reopening replaces the window).
    pub multiple_project_reports: bool,
    /// Pop-up tips on node editing screens.
    pub show_tips: bool,
    /// Open the Messages window when a calculation produces new hints, warnings or errors.
    pub show_messages: bool,
    /// Folder holding rainfall library files (.hcr, .huh, .idf, *-Rain.txt); desktop only.
    pub rainfall_folder: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: ThemeKind::Classic, backup_minutes: 5.0, multiple_project_reports: false, show_tips: true, show_messages: true, rainfall_folder: String::new() }
    }
}

impl Prefs {
    /// Saved preferences, or the defaults when none are stored or they cannot be read.
    pub fn load() -> Self {
        store::read(KEY).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(t) = serde_json::to_string_pretty(self) {
            store::write(KEY, &t);
        }
    }

    /// Seconds between backups, or None when backups are off.
    pub fn backup_interval(&self) -> Option<f64> {
        (self.backup_minutes.is_finite() && self.backup_minutes > 0.0).then(|| self.backup_minutes * 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"theme":"ModernDark"}"#).unwrap();
        assert_eq!(p.theme, ThemeKind::ModernDark);
        assert_eq!(p.backup_minutes, 5.0);
        assert_eq!(Prefs { backup_minutes: 0.0, ..p }.backup_interval(), None);
    }
}
