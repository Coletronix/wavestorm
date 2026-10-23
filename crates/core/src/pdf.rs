//! A small PDF writer: pages of text (standard Type 1 fonts) and vector graphics.
//!
//! Coordinates are in points with the origin at the top-left of the page and y pointing down;
//! they are flipped when written.

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Sans,
    SansBold,
    SansItalic,
    Mono,
}

impl Font {
    fn resource(self) -> &'static str {
        match self {
            Font::Sans => "F1",
            Font::SansBold => "F2",
            Font::SansItalic => "F3",
            Font::Mono => "F4",
        }
    }
}

/// Text alignment relative to the given x.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// RGB colour, components 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
    pub const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);
    pub fn from_u8(r: u8, g: u8, b: u8) -> Rgb {
        Rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
}

/// Helvetica advance widths (1/1000 em) for ASCII 32..=126, from the standard font metrics.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584,
    584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667,
    611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500,
    722, 500, 500, 500, 334, 260, 334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584,
    584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667,
    611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556,
    778, 556, 556, 500, 389, 280, 389, 584,
];

/// Width of `text` in points at `size`.
pub fn text_width(text: &str, font: Font, size: f32) -> f32 {
    let em: u32 = text
        .chars()
        .map(|c| {
            let i = (c as u32).wrapping_sub(32) as usize;
            match font {
                Font::Mono => 600,
                Font::SansBold => HELVETICA_BOLD.get(i).copied().unwrap_or(556) as u32,
                _ => HELVETICA.get(i).copied().unwrap_or(556) as u32,
            }
        })
        .sum();
    em as f32 * size / 1000.0
}

/// Fill and stroke of a closed shape.
pub type Stroke = Option<(f32, Rgb)>;

/// One recorded drawing operation (coordinates in points, y down). Output formats (PDF, EMF)
/// are generated from the list.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Text with its baseline at `y`, aligned relative to `x`.
    Text { x: f32, y: f32, size: f32, font: Font, align: Align, color: Rgb, text: String },
    /// Text rotated 90 degrees counter-clockwise, centred on `y`, baseline at `x`.
    TextVertical { x: f32, y: f32, size: f32, font: Font, color: Rgb, text: String },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgb },
    /// Sets the dash pattern for later strokes (`None` = solid).
    Dash(Option<(f32, f32)>),
    Polyline { pts: Vec<(f32, f32)>, width: f32, color: Rgb },
    Polygon { pts: Vec<(f32, f32)>, fill: Option<Rgb>, stroke: Stroke },
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Option<Rgb>, stroke: Stroke },
    Circle { cx: f32, cy: f32, r: f32, fill: Option<Rgb>, stroke: Stroke },
}

/// One page being drawn.
pub struct Page {
    pub width: f32,
    pub height: f32,
    ops: Vec<Op>,
}

fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Escapes a string for a PDF literal, mapping Latin-1 characters and dropping others.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            '\u{a0}'..='\u{ff}' => {
                let _ = write!(out, "\\{:03o}", c as u32);
            }
            '\u{2013}' | '\u{2014}' => out.push('-'),
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            _ => out.push('?'),
        }
    }
    out
}

impl Page {
    pub fn new(width: f32, height: f32) -> Page {
        Page { width, height, ops: Vec::new() }
    }

    /// The recorded drawing operations, in order.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// Draws text with its baseline at `y`.
    pub fn text(&mut self, x: f32, y: f32, size: f32, font: Font, align: Align, color: Rgb, text: &str) {
        self.ops.push(Op::Text { x, y, size, font, align, color, text: text.to_string() });
    }

    /// Text rotated 90° counter-clockwise (for vertical axis titles), centred on (x, y).
    pub fn text_vertical(&mut self, x: f32, y: f32, size: f32, font: Font, color: Rgb, text: &str) {
        self.ops.push(Op::TextVertical { x, y, size, font, color, text: text.to_string() });
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgb) {
        self.ops.push(Op::Line { x1, y1, x2, y2, width, color });
    }

    pub fn dashed(&mut self, on: f32, off: f32) {
        self.ops.push(Op::Dash(Some((on, off))));
    }

