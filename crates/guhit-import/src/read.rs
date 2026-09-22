//! DXF to flat polylines. ASCII and binary DXF, model space only.
//!
//! Entities read: LINE, LWPOLYLINE (bulges tessellated), POLYLINE, ARC,
//! CIRCLE and INSERT (the referenced block is expanded one level, with the
//! insert's scale, rotation and translation). Everything else is counted and
//! reported in `skipped`.

use std::collections::BTreeMap;

use dxf::entities::EntityType as T;
use dxf::enums::Units;
use guhit_model::Point;

use crate::ImportError;

/// Angular step for arcs and bulges. 5 degrees keeps a 3 m radius arc within
/// 0.3 mm of the true curve, which is far below anything a plan needs.
const ARC_STEP_DEG: f64 = 5.0;

/// A DXF layer, with its color already turned into "#rrggbb".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLayer {
    pub name: String,
    pub color: String,
}

/// One polyline in drawing units. Closed shapes repeat their first point.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPath {
    pub layer: usize,
    pub pts: Vec<Point>,
}

/// Everything the reader found, still in drawing units.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub layers: Vec<RawLayer>,
    pub paths: Vec<RawPath>,
    /// "mm", "m", "in", "ft", "cm", or None when the file declares no unit.
    pub declared_unit: Option<String>,
    declared_mm_per_unit: Option<f64>,
    pub min: Point,
    pub max: Point,
    pub skipped: Vec<String>,
}

impl Raw {
    /// mm per drawing unit the app suggests. The declared `$INSUNITS` wins.
    /// Without one, guess from the bounding box: a building is a few metres to
    /// a few hundred metres across, so
    /// - under 200 units wide it reads as meters (1000 mm per unit),
    /// - under 2000 units wide as centimeters (10),
    /// - otherwise as millimeters (1).
    ///
    /// Feet and inches are never guessed: their plausible ranges sit inside
    /// the centimeter range, so the user has to say. The dialog shows the
    /// suggestion and the user confirms it.
    pub fn suggested_mm_per_unit(&self) -> f64 {
        if let Some(mm) = self.declared_mm_per_unit {
            return mm;
        }
        let w = (self.max.x - self.min.x).max(self.max.y - self.min.y);
        if !w.is_finite() || w <= 0.0 {
            1.0
        } else if w < 200.0 {
            1000.0
        } else if w < 2000.0 {
            10.0
        } else {
            1.0
        }
    }

    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }
}

fn unit_of(u: Units) -> Option<(&'static str, f64)> {
    match u {
        Units::Millimeters => Some(("mm", 1.0)),
        Units::Centimeters => Some(("cm", 10.0)),
        Units::Meters => Some(("m", 1000.0)),
        Units::Inches => Some(("in", 25.4)),
        Units::Feet => Some(("ft", 304.8)),
        _ => None,
    }
}

/// AutoCAD color index to "#rrggbb". The first 9 indices and the grey ramp
/// cover every layer color a hand-drawn plan uses; anything else falls back to
/// a mid grey, which still reads on both canvas themes.
pub fn aci_color(index: i16) -> String {
    match index {
        1 => "#e02a2a",
        2 => "#d6b800",
        3 => "#1f9d3a",
        4 => "#00a8b8",
        5 => "#2b5fd9",
        6 => "#b03ad6",
        // 7 is white on black and black on white. The canvas is light.
        7 => "#333333",
        8 => "#6b6b6b",
        9 => "#a8a8a8",
        250 => "#4d4d4d",
        251 => "#666666",
        252 => "#808080",
        253 => "#999999",
        254 => "#b3b3b3",
        255 => "#cccccc",
        _ => "#7a7a7a",
    }
    .to_string()
}

/// Reject anything that is not a DXF before handing it to the parser, so a
/// PDF or a ZIP gives a plain message instead of a parser error.
fn sniff(bytes: &[u8]) -> Result<(), ImportError> {
    let not_dxf = || {
        ImportError::NotDxf(
            "this is not a DXF file. Save the drawing as DXF, or point the app at the ODA File Converter to open DWG"
                .to_string(),
        )
    };
    if bytes.len() < 16 {
        return Err(not_dxf());
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Ok(());
    }
    // ASCII DXF: a group code, then SECTION, within the first few KB.
    let head = &bytes[..bytes.len().min(8192)];
    let text = String::from_utf8_lossy(head);
    if text.contains("SECTION") && (text.contains("HEADER") || text.contains("ENTITIES") || text.contains("TABLES")) {
        Ok(())
    } else {
        Err(not_dxf())
    }
}

/// 2D affine transform used to place a block's entities.
#[derive(Debug, Clone, Copy)]
struct Xform {
    sx: f64,
    sy: f64,
    cos: f64,
    sin: f64,
    base: Point,
    at: Point,
}

