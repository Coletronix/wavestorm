//! The Calculation Settings dialog.

use super::{Buttons, conv_field, ok_cancel_apply, opt_field};
use crate::doc::Doc;
use crate::reports::CurveState;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::{Window, WindowAction};
use egui::{RichText, Ui, vec2};
use stormwaves_core::cn::CnWeighting;
use stormwaves_core::project::{CalcSettings, PondMethod, Project, ReachMethod, RunoffMethod};
use stormwaves_core::rainfall::{Amc, DurationMode, RainEvent};
use stormwaves_core::units::Units;

pub struct SettingsState {
    pub draft: CalcSettings,
    pub tab: usize,
    pub sort: usize,
    /// Draft of the Rainfall Event name box and the event name it was taken from.
    pub name: String,
    pub name_src: String,
}

impl SettingsState {
    pub fn new(p: &Project) -> Self {
        SettingsState { draft: p.settings.clone(), tab: 1, sort: 0, name: String::new(), name_src: String::new() }
    }
}

pub fn show(ctx: &egui::Context, doc: &mut Doc, st: &mut SettingsState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let mut apply = false;
    let units = doc.project.units;
    let storms: Vec<String> = crate::library::storm_names(&doc.project);
    crate::frame::Win::new("Calculation Settings").id(egui::Id::new("calc-settings")).collapsible(false).resizable(false).default_width(560.0).show(ctx, |ui| {
        widgets::tabs(ui, &["General", "Rainfall", "Time Span", "Reports", "Unit Hydro", "Advanced"], &mut st.tab);
        widgets::tab_body(ui, |ui| {
            ui.set_min_size(vec2(530.0, 280.0));
            match st.tab {
                0 => general(ui, &mut st.draft),
                1 => rainfall(ui, st, &units, &storms, doc, actions),
                2 => time_span(ui, &mut st.draft),
                3 => reports_tab(ui, &mut st.draft),
                4 => uh_tab(ui, &mut st.draft, &doc.project, actions),
                _ => advanced(ui, &mut st.draft),
            }
        });
        let help = match st.tab {
            1 => "rainfall",
            2 | 3 => "start",
            4 | 0 => "runoff",
            _ => "cn",
        };
        match ok_cancel_apply(ui, true, help) {
            Buttons::Ok => {
                apply = true;
                keep = false;
            }
            Buttons::Apply => apply = true,
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    if apply {
        let mut draft = st.draft.clone();
        if draft.events.is_empty() {
            draft.events.push(RainEvent::new("Custom", "Type II 24-hr", 3.0));
        }
        draft.current_event = draft.current_event.min(draft.events.len() - 1);
        doc.edit("Calculation Settings", |p| {
            p.settings = draft.clone();
            // Library storms and unit hydrographs in use are copied into the project.
            crate::library::adopt(p);
        });
    }
    keep
}

fn general(ui: &mut Ui, s: &mut CalcSettings) {
    widgets::group(ui, "Runoff Method", |ui| {
        for m in RunoffMethod::ALL {
            let label = match m {
                RunoffMethod::ScsTr20 => "SCS TR-20 (NRCS unit hydrograph)",
                RunoffMethod::Sbuh => "SBUH (Santa Barbara Urban Hydrograph)",
                RunoffMethod::Rational => "Rational Method",
            };
            if widgets::radio(ui, s.runoff == m, label).clicked() {
                s.runoff = m;
                if m == RunoffMethod::Sbuh && s.cn_weighting == CnWeighting::WeightedCn {
                    s.cn_weighting = CnWeighting::PerviousImpervious;
                }
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let opts: Vec<(ReachMethod, String)> = ReachMethod::ALL.iter().map(|m| (*m, m.label().to_string())).collect();
        widgets::combo_field(ui, "rm", "Reach Routing:", &mut s.reach_method, &opts, 180.0);
        let opts: Vec<(PondMethod, String)> = PondMethod::ALL.iter().map(|m| (*m, m.label().to_string())).collect();
        widgets::combo_field(ui, "pm", "Pond Routing:", &mut s.pond_method, &opts, 180.0);
    });
}

fn rainfall(ui: &mut Ui, st: &mut SettingsState, u: &Units, storms: &[String], doc: &Doc, actions: &mut Vec<WindowAction>) {
    let s = &mut st.draft;
    if s.events.is_empty() {
        s.events.push(RainEvent::new("Custom", "Type II 24-hr", 3.0));
    }
    s.current_event = s.current_event.min(s.events.len() - 1);
    let t = Tokens::get(ui.ctx());
    let idx = s.current_event;
    let Some(mut ev) = s.events.get(idx).cloned() else { return };
    let e = &mut ev;
    ui.horizontal(|ui| {
        let opts: Vec<(String, String)> = storms.iter().map(|n| (n.clone(), n.clone())).collect();
        widgets::combo_field(ui, "storm", "Storm Type:", &mut e.storm, &opts, 230.0);
        // Storm Curve picks one curve of a storm family; the families here are single curves.
        let mut curve = String::new();
        ui.add_enabled_ui(false, |ui| {
            widgets::combo_field(ui, "curve", "Storm Curve:", &mut curve, &[(String::new(), String::new())], 110.0);
        });
        ui.vertical(|ui| {
            ui.add_space(2.0);
            if widgets::button(ui, "View Storm", 96.0).clicked() {
                let mut cs = CurveState::storms();
                cs.checked = vec![e.storm.clone()];
                actions.push(WindowAction::OpenWindow(Window::Storms(cs)));
            }
            if widgets::button(ui, "More Storms", 96.0).on_hover_text("Create a custom rainfall distribution").clicked() {
                actions.push(WindowAction::OpenWindow(Window::StormEdit(crate::dialogs::storm::StormEditor::default())));
            }
        });
    });
    let comment = crate::library::find_storm(&doc.project, &e.storm).map(|s| s.comment).unwrap_or_else(|| "(storm not found)".into());
    ui.label(RichText::new(comment).color(t.text_dim));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let opts: Vec<(DurationMode, String)> = DurationMode::ALL.iter().map(|m| (*m, m.label().to_string())).collect();
        widgets::combo_field(ui, "dm", "Duration Mode:", &mut e.duration_mode, &opts, 110.0);
        ui.add_enabled_ui(e.duration_mode != DurationMode::Default, |ui| {
            conv_field(ui, "dur", "Storm Duration:", "hours", &mut e.duration, |x| x, |x| x.max(0.01), 2, 130.0);
        });
        let mut b = e.back_to_back as f64;
        if conv_field(ui, "btb", "Back-to-Back Storms:", "", &mut b, |x| x, |x| x.clamp(1.0, 10.0), 0, 140.0) {
            e.back_to_back = b.round().clamp(1.0, 10.0) as u32;
        }
    });
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            conv_field(ui, "depth", "Depth:", u.rain_heading(), &mut e.depth, |x| u.rain(x), |x| u.rain_in(x).max(0.0), 2, 110.0);
            let mut amc = e.amc.number() as f64;
            if conv_field(ui, "amc", "AMC:", "", &mut amc, |x| x, |x| x.clamp(1.0, 3.0), 0, 80.0) {
                e.amc = Amc::from_number(amc.round().clamp(1.0, 3.0) as u8);
            }
            opt_field(ui, "ri", "Rational Intensity:", "in/hr", &mut e.rational_intensity, |x| x, |x| x, 2, 170.0);
            opt_field(ui, "rd", "Rational Duration:", "min", &mut e.rational_duration, |x| x * 60.0, |x| x / 60.0, 1, 170.0);
        });
        if let Some(slot) = s.events.get_mut(idx) {
            *slot = e.clone();
        }
        ui.add_space(8.0);
        widgets::group(ui, "Rainfall Event", |ui| {
            // The name box is a draft: Save renames the current event, picking from the list switches event.
            let cur_name = s.events.get(s.current_event).map(|e| e.name.clone()).unwrap_or_default();
            if st.name_src != cur_name {
                st.name_src = cur_name.clone();
                st.name = cur_name.clone();
            }
            let taken = s.events.iter().enumerate().any(|(i, e)| i != s.current_event && e.name == st.name);
            let can_save = !st.name.trim().is_empty() && st.name != cur_name && !taken;
            ui.horizontal(|ui| {
                ui.label("Name:");
                widgets::text_box(ui, &mut st.name, 96.0);
                let names: Vec<(usize, String)> = s.events.iter().enumerate().map(|(i, e)| (i, format!("{}  ({:.2})", e.name, u.rain(e.depth)))).collect();
                let mut pick = usize::MAX;
                if widgets::combo(ui, "ev", &mut pick, &names, 30.0) && pick < s.events.len() {
                    s.current_event = pick;
                }
                if widgets::button_enabled(ui, "Save", 56.0, can_save).on_hover_text("Rename this event").clicked() {
                    if let Some(cur) = s.events.get_mut(s.current_event) {
                        cur.name = st.name.trim().to_string();
                    }
                }
                if widgets::button_enabled(ui, "Delete", 56.0, s.events.len() > 1).clicked() {
                    s.events.remove(s.current_event);
                    s.current_event = s.current_event.saturating_sub(1);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Sort by:");
                let sort_events = |s: &mut CalcSettings, how: usize| {
                    let current = s.events.get(s.current_event).map(|e| e.name.clone());
                    match how {
                        1 => s.events.sort_by(|a, b| natural(&a.name).cmp(&natural(&b.name))),
                        _ => s.events.sort_by(|a, b| a.depth.total_cmp(&b.depth)),
                    }
                    s.current_event = current.and_then(|n| s.events.iter().position(|e| e.name == n)).unwrap_or(0);
                };
                if widgets::radio(ui, st.sort == 0, "Manual").clicked() {
                    st.sort = 0;
                }
                if widgets::radio(ui, st.sort == 1, "Name").clicked() {
                    st.sort = 1;
                    sort_events(s, 1);
                }
                if widgets::radio(ui, st.sort == 2, "Depth").clicked() {
                    st.sort = 2;
                    sort_events(s, 2);
                }
            });
            ui.horizontal(|ui| {
                let import = widgets::button(ui, "Import Events From...", 150.0);
                egui::Popup::menu(&import).show(|ui| {
                    if ui.button("County rainfall table / IDF file / NOAA table...").on_hover_text("Create events from a county table, an IDF curve file (provisional format) or a NOAA Atlas 14 table").clicked() {
                        actions.push(WindowAction::OpenWindow(Window::EventLookup(crate::dialogs::storm::EventLookup::new(doc))));
                        ui.close();
                    }
                    if ui.button("IDF file (provisional)...").on_hover_text(stormwaves_core::rainlib::IDF_PROVISIONAL).clicked() {
                        actions.push(WindowAction::OpenWindow(Window::EventLookup(crate::dialogs::storm::EventLookup::new(doc))));
                        actions.push(WindowAction::PickFile(crate::platform::Purpose::NoaaCsv));
                        ui.close();
                    }
                    if ui.button("Typical 1 to 100-year events").on_hover_text("Replace events with 1, 2, 5, 10, 25, 50, 100-year (edit depths for your site)").clicked() {
                        let storm = s.events.get(s.current_event).map(|e| e.storm.clone()).unwrap_or_else(|| "Type II 24-hr".into());
                        s.events = [("1-Year", 2.6), ("2-Year", 3.1), ("5-Year", 3.9), ("10-Year", 4.6), ("25-Year", 5.6), ("50-Year", 6.4), ("100-Year", 7.3)]
                            .iter()
                            .map(|(n, d)| RainEvent::new(n, &storm, *d))
                            .collect();
                        s.current_event = 3;
                        ui.close();
                    }
                });
                if widgets::button(ui, "New", 44.0).on_hover_text("Add a copy of the current event").clicked() {
                    let mut ne = s.events.get(s.current_event).cloned().unwrap_or_else(|| RainEvent::new("Event", "Type II 24-hr", 3.0));
                    ne.name = format!("Event {}", s.events.len() + 1);
                    s.events.push(ne);
                    s.current_event = s.events.len() - 1;
                }
            });
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, "Del All", 70.0, s.events.len() > 1).on_hover_text("Delete all events but the current one").clicked() {
                    if let Some(keep) = s.events.get(s.current_event).cloned() {
                        s.events = vec![keep];
                        s.current_event = 0;
                    }
                }
                let view = widgets::button(ui, "View All", 70.0);
                egui::Popup::menu(&view).show(|ui| {
                    for (i, ev) in s.events.iter().enumerate() {
                        if ui.selectable_label(i == s.current_event, format!("{}   {} {}   {}", ev.name, format_args!("{:.2}", u.rain(ev.depth)), u.rain_heading(), ev.storm)).clicked() {
                            s.current_event = i;
                            ui.close();
                        }
                    }
                });
                if widgets::button(ui, "IDF...", 50.0).on_hover_text("IDF curve for the Rational method").clicked() {
                    actions.push(WindowAction::OpenWindow(Window::Idf(crate::dialogs::storm::IdfEditor::new(doc))));
                }
            });
        });
    });
}

