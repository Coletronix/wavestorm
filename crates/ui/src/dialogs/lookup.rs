//! Lookup tables: the Rainfall Event Lookup (county tables, IDF files, NOAA Atlas 14 CSV),
//! curve numbers (TR-55 Table 2-2), Manning's n, culvert entrance losses.

use crate::doc::Doc;
use crate::library;
use crate::platform::Purpose;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{RichText, Sense, Ui, vec2};
use stormwaves_core::cn::{CN_TABLE, Hsg};
use stormwaves_core::noaa::PfTable;
use stormwaves_core::rainfall::RainEvent;
use stormwaves_core::noaa::parse_pfds_csv;
use stormwaves_core::rainlib::{IdfFile, RainTable, parse_idf, parse_rain_table, storm_for_code};

/// Where the events come from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A county lookup table (`*-Rain.txt`), by name.
    Table(String),
    /// An IDF file, by name.
    Idf(String),
    /// A NOAA Atlas 14 precipitation-frequency CSV loaded by the user.
    Noaa,
}

/// Rainfall Event Lookup: creates the project's rainfall events from a county lookup table, an
/// IDF file or a NOAA Atlas 14 table, with the right storm distribution and depths.
pub struct EventLookup {
    pub source: Option<Source>,
    ext_tables: Vec<RainTable>,
    ext_idfs: Vec<IdfFile>,
    noaa: Option<(String, PfTable)>,
    state: String,
    /// Selected county row (index into the state's rows).
    county: Option<usize>,
    /// Table, state and county the storm was last chosen for.
    last_key: String,
    pub storm: String,
    /// Which candidate events to create (by position), reset when the candidates change.
    pub checked: Vec<bool>,
    signature: String,
    /// NOAA: selected duration row.
    pub noaa_row: Option<usize>,
    /// IDF: storm duration (hours) used to turn intensity into depth, and a name suffix.
    idf_hours: f64,
    idf_suffix: String,
    message: String,
}

impl EventLookup {
    pub fn new(doc: &Doc) -> Self {
        let storm = doc.project.settings.events.get(doc.project.settings.current_event).map_or_else(|| "Type II 24-hr".to_string(), |e| e.storm.clone());
        let lib = library::current();
        let source = lib.tables.iter().find(|t| t.name.eq_ignore_ascii_case("NRCS2-Rain")).or(lib.tables.first()).map(|t| Source::Table(t.name.clone()));
        EventLookup {
            source,
            ext_tables: Vec::new(),
            ext_idfs: Vec::new(),
            noaa: None,
            state: String::new(),
            county: None,
            last_key: String::new(),
            storm,
            checked: Vec::new(),
            signature: String::new(),
            noaa_row: None,
            idf_hours: 24.0,
            idf_suffix: String::new(),
            message: String::new(),
        }
    }

    /// Loads a file chosen by the user: a `*-Rain.txt` table, an IDF file, or a NOAA Atlas 14 CSV.
    pub fn load_file(&mut self, file_name: &str, bytes: &[u8]) -> Result<(), String> {
        let text = String::from_utf8_lossy(bytes);
        let lower = file_name.to_ascii_lowercase();
        let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s).to_string();
        let first_line = text.trim_start_matches('\u{feff}').lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_ascii_lowercase();
        self.county = None;
        self.message.clear();
        if lower.ends_with("-rain.txt") || first_line.starts_with("id,state,county") {
            let table = parse_rain_table(&stem, &text)?;
            self.source = Some(Source::Table(table.name.clone()));
            self.ext_tables.retain(|t| t.name != table.name);
            self.ext_tables.push(table);
        } else if lower.ends_with(".idf") {
            let mut idf = parse_idf(&text)?;
            if idf.name == "IDF" || idf.name == "NOAA Atlas 14" {
                idf.name = stem;
            }
            self.source = Some(Source::Idf(idf.name.clone()));
            self.ext_idfs.retain(|t| t.name != idf.name);
            self.ext_idfs.push(idf);
        } else {
            let table = parse_pfds_csv(&text)?;
            let row = table.rows.iter().position(|r| (r.hours - 24.0).abs() < 1e-9).or(Some(0));
            self.noaa = Some((file_name.to_string(), table));
            self.noaa_row = row;
            self.source = Some(Source::Noaa);
        }
        Ok(())
    }
}

