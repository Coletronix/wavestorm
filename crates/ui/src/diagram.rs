//! The routing diagram: nodes, routing arrows, the background underlay and drawing tools.

use crate::doc::{Doc, View};
use crate::icons;
use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use wavestorm_core::project::{NodeData, NodeId, NodeType, Project};

/// The active diagram tool.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Tool {
    #[default]
    Select,
    /// Pick two points on the background, then enter the real distance between them.
    Calibrate { points: Vec<Pos2> },
    /// Click the corners of a subcatchment outline; close to create the subcatchment.
    Trace { points: Vec<Pos2> },
    /// Measure a polyline length (e.g. a Tc flow path).
    Measure { points: Vec<Pos2> },
    /// Drag the background image.
    MoveUnderlay,
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Calibrate { .. } => "Calibrate",
            Tool::Trace { .. } => "Trace Subcatchment",
            Tool::Measure { .. } => "Measure",
            Tool::MoveUnderlay => "Move Background",
        }
    }
}

/// Transient interaction state (not saved).
#[derive(Default)]
pub struct Interaction {
    pub drag: Option<Drag>,
    pub rubber: Option<(Pos2, Pos2)>,
    pub hover_world: Option<Pos2>,
    pub hover_node: Option<NodeId>,
}

pub enum Drag {
    /// Moving the selection; `before` restores for undo.
    Nodes { before: Project, last: Pos2, cloned: bool },
    /// Dragging the head of node `from`'s outflow `k`.
    Reroute { from: NodeId, k: usize, at: Pos2 },
    Pan,
    Underlay { last: Pos2, before: Project },
}

/// Requests from the canvas to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasAction {
    OpenReport(NodeId),
    OpenEdit(NodeId),
    Delete,
    ContextNode(NodeId),
    /// Finished calibration: two world points.
    Calibrated(Pos2, Pos2),
    /// Finished trace: outline in world coordinates.
    Traced(Vec<Pos2>),
    Measured(Vec<Pos2>),
    Convert(NodeId, NodeType),
    ClearOutflows(NodeId),
}

pub fn to_screen(view: &View, canvas: Rect, w: Pos2) -> Pos2 {
    canvas.min + view.offset + w.to_vec2() * view.zoom
}

pub fn to_world(view: &View, canvas: Rect, s: Pos2) -> Pos2 {
    ((s - canvas.min - view.offset) / view.zoom).to_pos2()
}

fn wp(p: [f64; 2]) -> Pos2 {
    pos2(p[0] as f32, p[1] as f32)
}

/// Point where the segment from the symbol centre `c` towards `toward` leaves the symbol.
fn boundary_point(t: NodeType, c: Pos2, d: f32, toward: Pos2) -> Pos2 {
    let pts = icons::node_outline(t, c, d);
    let dir = toward - c;
    if dir.length_sq() < 1e-6 {
        return c;
    }
    let mut best = c;
    let mut best_t = f32::INFINITY;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let e = b - a;
        let den = dir.x * e.y - dir.y * e.x;
        if den.abs() < 1e-9 {
            continue;
        }
        let w = a - c;
        let t = (w.x * e.y - w.y * e.x) / den;
        let u = (w.x * dir.y - w.y * dir.x) / den;
        if t > 0.0 && (0.0..=1.0).contains(&u) && t < best_t {
            best_t = t;
            best = c + dir * t;
        }
    }
    best
}

/// The node under a world position, topmost first.
pub fn node_at(project: &Project, w: Pos2, d: f32) -> Option<NodeId> {
    project.nodes.iter().rev().find(|n| (wp(n.pos) - w).length() <= d * 0.5).map(|n| n.id)
}

/// Polygon area by the shoelace formula (world units²).
pub fn polygon_area(pts: &[Pos2]) -> f32 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.x * q.y - q.x * p.y;
    }
    (a * 0.5).abs()
}

pub fn polygon_centroid(pts: &[Pos2]) -> Pos2 {
    let (mut cx, mut cy, mut a) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let cross = (p.x * q.y - q.x * p.y) as f64;
        a += cross;
        cx += (p.x + q.x) as f64 * cross;
        cy += (p.y + q.y) as f64 * cross;
    }
    if a.abs() < 1e-9 {
        let n = pts.len().max(1) as f32;
        return pos2(pts.iter().map(|p| p.x).sum::<f32>() / n, pts.iter().map(|p| p.y).sum::<f32>() / n);
    }
    pos2((cx / (3.0 * a)) as f32, (cy / (3.0 * a)) as f32)
}

