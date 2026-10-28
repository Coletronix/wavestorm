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
    /// Hydrologic soil group report: the soil map and the map unit and subcatchment tables.
    pub hsg_report: bool,
    /// Date string for the page headers (supplied by the caller).
    pub date: String,
    /// Print summaries for these nodes only (all nodes when empty).
    pub only: Vec<crate::project::NodeId>,
    /// The background raster to print behind the routing diagram (decoded by the caller).
    pub background: Option<PrintBackground>,
}

/// A decoded background for the printed diagram.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintBackground {
    /// Its rectangle in diagram units: x0, y0, x1, y1.
    pub rect: [f64; 4],
    pub width: u32,
    pub height: u32,
    /// 8-bit RGB, rows top to bottom (opacity already applied against white).
    pub rgb: std::sync::Arc<Vec<u8>>,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            diagram: true,
            node_listing: true,
            area_listing: true,
            summaries: true,
            graphs: true,
            tables: false,
            hsg_report: false,
            date: String::new(),
            only: Vec::new(),
            background: None,
        }
    }
}

/// Colours of the drawn sub-areas, in Area row order (as on the screen diagram).
const SUBAREA_COLORS: [(u8, u8, u8); 8] = [
    (245, 158, 11),
    (59, 130, 246),
    (168, 85, 247),
    (236, 72, 153),
    (20, 184, 166),
    (132, 204, 22),
    (249, 115, 22),
    (14, 165, 233),
];

/// Fill opacities of subcatchment outlines and drawn sub-areas (as on the screen diagram).
const OUTLINE_ALPHA: f32 = 34.0 / 255.0;
const SUBAREA_ALPHA: f32 = 80.0 / 255.0;
/// Text size of the sub-area curve-number labels, and the smallest they shrink to (so they stay
/// legible when zoomed in on the PDF).
const LABEL_SIZE: f32 = 8.0;
const MIN_LABEL_SIZE: f32 = 2.0;
/// Tc path legs without a flow type, and the point markers.
const TC_HINT: Rgb = Rgb(0.42, 0.42, 0.45);

fn subarea_color(i: usize) -> Rgb {
    let (r, g, b) = SUBAREA_COLORS[i % SUBAREA_COLORS.len()];
    Rgb::from_u8(r, g, b)
}

/// Text with a white halo, readable over a background image.
#[allow(clippy::too_many_arguments)]
fn halo_text(page: &mut Page, x: f32, y: f32, size: f32, font: Font, align: Align, color: Rgb, text: &str) {
    let r = (size * 0.1).max(0.3);
    for (dx, dy) in [(-r, 0.0), (r, 0.0), (0.0, -r), (0.0, r), (-r, -r), (r, -r), (-r, r), (r, r)] {
        page.text(x + dx, y + dy, size, font, align, Rgb::WHITE, text);
    }
    page.text(x, y, size, font, align, color, text);
}

/// The colour of a Tc flow type (the node colours, as on the screen diagram).
fn flow_color(f: crate::tc::PathFlow) -> Rgb {
    use crate::tc::PathFlow;
    match f {
        PathFlow::Sheet => node_color(NodeType::Subcat).1,
        PathFlow::Shallow => node_color(NodeType::Reach).1,
        PathFlow::Channel => node_color(NodeType::Pond).1,
        PathFlow::Pipe => Rgb::from_u8(140, 70, 210),
        PathFlow::Lake => Rgb::from_u8(0, 150, 160),
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
            page.text(
                MARGIN,
                57.0,
                9.5,
                Font::Sans,
                Align::Left,
                Rgb::BLACK,
                &format!("Prepared by {}", p.prepared_by),
            );
        }
        page.text(
            MARGIN,
            69.0,
            8.0,
            Font::Sans,
            Align::Left,
            Rgb::BLACK,
            &format!("WAVEStorm {}", env!("CARGO_PKG_VERSION")),
        );
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
                // Add the full page first so the new one is numbered after it.
                let full = std::mem::replace(page, Page::new(0.0, 0.0));
                self.doc.add(full);
                *page = self.new_page(false);
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
        NodeType::Subcat => (0..6)
            .map(|i| {
                let a = std::f32::consts::PI / 3.0 * i as f32;
                (cx + r * a.cos(), cy + r * 0.87 * a.sin())
            })
            .collect(),
        NodeType::Reach => {
            let h = r * 0.72;
            vec![(cx - h, cy - h), (cx + h, cy - h), (cx + h, cy + h), (cx - h, cy + h)]
        }
        NodeType::Pond => vec![(cx, cy - r * 0.95), (cx + r, cy + r * 0.72), (cx - r, cy + r * 0.72)],
        NodeType::Link => {
            let (w, h) = (r * 0.62, r * 0.82);
            let n = 12;
            let mut pts: Vec<(f32, f32)> = (0..=n)
                .map(|i| {
                    let f = i as f32 / n as f32;
                    (cx - w + 2.0 * w * f, cy - h + (f * std::f32::consts::TAU).sin() * h * 0.12)
                })
                .collect();
            pts.extend((0..=n).rev().map(|i| {
                let f = i as f32 / n as f32;
                (cx - w + 2.0 * w * f, cy + h + (f * std::f32::consts::TAU).sin() * h * 0.12)
            }));
            pts
        }
        NodeType::Text => {
            vec![(cx - r, cy - r * 0.4), (cx + r, cy - r * 0.4), (cx + r, cy + r * 0.4), (cx - r, cy + r * 0.4)]
        }
    }
}

