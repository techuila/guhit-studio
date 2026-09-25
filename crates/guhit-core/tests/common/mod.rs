//! Shared helpers for the engine integration tests.
#![allow(dead_code)]

use guhit_core::{CoreError, Document};
use guhit_model::*;

pub fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

pub const LEVEL: &str = "level-1";

/// A blank project with fixed ids, so every test run is identical.
pub fn blank_project() -> Project {
    let mut project = defaults::new_project("Test");
    project.id = "project-1".into();
    project.levels[0].id = LEVEL.into();
    project.created_at = "2026-01-01T00:00:00Z".into();
    project.updated_at = "2026-01-01T00:00:00Z".into();
    project
}

pub fn blank() -> Document {
    Document::new(blank_project())
}

pub fn add_wall(a: Point, b: Point) -> Command {
    Command::AddWall {
        start: a,
        end: b,
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: None,
    }
}

pub fn rect_room(x: f64, y: f64, w: f64, d: f64, name: Option<&str>) -> Command {
    Command::AddRectRoom {
        origin: p(x, y),
        width_mm: w,
        depth_mm: d,
        name: name.map(|s| s.to_string()),
        thickness_mm: None,
        level_id: None,
    }
}

pub fn opening(wall_id: &str, ty: OpeningType, offset: f64) -> Command {
    Command::AddOpening {
        wall_id: wall_id.into(),
        opening_type: ty,
        offset_mm: offset,
        width_mm: None,
        height_mm: None,
        sill_mm: None,
        style: None,
        flip_side: None,
        flip_hinge: None,
    }
}

pub fn walls(project: &Project) -> Vec<&Wall> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(w),
            _ => None,
        })
        .collect()
}

pub fn rooms(project: &Project) -> Vec<&Room> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        })
        .collect()
}

pub fn openings(project: &Project) -> Vec<&Opening> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) => Some(o),
            _ => None,
        })
        .collect()
}

pub fn dimensions(project: &Project) -> Vec<&Dimension> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dimension(d) => Some(d),
            _ => None,
        })
        .collect()
}

pub fn dimension<'a>(project: &'a Project, id: &str) -> &'a Dimension {
    dimensions(project)
        .into_iter()
        .find(|d| d.id == id)
        .unwrap_or_else(|| panic!("no dimension {id}"))
}

/// What a linear dimension reads: the distance between its two points.
pub fn dimension_reads(project: &Project, id: &str) -> f64 {
    let d = dimension(project, id);
    ((d.b.x - d.a.x).powi(2) + (d.b.y - d.a.y).powi(2)).sqrt()
}

pub fn wall<'a>(project: &'a Project, id: &str) -> &'a Wall {
    walls(project)
        .into_iter()
        .find(|w| w.id == id)
        .expect("wall exists")
}

pub fn room_named<'a>(project: &'a Project, name: &str) -> &'a Room {
    rooms(project)
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no room named {name}"))
}

pub fn room_geo<'a>(state: &'a DocState, room_id: &str) -> &'a RoomGeometry {
    state
        .derived
        .rooms
        .iter()
        .find(|g| g.room_id == room_id)
        .expect("room has geometry")
}

/// The wall of a rectangular room on one side, found by its midpoint.
pub fn wall_at(project: &Project, mid: Point) -> &Wall {
    walls(project)
        .into_iter()
        .find(|w| {
            ((w.start.x + w.end.x) / 2.0 - mid.x).abs() < 1.0
                && ((w.start.y + w.end.y) / 2.0 - mid.y).abs() < 1.0
        })
        .unwrap_or_else(|| panic!("no wall with midpoint {mid:?}"))
}