pub fn polyline_length(pts: &[Pos2]) -> f32 {
    pts.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

/// Draws and handles the diagram. `palette_drop` is a node type being dragged from the palette.
pub fn show(ui: &mut Ui, doc: &mut Doc, tool: &mut Tool, it: &mut Interaction, palette_drop: &mut Option<NodeType>) -> Vec<CanvasAction> {
    let t = Tokens::get(ui.ctx());
    let mut actions = Vec::new();
    let (canvas, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(canvas);
    if t.glass {
        crate::glass::canvas(&painter, canvas, &t);
    } else {
        painter.rect_filled(canvas, 0.0, t.canvas);
    }
    let d = doc.project.diagram.symbol_size as f32;
    if doc.fit_pending && canvas.width() > 50.0 {
        fit_view(doc, canvas, None);
        doc.fit_pending = false;
    }
    let view = doc.view;
    let pointer = ui.input(|i| i.pointer.hover_pos());
    it.hover_world = pointer.filter(|p| canvas.contains(*p)).map(|p| to_world(&view, canvas, p));

    // Zoom with the wheel (about the pointer), pinch, or ctrl+wheel; shift+wheel pans.
    if resp.hovered() {
        let (scroll, zoom, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
        let mut factor = zoom;
        if (zoom - 1.0).abs() < 1e-4 && scroll.y != 0.0 && !shift {
            factor = (scroll.y * 0.0025).exp();
        }
        if shift && scroll != Vec2::ZERO {
            doc.view.offset += vec2(scroll.y + scroll.x, 0.0);
        }
        if (factor - 1.0).abs() > 1e-4
            && let Some(p) = pointer
        {
            zoom_about(doc, canvas, p, factor);
        }
    }

    draw_grid(&painter, doc, canvas, &t);
    draw_underlay(&painter, doc, canvas);
    draw_outlines(&painter, doc, canvas, &t);
    draw_arrows(&painter, doc, canvas, it, &t);
    draw_nodes(&painter, doc, canvas, it, &t);

    let view = doc.view;
    let world_ptr = pointer.map(|p| to_world(&view, canvas, p));
    it.hover_node = world_ptr.filter(|_| resp.hovered()).and_then(|w| node_at(&doc.project, w, d));

    // Palette drop.
    if palette_drop.is_some()
        && ui.input(|i| i.pointer.any_released())
        && let (Some(kind), Some(p)) = (palette_drop.take(), pointer)
        && canvas.contains(p)
    {
        let w = to_world(&doc.view, canvas, p);
        let w = snap(doc, w);
        let mut new_id = 0;
        doc.edit("Node Create", |pr| new_id = pr.add_node(kind, [w.x as f64, w.y as f64]));
        doc.selection = vec![new_id];
    }

    match tool {
        Tool::Select => select_tool(ui, doc, canvas, &resp, it, &mut actions),
        Tool::MoveUnderlay => move_underlay(ui, doc, canvas, &resp, it),
        Tool::Calibrate { .. } | Tool::Measure { .. } => {
            let calibrate = matches!(tool, Tool::Calibrate { .. });
            let mut finished = None;
            if let Tool::Calibrate { points } | Tool::Measure { points } = tool {
                let need = if calibrate { 2 } else { usize::MAX };
                if resp.clicked()
                    && let Some(w) = world_ptr
                {
                    points.push(w);
                }
                let finish = (calibrate && points.len() >= need) || (!calibrate && (resp.double_clicked() || ui.input(|i| i.key_pressed(Key::Enter))) && points.len() >= 2);
                draw_polyline(&painter, doc, canvas, points, world_ptr, false, Color32::from_rgb(220, 30, 30));
                if finish {
                    finished = Some(if calibrate { CanvasAction::Calibrated(points[0], points[1]) } else { CanvasAction::Measured(points.clone()) });
                }
            }
            if let Some(a) = finished {
                actions.push(a);
                *tool = Tool::Select;
            }
        }
        Tool::Trace { points } => {
            let mut close = false;
            if let Some(w) = world_ptr {
                let near_first = points.len() >= 3 && (to_screen(&doc.view, canvas, points[0]) - to_screen(&doc.view, canvas, w)).length() < 8.0;
                if resp.double_clicked() && points.len() >= 3 {
                    close = true;
                } else if resp.clicked() {
                    if near_first { close = true } else { points.push(w) }
                }
            }
            if ui.input(|i| i.key_pressed(Key::Enter)) && points.len() >= 3 {
                close = true;
            }
            if ui.input(|i| i.key_pressed(Key::Backspace)) {
                points.pop();
            }
            draw_polyline(&painter, doc, canvas, points, if close { None } else { world_ptr }, true, t.subcat.1);
            if close {
                actions.push(CanvasAction::Traced(points.clone()));
                *tool = Tool::Select;
            }
        }
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) && *tool != Tool::Select {
        *tool = Tool::Select;
    }
    actions
}

fn snap(doc: &Doc, w: Pos2) -> Pos2 {
    let g = doc.project.diagram.grid as f32;
    if doc.project.diagram.snap_to_grid && g > 0.0 { pos2((w.x / g).round() * g, (w.y / g).round() * g) } else { w }
}

pub fn zoom_about(doc: &mut Doc, canvas: Rect, screen: Pos2, factor: f32) {
    let before = to_world(&doc.view, canvas, screen);
    doc.view.zoom = (doc.view.zoom * factor).clamp(0.02, 40.0);
    let after = to_screen(&doc.view, canvas, before);
    doc.view.offset += screen - after;
}

/// Fits all nodes (or the given ids) and the underlay in view.
pub fn fit_view(doc: &mut Doc, canvas: Rect, only: Option<&[NodeId]>) {
    let d = doc.project.diagram.symbol_size as f32;
    let mut bb = Rect::NOTHING;
    for n in &doc.project.nodes {
        if only.is_some_and(|o| !o.contains(&n.id)) {
            continue;
        }
        let c = wp(n.pos);
        bb = bb.union(Rect::from_center_size(c, vec2(d * 2.5, d * 1.8)).translate(vec2(0.0, d * 0.3)));
        if let Some(o) = &n.outline {
            for p in o {
                bb.extend_with(wp(*p));
            }
        }
    }
    if only.is_none()
        && let (Some(u), Some(tex)) = (&doc.project.diagram.underlay, &doc.underlay_tex)
        && u.visible
    {
        let s = tex.size_vec2() * u.scale as f32;
        bb = bb.union(Rect::from_min_size(wp(u.origin), s));
    }
    if !bb.is_finite() || bb.width() <= 0.0 {
        doc.view = View::default();
        return;
    }
    let zoom = (canvas.width() / bb.width()).min(canvas.height() / bb.height()).clamp(0.05, 4.0) * 0.95;
    doc.view.zoom = zoom;
    doc.view.offset = canvas.size() / 2.0 - bb.center().to_vec2() * zoom;
}

fn draw_grid(p: &egui::Painter, doc: &Doc, canvas: Rect, t: &Tokens) {
    if !doc.project.diagram.show_grid {
        return;
    }
    let g = doc.project.diagram.grid as f32 * doc.view.zoom;
    if g < 6.0 {
        return;
    }
    let origin = canvas.min + doc.view.offset;
    let mut x = origin.x - ((origin.x - canvas.min.x) / g).floor() * g;
    while x < canvas.max.x {
        let mut y = origin.y - ((origin.y - canvas.min.y) / g).floor() * g;
        while y < canvas.max.y {
            p.rect_filled(Rect::from_center_size(pos2(x, y), vec2(1.5, 1.5)), 0.0, t.shadow);
            y += g;
        }
        x += g;
    }
}

fn draw_underlay(p: &egui::Painter, doc: &Doc, canvas: Rect) {
    let (Some(u), Some(tex)) = (&doc.project.diagram.underlay, &doc.underlay_tex) else { return };
    if !u.visible {
        return;
    }
    let size = tex.size_vec2() * u.scale as f32;
    let min = to_screen(&doc.view, canvas, wp(u.origin));
    let rect = Rect::from_min_size(min, size * doc.view.zoom);
    let tint = Color32::from_white_alpha((u.opacity.clamp(0.05, 1.0) * 255.0) as u8);
    p.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
}

fn draw_outlines(p: &egui::Painter, doc: &Doc, canvas: Rect, t: &Tokens) {
    for n in &doc.project.nodes {
        let Some(o) = &n.outline else { continue };
        if o.len() < 3 {
            continue;
        }
        let pts: Vec<Pos2> = o.iter().map(|q| to_screen(&doc.view, canvas, wp(*q))).collect();
        let sel = doc.is_selected(n.id);
        let fill = Color32::from_rgba_unmultiplied(t.subcat.1.r(), t.subcat.1.g(), t.subcat.1.b(), if sel { 60 } else { 34 });
        // Fill via triangle fan (outlines may be concave; fan from centroid is a fair approximation).
        let c = to_screen(&doc.view, canvas, polygon_centroid(&o.iter().map(|q| wp(*q)).collect::<Vec<_>>()));
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, fill);
        for q in &pts {
            mesh.colored_vertex(*q, fill);
        }
        for i in 0..pts.len() as u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % pts.len() as u32);
        }
        p.add(Shape::mesh(mesh));
        p.add(Shape::closed_line(pts, Stroke::new(if sel { 2.5 } else { 1.5 }, t.subcat.1)));
    }
}