fn diagram_page(c: &mut Ctx) {
    let page = diagram_draw(c);
    c.doc.add(page);
}

fn diagram_draw(c: &mut Ctx) -> Page {
    let mut page = c.new_page(true);
    let (w, h) = (page.width, page.height);
    page.rect(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN, None, Some((1.5, Rgb::BLACK)));
    let nodes: Vec<&Node> = c.project.nodes.iter().collect();
    if nodes.is_empty() {
        return page;
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
    for p in nodes.iter().flat_map(|n| n.outline.iter().chain(n.tc_path.iter())).flatten() {
        x0 = x0.min(p[0] as f32);
        x1 = x1.max(p[0] as f32);
        y0 = y0.min(p[1] as f32);
        y1 = y1.max(p[1] as f32);
    }
    let background = c
        .opts
        .background
        .as_ref()
        .filter(|b| b.rect.iter().all(|v| v.is_finite()) && b.rect[2] > b.rect[0] && b.rect[3] > b.rect[1]);
    if let Some(b) = background {
        x0 = x0.min(b.rect[0] as f32);
        y0 = y0.min(b.rect[1] as f32);
        x1 = x1.max(b.rect[2] as f32);
        y1 = y1.max(b.rect[3] as f32);
    }
    let area = (MARGIN + 18.0, MARGIN + 18.0, w - MARGIN - 18.0, h - MARGIN - 80.0);
    let s = ((area.2 - area.0) / (x1 - x0)).min((area.3 - area.1) / (y1 - y0)).min(2.0);
    let ox = area.0 + ((area.2 - area.0) - (x1 - x0) * s) / 2.0;
    let oy = area.1 + ((area.3 - area.1) - (y1 - y0) * s) / 2.0;
    let tx = |x: f64| ox + (x as f32 - x0) * s;
    let ty = |y: f64| oy + (y as f32 - y0) * s;
    let d = sym * s;
    if let Some(b) = background {
        let (bx, by) = (tx(b.rect[0]), ty(b.rect[1]));
        page.image(bx, by, tx(b.rect[2]) - bx, ty(b.rect[3]) - by, b.width, b.height, b.rgb.clone());
    }
    // Subcatchment outlines and their drawn sub-areas, filled translucently as on the screen.
    let obstacles = crate::drawing::label_obstacles(c.project);
    let soft: Vec<&[[f64; 2]]> = obstacles.iter().map(Vec::as_slice).collect();
    let mut labels: Vec<(f32, f32, f32, String)> = Vec::new();
    let pts_of = |o: &[[f64; 2]]| -> Vec<(f32, f32)> { o.iter().map(|q| (tx(q[0]), ty(q[1]))).collect() };
    for n in &nodes {
        let Some(o) = n.outline.as_ref().filter(|o| o.len() >= 3) else { continue };
        // Subcatchment outlines and estimated pond footprints, each in its node colour.
        let edge = node_color(n.node_type()).1;
        let outline = pts_of(o);
        page.fill_alpha(OUTLINE_ALPHA);
        page.polygon(&outline, Some(edge), None);
        let NodeData::Subcat(sc) = &n.data else {
            page.fill_alpha(1.0);
            page.polygon(&outline, None, Some((1.2, edge)));
            continue;
        };
        page.fill_alpha(SUBAREA_ALPHA);
        for (i, area) in sc.areas.iter().enumerate() {
            let Some(so) = area.outline.as_ref().filter(|o| o.len() >= 3) else { continue };
            // Rows split by soil group share one drawing.
            if crate::drawing::shape_row(sc, i) != i {
                continue;
            }
            let col = subarea_color(i);
            page.polygon(&pts_of(so), Some(col), Some((1.2, col)));
        }
        page.fill_alpha(1.0);
        page.polygon(&outline, None, Some((1.2, edge)));
        // Curve-number labels: each drawn sub-area, and the remainder (placed clear of them),
        // away from node symbols and Tc paths where they fit. Drawn after the Tc paths.
        let drawn: Vec<&[[f64; 2]]> =
            sc.areas.iter().filter_map(|a| a.outline.as_deref()).filter(|o| o.len() >= 3).collect();
        for (shape, cn) in crate::drawing::cn_regions(sc) {
            let (region, hard) = match shape {
                Some(so) if so.len() >= 3 => (so, &[][..]),
                Some(_) => continue,
                None => (o.as_slice(), drawn.as_slice()),
            };
            // Every label is printed: at full size where it fits, else shrunk to its spot.
            let label = format!("CN {}", crate::units::group(cn, 0));
            let unit_width = crate::pdf::text_width(&label, Font::SansBold, 1.0);
            let fitting_size = |clear: f64| {
                let clear = clear as f32 * s;
                (clear / 0.7).min(clear / (unit_width * 0.45))
            };
            let (at, clear) = crate::drawing::place_label(region, hard, &soft, |c| fitting_size(c) >= LABEL_SIZE);
            let size = fitting_size(clear).clamp(MIN_LABEL_SIZE, LABEL_SIZE);
            labels.push((tx(at[0]), ty(at[1]) + size * 0.35, size, label));
        }
    }
    // Tc flow paths: dashed legs coloured by flow type, lettered segment ends, an arrowhead at
    // the outlet.
    for n in &nodes {
        let Some(path) = n.tc_path.as_ref().filter(|p| p.len() >= 2) else { continue };
        let pts = pts_of(path);
        let (flows, letters) = (crate::drawing::tc_leg_flows(n), crate::drawing::tc_path_labels(n));
        let leg_color = |i: usize| flows.get(i).copied().flatten().map_or(TC_HINT, flow_color);
        // A pale casing under the dashes keeps them visible over dark backgrounds.
        page.polyline(&pts, 2.8, Rgb(0.97, 0.97, 0.97));
        page.dashed(4.0, 2.5);
        for (i, w) in pts.windows(2).enumerate() {
            page.line(w[0].0, w[0].1, w[1].0, w[1].1, 1.6, leg_color(i));
        }
        page.solid();
        if let [.., a, b] = pts.as_slice() {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dy);
            if len > 0.5 {
                let (ux, uy, k) = (dx / len, dy / len, 6.0);
                let c = leg_color(pts.len() - 2);
                page.polygon(
                    &[
                        *b,
                        (b.0 - ux * k - uy * k * 0.45, b.1 - uy * k + ux * k * 0.45),
                        (b.0 - ux * k + uy * k * 0.45, b.1 - uy * k - ux * k * 0.45),
                    ],
                    Some(Rgb::WHITE),
                    Some((0.9, c)),
                );
            }
        }
        for (i, q) in pts.iter().enumerate() {
            match letters.get(i).cloned().flatten() {
                Some(letter) => {
                    page.circle(q.0, q.1, 2.2, Some(Rgb::WHITE), Some((0.9, TC_HINT)));
                    halo_text(
                        &mut page,
                        q.0 + 3.0,
                        q.1 - 3.5,
                        8.0,
                        Font::SansBold,
                        Align::Left,
                        Rgb::from_u8(40, 40, 48),
                        &letter,
                    );
                }
                None => page.circle(q.0, q.1, 1.1, Some(TC_HINT), None),
            }
        }
    }
    for (x, y, size, label) in &labels {
        halo_text(&mut page, *x, *y, *size, Font::SansBold, Align::Center, Rgb::from_u8(20, 20, 20), label);
    }
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
            page.polygon(
                &[
                    (ex, ey),
                    (ex - ux * a - uy * a * 0.45, ey - uy * a + ux * a * 0.45),
                    (ex - ux * a + uy * a * 0.45, ey - uy * a - ux * a * 0.45),
                ],
                Some(Rgb::WHITE),
                Some((0.9, color)),
            );
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
        page.text(
            cx,
            cy + d * 0.12 + if n.node_type() == NodeType::Pond { d * 0.12 } else { 0.0 },
            (d * 0.3).clamp(5.0, 16.0),
            Font::Sans,
            Align::Center,
            Rgb::BLACK,
            &n.label(),
        );
        if !n.name.is_empty() {
            page.text(
                cx,
                cy + d * 0.5 + 10.0,
                (d * 0.2).clamp(5.0, 10.0),
                Font::Sans,
                Align::Center,
                Rgb::BLACK,
                &n.name,
            );
        }
    }
    // Legend and title block.
    let ly = h - MARGIN - 34.0;
    for (i, (t, label)) in
        [(NodeType::Subcat, "Subcat"), (NodeType::Reach, "Reach"), (NodeType::Pond, "Pond"), (NodeType::Link, "Link")]
            .iter()
            .enumerate()
    {
        let cx = MARGIN + 50.0 + i as f32 * 60.0;
        let (fill, stroke) = node_color(*t);
        page.polygon(&symbol(*t, cx, ly, 36.0), Some(fill), Some((0.8, stroke)));
        page.text(cx, ly + 3.0, 7.0, Font::Sans, Align::Center, Rgb::BLACK, label);
    }
    let bx = w - MARGIN - 300.0;
    page.rect(bx, h - MARGIN - 52.0, 300.0, 52.0, Some(Rgb::WHITE), Some((1.2, Rgb::BLACK)));
    page.text(
        bx + 150.0,
        h - MARGIN - 36.0,
        10.0,
        Font::SansBold,
        Align::Center,
        Rgb::BLACK,
        &format!("Routing Diagram for {}", c.project.title),
    );
    if !c.project.prepared_by.is_empty() {
        page.text(
            bx + 150.0,
            h - MARGIN - 23.0,
            9.0,
            Font::Sans,
            Align::Center,
            Rgb::BLACK,
            &format!("Prepared by {}, Printed {}", c.project.prepared_by, c.opts.date),
        );
    } else {
        page.text(
            bx + 150.0,
            h - MARGIN - 23.0,
            9.0,
            Font::Sans,
            Align::Center,
            Rgb::BLACK,
            &format!("Printed {}", c.opts.date),
        );
    }
    page.text(
        bx + 150.0,
        h - MARGIN - 11.0,
        8.0,
        Font::Sans,
        Align::Center,
        Rgb::BLACK,
        &format!("WAVEStorm {}", env!("CARGO_PKG_VERSION")),
    );
    page
}