struct Candidates {
    events: Vec<RainEvent>,
    /// Names are the check boxes' labels; `default_on` marks the usual design events.
    default_on: Vec<bool>,
}

const COMMON_ARIS: [f64; 6] = [2.0, 5.0, 10.0, 25.0, 50.0, 100.0];

pub fn event_lookup(ctx: &egui::Context, doc: &mut Doc, st: &mut EventLookup, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let lib = library::current();
    let storm_names = library::storm_names(&doc.project);
    let storm_opts: Vec<(String, String)> = storm_names.iter().map(|n| (n.clone(), n.clone())).collect();
    crate::frame::Win::new("Rainfall Event Lookup").collapsible(false).resizable(true).default_size(vec2(800.0, 560.0)).show(ctx, |ui| {
        let t = Tokens::get(ui.ctx());
        // Sources: tables and IDF files from the library and from files loaded here.
        let mut sources: Vec<(Source, String)> = Vec::new();
        for tb in st.ext_tables.iter().chain(lib.tables.iter()) {
            if !sources.iter().any(|s| s.0 == Source::Table(tb.name.clone())) {
                sources.push((Source::Table(tb.name.clone()), tb.name.clone()));
            }
        }
        for i in st.ext_idfs.iter().chain(lib.idfs.iter()) {
            if !sources.iter().any(|s| s.0 == Source::Idf(i.name.clone())) {
                sources.push((Source::Idf(i.name.clone()), format!("{} (IDF)", i.name)));
            }
        }
        if let Some((name, _)) = &st.noaa {
            sources.push((Source::Noaa, format!("{name} (NOAA Atlas 14)")));
        }
        ui.horizontal(|ui| {
            let mut cur = st.source.clone().unwrap_or(Source::Noaa);
            if sources.is_empty() {
                ui.vertical(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new("No lookup tables available").color(t.text_dim));
                });
            } else if widgets::combo_field(ui, "lk-src", "Event Lookup File:", &mut cur, &sources, 250.0) {
                st.source = Some(cur);
                st.county = None;
            }
            ui.vertical(|ui| {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, "Load File...", 90.0).on_hover_text("Load a lookup table (*-Rain.txt), an IDF file or a NOAA Atlas 14 CSV for this session").clicked() {
                        actions.push(WindowAction::PickFile(Purpose::NoaaCsv));
                    }
                    if widgets::button(ui, "Import Library...", 110.0).on_hover_text("Import several rainfall files (.hcr, .huh, .idf, *-Rain.txt) into the rainfall library").clicked() {
                        actions.push(WindowAction::PickFile(Purpose::RainLib));
                    }
                });
            });
        });
        if !st.message.is_empty() {
            ui.colored_label(t.warning, &st.message);
        }
        let candidates = match st.source.clone() {
            Some(Source::Table(name)) => {
                // The tables are moved out while the pane borrows the rest of the state.
                let ext = std::mem::take(&mut st.ext_tables);
                let out = ext.iter().chain(lib.tables.iter()).find(|x| x.name == name).and_then(|tb| table_pane(ui, st, tb, &storm_names));
                st.ext_tables = ext;
                out
            }
            Some(Source::Idf(name)) => {
                let ext = std::mem::take(&mut st.ext_idfs);
                let out = ext.iter().chain(lib.idfs.iter()).find(|x| x.name == name).map(|i| idf_pane(ui, st, i, &storm_opts));
                st.ext_idfs = ext;
                out
            }
            Some(Source::Noaa) => match &st.noaa {
                Some((_, tb)) => Some(noaa_pane(ui, tb, &mut st.noaa_row, &mut st.storm, &storm_opts)),
                None => None,
            },
            None => None,
        };
        if st.source.is_none() || (candidates.is_none() && sources.is_empty()) {
            ui.add_space(8.0);
            ui.label("Choose a lookup table above. Set the rainfall library folder in Settings > General (desktop),");
            ui.label("or use Import Library... to add the rainfall files (*-Rain.txt, .idf, .hcr) from your computer.");
            ui.label("A NOAA Atlas 14 depth CSV from the Precipitation Frequency Data Server can be loaded with Load File...");
        }
        let mut events: Vec<RainEvent> = Vec::new();
        if let Some(c) = &candidates {
            let sig: String = c.events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join("|");
            if sig != st.signature || st.checked.len() != c.events.len() {
                st.signature = sig;
                st.checked = c.default_on.clone();
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Create events:").strong());
            ui.horizontal_wrapped(|ui| {
                for (i, e) in c.events.iter().enumerate() {
                    if let Some(on) = st.checked.get_mut(i) {
                        widgets::check(ui, on, &format!("{}  {:.2}\"", e.name, e.depth));
                        ui.add_space(6.0);
                    }
                }
            });
            events = c.events.iter().zip(&st.checked).filter(|(_, on)| **on).map(|(e, _)| e.clone()).collect();
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ok = !events.is_empty() && !st.storm.is_empty();
            if widgets::button_enabled(ui, "Replace Events", 110.0, ok).on_hover_text("Replace the project's rainfall events").clicked() {
                actions.push(WindowAction::SetEvents { events: events.clone(), replace: true });
                keep = false;
            }
            if widgets::button_enabled(ui, "Add Events", 100.0, ok).on_hover_text("Add to (or update) the project's rainfall events").clicked() {
                actions.push(WindowAction::SetEvents { events: events.clone(), replace: false });
                keep = false;
            }
            if widgets::button(ui, "Cancel", 78.0).clicked() {
                keep = false;
            }
            if widgets::button(ui, "Help", 78.0).clicked() {
                widgets::request_help(ui.ctx(), "rainfall");
            }
        });
    });
    keep
}