impl Xform {
    fn identity() -> Self {
        Self {
            sx: 1.0,
            sy: 1.0,
            cos: 1.0,
            sin: 0.0,
            base: Point::default(),
            at: Point::default(),
        }
    }

    fn apply(&self, p: Point) -> Point {
        let x = (p.x - self.base.x) * self.sx;
        let y = (p.y - self.base.y) * self.sy;
        Point {
            x: self.at.x + x * self.cos - y * self.sin,
            y: self.at.y + x * self.sin + y * self.cos,
        }
    }
}

struct Reader {
    layers: Vec<RawLayer>,
    index: BTreeMap<String, usize>,
    paths: Vec<RawPath>,
    skipped: BTreeMap<String, u32>,
}

impl Reader {
    fn layer(&mut self, name: &str) -> usize {
        let name = if name.trim().is_empty() { "0" } else { name };
        if let Some(i) = self.index.get(name) {
            return *i;
        }
        let i = self.layers.len();
        self.layers.push(RawLayer {
            name: name.to_string(),
            color: aci_color(7),
        });
        self.index.insert(name.to_string(), i);
        i
    }

    fn push(&mut self, layer: usize, pts: Vec<Point>, xf: &Xform) {
        let pts: Vec<Point> = pts.into_iter().map(|p| xf.apply(p)).collect();
        // Drop repeated points, then anything that is not a real path.
        let mut out: Vec<Point> = Vec::with_capacity(pts.len());
        for p in pts {
            if !p.x.is_finite() || !p.y.is_finite() {
                continue;
            }
            match out.last() {
                Some(q) if (q.x - p.x).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9 => {}
                _ => out.push(p),
            }
        }
        if out.len() >= 2 {
            self.paths.push(RawPath { layer, pts: out });
        }
    }

    fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_string()).or_insert(0) += 1;
    }
}

fn tessellate_arc(center: Point, r: f64, start_deg: f64, end_deg: f64) -> Vec<Point> {
    let mut sweep = end_deg - start_deg;
    while sweep <= 0.0 {
        sweep += 360.0;
    }
    while sweep > 360.0 {
        sweep -= 360.0;
    }
    let steps = ((sweep / ARC_STEP_DEG).ceil() as usize).clamp(2, 720);
    (0..=steps)
        .map(|i| {
            let a = (start_deg + sweep * (i as f64) / (steps as f64)).to_radians();
            Point {
                x: center.x + r * a.cos(),
                y: center.y + r * a.sin(),
            }
        })
        .collect()
}

/// Points of the arc a LWPOLYLINE bulge describes between `a` and `b`,
/// excluding `a` and `b` themselves. `bulge` is tan(quarter of the sweep).
fn bulge_points(a: Point, b: Point, bulge: f64) -> Vec<Point> {
    let sweep = 4.0 * bulge.atan();
    let chord = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    if !sweep.is_finite() || sweep.abs() < 1e-9 || chord < 1e-9 {
        return vec![];
    }
    let r = chord / (2.0 * (sweep / 2.0).sin()).abs();
    if !r.is_finite() {
        return vec![];
    }
    // Center is on the perpendicular bisector, on the side the sign picks.
    let mid = Point {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
    };
    let h2 = r * r - (chord / 2.0).powi(2);
    let h = if h2 > 0.0 { h2.sqrt() } else { 0.0 };
    let nx = -(b.y - a.y) / chord;
    let ny = (b.x - a.x) / chord;
    let sign = if sweep.abs() > std::f64::consts::PI { 1.0 } else { -1.0 } * bulge.signum();
    let c = Point {
        x: mid.x + nx * h * sign,
        y: mid.y + ny * h * sign,
    };
    let a0 = (a.y - c.y).atan2(a.x - c.x);
    let steps = ((sweep.abs().to_degrees() / ARC_STEP_DEG).ceil() as usize).clamp(2, 720);
    (1..steps)
        .map(|i| {
            let t = a0 + sweep * (i as f64) / (steps as f64);
            Point {
                x: c.x + r * t.cos(),
                y: c.y + r * t.sin(),
            }
        })
        .collect()
}

fn pt(p: &dxf::Point) -> Point {
    Point { x: p.x, y: p.y }
}