/// The routing diagram page as an EMF vector graphic (landscape letter, with title block).
pub fn diagram_emf(project: &Project, results: &Results, date: &str, background: Option<PrintBackground>) -> Vec<u8> {
    let opts = PrintOptions { date: date.to_string(), background, ..Default::default() };
    let mut c = Ctx { project, results, opts: &opts, doc: Document::new(&project.title) };
    crate::emf::page_to_emf(&diagram_draw(&mut c))
}

/// A node's hydrograph chart as an EMF vector graphic (7.5 x 4.5 in), or `None` when the node
/// has no results.
pub fn node_graph_emf(project: &Project, results: &Results, node_id: crate::project::NodeId) -> Option<Vec<u8>> {
    let node = project.node(node_id)?;
    results.node(node_id)?;
    let mut page = Page::new(540.0, 324.0);
    page.rect(0.0, 0.0, 540.0, 324.0, Some(Rgb::WHITE), None);
    node_chart(&mut page, project, results, node, 0.0, 0.0, 540.0, 324.0);
    Some(crate::emf::page_to_emf(&page))
}

/// A node's summary listing as EMF vector text (monospace, letter width).
pub fn node_summary_emf(project: &Project, results: &Results, node_id: crate::project::NodeId) -> Option<Vec<u8>> {
    let node = project.node(node_id)?;
    let lines = report::node_summary(project, results, node);
    let size = 8.0;
    let lh = size * 1.22;
    let mut page = Page::new(540.0, (lines.len() as f32 * lh + 2.0 * 8.0).max(40.0));
    for (i, l) in lines.iter().enumerate() {
        page.text(8.0, 8.0 + size + i as f32 * lh, size, Font::Mono, Align::Left, Rgb::BLACK, l);
    }
    Some(crate::emf::page_to_emf(&page))
}

