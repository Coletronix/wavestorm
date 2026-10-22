//! Printed (PDF) reports: routing diagram, node listing, and node summaries with hydrographs.

use crate::calc::{Detail, Results};
use crate::hydrograph::Hydrograph;
use crate::pdf::{Align, Document, Font, Page, Rgb};
use crate::project::{Node, NodeData, NodeType, Project};
use crate::report;

/// What to include.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintOptions {
    pub diagram: bool,
    pub node_listing: bool,
    pub area_listing: bool,
    pub summaries: bool,
    pub graphs: bool,
    pub tables: bool,
    /// Date string for the page headers (supplied by the caller).
    pub date: String,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions { diagram: true, node_listing: true, area_listing: true, summaries: true, graphs: true, tables: false, date: String::new() }
    }
}

const LETTER: (f32, f32) = (612.0, 792.0);
const MARGIN: f32 = 36.0;
const BODY_TOP: f32 = 92.0;

struct Ctx<'a> {
    project: &'a Project,
    results: &'a Results,
    opts: &'a PrintOptions,
    doc: Document,
}

impl Ctx<'_> {
    fn header(&self, page: &mut Page, number: usize) {
        let p = self.project;
        let right = page.width - MARGIN;
        page.text(MARGIN, 44.0, 11.0, Font::SansBold, Align::Left, Rgb::BLACK, &p.title);
        if !p.prepared_by.is_empty() {
            page.text(MARGIN, 57.0, 9.5, Font::Sans, Align::Left, Rgb::BLACK, &format!("Prepared by {}", p.prepared_by));
        }
        page.text(MARGIN, 69.0, 8.0, Font::Sans, Align::Left, Rgb::BLACK, &format!("Stormwaves {}", env!("CARGO_PKG_VERSION")));
        page.text(right, 44.0, 10.0, Font::SansItalic, Align::Right, Rgb::BLACK, &self.results.event_heading);
        page.text(right, 57.0, 9.5, Font::Sans, Align::Right, Rgb::BLACK, &format!("Page {number}"));
        page.text(right, 69.0, 8.0, Font::Sans, Align::Right, Rgb::BLACK, &self.opts.date);
        page.line(MARGIN, 74.0, right, 74.0, 0.8, Rgb::BLACK);
    }

    fn new_page(&mut self, landscape: bool) -> Page {
        let (w, h) = if landscape { (LETTER.1, LETTER.0) } else { LETTER };
        let mut page = Page::new(w, h);
        let n = self.doc.page_count() + 1;
        if !landscape {
            self.header(&mut page, n);
        }
        page
    }

    /// Writes monospace lines, continuing on new pages; returns the y after the last line.
    fn text_block(&mut self, page: &mut Page, mut y: f32, title: Option<&str>, lines: &[String], size: f32) -> f32 {
        let lh = size * 1.22;
        if let Some(t) = title {
            page.text(page.width / 2.0, y + 2.0, 11.0, Font::SansBold, Align::Center, Rgb::BLACK, t);
            y += 20.0;
        }
        for l in lines {
            if y + lh > page.height - MARGIN {
                let full = std::mem::replace(page, self.new_page(false));
                self.doc.add(full);
                y = BODY_TOP;
            }
            page.text(MARGIN, y + size, size, Font::Mono, Align::Left, Rgb::BLACK, l);
            y += lh;
        }
        y
    }
}

fn node_color(t: NodeType) -> (Rgb, Rgb) {
    match t {
        NodeType::Subcat => (Rgb::from_u8(196, 236, 188), Rgb::from_u8(44, 150, 44)),
        NodeType::Reach => (Rgb::from_u8(252, 214, 182), Rgb::from_u8(196, 110, 40)),
        NodeType::Pond => (Rgb::from_u8(200, 228, 252), Rgb::from_u8(40, 90, 230)),
        NodeType::Link => (Rgb::from_u8(252, 212, 222), Rgb::from_u8(214, 50, 90)),
        NodeType::Text => (Rgb::WHITE, Rgb::from_u8(128, 128, 128)),
    }
}

