//! Project Reports tables: area, soil, land-use, pollutant, loading and pipe listings, for all
//! nodes or a selection, with an optional bold TOTAL row.

use crate::calc::Results;
use crate::cn::Hsg;
use crate::geometry::{PipeShape, Section};
use crate::landuse::{LoadRow, loading_where};
use crate::outlets::DeviceKind;
use crate::project::{Node, NodeData, NodeId, Project};
use crate::units::{Units, group};

/// A report table: two-line headings (title over units) and rows of cells. Columns holding any
/// non-numeric cell are text (left-aligned); the rest are numbers (right-aligned).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub heads: Vec<(String, String)>,
    pub rows: Vec<Vec<String>>,
    /// Index of the TOTAL row, drawn bold.
    pub total: Option<usize>,
    /// Text shown when the table has no rows.
    pub empty: String,
}

impl Table {
    /// Whether column `c` holds text (left-aligned).
    pub fn is_text(&self, c: usize) -> bool {
        self.rows.iter().filter_map(|r| r.get(c)).any(|s| !s.is_empty() && s.replace(',', "").parse::<f64>().is_err())
    }

    /// Plain-text rendering for export and printing (columns padded to their widest cell).
    pub fn text_lines(&self) -> Vec<String> {
        let n = self.heads.len();
        let widths: Vec<usize> = (0..n)
            .map(|c| {
                let h = self.heads.get(c).map_or(0, |(a, b)| a.chars().count().max(b.chars().count()));
                self.rows.iter().filter_map(|r| r.get(c)).map(|s| s.chars().count()).fold(h, usize::max)
            })
            .collect();
        let line = |cells: Vec<&str>| {
            let mut s = String::new();
            for (c, w) in widths.iter().enumerate() {
                let v = cells.get(c).copied().unwrap_or("");
                if self.is_text(c) { s.push_str(&format!("{v:<w$}  ")) } else { s.push_str(&format!("{v:>w$}  ")) }
            }
            s.trim_end().to_string()
        };
        let mut out = vec![
            line(self.heads.iter().map(|h| h.0.as_str()).collect()),
            line(self.heads.iter().map(|h| h.1.as_str()).collect()),
        ];
        out.push("-".repeat(widths.iter().map(|w| w + 2).sum::<usize>().saturating_sub(2)));
        if self.rows.is_empty() && !self.empty.is_empty() {
            out.push(self.empty.clone());
        }
        for r in &self.rows {
            out.push(line(r.iter().map(String::as_str).collect()));
        }
        out
    }

    /// Comma-separated rendering (headings joined into one line).
    pub fn csv(&self) -> String {
        let q =
            |s: &str| if s.contains([',', '"']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
        let head: Vec<String> = self.heads.iter().map(|(a, b)| q(format!("{a} {b}").trim())).collect();
        let mut out = head.join(",");
        for r in &self.rows {
            out.push('\n');
            out.push_str(&r.iter().map(|c| q(c)).collect::<Vec<_>>().join(","));
        }
        out
    }
}

fn heading(title: &str, unit: &str) -> (String, String) {
    (title.to_string(), if unit.is_empty() { String::new() } else { format!("({unit})") })
}

fn subcats<'a>(
    project: &'a Project,
    only: Option<&'a [NodeId]>,
) -> impl Iterator<Item = (&'a Node, &'a crate::project::Subcatchment)> {
    project.nodes.iter().filter(move |n| only.is_none_or(|s| s.contains(&n.id))).filter_map(|n| match &n.data {
        NodeData::Subcat(sc) => Some((n, sc)),
        _ => None,
    })
}

fn area_heading(u: &Units) -> (String, String) {
    heading("Area", u.area_label())
}