/// A 2-D hydrograph chart in a box (x, y, w, h).
fn hydro_chart(
    page: &mut Page,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    title: &str,
    series: &[(&str, &Hydrograph, Rgb)],
    summary: &[String],
    flow_label: &str,
    to_flow: &dyn Fn(f64) -> f64,
) {
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
    page.text_vertical(
        plot.0 - 28.0,
        (plot.1 + plot.3) / 2.0,
        8.5,
        Font::SansBold,
        Rgb::BLACK,
        &format!("Flow ({flow_label})"),
    );
    for (_, hg, color) in series {
        let pts: Vec<(f32, f32)> = (0..hg.len()).map(|i| (sx(hg.time(i)), sy(to_flow(hg.q[i])))).collect();
        page.polyline(&pts, 1.3, *color);
    }
    // Peak labels, each placed clear of the summary lines and the labels already placed.
    let mut taken: Vec<(f32, f32, f32, f32)> = summary
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let w = crate::pdf::text_width(line, Font::SansBold, 9.5);
            let base = plot.1 + 14.0 + i as f32 * 12.0;
            (plot.2 - 4.0 - w, base - 9.5, w, 12.0)
        })
        .collect();
    for (_, hg, color) in series {
        let p = hg.peak();
        if p.value.is_nan() || p.value <= 0.0 {
            continue;
        }
        let label = format!("{:.2} {flow_label}", to_flow(p.value));
        let (lw, lh) = (crate::pdf::text_width(&label, Font::Sans, 7.0) + 6.0, 10.0);
        let (px, py) = (sx(p.time), sy(to_flow(p.value)));
        let fit =
            |x: f32, y: f32| (x.clamp(plot.0, (plot.2 - lw).max(plot.0)), y.clamp(plot.1, (plot.3 - lh).max(plot.1)));
        let overlaps = |r: (f32, f32)| {
            taken.iter().any(|t| r.0 < t.0 + t.2 && t.0 < r.0 + lw && r.1 < t.1 + t.3 && t.1 < r.1 + lh)
        };
        // Above-right of the peak, else above-left, then stepping further down the plot.
        let candidates = (0..12).flat_map(|k| {
            let y = py - 12.0 + k as f32 * (lh + 1.0);
            [fit(px + 4.0, y), fit(px - 4.0 - lw, y)]
        });
        let (lx, ly) = candidates.into_iter().find(|r| !overlaps(*r)).unwrap_or(fit(px + 4.0, py - 12.0));
        taken.push((lx, ly, lw, lh));
        page.rect(lx, ly, lw, lh, Some(Rgb::from_u8(255, 255, 220)), Some((0.5, *color)));
        page.text(lx + 3.0, ly + 7.5, 7.0, Font::Sans, Align::Left, Rgb::BLACK, &label);
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

// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn nice(raw: f64) -> f64 {
    if !(raw > 0.0) {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    (if n < 1.5 {
        1.0
    } else if n < 3.5 {
        2.0
    } else if n < 7.5 {
        5.0
    } else {
        10.0
    }) * mag
}

fn trim(v: f64, step: f64) -> String {
    let dp = if step >= 1.0 { 0 } else { (-step.log10().floor()) as usize };
    format!("{:.*}", dp.min(3), v)
}

/// Draws a node's hydrograph chart (series and summary legend) into the box (x, y, w, h).
#[allow(clippy::too_many_arguments)]
fn node_chart(page: &mut Page, project: &Project, results: &Results, node: &Node, x: f32, y: f32, w: f32, h: f32) {
    let Some(r) = results.node(node.id) else { return };
    let mut series: Vec<(&str, &Hydrograph, Rgb)> = Vec::new();
    match node.node_type() {
        NodeType::Subcat => series.push(("Runoff", &r.outflow, Rgb::from_u8(20, 160, 200))),
        _ => {
            series.push(("Inflow", &r.inflow, Rgb::from_u8(40, 170, 60)));
            series.push(("Outflow", &r.outflow, Rgb::from_u8(120, 120, 120)));
            let names = ["Primary", "Secondary", "Tertiary", "Discarded"];
            let colors = [
                Rgb::from_u8(30, 110, 250),
                Rgb::from_u8(230, 50, 50),
                Rgb::from_u8(230, 150, 30),
                Rgb::from_u8(150, 60, 190),
            ];
            if r.routes.iter().filter(|h| h.has_flow()).count() > 1 {
                for k in 0..4 {
                    if r.routes[k].has_flow() {
                        series.push((names[k], &r.routes[k], colors[k]));
                    }
                }
            }
        }
    }
    let u = &node.units(&project.units);
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
    let units = project.units;
    hydro_chart(page, x, y, w, h, &node.title(), &series, &summary, units.flow_label(), &|q| units.flow(q));
}

fn node_pages(c: &mut Ctx, node: &Node) {
    let mut page = c.new_page(false);
    let lines = report::node_summary(c.project, c.results, node);
    let mut y = c.text_block(&mut page, BODY_TOP, None, &lines, 7.6);
    if c.opts.graphs && c.results.node(node.id).is_some() {
        let chart_h = 300.0;
        if y + chart_h > page.height - MARGIN {
            let full = std::mem::replace(&mut page, Page::new(0.0, 0.0));
            c.doc.add(full);
            page = c.new_page(false);
            y = BODY_TOP;
        }
        let pw = page.width;
        node_chart(&mut page, c.project, c.results, node, MARGIN, y + 10.0, pw - 2.0 * MARGIN, chart_h);
    }
    c.doc.add(page);
    if c.opts.tables {
        let (heads, cols) = report::hydrograph_table(c.project, c.results, node, true, false);
        let mut lines =
            vec![heads.iter().map(|h| format!("{:>14}", h.split(" (").next().unwrap_or(h))).collect::<String>()];
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
    if opts.hsg_report {
        soil_pages(&mut c);
    }
    if opts.summaries {
        let mut nodes: Vec<&Node> = project
            .nodes
            .iter()
            .filter(|n| n.node_type() != NodeType::Text && (opts.only.is_empty() || opts.only.contains(&n.id)))
            .collect();
        nodes.sort_by_key(|n| {
            (
                match n.node_type() {
                    NodeType::Subcat => 0,
                    NodeType::Reach => 1,
                    NodeType::Pond => 2,
                    _ => 3,
                },
                n.number.parse::<u64>().unwrap_or(u64::MAX),
            )
        });
        for n in nodes {
            node_pages(&mut c, n);
        }
    }
    c.doc.finish()
}

/// Hydrologic Soil Group report: a soil map page (map units coloured by group and labelled,
/// subcatchment outlines, the area of interest, legend, north arrow and scale bar) when the
/// background is georeferenced, then the tables of `soils::report_lines`.
fn soil_pages(c: &mut Ctx) {
    let project = c.project;
    if let (Some(map), Some(soils)) = (&project.soils, crate::soils::project_soils(project)) {
        let mut page = c.new_page(false);
        let (w, h) = (page.width, page.height);
        page.text(w / 2.0, BODY_TOP + 2.0, 11.0, Font::SansBold, Align::Center, Rgb::BLACK, "Hydrologic Soil Groups");
        let to = project.diagram.underlay.as_ref().and_then(crate::geo::projector);
        let aoi: Vec<[f64; 2]> = match &to {
            Some(f) => map.aoi.iter().map(|q| f(*q)).collect(),
            None => Vec::new(),
        };
        let outlines: Vec<(&Node, &Vec<[f64; 2]>)> = project
            .nodes
            .iter()
            .filter(|n| n.node_type() == NodeType::Subcat)
            .filter_map(|n| n.outline.as_ref().filter(|o| o.len() >= 3).map(|o| (n, o)))
            .collect();
        let all: Vec<[f64; 2]> = aoi.iter().chain(outlines.iter().flat_map(|(_, o)| o.iter())).copied().collect();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &all {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let area = (MARGIN + 10.0, BODY_TOP + 24.0, w - MARGIN - 10.0, h - MARGIN - 70.0);
        if x1 > x0 && y1 > y0 {
            let s = ((area.2 - area.0) as f64 / (x1 - x0)).min((area.3 - area.1) as f64 / (y1 - y0));
            let ox = area.0 as f64 + ((area.2 - area.0) as f64 - (x1 - x0) * s) / 2.0;
            let oy = area.1 as f64 + ((area.3 - area.1) as f64 - (y1 - y0) * s) / 2.0;
            let tp = |q: [f64; 2]| (((q[0] - x0) * s + ox) as f32, ((q[1] - y0) * s + oy) as f32);
            let pts = |r: &[[f64; 2]]| r.iter().map(|q| tp(*q)).collect::<Vec<_>>();
            page.rect(area.0, area.1, area.2 - area.0, area.3 - area.1, None, Some((0.8, Rgb::BLACK)));
            // Map units, clipped to the AOI already; only those near the drawing are shown.
            page.fill_alpha(0.45);
            for soil in &soils {
                let Some(outer) = soil.rings.first() else { continue };
                let (r, g, b) = crate::soils::group_rgb(soil.group);
                page.polygon(&pts(outer), Some(Rgb::from_u8(r, g, b)), None);
            }
            page.fill_alpha(1.0);
            for soil in &soils {
                let (r, g, b) = crate::soils::group_rgb(soil.group);
                for ring in &soil.rings {
                    page.polygon(&pts(ring), None, Some((0.6, Rgb::from_u8(r, g, b))));
                }
            }
            for (n, o) in &outlines {
                page.polygon(&pts(o), None, Some((1.2, Rgb::BLACK)));
                let (cx, cy) = tp(crate::drawing::centroid(o));
                halo_text(&mut page, cx, cy + 3.0, 9.0, Font::SansBold, Align::Center, Rgb::BLACK, &n.label());
            }
            if aoi.len() >= 3 {
                page.dashed(5.0, 3.0);
                page.polygon(&pts(&aoi), None, Some((1.4, Rgb::from_u8(200, 40, 160))));
                page.solid();
            }
            for soil in &soils {
                let Some(outer) = soil.rings.first() else { continue };
                let holes: Vec<&[[f64; 2]]> = soil.rings.iter().skip(1).map(Vec::as_slice).collect();
                if let Some((at, clear)) = crate::drawing::label_spot(outer, &holes)
                    && clear * s > 8.0
                {
                    let symbol = map.unit(&soil.mukey).map(|u| u.symbol.as_str()).unwrap_or_default();
                    let text = format!("{symbol} ({})", soil.group.map_or('-', |g| g.letter()));
                    let (x, y) = tp(at);
                    halo_text(&mut page, x, y + 2.5, 7.0, Font::Sans, Align::Center, Rgb::BLACK, &text);
                }
            }
            // North arrow: the direction of north on the diagram.
            if let (Some(u), Some(f)) = (project.diagram.underlay.as_ref(), &to)
                && let Some(c0) = crate::geo::to_lon_lat(u, [(x0 + x1) / 2.0, (y0 + y1) / 2.0])
            {
                let a = f(c0);
                let b = f(crate::geo::offset_lon_lat(c0, [0.0, 100.0]));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len > 0.0 {
                    let (ux, uy) = ((dx / len) as f32, (dy / len) as f32);
                    let (cx, cy) = (area.2 - 22.0, area.1 + 26.0);
                    let tip = (cx + ux * 14.0, cy + uy * 14.0);
                    let tail = (cx - ux * 14.0, cy - uy * 14.0);
                    page.line(tail.0, tail.1, tip.0, tip.1, 1.2, Rgb::BLACK);
                    let side = (-uy * 4.0, ux * 4.0);
                    let back = (tip.0 - ux * 7.0, tip.1 - uy * 7.0);
                    page.polygon(
                        &[tip, (back.0 + side.0, back.1 + side.1), (back.0 - side.0, back.1 - side.1)],
                        Some(Rgb::BLACK),
                        None,
                    );
                    page.text(
                        tip.0 + ux * 7.0,
                        tip.1 + uy * 7.0 + 3.0,
                        9.0,
                        Font::SansBold,
                        Align::Center,
                        Rgb::BLACK,
                        "N",
                    );
                }
            }
            // Scale bar.
            let fpu = crate::drawing::feet_per_unit(project);
            let ft_per_pt = fpu / s;
            let target = ft_per_pt * 120.0;
            let step = nice(target);
            let bar = (step / ft_per_pt) as f32;
            let (bx, by) = (area.0 + 12.0, area.3 - 14.0);
            page.rect(bx - 4.0, by - 12.0, bar + 8.0 + 60.0, 20.0, Some(Rgb::WHITE), None);
            page.line(bx, by, bx + bar, by, 2.0, Rgb::BLACK);
            page.line(bx, by - 4.0, bx, by + 2.0, 1.0, Rgb::BLACK);
            page.line(bx + bar, by - 4.0, bx + bar, by + 2.0, 1.0, Rgb::BLACK);
            let u = &project.units;
            page.text(
                bx + bar + 6.0,
                by + 3.0,
                8.0,
                Font::Sans,
                Align::Left,
                Rgb::BLACK,
                &format!("{} {}", trim(u.len(step), u.len(step) / 10.0), u.len_label()),
            );
        }
        // Legend.
        let ly = h - MARGIN - 52.0;
        let mut lx = MARGIN + 10.0;
        let groups: [(Option<crate::cn::Hsg>, &str); 5] = [
            (Some(crate::cn::Hsg::A), "HSG A"),
            (Some(crate::cn::Hsg::B), "HSG B"),
            (Some(crate::cn::Hsg::C), "HSG C"),
            (Some(crate::cn::Hsg::D), "HSG D"),
            (None, "Not rated"),
        ];
        for (g, name) in groups {
            let (r, gg, b) = crate::soils::group_rgb(g);
            page.fill_alpha(0.45);
            page.rect(lx, ly, 14.0, 10.0, Some(Rgb::from_u8(r, gg, b)), None);
            page.fill_alpha(1.0);
            page.rect(lx, ly, 14.0, 10.0, None, Some((0.6, Rgb::from_u8(r, gg, b))));
            page.text(lx + 18.0, ly + 8.5, 8.5, Font::Sans, Align::Left, Rgb::BLACK, name);
            lx += 80.0;
        }
        page.dashed(5.0, 3.0);
        page.line(lx, ly + 5.0, lx + 18.0, ly + 5.0, 1.4, Rgb::from_u8(200, 40, 160));
        page.solid();
        page.text(lx + 22.0, ly + 8.5, 8.5, Font::Sans, Align::Left, Rgb::BLACK, "Area of interest");
        let note = format!(
            "Soils: USDA NRCS SSURGO via Soil Data Access, retrieved {}. {}",
            map.retrieved,
            if map.drained { "Dual groups as drained (first letter)." } else { "Dual groups as undrained (D)." }
        );
        page.text(MARGIN + 10.0, ly + 26.0, 7.5, Font::Sans, Align::Left, Rgb::BLACK, &note);
        c.doc.add(page);
    }
    let mut page = c.new_page(false);
    let lines = crate::soils::report_lines(project);
    c.text_block(&mut page, BODY_TOP, Some("Hydrologic Soil Group Report"), &lines, 7.6);
    c.doc.add(page);
}

/// A PDF of monospace text lines under the usual page header (Project Reports listings).
pub fn lines_pdf(project: &Project, results: &Results, title: &str, lines: &[String], date: &str) -> Vec<u8> {
    let opts = PrintOptions { date: date.to_string(), ..Default::default() };
    let mut c = Ctx { project, results, opts: &opts, doc: Document::new(&project.title) };
    let mut page = c.new_page(false);
    // Shrink wide listings to fit the page width.
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(1) as f32;
    let size = ((LETTER.0 - 2.0 * MARGIN) / (widest * 0.6)).clamp(5.0, 9.0);
    c.text_block(&mut page, BODY_TOP, Some(title), lines, size);
    c.doc.add(page);
    c.doc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emf_exports_for_nodes_and_diagram() {
        let mut p = Project::default();
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let r = p.add_node(NodeType::Reach, [100.0, 0.0]);
        p.node_mut(s).unwrap().outflows[0] = Some(r);
        let results = crate::calc::calculate(&p);
        for id in [s, r] {
            let g = node_graph_emf(&p, &results, id).unwrap();
            assert_eq!(&g[40..44], b" EMF");
            assert_eq!(u32::from_le_bytes([g[48], g[49], g[50], g[51]]) as usize, g.len());
            let t = node_summary_emf(&p, &results, id).unwrap();
            assert_eq!(&t[40..44], b" EMF");
        }
        let d = diagram_emf(&p, &results, "1/2/2026", None);
        assert_eq!(&d[40..44], b" EMF");
        assert!(node_graph_emf(&p, &results, 9999).is_none());
    }

    #[test]
    fn diagram_prints_outlines_sub_areas_and_background() {
        let mut p = Project::default();
        let s = p.add_node(NodeType::Subcat, [50.0, 50.0]);
        {
            let n = p.node_mut(s).unwrap();
            n.outline = Some(vec![[0.0, 0.0], [300.0, 0.0], [300.0, 300.0], [0.0, 300.0]]);
            n.tc_path = Some(vec![[20.0, 20.0], [150.0, 250.0], [280.0, 280.0]]);
            if let NodeData::Subcat(sc) = &mut n.data {
                sc.areas.clear();
                let mut a = crate::cn::SubArea::new(1000.0, 80.0, "Brush");
                a.outline = Some(vec![[10.0, 10.0], [140.0, 10.0], [140.0, 140.0], [10.0, 140.0]]);
                sc.areas.push(a);
                // Far too small for a full-size label: printed shrunk.
                let mut tiny = crate::cn::SubArea::new(10.0, 55.0, "Pad");
                tiny.outline = Some(vec![[200.0, 200.0], [203.0, 200.0], [203.0, 203.0], [200.0, 203.0]]);
                sc.areas.push(tiny);
                let mut rest = crate::cn::SubArea::new(1000.0, 61.0, "Open");
                rest.remainder = true;
                sc.areas.push(rest);
            }
        }
        let results = crate::calc::calculate(&p);
        let bg = PrintBackground {
            rect: [-20.0, -20.0, 120.0, 120.0],
            width: 2,
            height: 2,
            rgb: std::sync::Arc::new(vec![200; 12]),
        };
        let with = PrintOptions {
            diagram: true,
            node_listing: false,
            area_listing: false,
            summaries: false,
            background: Some(bg.clone()),
            ..Default::default()
        };
        let pdf = report_pdf(&p, &results, &with);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Subtype /Image") && text.contains("/XObject") && text.contains("Do Q"));
        assert!(
            text.contains("CN 80") && text.contains("CN 61") && text.contains("CN 55"),
            "sub-area, remainder and tiny sub-area labels"
        );
        assert!(text.contains("/ExtGState") && text.contains("/ca 0.31"), "translucent fills");
        assert!(text.contains("(A) Tj") && text.contains("(C) Tj"), "Tc path letters");
        let without = report_pdf(&p, &results, &PrintOptions { background: None, ..with.clone() });
        assert!(!String::from_utf8_lossy(&without).contains("/Subtype /Image"));
        let mut c = Ctx { project: &p, results: &results, opts: &with, doc: Document::new("t") };
        let emf = crate::emf::page_to_emf(&diagram_draw(&mut c));
        assert_eq!(&emf[40..44], b" EMF");
        assert!(emf.len() > 100);
    }

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
