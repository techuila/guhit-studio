//! Minimal ASCII DXF writer, R12 (AC1009) flavor: LINE, POLYLINE, ARC,
//! CIRCLE and TEXT only, no handles, no objects section. This is the most
//! widely readable DXF there is: AutoCAD, BricsCAD, LibreCAD and QCAD open it.
//!
//! Model space, millimeters, 1:1, y north (DXF is y-up like the model).

use crate::geom::*;
use crate::plan::{items_bounds, Cat, HAlign, Item, Prim};
use crate::text::est_width;

/// AutoCAD color index per layer. Dark, readable on white and on black.
fn layer_color(cat: Cat) -> u8 {
    match cat {
        Cat::Wall => 7,
        Cat::Door => 4,
        Cat::Glaz => 5,
        Cat::Cols => 7,
        Cat::Strs => 6,
        Cat::Furn => 8,
        Cat::Area => 3,
        Cat::Dims => 1,
        Cat::Text => 7,
    }
}

fn f(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0');
    let s = if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    };
    if s == "-0.0" {
        "0.0".into()
    } else {
        s
    }
}

/// DXF R12 text is plain ASCII. Other characters use the \U+XXXX escape
/// that current CAD programs understand.
pub fn dxf_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\u{00b2}' => out.push('2'),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c if (c as u32) <= 0xffff => out.push_str(&format!("\\U+{:04X}", c as u32)),
            _ => out.push('?'),
        }
    }
    out
}

struct Writer {
    s: String,
}

impl Writer {
    fn pair(&mut self, code: i32, value: &str) {
        self.s.push_str(&format!("{code:>3}\n{value}\n"));
    }

    fn num(&mut self, code: i32, value: f64) {
        self.pair(code, &f(value));
    }

    fn int(&mut self, code: i32, value: i64) {
        self.pair(code, &format!("{value:>6}"));
    }

    fn point(&mut self, base: i32, p: V) {
        self.num(base, p.x);
        self.num(base + 10, p.y);
        self.num(base + 20, 0.0);
    }
}

/// Write the primitives as a complete DXF document. `scale` only goes into
/// the header comment; geometry is 1:1.
pub fn write(items: &[Item], scale: u32) -> String {
    let mut w = Writer { s: String::new() };
    let bounds = items_bounds(items);
    let (min, max) = if bounds.is_empty() {
        (v(0.0, 0.0), v(0.0, 0.0))
    } else {
        (bounds.min, bounds.max)
    };

    w.pair(999, "Guhit Studio plan export");
    w.pair(
        999,
        &format!("Units: millimeters, model space 1:1, y is north. Text sized for plotting at 1:{scale}"),
    );

    // HEADER
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, "AC1009");
    w.pair(9, "$INSBASE");
    w.point(10, v(0.0, 0.0));
    w.pair(9, "$EXTMIN");
    w.point(10, min);
    w.pair(9, "$EXTMAX");
    w.point(10, max);
    w.pair(9, "$LIMMIN");
    w.num(10, min.x);
    w.num(20, min.y);
    w.pair(9, "$LIMMAX");
    w.num(10, max.x);
    w.num(20, max.y);
    w.pair(9, "$LUNITS");
    w.int(70, 2);
    w.pair(9, "$LUPREC");
    w.int(70, 2);
    w.pair(0, "ENDSEC");

    // TABLES
    w.pair(0, "SECTION");
    w.pair(2, "TABLES");

    w.pair(0, "TABLE");
    w.pair(2, "LTYPE");
    w.int(70, 1);
    w.pair(0, "LTYPE");
    w.pair(2, "CONTINUOUS");
    w.int(70, 0);
    w.pair(3, "Solid line");
    w.int(72, 65);
    w.int(73, 0);
    w.num(40, 0.0);
    w.pair(0, "ENDTAB");

    w.pair(0, "TABLE");
    w.pair(2, "LAYER");
    w.int(70, Cat::ALL.len() as i64 + 1);
    w.pair(0, "LAYER");
    w.pair(2, "0");
    w.int(70, 0);
    w.int(62, 7);
    w.pair(6, "CONTINUOUS");
    for cat in dxf_layer_order() {
        w.pair(0, "LAYER");
        w.pair(2, cat.dxf_layer());
        w.int(70, 0);
        w.int(62, layer_color(cat) as i64);
        w.pair(6, "CONTINUOUS");
    }
    w.pair(0, "ENDTAB");

    w.pair(0, "TABLE");
    w.pair(2, "STYLE");
    w.int(70, 1);
    w.pair(0, "STYLE");
    w.pair(2, "STANDARD");
    w.int(70, 0);
    w.num(40, 0.0);
    w.num(41, 1.0);
    w.num(50, 0.0);
    w.int(71, 0);
    w.num(42, 2.5 * scale as f64);
    w.pair(3, "txt");
    w.pair(4, "");
    w.pair(0, "ENDTAB");

    w.pair(0, "ENDSEC");

    // BLOCKS (none)
    w.pair(0, "SECTION");
    w.pair(2, "BLOCKS");
    w.pair(0, "ENDSEC");

    // ENTITIES
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    for cat in dxf_layer_order() {
        for item in items.iter().filter(|i| i.cat == cat) {
            entity(&mut w, item);
        }
    }
    w.pair(0, "ENDSEC");
    w.pair(0, "EOF");
    w.s
}