/// County table: state combo, county list, storm distribution choice; returns the events.
fn table_pane(ui: &mut Ui, st: &mut EventLookup, table: &RainTable, storm_names: &[String]) -> Option<Candidates> {
    let t = Tokens::get(ui.ctx());
    let states = table.states();
    if !states.contains(&st.state) {
        st.state = states.first().cloned().unwrap_or_default();
        st.county = None;
    }
    ui.horizontal(|ui| {
        let opts: Vec<(String, String)> = states.iter().map(|s| (s.clone(), s.clone())).collect();
        if widgets::combo_field(ui, "lk-state", "State:", &mut st.state, &opts, 90.0) {
            st.county = None;
        }
        ui.vertical(|ui| {
            ui.add_space(16.0);
            ui.label(RichText::new(format!("{} counties/stations, {} depth columns", table.counties(&st.state).len(), table.columns.len())).color(t.text_dim));
        });
    });
    let rows = table.counties(&st.state);
    let depth_w = (((ui.available_width() - 24.0 - 18.0 - 250.0 - 80.0) / table.columns.len().max(1) as f32).clamp(38.0, 64.0)).floor();
    let mut cols: Vec<(String, f32, bool)> = vec![("County".into(), 250.0, false), ("Storm".into(), 80.0, false)];
    cols.extend(table.columns.iter().map(|c| (c.replace("-Year", "-yr"), depth_w, true)));
    let cols_ref: Vec<(&str, f32, bool)> = cols.iter().map(|(n, w, r)| (n.as_str(), *w, *r)).collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| [r.county.clone(), r.storm.clone()].into_iter().chain(r.depths.iter().map(|d| if *d > 0.0 { format!("{d:.2}") } else { String::new() })).collect())
        .collect();
    if st.county.is_none() && !rows.is_empty() {
        // The first county is selected until the user picks another.
        st.county = Some(0);
    }
    let h = (ui.available_height() - 170.0).max(110.0);
    widgets::list_table(ui, "lk-county", &cols_ref, &cells, &mut st.county, h);
    let row = st.county.and_then(|i| rows.get(i)).copied();
    let key = format!("{}|{}|{:?}", table.name, st.state, st.county);
    if key != st.last_key {
        st.last_key = key;
        if let Some(r) = row {
            let resolved = r.storm_codes().iter().find_map(|c| storm_for_code(c, storm_names));
            match resolved {
                Some(name) => {
                    st.storm = name;
                    st.message.clear();
                }
                None => {
                    let codes = r.storm_codes().join(", ");
                    st.message = if codes.is_empty() { "This table lists no storm distribution for the county; choose one below.".into() } else { format!("Storm distribution {codes} is not installed; add the rainfall files to the library or choose another storm.") };
                }
            }
        }
    }
    let Some(r) = row else {
        ui.label(RichText::new("Select a county or station.").color(t.text_dim));
        return None;
    };
    ui.horizontal(|ui| {
        let opts = storm_names.iter().map(|n| (n.clone(), n.clone())).collect::<Vec<_>>();
        widgets::combo_field(ui, "lk-storm", "Storm Type:", &mut st.storm, &opts, 240.0);
        let codes = r.storm_codes();
        if codes.len() > 1 {
            ui.vertical(|ui| {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label("Table offers:");
                    for c in codes {
                        if let Some(name) = storm_for_code(c, storm_names) {
                            if widgets::button(ui, c, 72.0).on_hover_text(&name).clicked() {
                                st.storm = name;
                            }
                        } else {
                            ui.label(RichText::new(c).color(t.text_dim));
                        }
                    }
                });
            });
        }
    });
    let events = table.events(r, &st.storm);
    if events.is_empty() {
        ui.label(RichText::new("This row has no rainfall depths.").color(t.warning));
        return None;
    }
    let default_on = vec![true; events.len()];
    Some(Candidates { events, default_on })
}