/// A routing arrowhead: white with a coloured edge, or in the glass theme a little glass
/// pyramid (its two faces lit and shaded from the top-left, with a cast shadow).
fn arrow_head(p: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, stroke: Stroke, t: &Tokens) {
    let dir = dir.normalized();
    let side = vec2(-dir.y, dir.x);
    let back = tip - dir * size;
    let (l, r) = (back + side * size * 0.45, back - side * size * 0.45);
    if !t.glass {
        p.add(Shape::convex_polygon(vec![tip, l, r], Color32::WHITE, stroke));
        return;
    }
    let drop = vec2(1.0, 2.5);
    p.add(Shape::convex_polygon(vec![tip + drop, l + drop, r + drop], crate::glass::alpha(Color32::BLACK, 0.5), Stroke::NONE));
    let light = vec2(-0.6, -0.8);
    let lit = |s: Vec2| if s.dot(light) > 0.0 { crate::widgets::lerp_color(stroke.color, Color32::WHITE, 0.45) } else { crate::widgets::lerp_color(stroke.color, Color32::BLACK, 0.35) };
    // The ridge runs from the tip back along the axis, slightly raised toward the light.
    let ridge = back + dir * size * 0.2;
    let mut mesh = egui::Mesh::default();
    for (a, s) in [(l, side), (r, -side)] {
        let i = mesh.vertices.len() as u32;
        let c = lit(s);
        mesh.colored_vertex(tip, crate::widgets::lerp_color(c, Color32::WHITE, 0.2));
        mesh.colored_vertex(a, c);
        mesh.colored_vertex(ridge, crate::widgets::lerp_color(c, stroke.color, 0.5));
        mesh.add_triangle(i, i + 1, i + 2);
    }
    // The back face between the two wings.
    let i = mesh.vertices.len() as u32;
    let back_col = crate::widgets::lerp_color(stroke.color, Color32::BLACK, 0.5);
    mesh.colored_vertex(l, back_col);
    mesh.colored_vertex(r, back_col);
    mesh.colored_vertex(ridge, crate::widgets::lerp_color(stroke.color, Color32::BLACK, 0.2));
    mesh.add_triangle(i, i + 1, i + 2);
    p.add(Shape::mesh(mesh));
    p.add(Shape::closed_line(vec![tip, l, r], Stroke::new(0.8, crate::widgets::lerp_color(stroke.color, Color32::WHITE, 0.3))));
}