pub fn bbox(poly: &[Point]) -> (f64, f64, f64, f64) {
    let xs = poly.iter().map(|p| p.x);
    let ys = poly.iter().map(|p| p.y);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

pub fn code(e: &CoreError) -> String {
    e.code().to_string()
}

pub fn error_ids(e: &CoreError) -> Vec<Id> {
    match e {
        CoreError::Invalid { element_ids, .. } => element_ids.clone(),
        CoreError::NotFound(id) => vec![id.clone()],
        _ => vec![],
    }
}

/// Preview, then apply, and require both to agree on elements, derived data
/// and diff. Returns the applied result.
pub fn apply_checked(doc: &mut Document, cmd: Command) -> ApplyResult {
    let preview = doc
        .preview(&cmd)
        .unwrap_or_else(|e| panic!("preview failed for {cmd:?}: {e}"));
    let applied = doc
        .apply(cmd.clone(), Origin::User)
        .unwrap_or_else(|e| panic!("apply failed for {cmd:?}: {e}"));
    assert_eq!(
        preview.state.project.elements, applied.state.project.elements,
        "elements differ for {cmd:?}"
    );
    assert_eq!(
        preview.state.project.materials,
        applied.state.project.materials
    );
    assert_eq!(
        preview.state.derived, applied.state.derived,
        "derived differs for {cmd:?}"
    );
    assert_eq!(preview.diff, applied.diff, "diff differs for {cmd:?}");
    assert_eq!(preview.state.revision, applied.state.revision);
    assert_invariants(&applied.state);
    applied
}

/// Apply a command that must fail, and require the document to be untouched.
pub fn apply_rejected(doc: &mut Document, cmd: Command) -> CoreError {
    let before = serde_json::to_string(&doc.state()).unwrap();
    assert!(
        doc.preview(&cmd).is_err(),
        "preview should fail for {cmd:?}"
    );
    let err = doc
        .apply(cmd.clone(), Origin::User)
        .expect_err("command should be rejected");
    assert_eq!(
        before,
        serde_json::to_string(&doc.state()).unwrap(),
        "failed command changed the document: {cmd:?}"
    );
    err
}

// ------------------------------------------------------------ invariants

fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn segs_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (o1, o2, o3, o4) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    (o1 * o2 < 0.0) && (o3 * o4 < 0.0)
}

pub fn area(poly: &[Point]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y)
        .sum::<f64>()
        / 2.0
}

