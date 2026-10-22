//! Lookup tables: curve numbers (TR-55 Table 2-2), Manning's n, culvert entrance losses.

use crate::theme::Tokens;
use crate::widgets;
use egui::{RichText, Sense, Ui, vec2};
use stormwaves_core::cn::{CN_TABLE, Hsg};

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