fn symbol(t: NodeType, cx: f32, cy: f32, d: f32) -> Vec<(f32, f32)> {
    let r = d / 2.0;
    match t {
        NodeType::Subcat => (0..6).map(|i| {
            let a = std::f32::consts::PI / 3.0 * i as f32;
            (cx + r * a.cos(), cy + r * 0.87 * a.sin())
        }).collect(),
        NodeType::Reach => {
            let h = r * 0.72;
            vec![(cx - h, cy - h), (cx + h, cy - h), (cx + h, cy + h), (cx - h, cy + h)]
        }
        NodeType::Pond => vec![(cx, cy - r * 0.95), (cx + r, cy + r * 0.72), (cx - r, cy + r * 0.72)],
        NodeType::Link => {
            let (w, h) = (r * 0.62, r * 0.82);
            let n = 12;
            let mut pts: Vec<(f32, f32)> = (0..=n).map(|i| {
                let f = i as f32 / n as f32;
                (cx - w + 2.0 * w * f, cy - h + (f * std::f32::consts::TAU).sin() * h * 0.12)
            }).collect();
            pts.extend((0..=n).rev().map(|i| {
                let f = i as f32 / n as f32;
                (cx - w + 2.0 * w * f, cy + h + (f * std::f32::consts::TAU).sin() * h * 0.12)
            }));
            pts
        }
        NodeType::Text => vec![(cx - r, cy - r * 0.4), (cx + r, cy - r * 0.4), (cx + r, cy + r * 0.4), (cx - r, cy + r * 0.4)],
    }
}

fn diagram_page(c: &mut Ctx) {
    let mut page = c.new_page(true);
    let (w, h) = (page.width, page.height);
    page.rect(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN, None, Some((1.5, Rgb::BLACK)));
    let nodes: Vec<&Node> = c.project.nodes.iter().collect();
    if nodes.is_empty() {
        c.doc.add(page);
        return;
    }
    let sym = c.project.diagram.symbol_size as f32;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for n in &nodes {
        let (x, y) = (n.pos[0] as f32, n.pos[1] as f32);
        x0 = x0.min(x - sym * 1.6);
        x1 = x1.max(x + sym * 1.6);
        y0 = y0.min(y - sym);
        y1 = y1.max(y + sym * 1.4);
    }
    let area = (MARGIN + 18.0, MARGIN + 18.0, w - MARGIN - 18.0, h - MARGIN - 80.0);
    let s = ((area.2 - area.0) / (x1 - x0)).min((area.3 - area.1) / (y1 - y0)).min(2.0);
    let ox = area.0 + ((area.2 - area.0) - (x1 - x0) * s) / 2.0;
    let oy = area.1 + ((area.3 - area.1) - (y1 - y0) * s) / 2.0;
    let tx = |x: f64| ox + (x as f32 - x0) * s;
    let ty = |y: f64| oy + (y as f32 - y0) * s;
    let d = sym * s;
    // Arrows first.
    for n in &nodes {
        for (k, t) in n.outflows.iter().enumerate() {
            let Some(target) = t.and_then(|id| c.project.node(id)) else { continue };
            let (ax, ay, bx, by) = (tx(n.pos[0]), ty(n.pos[1]), tx(target.pos[0]), ty(target.pos[1]));
            let len = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt().max(1e-3);
            let (ux, uy) = ((bx - ax) / len, (by - ay) / len);
            let (sx, sy) = (ax + ux * d * 0.5, ay + uy * d * 0.5);
            let (ex, ey) = (bx - ux * d * 0.55, by - uy * d * 0.55);
            let color = match k {
                0 => Rgb::from_u8(40, 40, 140),
                1 => Rgb::from_u8(214, 40, 40),
                _ => Rgb::from_u8(20, 140, 60),
            };
            if k > 0 {
                page.dashed(4.0, 3.0);
            }
            page.line(sx, sy, ex, ey, 0.9, color);
            page.solid();
            let a = d * 0.18;
            page.polygon(&[(ex, ey), (ex - ux * a - uy * a * 0.45, ey - uy * a + ux * a * 0.45), (ex - ux * a + uy * a * 0.45, ey - uy * a - ux * a * 0.45)], Some(Rgb::WHITE), Some((0.9, color)));
        }
    }
    for n in &nodes {
        let (cx, cy) = (tx(n.pos[0]), ty(n.pos[1]));
        if let NodeData::Text(t) = &n.data {
            page.text(cx, cy + 3.0, (t.size * s).clamp(5.0, 30.0), Font::Sans, Align::Center, Rgb::BLACK, &t.text);
            continue;
        }
        let (fill, stroke) = node_color(n.node_type());
        page.polygon(&symbol(n.node_type(), cx, cy, d), Some(fill), Some((1.0, stroke)));
        page.text(cx, cy + d * 0.12 + if n.node_type() == NodeType::Pond { d * 0.12 } else { 0.0 }, (d * 0.3).clamp(5.0, 16.0), Font::Sans, Align::Center, Rgb::BLACK, &n.label());
        if !n.name.is_empty() {
            page.text(cx, cy + d * 0.5 + 10.0, (d * 0.2).clamp(5.0, 10.0), Font::Sans, Align::Center, Rgb::BLACK, &n.name);
        }
    }
    // Legend and title block.
    let ly = h - MARGIN - 34.0;
    for (i, (t, label)) in [(NodeType::Subcat, "Subcat"), (NodeType::Reach, "Reach"), (NodeType::Pond, "Pond"), (NodeType::Link, "Link")].iter().enumerate() {
        let cx = MARGIN + 50.0 + i as f32 * 60.0;
        let (fill, stroke) = node_color(*t);
        page.polygon(&symbol(*t, cx, ly, 36.0), Some(fill), Some((0.8, stroke)));
        page.text(cx, ly + 3.0, 7.0, Font::Sans, Align::Center, Rgb::BLACK, label);
    }
    let bx = w - MARGIN - 300.0;
    page.rect(bx, h - MARGIN - 52.0, 300.0, 52.0, Some(Rgb::WHITE), Some((1.2, Rgb::BLACK)));
    page.text(bx + 150.0, h - MARGIN - 36.0, 10.0, Font::SansBold, Align::Center, Rgb::BLACK, &format!("Routing Diagram for {}", c.project.title));
    if !c.project.prepared_by.is_empty() {
        page.text(bx + 150.0, h - MARGIN - 23.0, 9.0, Font::Sans, Align::Center, Rgb::BLACK, &format!("Prepared by {}, Printed {}", c.project.prepared_by, c.opts.date));
    } else {
        page.text(bx + 150.0, h - MARGIN - 23.0, 9.0, Font::Sans, Align::Center, Rgb::BLACK, &format!("Printed {}", c.opts.date));
    }
    page.text(bx + 150.0, h - MARGIN - 11.0, 8.0, Font::Sans, Align::Center, Rgb::BLACK, &format!("Stormwaves {}", env!("CARGO_PKG_VERSION")));
    c.doc.add(page);
}