/// IDF file: depth = intensity at the storm duration x duration, for every event in the file.
fn idf_pane(ui: &mut Ui, st: &mut EventLookup, idf: &IdfFile, storm_opts: &[(String, String)]) -> Candidates {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(format!("PROVISIONAL: {}", stormwaves_core::rainlib::IDF_PROVISIONAL)).color(t.warning));
    ui.label(RichText::new(if idf.comment.is_empty() { idf.name.clone() } else { format!("{}: {}", idf.name, idf.comment) }).color(t.text_dim));
    ui.horizontal(|ui| {
        widgets::num_field(ui, "lk-idf-h", "Storm Duration:", "hours", &mut st.idf_hours, 2, 110.0);
        widgets::text_field(ui, "Event name suffix:", &mut st.idf_suffix, 110.0);
        widgets::combo_field(ui, "lk-storm", "Storm Type:", &mut st.storm, storm_opts, 220.0);
    });
    st.idf_hours = st.idf_hours.clamp(1.0 / 60.0, 24.0 * 30.0);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let marks = [5.0, 15.0, 30.0, 60.0, 120.0, 360.0, 1440.0];
    for e in &idf.events {
        let mut r = vec![e.name.clone(), if (e.factor - 1.0).abs() > 1e-9 { format!("{:.2}", e.factor) } else { String::new() }];
        r.extend(marks.iter().map(|m| e.intensity(*m).map(|i| format!("{i:.2}")).unwrap_or_default()));
        rows.push(r);
    }
    let heads = ["5 min", "15 min", "30 min", "1 hr", "2 hr", "6 hr", "24 hr"];
    let mut cols: Vec<(&str, f32, bool)> = vec![("Event", 120.0, false), ("Freq. factor", 90.0, true)];
    cols.extend(heads.iter().map(|h| (*h, 62.0, true)));
    ui.label("Intensity (in/hr) by duration:");
    let mut sel = None;
    widgets::list_table(ui, "lk-idf", &cols, &rows, &mut sel, (ui.available_height() - 110.0).max(100.0));
    let events = idf.events_for(&st.storm, st.idf_hours, &st.idf_suffix);
    let default_on = vec![true; events.len()];
    Candidates { events, default_on }
}