/// A routing line: plain, or in the glass theme a lit glass tube — a soft glow, a cast
/// shadow, dark edges, the coloured body and a highlight along the side facing the light.
/// Dashed lines keep their dashes (all layers dash in step).
fn route_line(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke, dashed: bool, t: &Tokens) {
    if !t.glass {
        line(p, a, b, s, dashed);
        return;
    }
    let dir = (b - a).normalized();
    let mut up = vec2(-dir.y, dir.x);
    if up.dot(vec2(-0.6, -0.8)) < 0.0 {
        up = -up;
    }
    let w = s.width;
    p.line_segment([a, b], Stroke::new(w * 5.0, crate::glass::alpha(s.color, 0.10)));
    let drop = vec2(1.0, w + 1.5);
    line(p, a + drop, b + drop, Stroke::new(w + 0.5, crate::glass::alpha(Color32::BLACK, 0.5)), dashed);
    line(p, a, b, Stroke::new(w + 1.6, crate::widgets::lerp_color(s.color, Color32::BLACK, 0.55)), dashed);
    line(p, a, b, s, dashed);
    let hl = up * w * 0.28;
    line(p, a + hl, b + hl, Stroke::new((w * 0.38).max(0.7), crate::widgets::lerp_color(s.color, Color32::WHITE, 0.75)), dashed);
}

/// Where a routing line stops so the glass tube ends inside its arrowhead.
fn line_end(tip: Pos2, from: Pos2, head: f32, t: &Tokens) -> Pos2 {
    if t.glass { tip - (tip - from).normalized() * head * 0.75 } else { tip }
}

/// Screen geometry of node `n`'s outflow `k`: (start, end) with `end` at the arrowhead.
fn arrow_geom(doc: &Doc, canvas: Rect, from: NodeId, k: usize) -> Option<(Pos2, Pos2)> {
    let d = doc.project.diagram.symbol_size as f32;
    let n = doc.project.node(from)?;
    let target = doc.project.node(n.outflows.get(k).copied().flatten()?)?;
    let (a, b) = (wp(n.pos), wp(target.pos));
    let start = boundary_point(n.node_type(), a, d, b);
    let end = boundary_point(target.node_type(), b, d, a);
    let (s, e) = (to_screen(&doc.view, canvas, start), to_screen(&doc.view, canvas, end));
    // Several outflows to the same node are drawn side by side.
    let same: Vec<usize> = (0..3).filter(|j| n.outflows[*j] == Some(target.id)).collect();
    if same.len() > 1 {
        let pos = same.iter().position(|j| *j == k).unwrap_or(0) as f32 - (same.len() - 1) as f32 / 2.0;
        let dir = (e - s).normalized();
        let off = egui::vec2(-dir.y, dir.x) * pos * 7.0;
        return Some((s + off, e + off));
    }
    Some((s, e))
}

fn route_stroke(k: usize, t: &Tokens, zoom: f32) -> (Stroke, bool) {
    // Glass tubes are a little thicker so their shading reads.
    let w = (1.3 * zoom.sqrt()).clamp(1.0, 2.5) * if t.glass { 1.5 } else { 1.0 };
    match k {
        0 => (Stroke::new(w, t.arrow), false),
        1 => (Stroke::new(w, t.arrow_secondary), true),
        _ => (Stroke::new(w, t.arrow_tertiary), true),
    }
}

fn draw_arrows(p: &egui::Painter, doc: &Doc, canvas: Rect, it: &Interaction, t: &Tokens) {
    let head = (doc.project.diagram.symbol_size as f32 * 0.22 * doc.view.zoom).clamp(6.0, 18.0);
    for n in &doc.project.nodes {
        for k in 0..n.node_type().outflow_count() {
            if let Some(Drag::Reroute { from, k: dk, at }) = &it.drag
                && *from == n.id
                && *dk == k
            {
                let a = to_screen(&doc.view, canvas, wp(n.pos));
                let (s, dashed) = route_stroke(k, t, doc.view.zoom);
                route_line(p, a, line_end(*at, a, head, t), s, dashed, t);
                arrow_head(p, *at, *at - a, head, s, t);
                continue;
            }
            if let Some((a, b)) = arrow_geom(doc, canvas, n.id, k) {
                let (s, dashed) = route_stroke(k, t, doc.view.zoom);
                route_line(p, a, line_end(b, a, head, t), s, dashed, t);
                arrow_head(p, b, b - a, head, s, t);
            }
        }
    }
}

fn line(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke, dashed: bool) {
    if dashed {
        p.extend(Shape::dashed_line(&[a, b], s, 7.0, 4.0));
    } else {
        p.line_segment([a, b], s);
    }
}

