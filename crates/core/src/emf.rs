//! Windows Enhanced Metafile (EMF) writer for a [`Page`], following the public [MS-EMF]
//! specification.
//!
//! The metafile uses one logical unit = 0.01 mm (the reference device is declared as
//! 100000 x 100000 pixels over 1000 x 1000 mm), so MM_TEXT coordinates equal the frame units and
//! no window/viewport records are needed. All records are little-endian and 4-byte aligned.

use crate::pdf::{text_width, Align, Font, Op, Page, Rgb, Stroke};

const EMR_HEADER: u32 = 1;
const EMR_POLYGON: u32 = 3;
const EMR_POLYLINE: u32 = 4;
const EMR_SETBKMODE: u32 = 18;
const EMR_SETPOLYFILLMODE: u32 = 19;
const EMR_SETTEXTALIGN: u32 = 22;
const EMR_SETTEXTCOLOR: u32 = 24;
const EMR_SELECTOBJECT: u32 = 37;
const EMR_CREATEPEN: u32 = 38;
const EMR_CREATEBRUSHINDIRECT: u32 = 39;
const EMR_DELETEOBJECT: u32 = 40;
const EMR_ELLIPSE: u32 = 42;
const EMR_RECTANGLE: u32 = 43;
const EMR_EOF: u32 = 14;
const EMR_EXTCREATEFONTINDIRECTW: u32 = 82;
const EMR_EXTTEXTOUTW: u32 = 84;

const PEN_HANDLE: u32 = 1;
const BRUSH_HANDLE: u32 = 2;
const FONT_HANDLE: u32 = 3;
/// Stock objects (high bit set): NULL_BRUSH, NULL_PEN.
const STOCK_NULL_BRUSH: u32 = 0x8000_0005;
const STOCK_NULL_PEN: u32 = 0x8000_0008;

/// 0.01 mm per point.
const UNITS_PER_PT: f32 = 2540.0 / 72.0;

const TA_LEFT: u32 = 0;
const TA_RIGHT: u32 = 2;
const TA_CENTER: u32 = 6;
const TA_BASELINE: u32 = 24;

fn u(v: f32) -> i32 {
    if v.is_finite() {
        (v * UNITS_PER_PT).round().clamp(-1.0e9, 1.0e9) as i32
    } else {
        0
    }
}

fn colorref(c: Rgb) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c.0) | (b(c.1) << 8) | (b(c.2) << 16)
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    records: u32,
}

impl Writer {
    /// Appends one record: `body` is padded to 4 bytes.
    fn rec(&mut self, ty: u32, body: &[u8]) {
        let mut body = body.to_vec();
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
        let size = (body.len() + 8) as u32;
        self.bytes.extend_from_slice(&ty.to_le_bytes());
        self.bytes.extend_from_slice(&size.to_le_bytes());
        self.bytes.extend_from_slice(&body);
        self.records += 1;
    }
}

