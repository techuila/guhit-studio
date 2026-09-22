//! Hand-written DXF fixtures. Every drawing here is written as ASCII DXF text
//! by the helpers below, so the tests say exactly what the file contains.

use super::*;

// ------------------------------------------------------------ DXF fixtures

struct Dxf {
    header: Vec<(String, i16)>,
    layers: Vec<(String, i16)>,
    entities: String,
    blocks: String,
}

impl Dxf {
    fn new() -> Self {
        Self {
            header: vec![],
            layers: vec![("0".into(), 7)],
            entities: String::new(),
            blocks: String::new(),
        }
    }

    fn insunits(mut self, code: i16) -> Self {
        self.header.push(("$INSUNITS".into(), code));
        self
    }

    fn layer(mut self, name: &str, color: i16) -> Self {
        self.layers.push((name.into(), color));
        self
    }

    fn line(mut self, layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        self.entities
            .push_str(&line_text(layer, x0, y0, x1, y1));
        self
    }

    /// Closed rectangle as an LWPOLYLINE.
    fn rect(mut self, layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        let mut s = String::from("  0\nLWPOLYLINE\n  8\n");
        s.push_str(layer);
        s.push_str("\n 90\n4\n 70\n1\n");
        for (x, y) in pts {
            s.push_str(&format!(" 10\n{x}\n 20\n{y}\n"));
        }
        self.entities.push_str(&s);
        self
    }