/// Where the outflow handle of an unconnected outflow `k` sits (screen).
fn handle_pos(doc: &Doc, canvas: Rect, from: NodeId, k: usize) -> Option<Pos2> {
    let d = doc.project.diagram.symbol_size as f32 * doc.view.zoom;
    let n = doc.project.node(from)?;
    if let Some((_, b)) = arrow_geom(doc, canvas, from, k) {
        return Some(b);
    }
    let c = to_screen(&doc.view, canvas, wp(n.pos));
    let off = [vec2(0.75, -0.15), vec2(0.6, 0.55), vec2(0.0, 0.85)];
    Some(c + off.get(k).copied().unwrap_or(Vec2::ZERO) * d)
}

fn draw_nodes(p: &egui::Painter, doc: &Doc, canvas: Rect, it: &Interaction, t: &Tokens) {
    let d = doc.project.diagram.symbol_size as f32 * doc.view.zoom;
    let font = (d * 0.27).clamp(6.0, 22.0);
    for n in &doc.project.nodes {
        let c = to_screen(&doc.view, canvas, wp(n.pos));
        if !canvas.expand(d * 3.0).contains(c) {
            continue;
        }
        let sel = doc.is_selected(n.id);
        match &n.data {
            NodeData::Text(tx) => {
                let size = tx.size * doc.view.zoom;
                let galley = p.layout(tx.text.clone(), FontId::proportional(size.max(4.0)), t.label_text, f32::INFINITY);
                let r = Rect::from_center_size(c, galley.size() + vec2(8.0, 6.0));
                if t.glass {
                    // A glass plaque (when bordered) and lettering with a cast shadow.
                    if tx.border || sel {
                        crate::glass::surface(p, r, 6.0, if sel { crate::glass::Surface::Selected } else { crate::glass::Surface::Key }, t);
                    }
                    p.galley_with_override_text_color(r.min + vec2(4.8, 4.4), galley.clone(), crate::glass::alpha(Color32::BLACK, 0.75));
                } else if tx.border || sel {
                    p.rect_stroke(r, 0.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { t.select } else { t.shadow }), egui::StrokeKind::Inside);
                }
                p.galley(r.min + vec2(4.0, 3.0), galley, t.label_text);
            }
            data => {
                let external = matches!(data, NodeData::Link(l) if !matches!(l.source, wavestorm_core::link::LinkSource::Inflows)
                    || !matches!(l.elevation, wavestorm_core::link::LinkElevation::None));
                let problem = doc.results.nodes.get(&n.id).is_some_and(|r| r.undefined.is_some())
                    || doc.results.messages.iter().any(|m| m.node == Some(n.id) && m.severity >= wavestorm_core::calc::Severity::Error);
                icons::draw_node_ex(p, n.node_type(), c, d, &n.label(), sel, external, problem, t);
                if !n.name.is_empty() {
                    let galley = p.layout(n.name.clone(), FontId::proportional(font * 0.85), t.label_text, d * 3.2);
                    let top = c.y + d * 0.5 + 3.0 + if external { d * 0.24 } else { 0.0 } + if t.glass { d * 0.09 } else { 0.0 };
                    let at = pos2(c.x - galley.size().x / 2.0, top);
                    if t.glass {
                        p.galley_with_override_text_color(at + vec2(0.7, 1.2), galley.clone(), crate::glass::alpha(Color32::BLACK, 0.8));
                    }
                    p.galley(at, galley, t.label_text);
                }
            }
        }
        // Outflow handles for unconnected outflows of selected or hovered nodes.
        if sel || it.hover_node == Some(n.id) {
            for k in 0..n.node_type().outflow_count() {
                if n.outflows[k].is_none()
                    && let Some(h) = handle_pos(doc, canvas, n.id, k)
                {
                    let (s, dashed) = route_stroke(k, t, doc.view.zoom);
                    let from = boundary_point(n.node_type(), c, d, h);
                    let size = (d * 0.2).clamp(6.0, 14.0);
                    route_line(p, from, line_end(h, from, size, t), s, dashed, t);
                    arrow_head(p, h, h - c, size, s, t);
                }
            }
        }
    }
}

fn draw_polyline(p: &egui::Painter, doc: &Doc, canvas: Rect, pts: &[Pos2], cursor: Option<Pos2>, closed_preview: bool, color: Color32) {
    let mut s: Vec<Pos2> = pts.iter().map(|q| to_screen(&doc.view, canvas, *q)).collect();
    if let Some(c) = cursor {
        s.push(to_screen(&doc.view, canvas, c));
    }
    if closed_preview && s.len() >= 3 {
        let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40);
        let c = polygon_centroid(&s);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, fill);
        for q in &s {
            mesh.colored_vertex(*q, fill);
        }
        for i in 0..s.len() as u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % s.len() as u32);
        }
        p.add(Shape::mesh(mesh));
        p.add(Shape::closed_line(s.clone(), Stroke::new(2.0, color)));
    } else if s.len() >= 2 {
        p.add(Shape::line(s.clone(), Stroke::new(2.0, color)));
    }
    for q in s.iter().take(pts.len()) {
        p.rect_filled(Rect::from_center_size(*q, vec2(6.0, 6.0)), 0.0, color);
    }
}