/// A 2-D hydrograph chart in a box (x, y, w, h).
fn hydro_chart(page: &mut Page, x: f32, y: f32, w: f32, h: f32, title: &str, series: &[(&str, &Hydrograph, Rgb)], summary: &[String], flow_label: &str, to_flow: &dyn Fn(f64) -> f64) {
    page.text(x + w / 2.0, y + 12.0, 11.0, Font::SansBold, Align::Center, Rgb::BLACK, title);
    page.text(x + w / 2.0, y + 25.0, 9.0, Font::SansBold, Align::Center, Rgb::from_u8(0, 0, 200), "Hydrograph");
    let plot = (x + 44.0, y + 34.0, x + w - 90.0, y + h - 30.0);
    let (mut t0, mut t1, mut qmax) = (f64::INFINITY, f64::NEG_INFINITY, 0.0f64);
    for (_, hg, _) in series {
        if hg.is_empty() {
            continue;
        }
        t0 = t0.min(hg.t0);
        t1 = t1.max(hg.end_time());
        qmax = qmax.max(hg.q.iter().map(|q| to_flow(*q)).fold(0.0, f64::max));
    }
    if !t0.is_finite() || t1 <= t0 {
        return;
    }
    let step = nice(qmax.max(1e-6) * 1.1 / 8.0);
    let ymax = ((qmax * 1.1) / step).ceil().max(1.0) * step;
    let sx = |t: f64| plot.0 + ((t - t0) / (t1 - t0)) as f32 * (plot.2 - plot.0);
    let sy = |q: f64| plot.3 - (q / ymax) as f32 * (plot.3 - plot.1);
    let grid = Rgb::from_u8(190, 190, 190);
    page.dashed(1.5, 1.5);
    let tstep = nice((t1 - t0) / 15.0);
    let mut t = (t0 / tstep).ceil() * tstep;
    while t <= t1 + 1e-9 {
        page.line(sx(t), plot.1, sx(t), plot.3, 0.4, grid);
        t += tstep;
    }
    let mut q = 0.0;
    while q <= ymax + 1e-9 {
        page.line(plot.0, sy(q), plot.2, sy(q), 0.4, grid);
        q += step;
    }
    page.solid();
    page.rect(plot.0, plot.1, plot.2 - plot.0, plot.3 - plot.1, None, Some((0.8, Rgb::BLACK)));
    let mut t = (t0 / tstep).ceil() * tstep;
    while t <= t1 + 1e-9 {
        page.text(sx(t), plot.3 + 10.0, 7.0, Font::Sans, Align::Center, Rgb::BLACK, &trim(t, tstep));
        t += tstep;
    }
    let mut q = 0.0;
    while q <= ymax + 1e-9 {
        page.text(plot.0 - 3.0, sy(q) + 2.5, 7.0, Font::Sans, Align::Right, Rgb::BLACK, &trim(q, step));
        q += step;
    }
    page.text((plot.0 + plot.2) / 2.0, plot.3 + 22.0, 8.5, Font::SansBold, Align::Center, Rgb::BLACK, "Time (hours)");
    page.text_vertical(plot.0 - 28.0, (plot.1 + plot.3) / 2.0, 8.5, Font::SansBold, Rgb::BLACK, &format!("Flow ({flow_label})"));
    for (name, hg, color) in series {
        let pts: Vec<(f32, f32)> = (0..hg.len()).map(|i| (sx(hg.time(i)), sy(to_flow(hg.q[i])))).collect();
        page.polyline(&pts, 1.3, *color);
        let p = hg.peak();
        if p.value > 0.0 {
            let label = format!("{:.2} {flow_label}", to_flow(p.value));
            let lw = crate::pdf::text_width(&label, Font::Sans, 7.0) + 6.0;
            let (lx, ly) = ((sx(p.time) + 4.0).min(plot.2 - lw), (sy(to_flow(p.value)) - 12.0).max(plot.1));
            page.rect(lx, ly, lw, 10.0, Some(Rgb::from_u8(255, 255, 220)), Some((0.5, *color)));
            page.text(lx + 3.0, ly + 7.5, 7.0, Font::Sans, Align::Left, Rgb::BLACK, &label);
        }
        let _ = name;
    }
    // Legend.
    let lx = plot.2 + 8.0;
    for (i, (name, _, color)) in series.iter().enumerate() {
        let ly = plot.1 + 4.0 + i as f32 * 12.0;
        page.rect(lx, ly, 8.0, 8.0, Some(*color), Some((0.5, Rgb::BLACK)));
        page.text(lx + 11.0, ly + 7.0, 7.5, Font::Sans, Align::Left, Rgb::BLACK, name);
    }
    for (i, line) in summary.iter().enumerate() {
        page.text(plot.2 - 4.0, plot.1 + 14.0 + i as f32 * 12.0, 9.5, Font::SansBold, Align::Right, Rgb::BLACK, line);
    }
}