    fn arc(mut self, layer: &str, cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Self {
        self.entities.push_str(&format!(
            "  0\nARC\n  8\n{layer}\n 10\n{cx}\n 20\n{cy}\n 40\n{r}\n 50\n{a0}\n 51\n{a1}\n"
        ));
        self
    }

    fn block(mut self, name: &str, bx: f64, by: f64, body: &str) -> Self {
        self.blocks.push_str(&format!(
            "  0\nBLOCK\n  8\n0\n  2\n{name}\n 70\n0\n 10\n{bx}\n 20\n{by}\n  3\n{name}\n  1\n\n{body}  0\nENDBLK\n  8\n0\n"
        ));
        self
    }

    fn insert(mut self, layer: &str, name: &str, x: f64, y: f64, scale: f64, rot: f64) -> Self {
        self.entities.push_str(&format!(
            "  0\nINSERT\n  8\n{layer}\n  2\n{name}\n 10\n{x}\n 20\n{y}\n 41\n{scale}\n 42\n{scale}\n 43\n{scale}\n 50\n{rot}\n"
        ));
        self
    }

    fn build(&self) -> String {
        let mut s = String::from("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1009\n");
        for (name, value) in &self.header {
            s.push_str(&format!("  9\n{name}\n 70\n{value}\n"));
        }
        s.push_str("  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n 70\n");
        s.push_str(&format!("{}\n", self.layers.len()));
        for (name, color) in &self.layers {
            s.push_str(&format!("  0\nLAYER\n  2\n{name}\n 70\n0\n 62\n{color}\n  6\nCONTINUOUS\n"));
        }
        s.push_str("  0\nENDTAB\n  0\nENDSEC\n");
        s.push_str("  0\nSECTION\n  2\nBLOCKS\n");
        s.push_str(&self.blocks);
        s.push_str("  0\nENDSEC\n");
        s.push_str("  0\nSECTION\n  2\nENTITIES\n");
        s.push_str(&self.entities);
        s.push_str("  0\nENDSEC\n  0\nEOF\n");
        s
    }
}

fn line_text(layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    format!("  0\nLINE\n  8\n{layer}\n 10\n{x0}\n 20\n{y0}\n 11\n{x1}\n 21\n{y1}\n")
}

/// Two rooms side by side, drawn the way a CAD plan draws them: the outer
/// face of the building and the inner face of each room, 150 mm apart.
/// Centerlines: an 8000 x 5000 rectangle with a divider at x = 4000.
fn two_rooms(scale: f64) -> Dxf {
    let s = |v: f64| v * scale;
    Dxf::new()
        .layer("A-WALL", 7)
        .rect("A-WALL", s(-75.0), s(-75.0), s(8075.0), s(5075.0))
        .rect("A-WALL", s(75.0), s(75.0), s(3925.0), s(4925.0))
        .rect("A-WALL", s(4075.0), s(75.0), s(7925.0), s(4925.0))
}

fn options(mode: ImportMode, mm: f64) -> ImportOptions {
    ImportOptions {
        mm_per_unit: mm,
        layers: vec![],
        mode,
        offset: Point::default(),
        level_id: Some("level-1".to_string()),
    }
}

fn wall_commands(plan: &ImportPlan) -> Vec<(Point, Point, f64)> {
    let mut out = vec![];
    for c in &plan.commands {
        match c {
            Command::AddWall {
                start,
                end,
                thickness_mm,
                ..
            } => out.push((*start, *end, thickness_mm.unwrap_or(0.0))),
            Command::AddWallChain {
                points, thickness_mm, ..
            } => {
                for w in points.windows(2) {
                    out.push((w[0], w[1], thickness_mm.unwrap_or(0.0)));
                }
            }
            _ => {}
        }
    }
    out
}

fn linework(plan: &ImportPlan) -> Vec<Linework> {
    plan.commands
        .iter()
        .filter_map(|c| match c {
            Command::AddElement {
                element: Element::Linework(l),
            } => Some(l.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------- the tests

#[test]
fn two_rooms_become_seven_joined_walls() {
    let text = two_rooms(1.0).insunits(4).build();
    let inspection = inspect("plan.dxf", text.as_bytes()).unwrap();
    assert_eq!(inspection.declared_unit.as_deref(), Some("mm"));
    assert_eq!(inspection.suggested_mm_per_unit, 1.0);
    let layer = inspection.layers.iter().find(|l| l.name == "A-WALL").unwrap();
    assert_eq!(layer.detected_walls, 7, "the recognizer sees 7 walls on A-WALL");

    let plan = plan("plan.dxf", text.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert_eq!(plan.walls, 7);
    assert_eq!(plan.linework, 0, "every line was used by a wall");

    let walls = wall_commands(&plan);
    assert_eq!(walls.len(), 7);
    assert!(
        walls.iter().all(|(_, _, t)| (*t - 150.0).abs() < 1e-9),
        "every wall is 150 mm: {walls:?}"
    );

    // The corners meet exactly: every endpoint is shared with another wall.
    let mut counts: std::collections::BTreeMap<(i64, i64), u32> = Default::default();
    for (a, b, _) in &walls {
        for p in [a, b] {
            *counts
                .entry(((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64))
                .or_insert(0) += 1;
        }
    }
    assert!(counts.values().all(|n| *n >= 2), "no loose ends: {counts:?}");
    let nodes: Vec<(i64, i64)> = counts.keys().copied().collect();
    assert_eq!(
        nodes,
        vec![
            (0, 0),
            (0, 500_000),
            (400_000, 0),
            (400_000, 500_000),
            (800_000, 0),
            (800_000, 500_000)
        ],
        "the centerline grid is 0/4000/8000 by 0/5000"
    );
    // The batch is one undo step.
    match plan.batch("plan.dxf") {
        Command::Batch { label, commands } => {
            assert_eq!(label, "Import plan.dxf");
            assert_eq!(commands.len(), plan.commands.len());
        }
        other => panic!("expected a batch, got {other:?}"),
    }
}

#[test]
fn a_drawing_in_meters_is_recognized() {
    // Same plan drawn in meters, with no declared unit at all.
    let text = two_rooms(0.001).build();
    let inspection = inspect("meters.dxf", text.as_bytes()).unwrap();
    assert_eq!(inspection.declared_unit, None, "the file declares no unit");
    assert_eq!(
        inspection.suggested_mm_per_unit, 1000.0,
        "8.15 units wide reads as meters"
    );

    let plan = plan("meters.dxf", text.as_bytes(), &options(ImportMode::Walls, 1000.0)).unwrap();
    assert_eq!(plan.walls, 7);
    let walls = wall_commands(&plan);
    assert!(
        walls.iter().all(|(_, _, t)| (*t - 150.0).abs() < 1.0),
        "thickness comes out in mm: {:?}",
        walls.iter().map(|w| w.2).collect::<Vec<_>>()
    );
    let longest = walls
        .iter()
        .map(|(a, b, _)| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt())
        .fold(0.0f64, f64::max);
    assert!((longest - 5000.0).abs() < 1.0, "5 m wall, got {longest}");
}

#[test]
fn a_declared_unit_wins_over_the_bounding_box() {
    let text = two_rooms(0.001).insunits(6).build();
    let inspection = inspect("m.dxf", text.as_bytes()).unwrap();
    assert_eq!(inspection.declared_unit.as_deref(), Some("m"));
    assert_eq!(inspection.suggested_mm_per_unit, 1000.0);

    let feet = Dxf::new().insunits(2).line("0", 0.0, 0.0, 10.0, 0.0).build();
    let i = inspect("ft.dxf", feet.as_bytes()).unwrap();
    assert_eq!(i.declared_unit.as_deref(), Some("ft"));
    assert_eq!(i.suggested_mm_per_unit, 304.8);

    let inches = Dxf::new().insunits(1).line("0", 0.0, 0.0, 10.0, 0.0).build();
    assert_eq!(inspect("in.dxf", inches.as_bytes()).unwrap().suggested_mm_per_unit, 25.4);
}

#[test]
fn a_block_insert_is_expanded_with_its_transform() {
    // A 1000 x 500 rectangle drawn in a block, inserted twice: once at the
    // origin, once moved, scaled by 2 and turned a quarter turn.
    let body = format!(
        "{}{}{}{}",
        line_text("0", 0.0, 0.0, 1000.0, 0.0),
        line_text("0", 1000.0, 0.0, 1000.0, 500.0),
        line_text("0", 1000.0, 500.0, 0.0, 500.0),
        line_text("0", 0.0, 500.0, 0.0, 0.0),
    );
    let text = Dxf::new()
        .layer("FURN", 3)
        .block("DESK", 0.0, 0.0, &body)
        .insert("FURN", "DESK", 0.0, 0.0, 1.0, 0.0)
        .insert("FURN", "DESK", 5000.0, 2000.0, 2.0, 90.0)
        .insunits(4)
        .build();

    let plan = plan("blocks.dxf", text.as_bytes(), &options(ImportMode::Linework, 1.0)).unwrap();
    let work = linework(&plan);
    assert_eq!(work.len(), 1, "one Linework element per layer");
    assert_eq!(work[0].name, "blocks.dxf / FURN", "block geometry takes the insert's layer");
    assert_eq!(work[0].polylines.len(), 8, "4 lines per insert, twice");
    assert!(work[0].locked);
    assert_eq!(work[0].color, "#1f9d3a", "ACI 3 is green");

    let xs: Vec<f64> = work[0].polylines.iter().flatten().map(|p| p.x).collect();
    let ys: Vec<f64> = work[0].polylines.iter().flatten().map(|p| p.y).collect();
    let max_x = xs.iter().cloned().fold(f64::MIN, f64::max);
    let max_y = ys.iter().cloned().fold(f64::MIN, f64::max);
    // Second insert: 1000 x 500 scaled by 2 and rotated 90 degrees puts the
    // long side along +y from (5000, 2000).
    assert!((max_x - 5000.0).abs() < 1e-6, "rotated block ends at x = 5000, got {max_x}");
    assert!((max_y - 4000.0).abs() < 1e-6, "and reaches y = 4000, got {max_y}");
}

#[test]
fn arcs_are_tessellated_into_short_segments() {
    let text = Dxf::new()
        .insunits(4)
        .layer("SITE", 5)
        .arc("SITE", 0.0, 0.0, 1000.0, 0.0, 90.0)
        .build();
    let plan = plan("arc.dxf", text.as_bytes(), &options(ImportMode::Linework, 1.0)).unwrap();
    let work = linework(&plan);
    assert_eq!(work.len(), 1);
    let pts = &work[0].polylines[0];
    assert_eq!(pts.len(), 19, "90 degrees at 5 degree steps");
    for p in pts {
        let r = (p.x * p.x + p.y * p.y).sqrt();
        assert!((r - 1000.0).abs() < 1e-6, "every point is on the arc, got r = {r}");
    }
    for w in pts.windows(2) {
        let d = ((w[0].x - w[1].x).powi(2) + (w[0].y - w[1].y).powi(2)).sqrt();
        assert!(d < 100.0, "segments stay short, got {d}");
    }
}

#[test]
fn leftover_lines_become_locked_linework() {
    // Wall faces plus a stray line that pairs with nothing.
    let text = two_rooms(1.0)
        .insunits(4)
        .layer("A-ANNO", 2)
        .line("A-ANNO", 0.0, -2000.0, 8000.0, -2000.0)
        .build();
    let walls_plan = plan("mixed.dxf", text.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert_eq!(walls_plan.walls, 7);
    assert_eq!(walls_plan.linework, 1);
    let work = linework(&walls_plan);
    assert_eq!(work[0].name, "mixed.dxf / A-ANNO");
    assert_eq!(work[0].color, "#d6b800");
    assert!(work[0].locked);
    assert_eq!(work[0].level_id, "level-1");

    // Linework mode keeps everything and makes no walls at all.
    let all = plan("mixed.dxf", text.as_bytes(), &options(ImportMode::Linework, 1.0)).unwrap();
    assert_eq!(all.walls, 0);
    assert_eq!(all.linework, 2, "one element per layer");
}

#[test]
fn the_offset_moves_the_whole_import() {
    let text = two_rooms(1.0).insunits(4).build();
    let mut opts = options(ImportMode::Walls, 1.0);
    opts.offset = Point { x: -1000.0, y: 2000.0 };
    let plan = plan("plan.dxf", text.as_bytes(), &opts).unwrap();
    let walls = wall_commands(&plan);
    let min_x = walls.iter().map(|(a, b, _)| a.x.min(b.x)).fold(f64::MAX, f64::min);
    let min_y = walls.iter().map(|(a, b, _)| a.y.min(b.y)).fold(f64::MAX, f64::min);
    assert!((min_x + 1000.0).abs() < 1e-6);
    assert!((min_y - 2000.0).abs() < 1e-6);
}

#[test]
fn only_the_chosen_layers_come_in() {
    let text = two_rooms(1.0)
        .insunits(4)
        .layer("A-ANNO", 2)
        .line("A-ANNO", 0.0, -2000.0, 8000.0, -2000.0)
        .build();
    let mut opts = options(ImportMode::Walls, 1.0);
    opts.layers = vec!["A-WALL".into()];
    let plan = plan("plan.dxf", text.as_bytes(), &opts).unwrap();
    assert_eq!(plan.walls, 7);
    assert_eq!(plan.linework, 0, "A-ANNO was not picked");
}

#[test]
fn unsupported_entities_are_reported_not_dropped_silently() {
    let mut text = two_rooms(1.0).insunits(4).build();
    text = text.replace(
        "  0\nENDSEC\n  0\nEOF\n",
        "  0\nSPLINE\n  8\nA-WALL\n 70\n8\n  0\nENDSEC\n  0\nEOF\n",
    );
    let plan = plan("plan.dxf", text.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert!(
        plan.skipped.iter().any(|s| s.contains("SPLINE")),
        "the spline is reported: {:?}",
        plan.skipped
    );
}

#[test]
fn garbage_input_gives_a_clear_error() {
    let err = inspect("notes.txt", b"this is not a drawing, it is a shopping list").unwrap_err();
    assert!(matches!(err, ImportError::NotDxf(_)), "{err}");
    assert!(err.to_string().contains("not a DXF file"), "{err}");

    let pdf = inspect("plan.pdf", b"%PDF-1.7\n1 0 obj<</Type/Catalog>>endobj\n").unwrap_err();
    assert!(matches!(pdf, ImportError::NotDxf(_)), "{pdf}");

    // A file that looks like a DXF but is truncated fails in the parser.
    let broken = "  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n 10\nnot-a-number\n";
    let err = inspect("broken.dxf", broken.as_bytes()).unwrap_err();
    assert!(matches!(err, ImportError::Parse(_)), "{err}");
}

#[test]
fn a_file_over_ten_megabytes_is_refused() {
    let mut big = two_rooms(1.0).insunits(4).build().into_bytes();
    big.resize(MAX_BYTES + 1, b' ');
    let err = inspect("big.dxf", &big).unwrap_err();
    assert!(matches!(err, ImportError::TooLarge(_)), "{err}");
    assert!(err.to_string().contains("10 MB"), "{err}");
    assert!(plan("big.dxf", &big, &options(ImportMode::Walls, 1.0)).is_err());

    // One byte under the cap still works.
    let ok = two_rooms(1.0).insunits(4).build();
    assert!(ok.len() < MAX_BYTES);
    assert!(inspect("ok.dxf", ok.as_bytes()).is_ok());
}

#[test]
fn a_gap_too_wide_or_too_narrow_is_not_a_wall() {
    // 700 mm apart: over the 600 mm limit.
    let wide = Dxf::new()
        .insunits(4)
        .line("A-WALL", 0.0, 0.0, 5000.0, 0.0)
        .line("A-WALL", 0.0, 700.0, 5000.0, 700.0)
        .build();
    let p = plan("w.dxf", wide.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert_eq!(p.walls, 0);
    assert_eq!(p.linework, 1, "both lines stay as linework");

    // 40 mm apart: under the 50 mm limit.
    let thin = Dxf::new()
        .insunits(4)
        .line("A-WALL", 0.0, 0.0, 5000.0, 0.0)
        .line("A-WALL", 0.0, 40.0, 5000.0, 40.0)
        .build();
    assert_eq!(plan("t.dxf", thin.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap().walls, 0);

    // 150 mm apart but only 200 mm of overlap: under the 300 mm limit.
    let short = Dxf::new()
        .insunits(4)
        .line("A-WALL", 0.0, 0.0, 5000.0, 0.0)
        .line("A-WALL", 4800.0, 150.0, 9000.0, 150.0)
        .build();
    assert_eq!(plan("s.dxf", short.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap().walls, 0);

    // 150 mm apart with 4000 mm of overlap: a wall.
    let good = Dxf::new()
        .insunits(4)
        .line("A-WALL", 0.0, 0.0, 5000.0, 0.0)
        .line("A-WALL", 1000.0, 150.0, 9000.0, 150.0)
        .build();
    let p = plan("g.dxf", good.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert_eq!(p.walls, 1);
    let w = wall_commands(&p)[0];
    assert!((w.2 - 150.0).abs() < 1e-9);
    assert!((w.0.y - 75.0).abs() < 1e-9 && (w.1.y - 75.0).abs() < 1e-9, "centered between the faces");
    let lo = w.0.x.min(w.1.x);
    let hi = w.0.x.max(w.1.x);
    assert!((lo - 1000.0).abs() < 1e-9 && (hi - 5000.0).abs() < 1e-9, "trimmed to the overlap");
}

#[test]
fn thickness_is_rounded_to_five_millimeters() {
    let text = Dxf::new()
        .insunits(4)
        .line("A-WALL", 0.0, 0.0, 5000.0, 0.0)
        .line("A-WALL", 0.0, 152.0, 5000.0, 152.0)
        .build();
    let p = plan("r.dxf", text.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert_eq!(wall_commands(&p)[0].2, 150.0);
}

#[test]
fn an_empty_drawing_inspects_without_failing() {
    let text = Dxf::new().insunits(4).build();
    let i = inspect("empty.dxf", text.as_bytes()).unwrap();
    assert!(i.layers.is_empty(), "no layer carries anything");
    let p = plan("empty.dxf", text.as_bytes(), &options(ImportMode::Walls, 1.0)).unwrap();
    assert!(p.is_empty());
}