fn select_tool(ui: &mut Ui, doc: &mut Doc, canvas: Rect, resp: &Response, it: &mut Interaction, actions: &mut Vec<CanvasAction>) {
    let d = doc.project.diagram.symbol_size as f32;
    let (pointer, mods) = ui.input(|i| (i.pointer.interact_pos(), i.modifiers));
    let world = pointer.map(|p| to_world(&doc.view, canvas, p));

    // Drag start.
    if resp.drag_started_by(egui::PointerButton::Middle) || (resp.drag_started_by(egui::PointerButton::Primary) && ui.input(|i| i.key_down(Key::Space))) {
        it.drag = Some(Drag::Pan);
    } else if resp.drag_started_by(egui::PointerButton::Primary)
        && let (Some(p), Some(_w)) = (resp.interact_pointer_pos(), world)
    {
        // Arrow handles first (only when not over a symbol's body).
        let mut handle = None;
        for n in &doc.project.nodes {
            if !(doc.is_selected(n.id) || n.outflows.iter().any(|o| o.is_some())) {
                continue;
            }
            for k in 0..n.node_type().outflow_count() {
                if n.outflows[k].is_none() && !doc.is_selected(n.id) {
                    continue;
                }
                if let Some(h) = handle_pos(doc, canvas, n.id, k)
                    && (h - p).length() < 9.0
                {
                    handle = Some((n.id, k));
                }
            }
        }
        let start = resp.interact_pointer_pos().map(|q| q - resp.drag_delta()).unwrap_or(p);
        let start_world = to_world(&doc.view, canvas, start);
        if let Some((from, k)) = handle {
            it.drag = Some(Drag::Reroute { from, k, at: p });
        } else if let Some(id) = node_at(&doc.project, start_world, d) {
            if !doc.is_selected(id) {
                if mods.ctrl || mods.command || mods.shift {
                    doc.selection.push(id);
                } else {
                    doc.selection = vec![id];
                }
            }
            let before = doc.project.clone();
            let mut cloned = false;
            if mods.ctrl || mods.command {
                // Ctrl-drag clones the selection.
                let ids = doc.selection.clone();
                let mut new_ids = Vec::new();
                for id in ids {
                    if let Some(n) = doc.project.node(id).cloned() {
                        let nid = doc.project.add_node(n.node_type(), n.pos);
                        if let Some(m) = doc.project.node_mut(nid) {
                            m.data = n.data.clone();
                            m.name = n.name.clone();
                            m.notes = n.notes.clone();
                        }
                        new_ids.push(nid);
                    }
                }
                doc.selection = new_ids;
                cloned = true;
            }
            it.drag = Some(Drag::Nodes { before, last: start_world, cloned });
        } else {
            if !(mods.ctrl || mods.shift) {
                doc.selection.clear();
            }
            it.rubber = Some((start, p));
        }
    }

    // Drag progress.
    if let (Some(drag), Some(p)) = (&mut it.drag, pointer) {
        match drag {
            Drag::Pan => doc.view.offset += resp.drag_delta(),
            Drag::Nodes { last, .. } => {
                let w = to_world(&doc.view, canvas, p);
                let delta = w - *last;
                *last = w;
                let sel = doc.selection.clone();
                for id in sel {
                    if let Some(n) = doc.project.node_mut(id) {
                        n.pos[0] += delta.x as f64;
                        n.pos[1] += delta.y as f64;
                        if let Some(o) = &mut n.outline {
                            for q in o.iter_mut() {
                                q[0] += delta.x as f64;
                                q[1] += delta.y as f64;
                            }
                        }
                    }
                }
            }
            Drag::Reroute { at, .. } => *at = p,
            Drag::Underlay { .. } => {}
        }
    }
    if let (Some((_, b)), Some(p)) = (&mut it.rubber, pointer) {
        *b = p;
        let (a, b2) = (it.rubber.map(|r| r.0).unwrap_or(p), p);
        let r = Rect::from_two_pos(a, b2);
        let t = Tokens::get(ui.ctx());
        let edge = if t.is_dark() { t.hint } else { Color32::from_rgb(10, 36, 106) };
        ui.painter().rect(r, if t.glass { 4.0 } else { 0.0 }, Color32::from_rgba_unmultiplied(edge.r(), edge.g(), edge.b(), 24), Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    }

    // Drag end.
    if resp.drag_stopped() || (it.drag.is_some() && !ui.input(|i| i.pointer.any_down())) {
        match it.drag.take() {
            Some(Drag::Nodes { before, cloned, .. }) => {
                if doc.project.diagram.snap_to_grid {
                    let sel = doc.selection.clone();
                    for id in sel {
                        if let Some(n) = doc.project.node(id).map(|n| wp(n.pos)) {
                            let s = snap(doc, n);
                            if let Some(m) = doc.project.node_mut(id) {
                                m.pos = [s.x as f64, s.y as f64];
                            }
                        }
                    }
                }
                doc.commit_from(if cloned { "Node Clone" } else { "Node Move" }, before);
            }
            Some(Drag::Reroute { from, k, at }) => {
                let w = to_world(&doc.view, canvas, at);
                let target = node_at(&doc.project, w, d).filter(|t| *t != from);
                let ok = target.and_then(|t| doc.project.node(t)).is_some_and(|n| n.node_type().accepts_inflow());
                doc.edit("Node Reroute", |pr| {
                    if let Some(n) = pr.node_mut(from)
                        && let Some(slot) = n.outflows.get_mut(k)
                    {
                        *slot = if ok { target } else { None };
                    }
                });
            }
            _ => {}
        }
        if let Some((a, b)) = it.rubber.take() {
            let r = Rect::from_two_pos(a, b);
            for n in &doc.project.nodes {
                if r.contains(to_screen(&doc.view, canvas, wp(n.pos))) && !doc.selection.contains(&n.id) {
                    doc.selection.push(n.id);
                }
            }
        }
    }

    // Clicks.
    if resp.clicked()
        && let Some(w) = world
    {
        match node_at(&doc.project, w, d) {
            Some(id) if mods.ctrl || mods.shift => {
                if let Some(i) = doc.selection.iter().position(|s| *s == id) {
                    doc.selection.remove(i);
                } else {
                    doc.selection.push(id);
                }
            }
            Some(id) => doc.selection = vec![id],
            None => doc.selection.clear(),
        }
    }
    if resp.double_clicked()
        && let Some(id) = world.and_then(|w| node_at(&doc.project, w, d))
    {
        actions.push(CanvasAction::OpenReport(id));
    }
    if resp.secondary_clicked()
        && let Some(id) = world.and_then(|w| node_at(&doc.project, w, d))
    {
        if !doc.is_selected(id) {
            doc.selection = vec![id];
        }
        actions.push(CanvasAction::ContextNode(id));
    }
    if resp.hovered() && ui.input(|i| i.key_pressed(Key::Delete)) && !doc.selection.is_empty() && !ui.ctx().egui_wants_keyboard_input() {
        actions.push(CanvasAction::Delete);
    }
    if let Some(id) = it.hover_node
        && let Some(n) = doc.project.node(id)
    {
        resp.clone().on_hover_text_at_pointer(n.title());
    }
}

fn move_underlay(ui: &mut Ui, doc: &mut Doc, canvas: Rect, resp: &Response, it: &mut Interaction) {
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = pointer
    {
        it.drag = Some(Drag::Underlay { last: to_world(&doc.view, canvas, p), before: doc.project.clone() });
    }
    if let (Some(Drag::Underlay { last, .. }), Some(p)) = (&mut it.drag, pointer) {
        let w = to_world(&doc.view, canvas, p);
        let delta = w - *last;
        *last = w;
        if let Some(u) = &mut doc.project.diagram.underlay {
            u.origin[0] += delta.x as f64;
            u.origin[1] += delta.y as f64;
        }
    }
    if resp.drag_stopped()
        && let Some(Drag::Underlay { before, .. }) = it.drag.take()
    {
        doc.commit_from("Move Background", before);
    }
    let _ = vec2(0.0, 0.0);
    let _ = Align2::CENTER_CENTER;
}

/// Classic scrollbars for the diagram: `canvas` is the diagram area, `vbar`/`hbar` the strips.
pub fn scrollbars(ui: &mut Ui, doc: &mut Doc, canvas: Rect, vbar: Rect, hbar: Rect) {
    let t = Tokens::get(ui.ctx());
    let d = doc.project.diagram.symbol_size as f32;
    let view_world = Rect::from_min_max(to_world(&doc.view, canvas, canvas.min), to_world(&doc.view, canvas, canvas.max));
    let mut content = view_world;
    for n in &doc.project.nodes {
        content = content.union(Rect::from_center_size(wp(n.pos), vec2(d * 6.0, d * 6.0)));
    }
    if let (Some(u), Some(tex)) = (&doc.project.diagram.underlay, &doc.underlay_tex) {
        content = content.union(Rect::from_min_size(wp(u.origin), tex.size_vec2() * u.scale as f32));
    }
    if t.glass {
        overlay_scrollbars(ui, doc, vbar, hbar, content, view_world);
        return;
    }
    for (bar, vertical) in [(vbar, true), (hbar, false)] {
        let p = ui.painter();
        p.rect_filled(bar, 0.0, t.face.gamma_multiply(0.92));
        let arrow = 16.0f32;
        let (a1, a2, track) = if vertical {
            (
                Rect::from_min_size(bar.min, vec2(bar.width(), arrow)),
                Rect::from_min_size(pos2(bar.min.x, bar.max.y - arrow), vec2(bar.width(), arrow)),
                Rect::from_min_max(pos2(bar.min.x, bar.min.y + arrow), pos2(bar.max.x, bar.max.y - arrow)),
            )
        } else {
            (
                Rect::from_min_size(bar.min, vec2(arrow, bar.height())),
                Rect::from_min_size(pos2(bar.max.x - arrow, bar.min.y), vec2(arrow, bar.height())),
                Rect::from_min_max(pos2(bar.min.x + arrow, bar.min.y), pos2(bar.max.x - arrow, bar.max.y)),
            )
        };
        let (c0, c1, v0, v1) = if vertical {
            (content.min.y, content.max.y, view_world.min.y, view_world.max.y)
        } else {
            (content.min.x, content.max.x, view_world.min.x, view_world.max.x)
        };
        let span = (c1 - c0).max(1e-3);
        let len = if vertical { track.height() } else { track.width() };
        let start = (v0 - c0) / span * len;
        let size = ((v1 - v0) / span * len).max(14.0).min(len);
        let thumb = if vertical {
            Rect::from_min_size(pos2(track.min.x, track.min.y + start), vec2(track.width(), size))
        } else {
            Rect::from_min_size(pos2(track.min.x + start, track.min.y), vec2(size, track.height()))
        };
        for (r, dir) in [(a1, -1.0f32), (a2, 1.0)] {
            let resp = ui.interact(r, ui.id().with(("sb", vertical, dir as i32)), Sense::click_and_drag());
            let p = ui.painter();
            p.rect_filled(r, 0.0, t.face);
            crate::widgets::bevel(p, r, !resp.is_pointer_button_down_on(), &t);
            let c = r.center();
            let tri = if vertical {
                if dir < 0.0 { vec![c + vec2(-4.0, 2.0), c + vec2(4.0, 2.0), c + vec2(0.0, -2.0)] } else { vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.0)] }
            } else if dir < 0.0 {
                vec![c + vec2(2.0, -4.0), c + vec2(2.0, 4.0), c + vec2(-2.0, 0.0)]
            } else {
                vec![c + vec2(-2.0, -4.0), c + vec2(-2.0, 4.0), c + vec2(2.0, 0.0)]
            };
            p.add(Shape::convex_polygon(tri, t.text, Stroke::NONE));
            if resp.clicked() || resp.is_pointer_button_down_on() && resp.dragged() {
                let step = 30.0 * dir;
                if vertical { doc.view.offset.y -= step } else { doc.view.offset.x -= step }
            }
        }
        let resp = ui.interact(thumb, ui.id().with(("thumb", vertical)), Sense::drag());
        let p = ui.painter();
        p.rect_filled(thumb, 0.0, t.face);
        crate::widgets::bevel(p, thumb, true, &t);
        if resp.dragged() {
            let delta = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
            let world = delta / len * span;
            let screen = world * doc.view.zoom;
            if vertical { doc.view.offset.y -= screen } else { doc.view.offset.x -= screen }
        }
        // Clicking the track pages the view.
        let track_resp = ui.interact(track, ui.id().with(("track", vertical)), Sense::click());
        if track_resp.clicked()
            && let Some(pp) = track_resp.interact_pointer_pos()
        {
            let before = if vertical { pp.y < thumb.min.y } else { pp.x < thumb.min.x };
            let page = if vertical { canvas.height() } else { canvas.width() } * 0.9 * if before { -1.0 } else { 1.0 };
            if vertical { doc.view.offset.y -= page } else { doc.view.offset.x -= page }
        }
    }
}

