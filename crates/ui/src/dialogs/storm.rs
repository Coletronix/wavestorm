//! Custom rainfall distributions and IDF curves.

use super::{Buttons, conv_field, ok_cancel_apply};
use crate::chart::{self, ChartSpec, ChartView, Series};
use crate::doc::Doc;
use crate::theme::Tokens;
use crate::widgets;
use egui::{Color32, RichText, vec2};
use stormwaves_core::project::IdfCurve;
use stormwaves_core::rainfall::Storm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Mass,
    Intensity,
    Incremental,
    Chicago,
}

pub struct StormEditor {
    pub name: String,
    pub comment: String,
    pub duration: f64,
    pub kind: DataKind,
    pub values: String,
    pub smooth: bool,
    pub coef: (f64, f64, f64),
    pub peak: Option<f64>,
    pub view: ChartView,
}

impl Default for StormEditor {
    fn default() -> Self {
        StormEditor {
            name: "Custom 24-hr".into(),
            comment: String::new(),
            duration: 24.0,
            kind: DataKind::Mass,
            values: "0 0.05 0.12 0.22 0.36 0.66 0.80 0.88 0.94 0.98 1.0".into(),
            smooth: true,
            coef: (1.5, 0.2, 0.75),
            peak: None,
            view: ChartView::default(),
        }
    }
}

impl StormEditor {
    pub fn from_storm(s: &Storm) -> Self {
        StormEditor {
            name: s.name.clone(),
            comment: s.comment.clone(),
            duration: s.duration,
            kind: DataKind::Mass,
            values: s.fractions.iter().map(|v| format!("{v:.5}")).collect::<Vec<_>>().join(" "),
            smooth: s.smooth,
            ..Default::default()
        }
    }

    fn numbers(&self) -> Vec<f64> {
        self.values.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter_map(|t| t.trim().parse::<f64>().ok()).collect()
    }

    /// Builds the storm, or explains what is wrong.
    pub fn build(&self) -> Result<Storm, String> {
        if self.name.trim().is_empty() {
            return Err("Enter a name.".into());
        }
        let v = self.numbers();
        let s = match self.kind {
            DataKind::Mass => Storm::from_mass_points(&self.name, &self.comment, self.duration, &v, self.smooth).ok_or("Mass-curve values must start at 0, never decrease, and end above 0 (at least 2 values).")?,
            DataKind::Intensity => Storm::from_intensity_points(&self.name, &self.comment, self.duration, &v).ok_or("Intensity values must be non-negative (at least 2 values).")?,
            DataKind::Incremental => Storm::from_increments(&self.name, &self.comment, self.duration, &v, self.smooth).ok_or("Incremental depths must be non-negative (at least 1 value).")?,
            DataKind::Chicago => Storm::chicago(&self.name, self.duration, self.coef.0, self.coef.1, self.coef.2, self.peak, 0.05).ok_or("The IDF coefficients give no rainfall.")?,
        };
        let mut s = s;
        if !self.comment.is_empty() {
            s.comment = self.comment.clone();
        }
        Ok(s)
    }
}