/// Paths of one entity in its own coordinate system, or None when the entity
/// carries no plan geometry.
fn entity_paths(e: &dxf::entities::Entity) -> Option<Vec<Vec<Point>>> {
    match &e.specific {
        T::Line(l) => Some(vec![vec![pt(&l.p1), pt(&l.p2)]]),
        T::LwPolyline(pl) => {
            let n = pl.vertices.len();
            if n < 2 {
                return Some(vec![]);
            }
            let vertex = |i: usize| Point {
                x: pl.vertices[i].x,
                y: pl.vertices[i].y,
            };
            let mut pts = vec![vertex(0)];
            let last = if pl.is_closed() { n } else { n - 1 };
            for i in 0..last {
                let a = vertex(i);
                let b = vertex((i + 1) % n);
                let bulge = pl.vertices[i].bulge;
                if bulge.abs() > 1e-9 {
                    pts.extend(bulge_points(a, b, bulge));
                }
                pts.push(b);
            }
            Some(vec![pts])
        }
        T::Polyline(pl) => {
            let mut pts: Vec<Point> = pl.vertices().map(|v| pt(&v.location)).collect();
            if pts.len() < 2 {
                return Some(vec![]);
            }
            if pl.is_closed() {
                pts.push(pts[0]);
            }
            Some(vec![pts])
        }
        T::Arc(a) => Some(vec![tessellate_arc(pt(&a.center), a.radius, a.start_angle, a.end_angle)]),
        T::Circle(c) => Some(vec![tessellate_arc(pt(&c.center), c.radius, 0.0, 360.0)]),
        _ => None,
    }
}

fn entity_name(e: &dxf::entities::Entity) -> String {
    let debug = format!("{:?}", e.specific);
    debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("entity")
        .to_ascii_uppercase()
}

pub fn read(bytes: &[u8]) -> Result<Raw, ImportError> {
    sniff(bytes)?;
    let drawing = dxf::Drawing::load(&mut std::io::Cursor::new(bytes))
        .map_err(|e| ImportError::Parse(e.to_string()))?;

    let mut r = Reader {
        layers: vec![],
        index: BTreeMap::new(),
        paths: vec![],
        skipped: BTreeMap::new(),
    };
    // The layer table first, so layer order and colors come from the file.
    for layer in drawing.layers() {
        let i = r.layer(&layer.name);
        r.layers[i].color = aci_color(layer.color.index().map(|c| c as i16).unwrap_or(7));
    }

    let blocks: BTreeMap<&str, &dxf::Block> = drawing
        .blocks()
        .filter(|b| !b.is_in_paperspace)
        .map(|b| (b.name.as_str(), b))
        .collect();

    let identity = Xform::identity();
    for e in drawing.entities() {
        if e.common.is_in_paper_space {
            continue;
        }
        if let T::Insert(ins) = &e.specific {
            let Some(block) = blocks.get(ins.name.as_str()) else {
                r.skip(&format!("INSERT of missing block `{}`", ins.name));
                continue;
            };
            if ins.column_count > 1 || ins.row_count > 1 {
                r.skip(&format!("array INSERT of `{}` (only the first copy)", ins.name));
            }
            let xf = Xform {
                sx: if ins.x_scale_factor.is_finite() && ins.x_scale_factor != 0.0 {
                    ins.x_scale_factor
                } else {
                    1.0
                },
                sy: if ins.y_scale_factor.is_finite() && ins.y_scale_factor != 0.0 {
                    ins.y_scale_factor
                } else {
                    1.0
                },
                cos: ins.rotation.to_radians().cos(),
                sin: ins.rotation.to_radians().sin(),
                base: pt(&block.base_point),
                at: pt(&ins.location),
            };
            let insert_layer = e.common.layer.clone();
            for inner in &block.entities {
                // DXF rule: geometry drawn on layer 0 inside a block takes the
                // layer of the insert.
                let name = if inner.common.layer.trim().is_empty() || inner.common.layer == "0" {
                    insert_layer.clone()
                } else {
                    inner.common.layer.clone()
                };
                let layer = r.layer(&name);
                match entity_paths(inner) {
                    Some(paths) => {
                        for p in paths {
                            r.push(layer, p, &xf);
                        }
                    }
                    None => {
                        if matches!(inner.specific, T::Insert(_)) {
                            r.skip(&format!("nested INSERT inside block `{}`", block.name));
                        } else {
                            r.skip(&format!("{} inside a block", entity_name(inner)));
                        }
                    }
                }
            }
            continue;
        }
        let layer = r.layer(&e.common.layer);
        match entity_paths(e) {
            Some(paths) => {
                for p in paths {
                    r.push(layer, p, &identity);
                }
            }
            None => r.skip(&entity_name(e)),
        }
    }

    // Bounding box over everything that was read.
    let (mut min, mut max) = (
        Point {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        Point {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        },
    );
    for path in &r.paths {
        for p in &path.pts {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
    }
    if !min.x.is_finite() {
        min = Point::default();
        max = Point::default();
    }

    let units = unit_of(drawing.header.default_drawing_units);
    let skipped = r
        .skipped
        .iter()
        .map(|(what, n)| {
            if *n == 1 {
                format!("1 {what} was not imported")
            } else {
                format!("{n} {what} entities were not imported")
            }
        })
        .collect();

    Ok(Raw {
        layers: r.layers,
        paths: r.paths,
        declared_unit: units.map(|(name, _)| name.to_string()),
        declared_mm_per_unit: units.map(|(_, mm)| mm),
        min,
        max,
        skipped,
    })
}