/// NOAA Atlas 14 table: duration rows by recurrence interval.
fn noaa_pane(ui: &mut Ui, table: &PfTable, row: &mut Option<usize>, storm: &mut String, storm_opts: &[(String, String)]) -> Candidates {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(&table.source).color(t.text_dim));
    widgets::combo_field(ui, "lk-storm", "Storm Type:", storm, storm_opts, 240.0);
    let mut cols: Vec<(String, f32, bool)> = vec![("Duration".into(), 80.0, false)];
    cols.extend(table.aris.iter().map(|a| (format!("{}-Year", *a as i64), 62.0, true)));
    let cols_ref: Vec<(&str, f32, bool)> = cols.iter().map(|(n, w, r)| (n.as_str(), *w, *r)).collect();
    let rows: Vec<Vec<String>> = table.rows.iter().map(|r| std::iter::once(r.label.clone()).chain(r.depths.iter().map(|d| if d.is_finite() { format!("{d:.2}") } else { String::new() })).collect()).collect();
    let h = (ui.available_height() - 150.0).max(100.0);
    widgets::list_table(ui, "noaa-table", &cols_ref, &rows, row, h);
    let all: Vec<usize> = (0..table.aris.len()).collect();
    let events = stormwaves_core::noaa::events(table, row.unwrap_or(0), &all, storm.as_str());
    let default_on = events.iter().map(|e| COMMON_ARIS.iter().any(|a| stormwaves_core::noaa::event_name(*a) == e.name)).collect();
    Candidates { events, default_on }
}


/// Manning's n for channels and conduits (typical values from standard hydraulics texts).
pub const MANNING: &[(&str, f64)] = &[
    ("PVC / smooth plastic pipe", 0.011),
    ("Steel, smooth", 0.012),
    ("Corrugated PE, smooth interior", 0.012),
    ("Concrete pipe (RCP)", 0.013),
    ("Concrete, finished", 0.012),
    ("Concrete, unfinished", 0.014),
    ("Cast iron / vitrified clay", 0.013),
    ("Brick", 0.015),
    ("Asphalt", 0.016),
    ("Corrugated metal pipe (CMP)", 0.024),
    ("Corrugated PE, corrugated interior", 0.022),
    ("Earth, clean & straight", 0.022),
    ("Earth, gravel bottom", 0.025),
    ("Earth, clean & winding", 0.025),
    ("Earth, short grass", 0.027),
    ("Earth, grass & some weeds", 0.030),
    ("Earth, dense weeds", 0.035),
    ("Earth, stony bottom & weedy banks", 0.035),
    ("Riprap", 0.035),
    ("Natural stream, clean & straight", 0.030),
    ("Natural stream, clean & winding", 0.040),
    ("Natural stream, winding with weeds & stones", 0.048),
    ("Natural stream, sluggish & weedy", 0.070),
    ("Natural stream, very weedy", 0.100),
    ("Floodplain, short grass pasture", 0.030),
    ("Floodplain, high grass", 0.035),
    ("Floodplain, light brush", 0.050),
    ("Floodplain, medium to dense brush", 0.070),
    ("Floodplain, trees", 0.100),
];