/// Returns false when closed.
pub fn show(ctx: &egui::Context, doc: &mut Doc, st: &mut StormEditor) -> bool {
    let mut keep = true;
    crate::frame::Win::new("Custom Rainfall Distribution").resizable(true).default_size(vec2(640.0, 520.0)).show(ctx, |ui| {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            widgets::text_field(ui, "Name:", &mut st.name, 200.0);
            conv_field(ui, "dur", "Duration:", "hours", &mut st.duration, |x| x, |x| x.max(0.01), 2, 110.0);
            ui.vertical(|ui| {
                ui.add_space(16.0);
                widgets::check(ui, &mut st.smooth, "Smooth curve");
            });
        });
        widgets::text_field(ui, "Comment:", &mut st.comment, 420.0);
        ui.horizontal(|ui| {
            for (k, label) in [(DataKind::Mass, "Mass curve (cumulative)"), (DataKind::Intensity, "Intensity curve"), (DataKind::Incremental, "Incremental depths"), (DataKind::Chicago, "IDF coefficients (Chicago)")] {
                if widgets::radio(ui, st.kind == k, label).clicked() {
                    st.kind = k;
                }
            }
        });
        match st.kind {
            DataKind::Chicago => {
                ui.label("Depth(t) = t · a / (b + t)^c, nested about the peak.");
                ui.horizontal(|ui| {
                    conv_field(ui, "a", "a:", "", &mut st.coef.0, |x| x, |x| x, 4, 90.0);
                    conv_field(ui, "b", "b:", "", &mut st.coef.1, |x| x, |x| x, 4, 90.0);
                    conv_field(ui, "c", "c:", "", &mut st.coef.2, |x| x, |x| x, 4, 90.0);
                    super::opt_field(ui, "pk", "Peak Time:", "hours", &mut st.peak, |x| x, |x| x, 2, 110.0);
                });
            }
            _ => {
                let hint = match st.kind {
                    DataKind::Mass => "Values at even steps from 0 to the duration (first value 0).",
                    DataKind::Intensity => "Relative intensities at even steps from 0 to the duration.",
                    _ => "Depth for each even step (no value at time 0).",
                };
                ui.label(RichText::new(hint).color(t.text_dim));
                ui.add_sized(vec2(ui.available_width(), 90.0), egui::TextEdit::multiline(&mut st.values).font(egui::TextStyle::Monospace));
                let n = st.numbers().len();
                ui.label(format!("{n} values"));
            }
        }
        match st.build() {
            Ok(storm) => {
                let n = 200;
                let xs: Vec<f64> = (0..=n).map(|k| storm.duration * k as f64 / n as f64).collect();
                let ys: Vec<f64> = xs.iter().map(|x| storm.fraction_at(*x)).collect();
                let series = vec![Series { name: storm.name.clone(), color: Color32::from_rgb(220, 40, 40), x: xs, y: ys, peak_label: None }];
                let spec = ChartSpec { title: "Rainfall Depth vs. Time", subtitle: "", x_label: "Elapsed Time (hours)", y_label: "Fractional Depth", series: &series, summary: &[], three_d: false, marks: &[] };
                chart::chart(ui, &spec, &mut st.view, vec2(ui.available_width(), 200.0));
            }
            Err(e) => {
                ui.colored_label(t.warning, e);
            }
        }
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                if let Ok(storm) = st.build() {
                    doc.edit("Custom Rainfall", |p| {
                        p.storms.retain(|s| s.name != storm.name);
                        p.storms.push(storm.clone());
                    });
                    keep = false;
                }
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

pub struct IdfEditor {
    pub draft: IdfCurve,
}

impl IdfEditor {
    pub fn new(doc: &Doc) -> Self {
        let events = &doc.project.settings.events;
        let draft = doc.project.settings.idf.clone().unwrap_or_else(|| IdfCurve {
            name: "Local IDF".into(),
            events: events.iter().map(|e| (e.name.clone(), 60.0, 10.0, 0.8)).collect(),
        });
        IdfEditor { draft }
    }
}

/// IDF coefficient editor: i = a / (b + t)^c with t in minutes, i in in/hr.
pub fn idf(ctx: &egui::Context, doc: &mut Doc, st: &mut IdfEditor) -> bool {
    let mut keep = true;
    crate::frame::Win::new("IDF Curve (Rational Method)").resizable(true).default_size(vec2(560.0, 420.0)).show(ctx, |ui| {
        widgets::text_field(ui, "Name:", &mut st.draft.name, 240.0);
        ui.label("Intensity i = a / (b + t)^c   (t = duration in minutes, i in in/hr)");
        egui::Grid::new("idf").spacing(vec2(10.0, 3.0)).show(ui, |ui| {
            for h in ["Event", "a", "b", "c", "i(10)", "i(30)", "i(60)"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            let mut del = None;
            for (i, row) in st.draft.events.iter_mut().enumerate() {
                widgets::text_box(ui, &mut row.0, 100.0);
                widgets::num_box(ui, ui.id().with(("a", i)), &mut row.1, 3, 70.0, None);
                widgets::num_box(ui, ui.id().with(("b", i)), &mut row.2, 3, 70.0, None);
                widgets::num_box(ui, ui.id().with(("c", i)), &mut row.3, 4, 70.0, None);
                for d in [10.0, 30.0, 60.0] {
                    let v = row.1 / (row.2 + d).powf(row.3);
                    ui.label(format!("{v:.2}"));
                }
                if widgets::button(ui, "X", 22.0).clicked() {
                    del = Some(i);
                }
                ui.end_row();
            }
            if let Some(i) = del {
                st.draft.events.remove(i);
            }
        });
        if widgets::button(ui, "Add Event", 90.0).clicked() {
            st.draft.events.push(("New".into(), 60.0, 10.0, 0.8));
        }
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                let d = st.draft.clone();
                doc.edit("IDF Curve", |p| p.settings.idf = Some(d.clone()));
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}