/// Independent brute-force check: no two non-adjacent edges cross, and no
/// vertex repeats.
pub fn is_simple(poly: &[Point]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if (poly[i].x - poly[j].x).abs() < 1e-9 && (poly[i].y - poly[j].y).abs() < 1e-9 {
                return false;
            }
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segs_cross(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

pub fn inside(pt: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    let mut c = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < a.x + (pt.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
        j = i;
    }
    c
}

/// Everything that must hold for any document state the engine produces.
pub fn assert_invariants(state: &DocState) {
    let project = &state.project;
    let derived = &state.derived;

    // No NaN or infinity anywhere: serde_json writes those as null, which
    // does not read back as a number.
    let json = serde_json::to_string(state).expect("state serializes");
    let back: DocState = serde_json::from_str(&json).expect("state has a non-finite number");
    assert_eq!(back.project.elements.len(), project.elements.len());

    // Unique ids.
    let mut ids: Vec<&Id> = project.elements.iter().map(|e| e.id()).collect();
    ids.sort();
    let count = ids.len();
    ids.dedup();
    assert_eq!(count, ids.len(), "duplicate element ids");

    // Walls.
    for w in walls(project) {
        let len = ((w.end.x - w.start.x).powi(2) + (w.end.y - w.start.y).powi(2)).sqrt();
        assert!(len >= 50.0 - 1e-6, "wall {} is {len} mm long", w.id);
        assert!((50.0..=1000.0).contains(&w.thickness_mm));
        assert!(project.levels.iter().any(|l| l.id == w.level_id));
        let g = derived
            .walls
            .iter()
            .find(|g| g.wall_id == w.id)
            .expect("wall has geometry");
        assert!((g.length_mm - len).abs() < 1e-6);
        assert!(g.outline.len() >= 4, "wall {} has no outline", w.id);
        assert!(
            area(&g.outline) > 0.0,
            "wall outline is not counter-clockwise"
        );
    }
    assert_eq!(derived.walls.len(), walls(project).len());

    // Openings: host exists, fits with 50 mm clearance, no overlaps, below the wall top.
    for o in openings(project) {
        let host = walls(project)
            .into_iter()
            .find(|w| w.id == o.wall_id)
            .unwrap_or_else(|| panic!("opening {} has no host", o.id));
        let len =
            ((host.end.x - host.start.x).powi(2) + (host.end.y - host.start.y).powi(2)).sqrt();
        assert!(
            o.offset_mm - o.width_mm / 2.0 >= 50.0 - 1e-5,
            "opening {} too close to wall start",
            o.id
        );
        assert!(
            o.offset_mm + o.width_mm / 2.0 <= len - 50.0 + 1e-5,
            "opening {} too close to wall end",
            o.id
        );
        let height = host.height_mm.unwrap_or_else(|| {
            project
                .levels
                .iter()
                .find(|l| l.id == host.level_id)
                .unwrap()
                .height_mm
        });
        assert!(o.sill_mm + o.height_mm <= height + 1e-5);
        for other in openings(project) {
            if other.id != o.id && other.wall_id == o.wall_id {
                let gap =
                    (other.offset_mm - o.offset_mm).abs() - (other.width_mm + o.width_mm) / 2.0;
                assert!(gap >= -1e-5, "openings {} and {} overlap", o.id, other.id);
            }
        }
    }

    // Dimensions: two distinct points, whatever dragged them.
    for d in dimensions(project) {
        let len = ((d.b.x - d.a.x).powi(2) + (d.b.y - d.a.y).powi(2)).sqrt();
        assert!(len >= 1.0, "dimension {} has no length: {len}", d.id);
        assert!(project.levels.iter().any(|l| l.id == d.level_id));
    }

    // Rooms: every room owns a face, and only one room per face.
    assert_eq!(
        derived.rooms.len(),
        rooms(project).len(),
        "a room has no closed face"
    );
    let mut auto_names: Vec<&str> = vec![];
    for r in rooms(project) {
        let g = room_geo(state, &r.id);
        assert!(g.polygon.len() >= 3);
        assert!(
            is_simple(&g.polygon),
            "room {} polygon is not simple: {:?}",
            r.name,
            g.polygon
        );
        assert!(
            area(&g.polygon) > 0.0,
            "room polygon is not counter-clockwise"
        );
        assert!(
            area(&g.centerline_polygon) > 0.0,
            "centerline polygon is not counter-clockwise"
        );
        assert!(g.area_mm2 > 0.0);
        assert!(
            g.area_mm2 <= area(&g.polygon) + 1e-3,
            "net area is more than its polygon"
        );
        assert!(
            g.area_mm2 <= area(&g.centerline_polygon) + 1e-3,
            "net area is more than the centerline area"
        );
        assert!(
            inside(r.seed, &g.polygon),
            "seed of {} is outside its polygon",
            r.name
        );
        assert!(inside(r.seed, &g.centerline_polygon));
        assert!(
            inside(g.label_point, &g.polygon),
            "label of {} is outside its polygon",
            r.name
        );
        assert!(!g.wall_ids.is_empty());
        for id in &g.wall_ids {
            assert!(
                walls(project).iter().any(|w| &w.id == id),
                "room lists a missing wall"
            );
        }
        assert!(!r.name.trim().is_empty());
        if r.auto_named {
            assert!(
                !auto_names.contains(&r.name.as_str()),
                "auto name {} is used twice",
                r.name
            );
            auto_names.push(&r.name);
        }
    }
    let level_of = |id: &str| {
        rooms(project)
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| r.level_id.clone())
    };
    for (i, a) in derived.rooms.iter().enumerate() {
        for b in derived.rooms.iter().skip(i + 1) {
            if level_of(&a.room_id) == level_of(&b.room_id) {
                assert!(
                    a.centerline_polygon != b.centerline_polygon,
                    "two rooms share one face"
                );
            }
        }
    }

    // Footprints and totals.
    let mut gross = 0.0;
    for f in &derived.footprints {
        if !f.polygon.is_empty() {
            assert!(area(&f.polygon) > 0.0, "footprint is not counter-clockwise");
            assert!((area(&f.polygon) - f.area_mm2).abs() < 1e-3);
        }
        gross += f.area_mm2;
    }
    assert!((derived.totals.gross_area_m2 - gross / 1e6).abs() < 1e-6);
    let net: f64 = derived.rooms.iter().map(|r| r.area_mm2).sum();
    assert!((derived.totals.floor_area_m2 - net / 1e6).abs() < 1e-6);
    assert_eq!(derived.totals.room_count as usize, rooms(project).len());

    // Review items stay calm.
    let mut issue_ids: Vec<&String> = derived.issues.iter().map(|i| &i.id).collect();
    issue_ids.sort();
    let n = issue_ids.len();
    issue_ids.dedup();
    assert_eq!(n, issue_ids.len(), "duplicate issue ids");
    for i in &derived.issues {
        assert!(
            i.severity != Severity::Error,
            "design opinions are never errors"
        );
        for id in &i.element_ids {
            assert!(
                project.elements.iter().any(|e| e.id() == id),
                "issue names a missing element"
            );
        }
    }

    assert_pipe_invariants(state);
    assert_service_invariants(state);
}

/// Pipes follow the validation rules, and `Derived::pipes` agrees with them.
pub fn assert_pipe_invariants(state: &DocState) {
    let project = &state.project;
    let pipes = &state.derived.pipes;
    let is_pipe = |id: &str| {
        project
            .elements
            .iter()
            .any(|e| e.id() == id && e.kind() == ElementKind::Pipe)
    };
    let mut pipe_count = 0;
    for e in &project.elements {
        let Element::Pipe(p) = e else { continue };
        pipe_count += 1;
        assert!(p.points.len() >= 2, "pipe {} has one point", p.id);
        assert!((6.0..=300.0).contains(&p.diameter_mm));
        assert!(project.levels.iter().any(|l| l.id == p.level_id));
        for w in p.points.windows(2) {
            let d =
                ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2) + (w[1].z - w[0].z).powi(2))
                    .sqrt();
            assert!(d >= 1.0 - 1e-9, "pipe {} has two points {d} mm apart", p.id);
        }
    }
    let kinds = |k: FittingKind| pipes.fittings.iter().filter(|f| f.kind == k).count() as u32;
    assert_eq!(pipes.elbow_count, kinds(FittingKind::Elbow));
    assert_eq!(pipes.tee_count, kinds(FittingKind::Tee));
    assert_eq!(pipes.sleeve_count as usize, pipes.penetrations.len());
    for f in &pipes.fittings {
        assert!(is_pipe(&f.pipe_id), "fitting on a missing pipe");
        assert_eq!(f.branch_pipe_id.is_some(), f.kind == FittingKind::Tee);
        if let Some(b) = &f.branch_pipe_id {
            assert!(is_pipe(b) && b != &f.pipe_id, "tee with a bad branch");
        }
        assert!((0.0..=180.0).contains(&f.angle_deg));
    }
    for p in &pipes.penetrations {
        assert!(is_pipe(&p.pipe_id), "penetration of a missing pipe");
        assert_eq!(p.host_id.is_some(), p.kind == PenetrationKind::Wall);
        if let Some(h) = &p.host_id {
            assert!(
                walls(project).iter().any(|w| &w.id == h),
                "penetration of a missing wall"
            );
        }
        let len = (p.direction.x.powi(2) + p.direction.y.powi(2) + p.direction.z.powi(2)).sqrt();
        assert!(
            (len - 1.0).abs() < 1e-6,
            "sleeve direction is not a unit vector"
        );
    }
    let runs: u32 = pipes.takeoff.iter().map(|r| r.run_count).sum();
    assert_eq!(
        runs as usize, pipe_count,
        "a pipe is missing from the take-off"
    );
    let rows: f64 = pipes.takeoff.iter().map(|r| r.length_m).sum();
    assert!(
        (rows - pipes.total_length_m).abs() < 1e-9,
        "take-off rows do not add up"
    );
    for r in &pipes.takeoff {
        let mm = r.length_m * 1000.0;
        assert!((mm - mm.round()).abs() < 1e-6, "row not rounded to the mm");
    }
    for i in &state.derived.issues {
        assert_eq!(i.location.is_some(), is_located(&i.code), "{} location", i.code);
    }
}