fn natural(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), s.to_string())
}

fn time_span(ui: &mut Ui, s: &mut CalcSettings) {
    ui.horizontal(|ui| {
        conv_field(ui, "ts", "Time Span Start:", "hours", &mut s.span_start, |x| x, |x| x, 2, 130.0);
        conv_field(ui, "te", "Time Span End:", "hours", &mut s.span_end, |x| x, |x| x, 2, 130.0);
        conv_field(ui, "dt", "Time Step (dt):", "hours", &mut s.dt, |x| x, |x| x.clamp(0.001, 1.0), 3, 130.0);
    });
    let n = s.steps();
    ui.label(format!("{n} calculation points"));
    ui.add_space(4.0);
    ui.label(RichText::new("For 24-hour storms a span of 0-30 hours captures the full runoff volume. Use a dt no more than half the shortest Tc.").color(Tokens::get(ui.ctx()).text_dim));
}

fn reports_tab(ui: &mut Ui, s: &mut CalcSettings) {
    conv_field(ui, "step", "Tabular report step (0 = automatic shrink):", "hours", &mut s.table_step, |x| x, |x| x.max(0.0), 2, 280.0);
    ui.add_space(6.0);
    widgets::check(ui, &mut s.report_impervious, "Report pervious and impervious areas");
}