    pub fn solid(&mut self) {
        self.ops.push(Op::Dash(None));
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], width: f32, color: Rgb) {
        if pts.is_empty() {
            return;
        }
        self.ops.push(Op::Polyline { pts: pts.to_vec(), width, color });
    }

    /// Filled (and optionally stroked) polygon.
    pub fn polygon(&mut self, pts: &[(f32, f32)], fill: Option<Rgb>, stroke: Stroke) {
        if pts.is_empty() {
            return;
        }
        self.ops.push(Op::Polygon { pts: pts.to_vec(), fill, stroke });
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgb>, stroke: Stroke) {
        self.ops.push(Op::Rect { x, y, w, h, fill, stroke });
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Option<Rgb>, stroke: Stroke) {
        self.ops.push(Op::Circle { cx, cy, r, fill, stroke });
    }

    /// The page content as a PDF content stream.
    fn pdf_content(&self) -> String {
        let mut o = String::new();
        let y = |v: f32| self.height - v;
        for op in &self.ops {
            match op {
                Op::Text { x, y: ty, size, font, align, color, text } => {
                    let w = text_width(text, *font, *size);
                    let x = match align {
                        Align::Left => *x,
                        Align::Center => x - w / 2.0,
                        Align::Right => x - w,
                    };
                    let _ = writeln!(o, "BT {} {} {} rg /{} {} Tf {} {} Td ({}) Tj ET", num(color.0), num(color.1), num(color.2), font.resource(), num(*size), num(x), num(y(*ty)), escape(text));
                }
                Op::TextVertical { x, y: ty, size, font, color, text } => {
                    let w = text_width(text, *font, *size);
                    let _ = writeln!(o, "BT {} {} {} rg /{} {} Tf 0 1 -1 0 {} {} Tm ({}) Tj ET", num(color.0), num(color.1), num(color.2), font.resource(), num(*size), num(*x), num(y(*ty) - w / 2.0), escape(text));
                }
                Op::Line { x1, y1, x2, y2, width, color } => {
                    let _ = writeln!(o, "{} {} {} RG {} w {} {} m {} {} l S", num(color.0), num(color.1), num(color.2), num(*width), num(*x1), num(y(*y1)), num(*x2), num(y(*y2)));
                }
                Op::Dash(Some((on, off))) => {
                    let _ = writeln!(o, "[{} {}] 0 d", num(*on), num(*off));
                }
                Op::Dash(None) => o.push_str("[] 0 d\n"),
                Op::Polyline { pts, width, color } => {
                    let Some(first) = pts.first() else { continue };
                    let _ = write!(o, "{} {} {} RG {} w 1 j {} {} m", num(color.0), num(color.1), num(color.2), num(*width), num(first.0), num(y(first.1)));
                    for p in &pts[1..] {
                        let _ = write!(o, " {} {} l", num(p.0), num(y(p.1)));
                    }
                    o.push_str(" S\n");
                }
                Op::Polygon { pts, fill, stroke } => self.pdf_polygon(&mut o, pts, *fill, *stroke),
                Op::Rect { x, y: ry, w, h, fill, stroke } => {
                    self.pdf_polygon(&mut o, &[(*x, *ry), (x + w, *ry), (x + w, ry + h), (*x, ry + h)], *fill, *stroke);
                }
                Op::Circle { cx, cy, r, fill, stroke } => {
                    self.pdf_polygon(&mut o, &circle_points(*cx, *cy, *r), *fill, *stroke);
                }
            }
        }
        o
    }

    fn pdf_polygon(&self, o: &mut String, pts: &[(f32, f32)], fill: Option<Rgb>, stroke: Stroke) {
        let Some(first) = pts.first() else { return };
        let y = |v: f32| self.height - v;
        if let Some(f) = fill {
            let _ = write!(o, "{} {} {} rg ", num(f.0), num(f.1), num(f.2));
        }
        if let Some((w, c)) = stroke {
            let _ = write!(o, "{} {} {} RG {} w ", num(c.0), num(c.1), num(c.2), num(w));
        }
        let _ = write!(o, "{} {} m", num(first.0), num(y(first.1)));
        for p in &pts[1..] {
            let _ = write!(o, " {} {} l", num(p.0), num(y(p.1)));
        }
        o.push_str(match (fill.is_some(), stroke.is_some()) {
            (true, true) => " b\n",
            (true, false) => " f\n",
            (false, true) => " s\n",
            (false, false) => " n\n",
        });
    }
}

/// The 32-gon used to draw circles in PDF output.
fn circle_points(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// A multi-page document.
#[derive(Default)]
pub struct Document {
    pages: Vec<Page>,
    pub title: String,
}

impl Document {
    pub fn new(title: &str) -> Document {
        Document { pages: Vec::new(), title: title.into() }
    }

    pub fn add(&mut self, page: Page) {
        self.pages.push(page);
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Serialises the document.
    pub fn finish(self) -> Vec<u8> {
        let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets: Vec<usize> = Vec::new();
        let n = self.pages.len();
        // Object numbers: 1 catalog, 2 pages, 3-6 fonts, 7 info, then (page, content) pairs.
        let page_obj = |i: usize| 8 + 2 * i;
        let mut objects: Vec<Vec<u8>> = Vec::new();
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
        objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n).into_bytes());
        for base in ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Courier"] {
            objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>").into_bytes());
        }
        objects.push(format!("<< /Title ({}) /Producer (WAVEStorm) >>", escape(&self.title)).into_bytes());
        for (i, p) in self.pages.iter().enumerate() {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R /F4 6 0 R >> >> /Contents {} 0 R >>",
                    num(p.width),
                    num(p.height),
                    page_obj(i) + 1
                )
                .into_bytes(),
            );
            let content = p.pdf_content();
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content.as_bytes());
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_parseable_document() {
        let mut d = Document::new("Test (1)");
        let mut p = Page::new(612.0, 792.0);
        p.text(72.0, 72.0, 12.0, Font::SansBold, Align::Left, Rgb::BLACK, "Hello (world) \\ 25 m² ±");
        p.line(72.0, 80.0, 300.0, 80.0, 1.0, Rgb::BLACK);
        p.polygon(&[(100.0, 100.0), (150.0, 100.0), (125.0, 140.0)], Some(Rgb(0.8, 0.9, 1.0)), Some((1.0, Rgb::BLACK)));
        d.add(p);
        d.add(Page::new(792.0, 612.0));
        let bytes = d.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("\\(world\\)"));
        assert!(text.contains("\\262"), "superscript two in WinAnsi");
        // Each xref offset points at "n 0 obj".
        let xref_at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        let table = String::from_utf8_lossy(&bytes[xref_at..]).into_owned();
        // Offsets are byte positions; the lossy text has the same length after the header.
        let shift = text.len() as isize - bytes.len() as isize;
        for (k, line) in table.lines().skip(3).take(5).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(bytes[off..].starts_with(format!("{} 0 obj", k + 1).as_bytes()), "object {}", k + 1);
        }
        let _ = shift;
    }

    #[test]
    fn helvetica_widths() {
        assert!((text_width("W", Font::Sans, 10.0) - 9.44).abs() < 1e-3);
        assert!((text_width("abc", Font::Mono, 10.0) - 18.0).abs() < 1e-3);
    }
}