/// Review items that point at one place: every pipe and device item except
/// the two summaries of a whole run or project.
pub fn is_located(code: &str) -> bool {
    matches!(
        code,
        "pipe_through_column"
            | "pipe_across_opening"
            | "pipes_cross"
            | "drain_slope_low"
            | "condensate_slope_low"
            | "condensate_open_end"
            | "light_no_switch"
            | "switch_no_load"
            | "switch_behind_door"
            | "aircon_no_outlet"
            | "lineset_long"
            | "lineset_rise"
            | "lineset_short"
            | "indoor_unit_clearance"
            | "outdoor_unit_clearance"
            | "outdoor_unit_unsupported"
            | "unit_near_tv"
    )
}

/// Links, the schedule and review marks agree with the model; levels are
/// unique and every element stands on one of them.
pub fn assert_service_invariants(state: &DocState) {
    let project = &state.project;
    let derived = &state.derived;
    assert!(!project.levels.is_empty(), "a project has a level");
    for (i, l) in project.levels.iter().enumerate() {
        assert!(!project.levels[..i].iter().any(|o| o.id == l.id), "level id {} twice", l.id);
        assert!(!l.name.trim().is_empty());
    }
    for e in &project.elements {
        let v = serde_json::to_value(e).unwrap();
        if let Some(level) = v.get("level_id").and_then(|l| l.as_str()) {
            assert!(
                project.levels.iter().any(|l| l.id == level),
                "{} stands on a level that is gone",
                e.id()
            );
        }
    }
    let objects: Vec<&Asset> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => Some(a),
            _ => None,
        })
        .collect();
    for a in &objects {
        for (i, link) in a.links.iter().enumerate() {
            assert_ne!(link, &a.id, "{} links itself", a.id);
            assert!(!a.links[..i].contains(link), "{} links {link} twice", a.id);
            assert!(
                objects.iter().any(|o| &o.id == link),
                "{} links {link}, which is not an object",
                a.id
            );
        }
        assert!(a.circuit.chars().count() <= 16);
        assert_eq!(a.circuit, a.circuit.trim());
        if let Some(l) = a.light {
            assert!((0.0..=20_000.0).contains(&l.lumens));
            assert!((1500.0..=10_000.0).contains(&l.kelvin));
        }
    }

    // Schedule: one row per level, room and key, counts add up to the
    // counted objects, rows name real levels and rooms.
    let catalog = defaults::asset_catalog();
    let counted = objects
        .iter()
        .filter(|a| {
            let (category, device) = match catalog.iter().find(|c| c.key == a.catalog_key) {
                Some(i) => (i.category, i.device),
                None => (a.category, None),
            };
            device.is_some()
                || matches!(a.catalog_key.as_str(), "kitchen-sink" | "washing-machine")
                || matches!(
                    category,
                    AssetCategory::Sanitary
                        | AssetCategory::Lighting
                        | AssetCategory::Electrical
                        | AssetCategory::Aircon
                        | AssetCategory::Utility
                )
        })
        .count();
    let total: u32 = derived.schedule.iter().map(|r| r.count).sum();
    assert_eq!(total as usize, counted, "schedule counts do not add up");
    for (i, r) in derived.schedule.iter().enumerate() {
        assert!(r.count > 0);
        if matches!(r.catalog_key.as_str(), "kitchen-sink" | "washing-machine") {
            assert_eq!(r.group, ScheduleGroup::Plumbing, "{} is a plumbing fixture", r.catalog_key);
        }
        if let Some(room) = &r.room_id {
            assert!(rooms(project).iter().any(|x| &x.id == room), "schedule names a missing room");
        }
        for other in &derived.schedule[..i] {
            assert!(
                !(other.level_id == r.level_id
                    && other.room_id == r.room_id
                    && other.catalog_key == r.catalog_key),
                "two schedule rows for one key in one room"
            );
        }
    }

    // Every finding comes from a known check. Review status follows the
    // marks, and only marks for one finding are resolved.
    for i in &derived.issues {
        assert!(guhit_core::is_review_code(&i.code), "unknown code {}", i.code);
        if i.status == IssueStatus::Open {
            assert!(i.note.is_empty(), "an open item carries a note: {}", i.id);
        }
    }
    for m in &derived.review_resolved {
        match &m.target {
            ReviewTarget::Issue { id } => {
                assert!(!derived.issues.iter().any(|i| &i.id == id), "a present finding is resolved")
            }
            other => panic!("only single findings resolve: {other:?}"),
        }
        assert!(project.review.contains(m));
    }
}