/// Area by curve number and description, with the subcatchments using each.
pub fn area_table(project: &Project, only: Option<&[NodeId]>, totals: bool) -> Table {
    let u = &project.units;
    let mut rows: Vec<(String, f64, f64, Vec<String>)> = Vec::new();
    for (n, sc) in subcats(project, only) {
        for a in &sc.areas {
            match rows.iter_mut().find(|r| r.0 == a.description && (r.1 - a.cn).abs() < 1e-9) {
                Some(r) => {
                    r.2 += a.area;
                    if !r.3.contains(&n.number) {
                        r.3.push(n.number.clone());
                    }
                }
                None => rows.push((a.description.clone(), a.cn, a.area, vec![n.number.clone()])),
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut t = Table {
        heads: vec![area_heading(u), heading("CN", ""), heading("Description", ""), heading("Subcatchment", "Numbers")],
        ..Default::default()
    };
    let (mut tot, mut wsum) = (0.0, 0.0);
    for (d, cn, a, nodes) in &rows {
        tot += a;
        wsum += a * cn;
        t.rows.push(vec![group(u.area(*a), u.area_dp()), fmt_cn(*cn), d.clone(), nodes.join(", ")]);
    }
    if totals && tot > 0.0 {
        t.total = Some(t.rows.len());
        t.rows.push(vec![
            group(u.area(tot), u.area_dp()),
            fmt_cn((wsum / tot * 10.0).round() / 10.0),
            "TOTAL AREA".into(),
            String::new(),
        ]);
    }
    t.empty = "(no subcatchments)".into();
    t
}

/// Area by hydrologic soil group.
pub fn soil_table(project: &Project, only: Option<&[NodeId]>, totals: bool) -> Table {
    let u = &project.units;
    let mut groups: Vec<(String, f64, Vec<String>)> =
        Hsg::ALL.iter().map(|h| (format!("HSG {}", h.letter()), 0.0, Vec::new())).collect();
    groups.push(("Other".into(), 0.0, Vec::new()));
    for (n, sc) in subcats(project, only) {
        for a in &sc.areas {
            let idx = a.hsg.map(|h| h as usize).unwrap_or(4);
            if let Some(g) = groups.get_mut(idx) {
                g.1 += a.area;
                if !g.2.contains(&n.number) {
                    g.2.push(n.number.clone());
                }
            }
        }
    }
    let mut t = Table {
        heads: vec![area_heading(u), heading("Soil", "Group"), heading("Subcatchment", "Numbers")],
        ..Default::default()
    };
    let mut tot = 0.0;
    for (name, a, nodes) in &groups {
        tot += a;
        t.rows.push(vec![group(u.area(*a), u.area_dp()), name.clone(), nodes.join(", ")]);
    }
    if totals {
        t.total = Some(t.rows.len());
        t.rows.push(vec![group(u.area(tot), u.area_dp()), "TOTAL AREA".into(), String::new()]);
    }
    t
}

/// Area and impervious fraction by land use, with the subcatchments using each.
pub fn land_use_table(project: &Project, only: Option<&[NodeId]>, totals: bool) -> Table {
    let u = &project.units;
    let mut rows: Vec<(String, f64, f64, Vec<String>)> = Vec::new();
    for (n, sc) in subcats(project, only) {
        for a in &sc.areas {
            let name = a.land_use.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "(unassigned)".into());
            let imp = a.impervious_fraction() * a.area;
            match rows.iter_mut().find(|r| r.0 == name) {
                Some(r) => {
                    r.1 += a.area;
                    r.2 += imp;
                    if !r.3.contains(&n.number) {
                        r.3.push(n.number.clone());
                    }
                }
                None => rows.push((name, a.area, imp, vec![n.number.clone()])),
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut t = Table {
        heads: vec![area_heading(u), heading("Imp.", "%"), heading("Land", "Use"), heading("Subcatchment", "Numbers")],
        ..Default::default()
    };
    let (mut tot, mut timp) = (0.0, 0.0);
    let pct = |imp: f64, a: f64| format!("{:.2}", if a > 0.0 { imp / a * 100.0 } else { 0.0 });
    for (name, a, imp, nodes) in &rows {
        tot += a;
        timp += imp;
        t.rows.push(vec![group(u.area(*a), u.area_dp()), pct(*imp, *a), name.clone(), nodes.join(", ")]);
    }
    if totals && tot > 0.0 {
        t.total = Some(t.rows.len());
        t.rows.push(vec![group(u.area(tot), u.area_dp()), pct(timp, tot), "TOTAL".into(), String::new()]);
    }
    t.empty = "(no subcatchments)".into();
    t
}

/// Event-mean concentrations (mg/L) for each land use and pollutant.
pub fn pollutants_table(project: &Project) -> Table {
    let s = &project.land_use;
    let mut heads = vec![heading("Land", "Use")];
    heads.extend(s.pollutants.iter().map(|p| heading(p, "mg/L")));
    let rows = s
        .uses
        .iter()
        .map(|(name, c)| {
            std::iter::once(name.clone())
                .chain((0..s.pollutants.len()).map(|k| format!("{:.3}", c.get(k).copied().unwrap_or(0.0))))
                .collect()
        })
        .collect();
    Table { heads, rows, total: None, empty: "(no land uses defined: Settings > Land-Use)".into() }
}

fn load_heads(project: &Project, first: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut heads = first;
    heads.push(heading("Imp.", "%"));
    heads.push(heading("Rv", ""));
    heads.push(heading("Runoff", project.units.rain_heading()));
    heads.extend(project.land_use.pollutants.iter().map(|p| heading(p, "pounds")));
    heads
}

fn load_cells(u: &Units, r: &LoadRow) -> Vec<String> {
    let mut v =
        vec![format!("{:.2}", r.impervious * 100.0), format!("{:.3}", r.rv), format!("{:.2}", u.rain(r.runoff))];
    v.extend(r.loads.iter().map(|l| group(*l, 2)));
    v
}

fn sum_rows(rows: &[LoadRow], name: &str, npoll: usize) -> LoadRow {
    let mut t =
        LoadRow { land_use: name.into(), area: 0.0, impervious: 0.0, rv: 0.0, runoff: 0.0, loads: vec![0.0; npoll] };
    for r in rows {
        t.area += r.area;
        t.impervious += r.impervious * r.area;
        t.rv += r.rv * r.area;
        t.runoff += r.runoff * r.area;
        for (k, l) in r.loads.iter().enumerate() {
            if let Some(x) = t.loads.get_mut(k) {
                *x += l;
            }
        }
    }
    if t.area > 0.0 {
        t.impervious /= t.area;
        t.rv /= t.area;
        t.runoff /= t.area;
    }
    t
}

/// Pollutant loading per subcatchment.
pub fn subcat_loading_table(project: &Project, results: &Results, only: Option<&[NodeId]>, totals: bool) -> Table {
    let u = &project.units;
    let np = project.land_use.pollutants.len();
    let mut t =
        Table { heads: load_heads(project, vec![area_heading(u), heading("Subcatchment", "")]), ..Default::default() };
    let mut all = Vec::new();
    for (n, _) in subcats(project, only) {
        let id = n.id;
        let r = sum_rows(&loading_where(project, results, &|x| x == id), &n.title(), np);
        let mut row = vec![group(u.area(r.area), u.area_dp()), r.land_use.clone()];
        row.extend(load_cells(u, &r));
        t.rows.push(row);
        all.push(r);
    }
    if totals && !all.is_empty() {
        let r = sum_rows(&all, "TOTAL", np);
        let mut row = vec![group(u.area(r.area), u.area_dp()), "TOTAL".into()];
        row.extend(load_cells(u, &r));
        t.total = Some(t.rows.len());
        t.rows.push(row);
    }
    t.empty = "(no subcatchments)".into();
    t
}

/// Pollutant loading per land use.
pub fn project_loading_table(project: &Project, results: &Results, only: Option<&[NodeId]>, totals: bool) -> Table {
    let u = &project.units;
    let np = project.land_use.pollutants.len();
    let rows = loading_where(project, results, &|id| only.is_none_or(|s| s.contains(&id)));
    let mut t =
        Table { heads: load_heads(project, vec![area_heading(u), heading("Land", "Use")]), ..Default::default() };
    for r in &rows {
        let mut row = vec![group(u.area(r.area), u.area_dp()), r.land_use.clone()];
        row.extend(load_cells(u, r));
        t.rows.push(row);
    }
    if totals && !rows.is_empty() {
        let r = sum_rows(&rows, "TOTAL", np);
        let mut row = vec![group(u.area(r.area), u.area_dp()), "TOTAL".into()];
        row.extend(load_cells(u, &r));
        t.total = Some(t.rows.len());
        t.rows.push(row);
    }
    t.empty = "(no subcatchments)".into();
    t
}

fn shape_name(shape: PipeShape, width: f64, height: f64, u: &Units) -> (String, String) {
    let d = |v: f64| format!("{:.1}", u.small_len(v));
    match shape {
        PipeShape::Round => ("Round".into(), d(width)),
        _ => (format!("{shape:?}"), format!("{}x{}", d(width), d(height))),
    }
}

/// Every pipe in the project: reaches with a closed-conduit section and pond culverts.
pub fn pipe_table(project: &Project, only: Option<&[NodeId]>) -> Table {
    let u = &project.units;
    let size = format!("Size ({})", u.small_len_label());
    let mut t = Table {
        heads: vec![
            heading("Line#", ""),
            heading("Node", "Number"),
            heading("In-Invert", u.len_label()),
            heading("Out-Invert", u.len_label()),
            heading("Length", u.len_label()),
            heading("Slope", &format!("{0}/{0}", u.len_label())),
            heading("n", ""),
            heading("Shape", ""),
            (size, String::new()),
            heading("Inside-Fill", u.small_len_label()),
        ],
        empty: "(no pipes or culverts)".into(),
        ..Default::default()
    };
    let mut push =
        |node: &Node, inv_in: f64, inv_out: f64, len: f64, n: f64, shape: PipeShape, w: f64, h: f64, fill: f64| {
            let slope = if len > 0.0 { (inv_in - inv_out) / len } else { 0.0 };
            let (s, dim) = shape_name(shape, w, h, u);
            let line = t.rows.len() + 1;
            t.rows.push(vec![
                line.to_string(),
                node.number.clone(),
                format!("{:.2}", u.len(inv_in)),
                format!("{:.2}", u.len(inv_out)),
                format!("{:.1}", u.len(len)),
                format!("{slope:.4}"),
                format!("{n:.3}"),
                s,
                dim,
                format!("{:.1}", u.small_len(fill)),
            ]);
        };
    for node in project.nodes.iter().filter(|n| only.is_none_or(|s| s.contains(&n.id))) {
        match &node.data {
            NodeData::Reach(r) => {
                if let Section::Pipe { shape, width, height, fill } = &r.section {
                    push(node, r.inlet_invert, r.outlet_invert, r.length, r.n, *shape, *width, *height, *fill);
                }
            }
            NodeData::Pond(p) => {
                for d in &p.devices {
                    if let DeviceKind::Culvert { shape, width, height, length, outlet_invert, n, fill, .. } = &d.kind {
                        push(node, d.invert, *outlet_invert, *length, *n, *shape, *width, *height, *fill);
                    }
                }
            }
            _ => {}
        }
    }
    t
}

fn fmt_cn(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{v:.0}") } else { format!("{v:.1}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn::SubArea;
    use crate::project::{NodeType, Reach, Subcatchment};

    fn project() -> (Project, NodeId, NodeId) {
        let mut p = Project::default();
        let a = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let b = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let mut wood = SubArea::new(43_560.0, 55.0, "Woods, Good, HSG B");
        wood.hsg = Some(Hsg::B);
        wood.land_use = Some("Woods".into());
        let mut lot = SubArea::new(21_780.0, 98.0, "Paved parking, HSG B");
        lot.hsg = Some(Hsg::B);
        lot.land_use = Some("Pavement".into());
        if let Some(n) = p.node_mut(a) {
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![wood.clone(), lot],
                tc: vec![],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(b) {
            n.data = NodeData::Subcat(Subcatchment { areas: vec![wood], tc: vec![], uh: None, large_units: true });
        }
        (p, a, b)
    }

    #[test]
    fn totals_and_selection() {
        let (p, a, _) = project();
        let t = area_table(&p, None, true);
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.total, Some(2));
        assert_eq!(t.rows[2][0], "2.500");
        assert_eq!(area_table(&p, Some(&[a]), false).rows.len(), 2);
        let s = soil_table(&p, None, true);
        assert_eq!(s.rows[1][0], "2.500");
        let lu = land_use_table(&p, Some(&[a]), true);
        // 0.5 ac of 1.5 ac is impervious.
        assert_eq!(lu.rows.last().map(|r| r[1].clone()), Some("33.33".into()));
        assert!(t.text_lines().iter().any(|l| l.contains("TOTAL AREA")));
        assert!(t.csv().lines().count() == 4);
    }

    #[test]
    fn loading_tables_add_up() {
        let (mut p, _, _) = project();
        p.land_use.uses = vec![("Woods".into(), vec![50.0, 0.1, 1.0]), ("Pavement".into(), vec![100.0, 0.3, 2.0])];
        let r = crate::calc::calculate(&p);
        let by_sub = subcat_loading_table(&p, &r, None, true);
        let by_use = project_loading_table(&p, &r, None, true);
        let last = |t: &Table| t.rows.last().and_then(|r| r.get(5)).cloned();
        assert_eq!(last(&by_sub), last(&by_use));
        assert_eq!(pollutants_table(&p).rows.len(), 2);
    }

    #[test]
    fn pipes_are_listed() {
        let mut p = Project::default();
        let r = p.add_node(NodeType::Reach, [0.0, 0.0]);
        if let Some(n) = p.node_mut(r) {
            n.data = NodeData::Reach(Reach {
                section: Section::circular(1.5),
                length: 100.0,
                inlet_invert: 101.0,
                outlet_invert: 100.0,
                n: 0.013,
                ..Default::default()
            });
        }
        let t = pipe_table(&p, None);
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0][5], "0.0100");
        assert_eq!(t.rows[0][8], "18.0");
    }
}