/// Culvert entrance loss coefficients (FHWA HDS-5).
pub const ENTRANCES: &[(&str, f64)] = &[
    ("RCP, groove end projecting", 0.2),
    ("RCP, square edge projecting", 0.5),
    ("RCP, groove end w/headwall", 0.2),
    ("RCP, square edge headwall", 0.5),
    ("RCP, rounded edge headwall", 0.2),
    ("RCP, mitered to conform to fill", 0.7),
    ("RCP, end-section conforming to fill", 0.5),
    ("RCP, beveled edges", 0.2),
    ("CMP, projecting, no headwall", 0.9),
    ("CMP, square edge headwall", 0.5),
    ("CMP, mitered to conform to fill", 0.7),
    ("CMP, end-section conforming to fill", 0.5),
    ("CMP, beveled edges", 0.2),
    ("Box, headwall, square edges", 0.5),
    ("Box, headwall, rounded/beveled edges", 0.2),
    ("Box, wingwalls 30-75 deg, square crown", 0.4),
    ("Box, wingwalls 10-25 deg, square crown", 0.5),
    ("Box, wingwalls parallel, square crown", 0.7),
    ("Side- or slope-tapered inlet", 0.2),
];

/// State of the CN lookup popup.
#[derive(Default)]
pub struct CnLookup {
    pub open: bool,
    pub group: String,
}

/// Shows the curve-number table; returns (CN, description) when the user picks a cell.
pub fn cn_lookup(ctx: &egui::Context, st: &mut CnLookup) -> Option<(f64, String)> {
    if !st.open {
        return None;
    }
    let mut picked = None;
    let mut open = true;
    let t = Tokens::get(ctx);
    crate::frame::Win::new("Curve Number Lookup (TR-55 Table 2-2)").open(&mut open).default_size(vec2(560.0, 440.0)).resizable(true).show(ctx, |ui| {
        let groups: Vec<&str> = {
            let mut g: Vec<&str> = CN_TABLE.iter().map(|r| r.group).collect();
            g.dedup();
            g
        };
        ui.horizontal(|ui| {
            for g in &groups {
                if crate::widgets::radio(ui, st.group == *g || (st.group.is_empty() && *g == groups[0]), g).clicked() {
                    st.group = g.to_string();
                }
            }
        });
        let group = if st.group.is_empty() { groups.first().copied().unwrap_or("") } else { st.group.as_str() };
        ui.label(RichText::new("Click a soil-group value to use it.").color(t.text_dim));
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("cn-grid").striped(true).spacing(vec2(10.0, 3.0)).show(ui, |ui| {
                ui.label(RichText::new("Cover description").strong());
                for h in Hsg::ALL {
                    ui.label(RichText::new(format!("HSG {}", h.letter())).strong());
                }
                ui.end_row();
                for row in CN_TABLE.iter().filter(|r| r.group == group) {
                    let imp = row.impervious_pct.filter(|p| *p > 0.0 && *p < 100.0).map(|p| format!(" ({p}% imp)")).unwrap_or_default();
                    ui.label(format!("{}{imp}", row.cover));
                    for h in Hsg::ALL {
                        match row.cn_for(h) {
                            Some(cn) => {
                                if ui.add(egui::Label::new(RichText::new(format!("{cn}")).color(t.select)).sense(Sense::click())).on_hover_text(row.description(h)).clicked() {
                                    picked = Some((cn as f64, row.description(h)));
                                }
                            }
                            None => {
                                ui.label("-");
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        });
    });
    if picked.is_some() || !open {
        st.open = false;
    }
    picked
}

/// A Manning's n lookup popup attached to a button; returns the value picked.
pub fn manning_button(ui: &mut Ui, id: &str) -> Option<f64> {
    let mut picked = None;
    let resp = widgets::button(ui, "Lookup", 60.0);
    let _ = id;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(300.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (name, n) in MANNING {
                if ui.button(format!("{n:.3}  {name}")).clicked() {
                    picked = Some(*n);
                    ui.close();
                }
            }
        });
    });
    picked
}