fn nice(raw: f64) -> f64 {
    if !(raw > 0.0) {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    (if n < 1.5 { 1.0 } else if n < 3.5 { 2.0 } else if n < 7.5 { 5.0 } else { 10.0 }) * mag
}

fn trim(v: f64, step: f64) -> String {
    let dp = if step >= 1.0 { 0 } else { (-step.log10().floor()) as usize };
    format!("{:.*}", dp.min(3), v)
}

fn node_pages(c: &mut Ctx, node: &Node) {
    let mut page = c.new_page(false);
    let lines = report::node_summary(c.project, c.results, node);
    let mut y = c.text_block(&mut page, BODY_TOP, None, &lines, 7.6);
    if c.opts.graphs {
        if let Some(r) = c.results.node(node.id) {
            let mut series: Vec<(&str, &Hydrograph, Rgb)> = Vec::new();
            match node.node_type() {
                NodeType::Subcat => series.push(("Runoff", &r.outflow, Rgb::from_u8(20, 160, 200))),
                _ => {
                    series.push(("Inflow", &r.inflow, Rgb::from_u8(40, 170, 60)));
                    series.push(("Outflow", &r.outflow, Rgb::from_u8(120, 120, 120)));
                    let names = ["Primary", "Secondary", "Tertiary", "Discarded"];
                    let colors = [Rgb::from_u8(30, 110, 250), Rgb::from_u8(230, 50, 50), Rgb::from_u8(230, 150, 30), Rgb::from_u8(150, 60, 190)];
                    if r.routes.iter().filter(|h| h.has_flow()).count() > 1 {
                        for k in 0..4 {
                            if r.routes[k].has_flow() {
                                series.push((names[k], &r.routes[k], colors[k]));
                            }
                        }
                    }
                }
            }
            let u = &c.project.units;
            let mut summary = Vec::new();
            if let Detail::Subcat(d) = &r.detail {
                summary.push(format!("Runoff={}", u.fmt_flow(r.outflow.peak().value)));
                summary.push(format!("Area={}", u.fmt_area(d.composite.total_area)));
                summary.push(format!("Volume={}", u.fmt_volume(r.outflow.volume_cf())));
                summary.push(format!("Tc={:.1} min", d.tc_minutes));
                summary.push(format!("CN={}", d.composite.cn));
            } else {
                summary.push(format!("Inflow={}", u.fmt_flow(r.inflow.peak().value)));
                summary.push(format!("Outflow={}", u.fmt_flow(r.outflow.peak().value)));
                if let Detail::Pond(d) = &r.detail {
                    summary.push(format!("Peak Elev={}", u.fmt_elev(d.peak_elevation)));
                    summary.push(format!("Storage={}", u.fmt_volume(d.peak_storage)));
                }
            }
            let chart_h = 300.0;
            if y + chart_h > page.height - MARGIN {
                let full = std::mem::replace(&mut page, c.new_page(false));
                c.doc.add(full);
                y = BODY_TOP;
            }
            let units = c.project.units;
            let pw = page.width;
            hydro_chart(&mut page, MARGIN, y + 10.0, pw - 2.0 * MARGIN, chart_h, &node.title(), &series, &summary, units.flow_label(), &|q| units.flow(q));
        }
    }
    c.doc.add(page);
    if c.opts.tables {
        let (heads, cols) = report::hydrograph_table(c.project, c.results, node, true);
        let mut lines = vec![heads.iter().map(|h| format!("{:>14}", h.split(" (").next().unwrap_or(h))).collect::<String>()];
        let rows = cols.first().map(|c| c.len()).unwrap_or(0);
        for i in 0..rows {
            lines.push(cols.iter().map(|c| format!("{:>14.3}", c.get(i).copied().unwrap_or(0.0))).collect());
        }
        let mut page = c.new_page(false);
        c.text_block(&mut page, BODY_TOP, Some(&format!("Hydrograph for {}", node.title())), &lines, 7.6);
        c.doc.add(page);
    }
}

/// Builds the PDF report.
pub fn report_pdf(project: &Project, results: &Results, opts: &PrintOptions) -> Vec<u8> {
    let mut c = Ctx { project, results, opts, doc: Document::new(&project.title) };
    if opts.diagram {
        diagram_page(&mut c);
    }
    if opts.node_listing {
        let mut page = c.new_page(false);
        let lines = report::node_listing(project, results);
        let y = c.text_block(&mut page, BODY_TOP, Some("Node Listing"), &lines, 6.9);
        let _ = y;
        c.doc.add(page);
    }
    if opts.area_listing {
        let mut page = c.new_page(false);
        let mut lines = report::area_listing(project);
        lines.push(String::new());
        lines.extend(report::soil_listing(project));
        c.text_block(&mut page, BODY_TOP, Some("Area and Soil Listing"), &lines, 8.0);
        c.doc.add(page);
    }
    if opts.summaries {
        let mut nodes: Vec<&Node> = project.nodes.iter().filter(|n| n.node_type() != NodeType::Text).collect();
        nodes.sort_by_key(|n| (match n.node_type() {
            NodeType::Subcat => 0,
            NodeType::Reach => 1,
            NodeType::Pond => 2,
            _ => 3,
        }, n.number.parse::<u64>().unwrap_or(u64::MAX)));
        for n in nodes {
            node_pages(&mut c, n);
        }
    }
    c.doc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_has_pages() {
        let mut p = Project::default();
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let r = p.add_node(NodeType::Reach, [100.0, 0.0]);
        p.node_mut(s).unwrap().outflows[0] = Some(r);
        let results = crate::calc::calculate(&p);
        let opts = PrintOptions { tables: true, date: "1/2/2026".into(), ..Default::default() };
        let bytes = report_pdf(&p, &results, &opts);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Type /Page "));
        let pages = text.matches("/Type /Page ").count();
        assert!(pages >= 5, "{pages} pages");
        assert!(text.contains("Routing Diagram for"));
    }
}