fn push32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn pushu(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

#[derive(Clone, Copy, PartialEq)]
struct PenState {
    width: i32,
    color: u32,
    style: u32,
}

#[derive(Clone, PartialEq)]
struct FontState {
    height: i32,
    escapement: i32,
    font: Font,
}

struct State {
    w: Writer,
    pen: Option<PenState>,
    brush: Option<Option<u32>>,
    font: Option<FontState>,
    text_color: Option<u32>,
    text_align: Option<u32>,
    dash: Option<(f32, f32)>,
    bounds: [i32; 4],
}

impl State {
    fn grow(&mut self, x: i32, y: i32) {
        let b = &mut self.bounds;
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }

    fn set_pen(&mut self, stroke: Option<(f32, Rgb)>) {
        let want = match stroke {
            Some((w, c)) => {
                // PS_SOLID = 0; PS_NULL = 5.
                PenState { width: u(w).max(1), color: colorref(c), style: 0 }
            }
            None => PenState { width: 0, color: 0, style: 5 },
        };
        if self.pen == Some(want) {
            return;
        }
        if self.pen.is_some() {
            self.w.rec(EMR_SELECTOBJECT, &STOCK_NULL_PEN.to_le_bytes());
            self.w.rec(EMR_DELETEOBJECT, &PEN_HANDLE.to_le_bytes());
        }
        // EMR_CREATEPEN: ihPen, LOGPEN { style, width (POINTL), color }.
        let mut b = Vec::new();
        pushu(&mut b, PEN_HANDLE);
        pushu(&mut b, want.style);
        push32(&mut b, want.width);
        push32(&mut b, 0);
        pushu(&mut b, want.color);
        self.w.rec(EMR_CREATEPEN, &b);
        self.w.rec(EMR_SELECTOBJECT, &PEN_HANDLE.to_le_bytes());
        self.pen = Some(want);
    }

    fn set_brush(&mut self, fill: Option<Rgb>) {
        let want = fill.map(colorref);
        if self.brush == Some(want) {
            return;
        }
        if matches!(self.brush, Some(Some(_))) {
            self.w.rec(EMR_SELECTOBJECT, &STOCK_NULL_BRUSH.to_le_bytes());
            self.w.rec(EMR_DELETEOBJECT, &BRUSH_HANDLE.to_le_bytes());
        }
        match want {
            Some(c) => {
                // EMR_CREATEBRUSHINDIRECT: ihBrush, LOGBRUSH32 { BS_SOLID, color, hatch }.
                let mut b = Vec::new();
                pushu(&mut b, BRUSH_HANDLE);
                pushu(&mut b, 0);
                pushu(&mut b, c);
                pushu(&mut b, 0);
                self.w.rec(EMR_CREATEBRUSHINDIRECT, &b);
                self.w.rec(EMR_SELECTOBJECT, &BRUSH_HANDLE.to_le_bytes());
            }
            None => self.w.rec(EMR_SELECTOBJECT, &STOCK_NULL_BRUSH.to_le_bytes()),
        }
        self.brush = Some(want);
    }

    fn set_font(&mut self, font: Font, size: f32, escapement: i32) {
        let want = FontState { height: -u(size).max(1), escapement, font };
        if self.font.as_ref() == Some(&want) {
            return;
        }
        if self.font.is_some() {
            // Stock object DEVICE_DEFAULT_FONT (0x8000000E) frees the handle for deletion.
            self.w.rec(EMR_SELECTOBJECT, &0x8000_000Eu32.to_le_bytes());
            self.w.rec(EMR_DELETEOBJECT, &FONT_HANDLE.to_le_bytes());
        }
        let (face, weight, italic, pitch) = match font {
            Font::Sans => ("Arial", 400, 0u8, 0x22u8),
            Font::SansBold => ("Arial", 700, 0, 0x22),
            Font::SansItalic => ("Arial", 400, 1, 0x22),
            Font::Mono => ("Courier New", 400, 0, 0x31),
        };
        // EMR_EXTCREATEFONTINDIRECTW: ihFont, LOGFONTW (92 bytes).
        let mut b = Vec::new();
        pushu(&mut b, FONT_HANDLE);
        push32(&mut b, want.height);
        push32(&mut b, 0); // width: default aspect
        push32(&mut b, escapement);
        push32(&mut b, escapement); // orientation
        push32(&mut b, weight);
        b.extend_from_slice(&[italic, 0, 0, 0, 0, 0, 4, pitch]); // italic, underline, strike, ANSI, outprec, clipprec, quality ANTIALIASED?, pitch
        let mut name: Vec<u16> = face.encode_utf16().collect();
        name.resize(32, 0);
        for c in name {
            b.extend_from_slice(&c.to_le_bytes());
        }
        self.w.rec(EMR_EXTCREATEFONTINDIRECTW, &b);
        self.w.rec(EMR_SELECTOBJECT, &FONT_HANDLE.to_le_bytes());
        self.font = Some(want);
    }

    fn set_text(&mut self, color: Rgb, align: u32) {
        let c = colorref(color);
        if self.text_color != Some(c) {
            self.w.rec(EMR_SETTEXTCOLOR, &c.to_le_bytes());
            self.text_color = Some(c);
        }
        if self.text_align != Some(align) {
            self.w.rec(EMR_SETTEXTALIGN, &align.to_le_bytes());
            self.text_align = Some(align);
        }
    }

    fn text(&mut self, x: f32, y: f32, size: f32, font: Font, align: Align, color: Rgb, text: &str, vertical: bool) {
        if text.is_empty() {
            return;
        }
        let ta = TA_BASELINE
            | if vertical {
                TA_CENTER
            } else {
                match align {
                    Align::Left => TA_LEFT,
                    Align::Center => TA_CENTER,
                    Align::Right => TA_RIGHT,
                }
            };
        self.set_font(font, size, if vertical { 900 } else { 0 });
        self.set_text(color, ta);
        let units: Vec<u16> = text.encode_utf16().collect();
        // Per-UTF-16-unit advances in logical units (the low surrogate gets zero).
        let mut dx: Vec<i32> = Vec::with_capacity(units.len());
        for ch in text.chars() {
            dx.push(u(text_width(&ch.to_string(), font, size)));
            if ch.len_utf16() == 2 {
                dx.push(0);
            }
        }
        let (rx, ry) = (u(x), u(y));
        let total = u(text_width(text, font, size));
        let (h, hh) = (u(size), u(size) / 4);
        // Conservative bounds of the text box.
        let (bx0, bx1) = match (vertical, align) {
            (true, _) => (rx - h, rx + hh),
            (false, Align::Left) => (rx, rx + total),
            (false, Align::Center) => (rx - total / 2, rx + total / 2),
            (false, Align::Right) => (rx - total, rx),
        };
        let (by0, by1) = if vertical { (ry - total / 2, ry + total / 2) } else { (ry - h, ry + hh) };
        self.grow(bx0, by0);
        self.grow(bx1, by1);
        let str_bytes = units.len() * 2;
        let str_padded = (str_bytes + 3) & !3;
        let off_string = 76u32; // 8 (header) + 68 (fixed body)
        let off_dx = off_string + str_padded as u32;
        let mut b = Vec::new();
        for v in [bx0, by0, bx1, by1] {
            push32(&mut b, v);
        }
        pushu(&mut b, 1); // GM_COMPATIBLE
        b.extend_from_slice(&1.0f32.to_le_bytes());
        b.extend_from_slice(&1.0f32.to_le_bytes());
        push32(&mut b, rx);
        push32(&mut b, ry);
        pushu(&mut b, units.len() as u32);
        pushu(&mut b, off_string);
        pushu(&mut b, 0); // options
        for v in [0, 0, 0, 0] {
            push32(&mut b, v); // rcl (unused without ETO_CLIPPED / ETO_OPAQUE)
        }
        pushu(&mut b, off_dx);
        for c in &units {
            b.extend_from_slice(&c.to_le_bytes());
        }
        while b.len() % 4 != 0 {
            b.push(0);
        }
        for d in dx {
            push32(&mut b, d);
        }
        self.w.rec(EMR_EXTTEXTOUTW, &b);
    }

    /// Emits a point-list record (POLYGON or POLYLINE).
    fn poly(&mut self, ty: u32, pts: &[(i32, i32)]) {
        let Some(first) = pts.first() else { return };
        let mut r = [first.0, first.1, first.0, first.1];
        for p in pts {
            r = [r[0].min(p.0), r[1].min(p.1), r[2].max(p.0), r[3].max(p.1)];
        }
        self.grow(r[0], r[1]);
        self.grow(r[2], r[3]);
        let mut b = Vec::new();
        for v in r {
            push32(&mut b, v);
        }
        pushu(&mut b, pts.len() as u32);
        for p in pts {
            push32(&mut b, p.0);
            push32(&mut b, p.1);
        }
        self.w.rec(ty, &b);
    }

    fn shape(&mut self, ty: u32, pts: &[(f32, f32)], fill: Option<Rgb>, stroke: Stroke) {
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.set_pen(stroke);
        self.set_brush(fill);
        let p: Vec<(i32, i32)> = pts.iter().map(|p| (u(p.0), u(p.1))).collect();
        self.poly(ty, &p);
    }

    fn boxed(&mut self, ty: u32, x0: f32, y0: f32, x1: f32, y1: f32, fill: Option<Rgb>, stroke: Stroke) {
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.set_pen(stroke);
        self.set_brush(fill);
        let r = [u(x0), u(y0), u(x1), u(y1)];
        self.grow(r[0], r[1]);
        self.grow(r[2], r[3]);
        let mut b = Vec::new();
        for v in r {
            push32(&mut b, v);
        }
        self.w.rec(ty, &b);
    }
}

/// Serialises `page` as an EMF file.
pub fn page_to_emf(page: &Page) -> Vec<u8> {
    let (fw, fh) = (u(page.width).max(1), u(page.height).max(1));
    let mut s = State { w: Writer::default(), pen: None, brush: None, font: None, text_color: None, text_align: None, dash: None, bounds: [i32::MAX, i32::MAX, i32::MIN, i32::MIN] };
    s.w.rec(EMR_SETBKMODE, &1u32.to_le_bytes()); // TRANSPARENT
    s.w.rec(EMR_SETPOLYFILLMODE, &1u32.to_le_bytes()); // ALTERNATE
    for op in page.ops() {
        match op {
            Op::Text { x, y, size, font, align, color, text } => s.text(*x, *y, *size, *font, *align, *color, text, false),
            Op::TextVertical { x, y, size, font, color, text } => s.text(*x, *y, *size, *font, Align::Center, *color, text, true),
            Op::Line { x1, y1, x2, y2, width, color } => {
                s.set_pen(Some((*width, *color)));
                // Dashes are emulated with explicit segments (built-in dashed pens only work for
                // one-pixel widths, which are invisible at 0.01 mm per pixel).
                let (dx, dy) = (x2 - x1, y2 - y1);
                let len = (dx * dx + dy * dy).sqrt();
                match s.dash {
                    Some((on, off)) if on > 0.0 && off > 0.0 && len.is_finite() && len / (on + off) < 5000.0 => {
                        let mut t = 0.0;
                        while t < len {
                            let t1 = (t + on).min(len);
                            let (a, b) = (t / len, t1 / len);
                            s.poly(EMR_POLYLINE, &[(u(x1 + dx * a), u(y1 + dy * a)), (u(x1 + dx * b), u(y1 + dy * b))]);
                            t += on + off;
                        }
                    }
                    _ => s.poly(EMR_POLYLINE, &[(u(*x1), u(*y1)), (u(*x2), u(*y2))]),
                }
            }
            Op::Dash(d) => s.dash = *d,
            Op::Polyline { pts, width, color } => {
                s.set_pen(Some((*width, *color)));
                let p: Vec<(i32, i32)> = pts.iter().map(|p| (u(p.0), u(p.1))).collect();
                s.poly(EMR_POLYLINE, &p);
            }
            Op::Polygon { pts, fill, stroke } => s.shape(EMR_POLYGON, pts, *fill, *stroke),
            Op::Rect { x, y, w, h, fill, stroke } => s.boxed(EMR_RECTANGLE, *x, *y, x + w, y + h, *fill, *stroke),
            Op::Circle { cx, cy, r, fill, stroke } => s.boxed(EMR_ELLIPSE, cx - r, cy - r, cx + r, cy + r, *fill, *stroke),
        }
    }
    // Release the GDI objects before the end of the file.
    if s.pen.is_some() {
        s.w.rec(EMR_SELECTOBJECT, &STOCK_NULL_PEN.to_le_bytes());
        s.w.rec(EMR_DELETEOBJECT, &PEN_HANDLE.to_le_bytes());
    }
    if matches!(s.brush, Some(Some(_))) {
        s.w.rec(EMR_SELECTOBJECT, &STOCK_NULL_BRUSH.to_le_bytes());
        s.w.rec(EMR_DELETEOBJECT, &BRUSH_HANDLE.to_le_bytes());
    }
    if s.font.is_some() {
        s.w.rec(EMR_SELECTOBJECT, &0x8000_000Eu32.to_le_bytes());
        s.w.rec(EMR_DELETEOBJECT, &FONT_HANDLE.to_le_bytes());
    }
    // EMR_EOF: nPalEntries, offPalEntries (16), nSizeLast (20).
    let mut eof = Vec::new();
    pushu(&mut eof, 0);
    pushu(&mut eof, 16);
    pushu(&mut eof, 20);
    s.w.rec(EMR_EOF, &eof);

    // The frame is the page; bounds cover what was drawn (clamped to the page).
    let mut bnd = s.bounds;
    if bnd[0] > bnd[2] {
        bnd = [0, 0, 0, 0];
    }
    let bnd = [bnd[0].max(0), bnd[1].max(0), bnd[2].min(fw - 1).max(0), bnd[3].min(fh - 1).max(0)];
    let description: Vec<u16> = "StormWAVES\0\0".encode_utf16().collect();
    let header_size = 100 + description.len() * 2;
    let total = header_size + s.w.bytes.len();
    let mut h = Vec::new();
    for v in bnd {
        push32(&mut h, v);
    }
    for v in [0, 0, fw, fh] {
        push32(&mut h, v);
    }
    pushu(&mut h, 0x464D_4520); // ENHMETA_SIGNATURE
    pushu(&mut h, 0x0001_0000);
    pushu(&mut h, total as u32);
    pushu(&mut h, s.w.records + 1);
    h.extend_from_slice(&4u16.to_le_bytes()); // handles: 0 reserved + pen, brush, font
    h.extend_from_slice(&0u16.to_le_bytes());
    pushu(&mut h, description.len() as u32);
    pushu(&mut h, 100); // offDescription
    pushu(&mut h, 0); // nPalEntries
    push32(&mut h, 100_000); // szlDevice (pixels = 0.01 mm)
    push32(&mut h, 100_000);
    push32(&mut h, 1000); // szlMillimeters
    push32(&mut h, 1000);
    pushu(&mut h, 0); // cbPixelFormat
    pushu(&mut h, 0); // offPixelFormat
    pushu(&mut h, 0); // bOpenGL
    for c in &description {
        h.extend_from_slice(&c.to_le_bytes());
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&EMR_HEADER.to_le_bytes());
    out.extend_from_slice(&(header_size as u32).to_le_bytes());
    out.extend_from_slice(&h);
    out.extend_from_slice(&s.w.bytes);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn rd(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    /// Splits a metafile into (type, offset, size) records.
    fn parse(b: &[u8]) -> Vec<(u32, usize, usize)> {
        let mut at = 0;
        let mut v = Vec::new();
        while at < b.len() {
            let (ty, size) = (rd(b, at), rd(b, at + 4) as usize);
            assert!(size >= 8 && size % 4 == 0, "record size {size}");
            assert!(at + size <= b.len());
            v.push((ty, at, size));
            at += size;
        }
        v
    }

    fn all_ops_page() -> Page {
        let mut p = Page::new(540.0, 324.0);
        p.text(10.0, 20.0, 10.0, Font::SansBold, Align::Left, Rgb::BLACK, "Left m\u{b2}");
        p.text(100.0, 20.0, 10.0, Font::Mono, Align::Center, Rgb(1.0, 0.0, 0.0), "Center");
        p.text(200.0, 20.0, 8.0, Font::SansItalic, Align::Right, Rgb::BLACK, "Right");
        p.text_vertical(15.0, 150.0, 8.5, Font::Sans, Rgb::BLACK, "Flow (cfs)");
        p.line(10.0, 30.0, 200.0, 30.0, 1.0, Rgb::BLACK);
        p.dashed(1.5, 1.5);
        p.line(10.0, 40.0, 200.0, 40.0, 0.4, Rgb(0.5, 0.5, 0.5));
        p.solid();
        p.polyline(&[(10.0, 50.0), (50.0, 90.0), (90.0, 60.0)], 1.3, Rgb(0.0, 0.0, 1.0));
        p.polygon(&[(100.0, 100.0), (150.0, 100.0), (125.0, 140.0)], Some(Rgb(0.8, 0.9, 1.0)), Some((1.0, Rgb::BLACK)));
        p.rect(200.0, 100.0, 40.0, 30.0, Some(Rgb::WHITE), None);
        p.rect(250.0, 100.0, 40.0, 30.0, None, Some((0.5, Rgb::BLACK)));
        p.circle(300.0, 200.0, 20.0, Some(Rgb(0.0, 1.0, 0.0)), Some((1.0, Rgb::BLACK)));
        p
    }

    #[test]
    fn header_and_records_are_consistent() {
        let b = page_to_emf(&all_ops_page());
        assert_eq!(rd(&b, 0), 1);
        assert_eq!(rd(&b, 40), 0x464D_4520, "signature");
        assert_eq!(rd(&b, 44), 0x10000, "version");
        assert_eq!(rd(&b, 48) as usize, b.len(), "bytes");
        let recs = parse(&b);
        assert_eq!(rd(&b, 52) as usize, recs.len(), "nRecords");
        assert_eq!(recs.iter().map(|r| r.2).sum::<usize>(), b.len());
        assert_eq!(recs.last().map(|r| r.0), Some(14), "EOF last");
        let hdr_size = rd(&b, 4) as usize;
        assert_eq!(hdr_size, 124);
        // Frame equals the page in 0.01 mm: 540 pt = 190.5 mm.
        assert_eq!(rd(&b, 24), 0);
        assert_eq!(rd(&b, 32), 19050);
        assert_eq!(rd(&b, 36), 11430);
        // Bounds are inside the frame.
        assert!(rd(&b, 16) <= 19050 && rd(&b, 20) <= 11430);
        // Description is UTF-16 "StormWAVES".
        let off = rd(&b, 64) as usize;
        assert_eq!(&b[off..off + 4], &[b'S', 0, b't', 0]);
    }

    #[test]
    fn every_op_type_round_trips() {
        let b = page_to_emf(&all_ops_page());
        let recs = parse(&b);
        let count = |ty: u32| recs.iter().filter(|r| r.0 == ty).count();
        assert_eq!(count(EMR_EXTTEXTOUTW), 4);
        assert!(count(EMR_POLYLINE) > 4); // solid line, dashed segments, polyline
        assert_eq!(count(EMR_POLYGON), 1);
        assert_eq!(count(EMR_RECTANGLE), 2);
        assert_eq!(count(EMR_ELLIPSE), 1);
        // Every created object is deleted.
        let creates = count(EMR_CREATEPEN) + count(EMR_CREATEBRUSHINDIRECT) + count(EMR_EXTCREATEFONTINDIRECTW);
        assert_eq!(creates, count(EMR_DELETEOBJECT));
        // Decode the first text record.
        let t = recs.iter().find(|r| r.0 == EMR_EXTTEXTOUTW).unwrap();
        let (at, size) = (t.1, t.2);
        let n = rd(&b, at + 44) as usize;
        let off = rd(&b, at + 48) as usize;
        let off_dx = rd(&b, at + 72) as usize;
        assert_eq!(n, 7);
        let chars: Vec<u16> = (0..n).map(|i| u16::from_le_bytes([b[at + off + 2 * i], b[at + off + 2 * i + 1]])).collect();
        assert_eq!(String::from_utf16(&chars).unwrap(), "Left m\u{b2}");
        assert_eq!(size, off_dx + 4 * n);
        // Reference point of the first text: (10 pt, 20 pt).
        assert_eq!(rd(&b, at + 36) as i32, u(10.0));
        assert_eq!(rd(&b, at + 40) as i32, u(20.0));
        // The vertical label uses a 90 degree escapement in its font record.
        let fonts: Vec<_> = recs.iter().filter(|r| r.0 == EMR_EXTCREATEFONTINDIRECTW).collect();
        assert!(fonts.iter().any(|f| rd(&b, f.1 + 8 + 12) == 900));
        assert!(fonts.iter().all(|f| f.2 == 104));
    }

    #[test]
    fn empty_page_is_valid() {
        let b = page_to_emf(&Page::new(100.0, 100.0));
        let recs = parse(&b);
        assert_eq!(rd(&b, 52) as usize, recs.len());
        assert_eq!(rd(&b, 48) as usize, b.len());
    }
}
