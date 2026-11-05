//! The DXF 2013 (AC1027) document and its writer.
//!
//! A `Doc` is plain data: layers, linetypes, dimension styles, blocks and
//! model space entities in model millimeters. `write` gives every object a
//! handle and an owner and writes the file AutoCAD 2013 and later, Revit,
//! BricsCAD, LibreCAD and ezdxf read: HEADER, an empty CLASSES section, the
//! nine symbol tables, BLOCKS, ENTITIES and an OBJECTS section with the
//! root dictionary, the group dictionary and the Model and Layout1 layouts.
//!
//! The file is UTF-8, as every DXF from AC1021 on, so text is written as
//! typed ("Parañaque", "m²"). Handles are numbered in writing order, so the
//! same document always gives the same bytes.

use crate::geom::*;

/// A linetype beyond ByBlock, ByLayer and CONTINUOUS: dashes positive, gaps
/// negative, a dot zero, in model mm.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineType {
    pub name: String,
    pub description: String,
    pub pattern: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layer {
    pub name: String,
    /// AutoCAD color index.
    pub color: i64,
    pub linetype: String,
    /// Hundredths of a mm, one of the standard weights.
    pub lineweight: i64,
}

/// A dimension style for one drawing scale.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DimStyle {
    pub name: String,
    /// 1:N. DIMSCALE, so the paper sizes plot right.
    pub scale: f64,
    /// Show meters with two decimals instead of whole millimeters.
    pub meters: bool,
}

/// An invisible attribute of a block, at its base point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttDef {
    pub tag: String,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Block {
    pub name: String,
    /// `*D1` style blocks a dimension owns.
    pub anonymous: bool,
    /// Geometry around the base point (0, 0). Entities on layer 0 take the
    /// layer of the insert.
    pub ents: Vec<Ent>,
    pub attdefs: Vec<AttDef>,
}

/// One entity. `color`, `linetype` and `lineweight` override the layer's
/// when set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ent {
    pub layer: String,
    pub linetype: Option<String>,
    /// ACI color; 0 is ByBlock.
    pub color: Option<i64>,
    /// Hundredths of a mm; -2 is ByBlock.
    pub lineweight: Option<i64>,
    pub kind: Kind,
}