/// Glass-theme scrollbars: thin pill thumbs over the canvas edges, no arrows or track; they
/// brighten under the pointer and drag like the classic thumbs.
fn overlay_scrollbars(ui: &mut Ui, doc: &mut Doc, vbar: Rect, hbar: Rect, content: Rect, view_world: Rect) {
    for (bar, vertical) in [(vbar, true), (hbar, false)] {
        let (c0, c1, v0, v1) = if vertical {
            (content.min.y, content.max.y, view_world.min.y, view_world.max.y)
        } else {
            (content.min.x, content.max.x, view_world.min.x, view_world.max.x)
        };
        let span = (c1 - c0).max(1e-3);
        let len = if vertical { bar.height() } else { bar.width() };
        let start = (v0 - c0) / span * len;
        let size = ((v1 - v0) / span * len).max(24.0).min(len);
        if size >= len - 0.5 {
            continue;
        }
        let thumb = if vertical {
            Rect::from_min_size(pos2(bar.min.x, bar.min.y + start), vec2(bar.width(), size))
        } else {
            Rect::from_min_size(pos2(bar.min.x + start, bar.min.y), vec2(size, bar.height()))
        };
        let resp = ui.interact(thumb.expand(3.0), ui.id().with(("glass-thumb", vertical)), Sense::drag());
        let hot = resp.hovered() || resp.dragged();
        let t = Tokens::get(ui.ctx());
        crate::glass::surface(ui.painter(), thumb, 4.0, if hot { crate::glass::Surface::Hover } else { crate::glass::Surface::Key }, &t);
        if resp.dragged() {
            let delta = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
            let screen = delta / len * span * doc.view.zoom;
            if vertical { doc.view.offset.y -= screen } else { doc.view.offset.x -= screen }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_helpers() {
        let sq = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        assert!((polygon_area(&sq) - 100.0).abs() < 1e-4);
        assert!((polygon_centroid(&sq) - pos2(5.0, 5.0)).length() < 1e-4);
        assert!((polyline_length(&sq) - 30.0).abs() < 1e-4);
        let b = boundary_point(NodeType::Reach, pos2(0.0, 0.0), 100.0, pos2(100.0, 0.0));
        assert!((b.x - 36.0).abs() < 1e-3);
        let view = View { offset: vec2(5.0, 7.0), zoom: 2.0 };
        let canvas = Rect::from_min_size(pos2(10.0, 20.0), vec2(500.0, 400.0));
        let w = pos2(3.0, 4.0);
        assert!((to_world(&view, canvas, to_screen(&view, canvas, w)) - w).length() < 1e-5);
    }
}