/// Layer table order, walls first.
pub fn dxf_layer_order() -> [Cat; 9] {
    [
        Cat::Wall,
        Cat::Door,
        Cat::Glaz,
        Cat::Cols,
        Cat::Strs,
        Cat::Furn,
        Cat::Area,
        Cat::Dims,
        Cat::Text,
    ]
}

fn entity(w: &mut Writer, item: &Item) {
    let layer = item.cat.dxf_layer();
    match &item.prim {
        Prim::Line { a, b } => {
            w.pair(0, "LINE");
            w.pair(8, layer);
            w.point(10, *a);
            w.point(11, *b);
        }
        Prim::Poly { pts, closed, .. } => {
            w.pair(0, "POLYLINE");
            w.pair(8, layer);
            w.int(66, 1);
            w.point(10, v(0.0, 0.0));
            w.int(70, if *closed { 1 } else { 0 });
            for p in pts {
                w.pair(0, "VERTEX");
                w.pair(8, layer);
                w.point(10, *p);
            }
            w.pair(0, "SEQEND");
            w.pair(8, layer);
        }
        Prim::Arc {
            c,
            r,
            start_deg,
            end_deg,
        } => {
            // DXF arcs run counter-clockwise from the start to the end angle,
            // the same convention as the primitive.
            w.pair(0, "ARC");
            w.pair(8, layer);
            w.point(10, *c);
            w.num(40, *r);
            w.num(50, norm_deg(*start_deg));
            w.num(51, norm_deg(*end_deg));
        }
        Prim::Circle { c, r, .. } => {
            w.pair(0, "CIRCLE");
            w.pair(8, layer);
            w.point(10, *c);
            w.num(40, *r);
        }
        Prim::Text {
            pos,
            height,
            rot_deg,
            align,
            text,
            bold,
        } => {
            let content = dxf_text(text);
            w.pair(0, "TEXT");
            w.pair(8, layer);
            match align {
                HAlign::Start => {
                    w.point(10, *pos);
                }
                HAlign::Middle => {
                    // First point is the left end of the baseline (estimate,
                    // CAD programs recompute it from the alignment point).
                    let half = est_width(text, *height, *bold) / 2.0;
                    w.point(10, *pos - dir(*rot_deg) * half);
                }
            }
            w.num(40, *height);
            w.pair(1, &content);
            if rot_deg.abs() > 1e-9 {
                w.num(50, norm_deg(*rot_deg));
            }
            w.pair(7, "STANDARD");
            if *align == HAlign::Middle {
                // 72 = 1: centered on the baseline, around the second point.
                w.int(72, 1);
                w.point(11, *pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_text() {
        assert_eq!(f(8000.0), "8000.0");
        assert_eq!(f(-75.5), "-75.5");
        assert_eq!(f(-0.00001), "0.0");
        assert_eq!(f(0.12345678), "0.1235");
        assert_eq!(dxf_text("28.52 m\u{00b2}"), "28.52 m2");
        assert_eq!(dxf_text("Ba\u{00f1}o"), "Ba\\U+00F1o");
    }
}