impl Ent {
    pub fn on(layer: &str, kind: Kind) -> Ent {
        Ent {
            layer: layer.to_string(),
            linetype: None,
            color: None,
            lineweight: None,
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Line {
        a: V,
        b: V,
    },
    /// LWPOLYLINE.
    Poly {
        pts: Vec<V>,
        closed: bool,
    },
    /// Counter-clockwise from `start` to `end`, degrees.
    Arc {
        c: V,
        r: f64,
        start: f64,
        end: f64,
    },
    Circle {
        c: V,
        r: f64,
    },
    /// `pos` is the left end of the baseline, or its middle when `middle`.
    /// `width` is the estimated width, for the first point of centered text.
    Text {
        pos: V,
        height: f64,
        rot: f64,
        middle: bool,
        text: String,
        width: f64,
    },
    /// A solid fill. Each path is closed: vertices with the bulge of the
    /// edge that leaves them.
    Hatch {
        paths: Vec<Vec<(V, f64)>>,
    },
    Insert {
        block: String,
        at: V,
        sx: f64,
        sy: f64,
        rot: f64,
        /// Tag, value and text height of invisible attributes.
        attrs: Vec<(String, String, f64)>,
    },
    /// A rotated linear dimension (DIMENSION with AcDbRotatedDimension).
    /// `p1` and `p2` are the measured points, `line_at` the point of the
    /// dimension line at the second extension line, `angle` the direction
    /// it measures along. `block` holds the drawn picture.
    Dim {
        block: String,
        style: String,
        p1: V,
        p2: V,
        line_at: V,
        text_mid: V,
        angle: f64,
        measurement: f64,
        text: Option<String>,
    },
    Point {
        p: V,
    },
}

/// Everything one file holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Doc {
    /// 999 comment lines at the top.
    pub comments: Vec<String>,
    pub linetypes: Vec<LineType>,
    /// Every layer but `0`, in table order.
    pub layers: Vec<Layer>,
    pub dimstyles: Vec<DimStyle>,
    pub blocks: Vec<Block>,
    pub ents: Vec<Ent>,
    /// Drawing extents, for the header and the initial view.
    pub bounds: Bounds,
}

impl Doc {
    pub fn new() -> Doc {
        Doc {
            comments: Vec::new(),
            linetypes: Vec::new(),
            layers: Vec::new(),
            dimstyles: Vec::new(),
            blocks: Vec::new(),
            ents: Vec::new(),
            bounds: Bounds::empty(),
        }
    }
}

/// The standard lineweights, in hundredths of a mm. LAYER and entity
/// lineweights must be one of these.
pub(crate) const LINEWEIGHTS: [i64; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211,
];

/// The standard lineweight nearest to a pen width in paper mm.
pub(crate) fn lineweight_of(mm: f64) -> i64 {
    let want = if mm.is_finite() { mm * 100.0 } else { 25.0 };
    LINEWEIGHTS
        .iter()
        .copied()
        .min_by(|a, b| {
            (*a as f64 - want)
                .abs()
                .partial_cmp(&(*b as f64 - want).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(25)
}

/// Text as a DXF 2013 string value: one line, no control characters.
pub(crate) fn text_value(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

// ----------------------------------------------------------------- writer

struct W {
    s: String,
    next: u64,
}

impl W {
    fn handle(&mut self) -> String {
        self.next += 1;
        format!("{:X}", self.next)
    }

    fn pair(&mut self, code: i32, value: &str) {
        super::put(&mut self.s, code, value);
    }

    fn num(&mut self, code: i32, value: f64) {
        super::put(&mut self.s, code, &super::f(value));
    }

    fn int(&mut self, code: i32, value: i64) {
        super::put(&mut self.s, code, &value.to_string());
    }

    fn point(&mut self, base: i32, p: V) {
        self.num(base, p.x);
        self.num(base + 10, p.y);
        self.num(base + 20, 0.0);
    }

    fn point2(&mut self, base: i32, p: V) {
        self.num(base, p.x);
        self.num(base + 10, p.y);
    }

    fn table(&mut self, name: &str, count: usize) -> String {
        let h = self.handle();
        self.pair(0, "TABLE");
        self.pair(2, name);
        self.pair(5, &h);
        self.pair(330, "0");
        self.pair(100, "AcDbSymbolTable");
        self.int(70, count as i64);
        h
    }

    /// The start of a symbol table record: type, handle, owner, markers.
    fn record(&mut self, kind: &str, owner: &str, marker: &str, handle_code: i32) -> String {
        let h = self.handle();
        self.pair(0, kind);
        self.pair(handle_code, &h);
        self.pair(330, owner);
        self.pair(100, "AcDbSymbolTableRecord");
        self.pair(100, marker);
        h
    }

    /// The common part of an entity: type, handle, owner and AcDbEntity.
    fn entity_head(&mut self, kind: &str, owner: &str, e: &Ent) -> String {
        let h = self.handle();
        self.pair(0, kind);
        self.pair(5, &h);
        self.pair(330, owner);
        self.pair(100, "AcDbEntity");
        self.pair(8, &e.layer);
        if let Some(lt) = &e.linetype {
            self.pair(6, lt);
        }
        if let Some(c) = e.color {
            self.int(62, c);
        }
        if let Some(lw) = e.lineweight {
            self.int(370, lw);
        }
        h
    }
}

/// Handles of the objects every file has, fixed before the sections are
/// written so records can point forward.
struct Spine {
    root: String,
    groups: String,
    layouts: String,
    layout_model: String,
    layout_paper: String,
    model: String,
    paper: String,
    style: String,
}

/// Write the document as DXF 2013 text.
pub(crate) fn write(doc: &Doc) -> String {
    let mut w = W {
        s: String::new(),
        next: 0,
    };
    // Model, paper and style are filled in once their records are written.
    let mut spine = Spine {
        root: w.handle(),
        groups: w.handle(),
        layouts: w.handle(),
        layout_model: w.handle(),
        layout_paper: w.handle(),
        model: String::new(),
        paper: String::new(),
        style: String::new(),
    };
    let mut body = W {
        s: String::new(),
        next: w.next,
    };
    // Block record handles: model space, paper space, then each block.
    let mut block_records: Vec<String> = Vec::new();

    body.pair(0, "SECTION");
    body.pair(2, "CLASSES");
    body.pair(0, "ENDSEC");

    // ------------------------------------------------------------ TABLES
    body.pair(0, "SECTION");
    body.pair(2, "TABLES");

    // VPORT: one active viewport looking at the extents.
    let (min, max) = extents(doc);
    let t = body.table("VPORT", 1);
    body.record("VPORT", &t, "AcDbViewportTableRecord", 5);
    body.pair(2, "*Active");
    body.int(70, 0);
    body.point2(10, v(0.0, 0.0));
    body.point2(11, v(1.0, 1.0));
    body.point2(12, (min + max) * 0.5);
    body.point2(13, v(0.0, 0.0));
    body.point2(14, v(10.0, 10.0));
    body.point2(15, v(10.0, 10.0));
    body.num(16, 0.0);
    body.num(26, 0.0);
    body.num(36, 1.0);
    body.num(17, 0.0);
    body.num(27, 0.0);
    body.num(37, 0.0);
    let height = ((max.y - min.y).max((max.x - min.x) / 1.6) * 1.1).max(1000.0);
    body.num(40, height);
    body.num(41, 1.6);
    body.num(42, 50.0);
    body.num(43, 0.0);
    body.num(44, 0.0);
    body.num(50, 0.0);
    body.num(51, 0.0);
    for (code, value) in [(71, 0), (72, 1000), (73, 1), (74, 3), (75, 0), (76, 0), (77, 0), (78, 0), (281, 0), (65, 1)] {
        body.int(code, value);
    }
    body.num(146, 0.0);
    body.pair(0, "ENDTAB");

    // LTYPE: ByBlock, ByLayer, CONTINUOUS, then the dash styles.
    let t = body.table("LTYPE", 3 + doc.linetypes.len());
    for name in ["ByBlock", "ByLayer", "CONTINUOUS"] {
        body.record("LTYPE", &t, "AcDbLinetypeTableRecord", 5);
        body.pair(2, name);
        body.int(70, 0);
        body.pair(3, if name == "CONTINUOUS" { "Solid line" } else { "" });
        body.int(72, 65);
        body.int(73, 0);
        body.num(40, 0.0);
    }
    for lt in &doc.linetypes {
        body.record("LTYPE", &t, "AcDbLinetypeTableRecord", 5);
        body.pair(2, &lt.name);
        body.int(70, 0);
        body.pair(3, &lt.description);
        body.int(72, 65);
        body.int(73, lt.pattern.len() as i64);
        body.num(40, lt.pattern.iter().map(|x| x.abs()).sum::<f64>());
        for x in &lt.pattern {
            body.num(49, *x);
            body.int(74, 0);
        }
    }
    body.pair(0, "ENDTAB");

    // LAYER: 0, then the document's layers.
    let t = body.table("LAYER", 1 + doc.layers.len());
    let zero = Layer {
        name: "0".into(),
        color: 7,
        linetype: "CONTINUOUS".into(),
        lineweight: -3,
    };
    for l in std::iter::once(&zero).chain(doc.layers.iter()) {
        body.record("LAYER", &t, "AcDbLayerTableRecord", 5);
        body.pair(2, &l.name);
        body.int(70, 0);
        body.int(62, l.color);
        body.pair(6, &l.linetype);
        body.int(370, l.lineweight);
    }
    body.pair(0, "ENDTAB");

    // STYLE: Standard on a TrueType font that has every Latin letter.
    let t = body.table("STYLE", 1);
    spine.style = body.record("STYLE", &t, "AcDbTextStyleTableRecord", 5);
    body.pair(2, "Standard");
    body.int(70, 0);
    body.num(40, 0.0);
    body.num(41, 1.0);
    body.num(50, 0.0);
    body.int(71, 0);
    body.num(42, 2.5);
    body.pair(3, "arial.ttf");
    body.pair(4, "");
    body.pair(0, "ENDTAB");

    for name in ["VIEW", "UCS"] {
        body.table(name, 0);
        body.pair(0, "ENDTAB");
    }

    let t = body.table("APPID", 1);
    body.record("APPID", &t, "AcDbRegAppTableRecord", 5);
    body.pair(2, "ACAD");
    body.int(70, 0);
    body.pair(0, "ENDTAB");

    // DIMSTYLE: Standard, then one style per drawing scale.
    let t = body.table("DIMSTYLE", 1 + doc.dimstyles.len());
    body.pair(100, "AcDbDimStyleTable");
    let standard = DimStyle {
        name: "Standard".into(),
        scale: 1.0,
        meters: false,
    };
    for d in std::iter::once(&standard).chain(doc.dimstyles.iter()) {
        dimstyle(&mut body, &t, d, &spine.style);
    }
    body.pair(0, "ENDTAB");

    // BLOCK_RECORD: model space, paper space, then every block.
    let t = body.table("BLOCK_RECORD", 2 + doc.blocks.len());
    let names: Vec<(&str, String)> = [("*Model_Space", spine.layout_model.clone()), ("*Paper_Space", spine.layout_paper.clone())]
        .into_iter()
        .chain(doc.blocks.iter().map(|b| (b.name.as_str(), "0".to_string())))
        .collect();
    for (name, layout) in &names {
        let h = body.record("BLOCK_RECORD", &t, "AcDbBlockTableRecord", 5);
        body.pair(2, name);
        body.pair(340, layout);
        body.int(70, 0);
        body.int(280, 1);
        body.int(281, 0);
        block_records.push(h);
    }
    spine.model = block_records[0].clone();
    spine.paper = block_records[1].clone();
    body.pair(0, "ENDTAB");
    body.pair(0, "ENDSEC");

    // ------------------------------------------------------------ BLOCKS
    body.pair(0, "SECTION");
    body.pair(2, "BLOCKS");
    for (i, (name, _)) in names.iter().enumerate() {
        let br = block_records[i].clone();
        let paper = i == 1;
        let block = if i >= 2 { Some(&doc.blocks[i - 2]) } else { None };
        let h = body.handle();
        body.pair(0, "BLOCK");
        body.pair(5, &h);
        body.pair(330, &br);
        body.pair(100, "AcDbEntity");
        if paper {
            body.int(67, 1);
        }
        body.pair(8, "0");
        body.pair(100, "AcDbBlockBegin");
        body.pair(2, name);
        let mut flags = 0;
        if let Some(b) = block {
            if b.anonymous {
                flags |= 1;
            }
            if !b.attdefs.is_empty() {
                flags |= 2;
            }
        }
        body.int(70, flags);
        body.point(10, v(0.0, 0.0));
        body.pair(3, name);
        body.pair(1, "");
        if let Some(b) = block {
            for e in &b.ents {
                entity(&mut body, &br, e);
            }
            for a in &b.attdefs {
                let h = body.handle();
                body.pair(0, "ATTDEF");
                body.pair(5, &h);
                body.pair(330, &br);
                body.pair(100, "AcDbEntity");
                body.pair(8, "0");
                body.pair(100, "AcDbText");
                body.point(10, v(0.0, 0.0));
                body.num(40, a.height);
                body.pair(1, "");
                body.pair(7, "Standard");
                body.pair(100, "AcDbAttributeDefinition");
                body.int(280, 0);
                body.pair(3, &a.tag);
                body.pair(2, &a.tag);
                body.int(70, 1);
            }
        }
        let h = body.handle();
        body.pair(0, "ENDBLK");
        body.pair(5, &h);
        body.pair(330, &br);
        body.pair(100, "AcDbEntity");
        if paper {
            body.int(67, 1);
        }
        body.pair(8, "0");
        body.pair(100, "AcDbBlockEnd");
    }
    body.pair(0, "ENDSEC");

    // ---------------------------------------------------------- ENTITIES
    body.pair(0, "SECTION");
    body.pair(2, "ENTITIES");
    let model = spine.model.clone();
    for e in &doc.ents {
        entity(&mut body, &model, e);
    }
    body.pair(0, "ENDSEC");

    // ----------------------------------------------------------- OBJECTS
    body.pair(0, "SECTION");
    body.pair(2, "OBJECTS");
    body.pair(0, "DICTIONARY");
    body.pair(5, &spine.root);
    body.pair(330, "0");
    body.pair(100, "AcDbDictionary");
    body.int(281, 1);
    body.pair(3, "ACAD_GROUP");
    body.pair(350, &spine.groups);
    body.pair(3, "ACAD_LAYOUT");
    body.pair(350, &spine.layouts);
    body.pair(0, "DICTIONARY");
    body.pair(5, &spine.groups);
    body.pair(330, &spine.root);
    body.pair(100, "AcDbDictionary");
    body.int(281, 1);
    body.pair(0, "DICTIONARY");
    body.pair(5, &spine.layouts);
    body.pair(330, &spine.root);
    body.pair(100, "AcDbDictionary");
    body.int(281, 1);
    body.pair(3, "Layout1");
    body.pair(350, &spine.layout_paper);
    body.pair(3, "Model");
    body.pair(350, &spine.layout_model);
    layout(&mut body, &spine.layout_model, &spine.layouts, "Model", 0, &spine.model, min, max);
    layout(&mut body, &spine.layout_paper, &spine.layouts, "Layout1", 1, &spine.paper, min, max);
    body.pair(0, "ENDSEC");
    body.pair(0, "EOF");

    // ------------------------------------------------------------ HEADER
    // Written last: $HANDSEED is the next free handle.
    for c in &doc.comments {
        w.pair(999, &text_value(c));
    }
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, "AC1027");
    w.pair(9, "$DWGCODEPAGE");
    w.pair(3, "ANSI_1252");
    w.pair(9, "$INSBASE");
    w.point(10, v(0.0, 0.0));
    w.pair(9, "$EXTMIN");
    w.point(10, min);
    w.pair(9, "$EXTMAX");
    w.point(10, max);
    w.pair(9, "$LIMMIN");
    w.point2(10, min);
    w.pair(9, "$LIMMAX");
    w.point2(10, max);
    w.pair(9, "$LTSCALE");
    w.num(40, 1.0);
    w.pair(9, "$TEXTSTYLE");
    w.pair(7, "Standard");
    w.pair(9, "$CLAYER");
    w.pair(8, "0");
    w.pair(9, "$DIMSTYLE");
    w.pair(2, doc.dimstyles.first().map(|d| d.name.as_str()).unwrap_or("Standard"));
    w.pair(9, "$LUNITS");
    w.int(70, 2);
    w.pair(9, "$LUPREC");
    w.int(70, 2);
    w.pair(9, "$INSUNITS");
    w.int(70, 4);
    w.pair(9, "$MEASUREMENT");
    w.int(70, 1);
    w.pair(9, "$LWDISPLAY");
    w.int(290, 1);
    w.pair(9, "$PSLTSCALE");
    w.int(70, 1);
    w.pair(9, "$HANDSEED");
    w.pair(5, &format!("{:X}", body.next + 1));
    w.pair(0, "ENDSEC");
    w.s.push_str(&body.s);
    w.s
}

/// Extents of the drawing; the origin when it is empty.
fn extents(doc: &Doc) -> (V, V) {
    if doc.bounds.is_empty() || !doc.bounds.min.is_finite() || !doc.bounds.max.is_finite() {
        (v(0.0, 0.0), v(0.0, 0.0))
    } else {
        (doc.bounds.min, doc.bounds.max)
    }
}

/// A DIMSTYLE record. Paper sizes match the plan's dimensions (`plan::paper`):
/// 2.5 mm text 0.9 mm over the line, oblique ticks, extension lines 1.5 mm
/// clear of the object and 1.5 mm past the line, the line 1.2 mm past them.
fn dimstyle(w: &mut W, table: &str, d: &DimStyle, style: &str) {
    use crate::plan::paper;
    w.record("DIMSTYLE", table, "AcDbDimStyleTableRecord", 105);
    w.pair(2, &d.name);
    w.int(70, 0);
    w.num(40, d.scale);
    w.num(41, paper::DIM_TICK);
    w.num(42, paper::DIM_EXT_GAP);
    w.num(43, 3.75);
    w.num(44, paper::DIM_EXT_OVERSHOOT);
    w.num(45, 0.0);
    w.num(46, paper::DIM_LINE_OVERSHOOT);
    w.num(140, paper::DIM_TEXT);
    w.num(141, 0.0);
    w.num(142, paper::DIM_TICK / 2.0);
    w.num(144, if d.meters { 0.001 } else { 1.0 });
    w.num(147, paper::DIM_TEXT_GAP);
    // Text above the line, aligned with it.
    w.int(73, 0);
    w.int(74, 0);
    w.int(77, 1);
    w.int(78, if d.meters { 0 } else { 8 });
    w.int(271, if d.meters { 2 } else { 0 });
    w.int(277, 2);
    w.int(278, 46);
    w.int(279, 0);
    w.int(280, 0);
    w.int(289, 3);
    w.pair(340, style);
    w.int(371, -2);
    w.int(372, -2);
}

/// A LAYOUT object with its plot settings: A3 landscape, model or paper.
#[allow(clippy::too_many_arguments)]
fn layout(w: &mut W, handle: &str, owner: &str, name: &str, tab: i64, block_record: &str, min: V, max: V) {
    w.pair(0, "LAYOUT");
    w.pair(5, handle);
    w.pair(330, owner);
    w.pair(100, "AcDbPlotSettings");
    w.pair(1, "");
    w.pair(4, "ISO_A3_(420.00_x_297.00_MM)");
    w.pair(6, "");
    for (code, value) in [(40, 7.5), (41, 20.0), (42, 7.5), (43, 20.0), (44, 420.0), (45, 297.0), (46, 0.0), (47, 0.0), (48, 0.0), (49, 0.0), (140, 0.0), (141, 0.0), (142, 1.0), (143, 1.0)] {
        w.num(code, value);
    }
    w.int(70, if tab == 0 { 1024 } else { 0 });
    w.int(72, 1);
    w.int(73, 0);
    w.int(74, 5);
    w.pair(7, "");
    w.int(75, 16);
    w.int(76, 0);
    w.int(77, 2);
    w.int(78, 300);
    w.num(147, 1.0);
    w.num(148, 0.0);
    w.num(149, 0.0);
    w.pair(100, "AcDbLayout");
    w.pair(1, name);
    w.int(70, 1);
    w.int(71, tab);
    w.point2(10, v(0.0, 0.0));
    w.point2(11, v(420.0, 297.0));
    w.point(12, v(0.0, 0.0));
    w.point(14, min);
    w.point(15, max);
    w.num(146, 0.0);
    w.point(13, v(0.0, 0.0));
    w.num(16, 1.0);
    w.num(26, 0.0);
    w.num(36, 0.0);
    w.num(17, 0.0);
    w.num(27, 1.0);
    w.num(37, 0.0);
    w.int(76, 1);
    w.pair(330, block_record);
}

/// One entity owned by the block record `owner`.
fn entity(w: &mut W, owner: &str, e: &Ent) {
    match &e.kind {
        Kind::Line { a, b } => {
            w.entity_head("LINE", owner, e);
            w.pair(100, "AcDbLine");
            w.point(10, *a);
            w.point(11, *b);
        }
        Kind::Poly { pts, closed } => {
            w.entity_head("LWPOLYLINE", owner, e);
            w.pair(100, "AcDbPolyline");
            w.int(90, pts.len() as i64);
            // 128: the linetype runs on through the vertices.
            w.int(70, if *closed { 129 } else { 128 });
            w.num(43, 0.0);
            for p in pts {
                w.point2(10, *p);
            }
        }
        Kind::Arc { c, r, start, end } => {
            w.entity_head("ARC", owner, e);
            w.pair(100, "AcDbCircle");
            w.point(10, *c);
            w.num(40, *r);
            w.pair(100, "AcDbArc");
            w.num(50, norm_deg(*start));
            w.num(51, norm_deg(*end));
        }
        Kind::Circle { c, r } => {
            w.entity_head("CIRCLE", owner, e);
            w.pair(100, "AcDbCircle");
            w.point(10, *c);
            w.num(40, *r);
        }
        Kind::Text {
            pos,
            height,
            rot,
            middle,
            text,
            width,
        } => {
            w.entity_head("TEXT", owner, e);
            w.pair(100, "AcDbText");
            // The first point is the left end of the baseline; centered
            // text also gives its alignment point, which CAD programs use.
            let first = if *middle { *pos - dir(*rot) * (width / 2.0) } else { *pos };
            w.point(10, first);
            w.num(40, *height);
            w.pair(1, &text_value(text));
            if rot.abs() > 1e-9 {
                w.num(50, norm_deg(*rot));
            }
            w.pair(7, "Standard");
            if *middle {
                w.int(72, 1);
                w.point(11, *pos);
            }
            w.pair(100, "AcDbText");
        }
        Kind::Hatch { paths } => {
            w.entity_head("HATCH", owner, e);
            w.pair(100, "AcDbHatch");
            w.point(10, v(0.0, 0.0));
            w.num(210, 0.0);
            w.num(220, 0.0);
            w.num(230, 1.0);
            w.pair(2, "SOLID");
            w.int(70, 1);
            w.int(71, 0);
            w.int(91, paths.len() as i64);
            for path in paths {
                let bulges = path.iter().any(|(_, b)| b.abs() > 1e-12);
                // 2: a polyline path, 1: an outer boundary.
                w.int(92, 3);
                w.int(72, i64::from(bulges));
                w.int(73, 1);
                w.int(93, path.len() as i64);
                for (p, b) in path {
                    w.point2(10, *p);
                    if bulges {
                        w.num(42, *b);
                    }
                }
                w.int(97, 0);
            }
            w.int(75, 0);
            w.int(76, 1);
            w.int(98, 1);
            let seed = paths.first().and_then(|p| p.first()).map(|(p, _)| *p).unwrap_or(v(0.0, 0.0));
            w.point2(10, seed);
        }
        Kind::Insert {
            block,
            at,
            sx,
            sy,
            rot,
            attrs,
        } => {
            let h = w.entity_head("INSERT", owner, e);
            w.pair(100, "AcDbBlockReference");
            if !attrs.is_empty() {
                w.int(66, 1);
            }
            w.pair(2, block);
            w.point(10, *at);
            if (sx - 1.0).abs() > 1e-12 || (sy - 1.0).abs() > 1e-12 {
                w.num(41, *sx);
                w.num(42, *sy);
                w.num(43, 1.0);
            }
            if rot.abs() > 1e-9 {
                w.num(50, norm_deg(*rot));
            }
            if !attrs.is_empty() {
                for (tag, value, height) in attrs {
                    let a = w.handle();
                    w.pair(0, "ATTRIB");
                    w.pair(5, &a);
                    w.pair(330, &h);
                    w.pair(100, "AcDbEntity");
                    w.pair(8, &e.layer);
                    w.pair(100, "AcDbText");
                    w.point(10, *at);
                    w.num(40, *height);
                    w.pair(1, &text_value(value));
                    w.pair(7, "Standard");
                    w.pair(100, "AcDbAttribute");
                    w.int(280, 0);
                    w.pair(2, tag);
                    w.int(70, 1);
                }
                let s = w.handle();
                w.pair(0, "SEQEND");
                w.pair(5, &s);
                w.pair(330, &h);
                w.pair(100, "AcDbEntity");
                w.pair(8, &e.layer);
            }
        }
        Kind::Dim {
            block,
            style,
            p1,
            p2,
            line_at,
            text_mid,
            angle,
            measurement,
            text,
        } => {
            w.entity_head("DIMENSION", owner, e);
            w.pair(100, "AcDbDimension");
            w.int(280, 0);
            w.pair(2, block);
            w.point(10, *line_at);
            w.point(11, *text_mid);
            // 0: rotated linear, 32: the block belongs to this dimension.
            w.int(70, 32);
            w.int(71, 5);
            w.num(42, *measurement);
            if let Some(t) = text {
                w.pair(1, &text_value(t));
            }
            w.pair(3, style);
            w.pair(100, "AcDbAlignedDimension");
            w.point(13, *p1);
            w.point(14, *p2);
            w.num(50, norm_deg(*angle));
            w.pair(100, "AcDbRotatedDimension");
        }
        Kind::Point { p } => {
            w.entity_head("POINT", owner, e);
            w.pair(100, "AcDbPoint");
            w.point(10, *p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Objects as (type, pairs), in file order.
    fn objects(text: &str) -> Vec<(String, Vec<(i32, String)>)> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
        for c in lines.chunks(2) {
            let code: i32 = c[0].trim().parse().unwrap();
            if code == 0 {
                out.push((c[1].to_string(), Vec::new()));
            } else if let Some(last) = out.last_mut() {
                last.1.push((code, c[1].to_string()));
            }
        }
        out
    }

    fn get(o: &[(i32, String)], code: i32) -> &str {
        o.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    fn sample() -> Doc {
        let mut doc = Doc::new();
        doc.layers.push(Layer { name: "A-DOOR".into(), color: 4, linetype: "CONTINUOUS".into(), lineweight: 25 });
        doc.blocks.push(Block {
            name: "D1".into(),
            anonymous: false,
            ents: vec![Ent::on("0", Kind::Arc { c: v(-400.0, 0.0), r: 800.0, start: 0.0, end: 90.0 })],
            attdefs: vec![AttDef { tag: "MARK".into(), height: 250.0 }],
        });
        doc.blocks.push(Block {
            name: "*D1".into(),
            anonymous: true,
            ents: vec![Ent::on("0", Kind::Line { a: v(0.0, 0.0), b: v(1.0, 0.0) })],
            attdefs: vec![],
        });
        doc.ents.push(Ent::on(
            "A-DOOR",
            Kind::Insert {
                block: "D1".into(),
                at: v(1000.0, 0.0),
                sx: -1.0,
                sy: 1.0,
                rot: 90.0,
                attrs: vec![("MARK".into(), "D1".into(), 250.0)],
            },
        ));
        doc.bounds.add(v(0.0, 0.0));
        doc.bounds.add(v(2000.0, 1000.0));
        doc
    }

    #[test]
    fn the_block_table_holds_every_block_once_with_its_owner() {
        let text = write(&sample());
        let all = objects(&text);
        let table = all.iter().find(|(t, o)| t == "TABLE" && get(o, 2) == "BLOCK_RECORD").unwrap();
        assert_eq!(get(&table.1, 70), "4");
        let records: Vec<&(String, Vec<(i32, String)>)> = all.iter().filter(|(t, _)| t == "BLOCK_RECORD").collect();
        let names: Vec<&str> = records.iter().map(|(_, o)| get(o, 2)).collect();
        assert_eq!(names, ["*Model_Space", "*Paper_Space", "D1", "*D1"]);
        for (_, r) in &records {
            assert_eq!(get(r, 330), get(&table.1, 5), "records belong to the table");
        }
        // Each BLOCK and ENDBLK belongs to its record; the flags say
        // anonymous and attributes.
        let blocks: Vec<&(String, Vec<(i32, String)>)> = all.iter().filter(|(t, _)| t == "BLOCK").collect();
        assert_eq!(blocks.len(), 4);
        for ((_, b), (_, r)) in blocks.iter().zip(&records) {
            assert_eq!(get(b, 330), get(r, 5));
            assert_eq!(get(b, 2), get(r, 2));
        }
        assert_eq!(get(&blocks[2].1, 70), "2");
        assert_eq!(get(&blocks[3].1, 70), "1");
        let ends: Vec<&(String, Vec<(i32, String)>)> = all.iter().filter(|(t, _)| t == "ENDBLK").collect();
        for ((_, e), (_, r)) in ends.iter().zip(&records) {
            assert_eq!(get(e, 330), get(r, 5));
        }
        // Model and paper space point at their layouts and back.
        let layouts: Vec<&(String, Vec<(i32, String)>)> = all.iter().filter(|(t, _)| t == "LAYOUT").collect();
        assert_eq!(layouts.len(), 2);
        for ((_, r), (_, l)) in records.iter().zip(&layouts) {
            assert_eq!(get(r, 340), get(l, 5));
            assert_eq!(l.iter().rev().find(|(c, _)| *c == 330).unwrap().1, get(r, 5));
        }
        assert_eq!(get(&records[2].1, 340), "0");
        // The insert, its attribute and its SEQEND hang off model space.
        let insert = all.iter().find(|(t, _)| t == "INSERT").unwrap();
        assert_eq!(get(&insert.1, 330), get(&records[0].1, 5));
        let attrib = all.iter().find(|(t, _)| t == "ATTRIB").unwrap();
        assert_eq!(get(&attrib.1, 330), get(&insert.1, 5));

        let d = dxf::Drawing::load(&mut text.as_bytes()).unwrap();
        let block = d.blocks().find(|b| b.name == "D1").unwrap();
        assert!(block.entities.iter().any(|e| matches!(e.specific, dxf::entities::EntityType::Arc(_))));
        assert!(block.entities.iter().any(|e| matches!(&e.specific, dxf::entities::EntityType::AttributeDefinition(a) if a.text_tag == "MARK")));
        let ins = d
            .entities()
            .find_map(|e| match &e.specific {
                dxf::entities::EntityType::Insert(i) => Some(i.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!((ins.name.as_str(), ins.x_scale_factor, ins.y_scale_factor, ins.rotation), ("D1", -1.0, 1.0, 90.0));
        let values: Vec<(String, String)> = ins.attributes().map(|a| (a.attribute_tag.clone(), a.value.clone())).collect();
        assert_eq!(values, [("MARK".to_string(), "D1".to_string())]);
    }

    #[test]
    fn lineweights_snap_to_the_standard_set() {
        assert_eq!(lineweight_of(0.35), 35);
        assert_eq!(lineweight_of(0.25), 25);
        assert_eq!(lineweight_of(0.18), 18);
        assert_eq!(lineweight_of(0.13), 13);
        assert_eq!(lineweight_of(0.36), 35);
        assert_eq!(lineweight_of(0.52), 53);
        assert_eq!(lineweight_of(9.0), 211);
        assert_eq!(text_value("Para\u{f1}aque\nCity"), "Para\u{f1}aque City");
    }
}