fn uh_tab(ui: &mut Ui, s: &mut CalcSettings, project: &Project, actions: &mut Vec<WindowAction>) {
    let opts: Vec<(String, String)> = crate::library::uh_names(project)
        .into_iter()
        .filter_map(|n| crate::library::find_uh(project, &n).map(|u| (n, format!("{}  (PF {:.0})", u.name, u.peak_factor()))))
        .collect();
    ui.horizontal(|ui| {
        widgets::combo_field(ui, "uh", "Unit Hydrograph:", &mut s.uh, &opts, 240.0);
        ui.vertical(|ui| {
            ui.add_space(14.0);
            if widgets::button(ui, "View UH", 80.0).clicked() {
                let mut cs = CurveState::uhs();
                cs.checked = vec![s.uh.clone()];
                actions.push(WindowAction::OpenWindow(Window::Storms(cs)));
            }
        });
    });
    if let Some(u) = crate::library::find_uh(project, &s.uh) {
        ui.label(RichText::new(u.comment).color(Tokens::get(ui.ctx()).text_dim));
    }
}

fn advanced(ui: &mut Ui, s: &mut CalcSettings) {
    ui.horizontal(|ui| {
        conv_field(ui, "ia", "Initial Abstraction Ratio Ia/S:", "", &mut s.ia_ratio, |x| x, |x| x.clamp(0.0, 1.0), 3, 200.0);
        opt_field(ui, "mintc", "Minimum Tc:", "min", &mut s.min_tc, |x| x, |x| x.max(0.0), 1, 110.0);
    });
    widgets::group(ui, "Curve Number Weighting", |ui| {
        for w in CnWeighting::ALL {
            if widgets::radio(ui, s.cn_weighting == w, w.label()).clicked() {
                s.cn_weighting = w;
            }
        }
        widgets::check(ui, &mut s.round_cn, "Round composite CN to a whole number");
    });
    ui.horizontal(|ui| {
        conv_field(ui, "rise", "Rational Rise Factor:", "", &mut s.rational_rise, |x| x, |x| x.max(0.01), 2, 150.0);
        conv_field(ui, "fall", "Rational Fall Factor:", "", &mut s.rational_fall, |x| x, |x| x.max(0.01), 2, 150.0);
    });
}
