//! Seeded fuzz: long random command sequences, valid and invalid, with every
//! engine invariant checked after every step. No external crates, a small
//! LCG keeps runs identical on every machine.
//!
//! After each step:
//! - preview and apply agree (both succeed with the same elements, derived
//!   data and diff, or both fail),
//! - a failed command leaves the document byte-identical,
//! - all invariants in `common::assert_invariants` hold,
//! - undo restores the exact prior project, redo the exact result.

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    /// A coordinate on a 250 mm grid, sometimes off grid.
    fn coord(&mut self) -> f64 {
        let v = (self.below(49) as f64) * 250.0;
        if self.chance(10) {
            v + (self.below(2000) as f64) / 7.0
        } else {
            v
        }
    }
    fn point(&mut self) -> Point {
        p(self.coord(), self.coord())
    }
    fn delta(&mut self) -> Point {
        p(
            (self.below(21) as f64 - 10.0) * 250.0,
            (self.below(21) as f64 - 10.0) * 250.0,
        )
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len())])
        }
    }
}

fn ids_of(project: &Project, kind: ElementKind) -> Vec<Id> {
    project
        .elements
        .iter()
        .filter(|e| e.kind() == kind)
        .map(|e| e.id().clone())
        .collect()
}

fn random_command(rng: &mut Lcg, project: &Project) -> Command {
    let wall_ids = ids_of(project, ElementKind::Wall);
    let room_ids = ids_of(project, ElementKind::Room);
    let all_ids: Vec<Id> = project.elements.iter().map(|e| e.id().clone()).collect();
    let some_wall = rng
        .pick(&wall_ids)
        .cloned()
        .unwrap_or_else(|| "missing-wall".into());
    let some_room = rng
        .pick(&room_ids)
        .cloned()
        .unwrap_or_else(|| "missing-room".into());
    let some_any = rng
        .pick(&all_ids)
        .cloned()
        .unwrap_or_else(|| "missing".into());
    let level = project.levels[rng.below(project.levels.len())].id.clone();
    let pick_many = |rng: &mut Lcg, max: usize| -> Vec<Id> {
        let n = 1 + rng.below(max);
        (0..n).filter_map(|_| rng.pick(&all_ids).cloned()).collect()
    };

    // Wall ends, so a random dimension is usually snapped to geometry and has
    // to follow it.
    let wall_ends: Vec<Point> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some([w.start, w.end]),
            _ => None,
        })
        .flatten()
        .collect();
    let snapped = |rng: &mut Lcg| -> Point {
        if rng.chance(75) {
            if let Some(q) = rng.pick(&wall_ends) {
                return *q;
            }
        }
        rng.point()
    };

    match rng.below(26) {
        0 | 1 => Command::AddWall {
            start: rng.point(),
            end: rng.point(),
            thickness_mm: if rng.chance(50) {
                Some(50.0 + rng.below(40) as f64 * 25.0)
            } else {
                None
            },
            height_mm: if rng.chance(20) {
                Some(2400.0 + rng.below(5) as f64 * 300.0)
            } else {
                None
            },
            material_id: None,
            level_id: Some(level),
        },
        2 => {
            let n = 2 + rng.below(4);
            Command::AddWallChain {
                points: (0..n).map(|_| rng.point()).collect(),
                closed: rng.chance(60),
                thickness_mm: Some(100.0 + rng.below(3) as f64 * 50.0),
                level_id: Some(level),
            }
        }
        3 | 4 => Command::AddRectRoom {
            origin: rng.point(),
            width_mm: 500.0 + rng.below(20) as f64 * 250.0,
            depth_mm: 500.0 + rng.below(20) as f64 * 250.0,
            name: if rng.chance(50) {
                Some(format!("Named {}", rng.below(5)))
            } else {
                None
            },
            thickness_mm: if rng.chance(30) { Some(100.0) } else { None },
            level_id: Some(level),
        },
        5 => Command::SetWallEndpoints {
            wall_id: some_wall,
            start: rng.point(),
            end: rng.point(),
        },
        6 => Command::SetWallLength {
            wall_id: some_wall,
            length_mm: rng.below(40) as f64 * 250.0,
            anchor: [WallAnchor::Start, WallAnchor::End, WallAnchor::Center][rng.below(3)],
        },
        7 => Command::SplitWall {
            wall_id: some_wall,
            at_mm: rng.below(30) as f64 * 250.0,
        },
        8..=10 => Command::AddOpening {
            wall_id: some_wall,
            opening_type: if rng.chance(50) {
                OpeningType::Door
            } else {
                OpeningType::Window
            },
            offset_mm: rng.below(40) as f64 * 200.0,
            width_mm: if rng.chance(40) {
                Some(400.0 + rng.below(10) as f64 * 200.0)
            } else {
                None
            },
            height_mm: None,
            sill_mm: None,
            style: None,
            flip_side: if rng.chance(30) { Some(true) } else { None },
            flip_hinge: if rng.chance(30) { Some(true) } else { None },
        },
        11 | 12 => Command::ResizeRoom {
            room_id: some_room,
            side: [Side::North, Side::South, Side::East, Side::West][rng.below(4)],
            delta_mm: (rng.below(17) as f64 - 8.0) * 250.0,
        },
        13 => Command::AddElement {
            element: Element::Asset(Asset {
                id: String::new(),
                level_id: level,
                catalog_key: "bed-double".into(),
                name: "Bed".into(),
                category: AssetCategory::Furniture,
                position: rng.point(),
                rotation_deg: 0.0,
                width_mm: 1370.0,
                depth_mm: 1900.0,
                height_mm: 500.0,
                elevation_mm: 0.0,
            }),
        },
        14 => {
            // Edit a random element in place: thickness, width or name.
            let mut el = rng.pick(&project.elements).cloned().unwrap_or_else(|| {
                Element::Annotation(Annotation {
                    id: "missing".into(),
                    level_id: level.clone(),
                    position: p(0.0, 0.0),
                    text: "x".into(),
                    size_mm: 200.0,
                    rotation_deg: 0.0,
                })
            });
            match &mut el {
                Element::Wall(w) => {
                    w.thickness_mm = 25.0 + rng.below(12) as f64 * 50.0;
                    if rng.chance(30) {
                        w.end = rng.point();
                    }
                }
                Element::Opening(o) => {
                    o.width_mm = 200.0 + rng.below(12) as f64 * 250.0;
                    o.offset_mm += (rng.below(9) as f64 - 4.0) * 250.0;
                }
                Element::Room(r) => {
                    r.name = format!("Renamed {}", rng.below(4));
                    r.usage = RoomUsage::Bedroom;
                    if rng.chance(30) {
                        r.seed = rng.point();
                    }
                }
                Element::Asset(a) => a.position = rng.point(),
                _ => {}
            }
            Command::UpdateElement { element: el }
        }
        15 | 16 => Command::DeleteElements {
            ids: pick_many(rng, 3),
        },
        17 | 18 => Command::MoveElements {
            ids: pick_many(rng, 4),
            delta: rng.delta(),
            stretch_connected: rng.chance(70),
        },
        19 => Command::RotateElements {
            ids: pick_many(rng, 5),
            pivot: rng.point(),
            angle_deg: [90.0, -90.0, 180.0, 45.0, 17.5][rng.below(5)],
        },
        20 => Command::DuplicateElements {
            ids: pick_many(rng, 4),
            delta: rng.delta(),
        },
        21 => Command::SetMaterial {
            ids: vec![some_any],
            material_id: if rng.chance(80) {
                "mat-chb-bare".into()
            } else {
                "mat-missing".into()
            },
        },
        23 | 24 => Command::AddElement {
            element: Element::Dimension(Dimension {
                id: String::new(),
                level_id: level,
                a: snapped(rng),
                b: snapped(rng),
                offset_mm: (rng.below(9) as f64 - 4.0) * 300.0,
                text_override: None,
            }),
        },
        22 => Command::SetLayer {
            layer: Layer {
                key: [
                    LayerKey::Walls,
                    LayerKey::Openings,
                    LayerKey::Rooms,
                    LayerKey::Assets,
                ][rng.below(4)],
                visible: true,
                // Mostly unlock, so the run does not get stuck.
                locked: rng.chance(25),
            },
        },
        _ => {
            let n = 1 + rng.below(3);
            Command::Batch {
                label: "Fuzz batch".into(),
                commands: (0..n).map(|_| random_command(rng, project)).collect(),
            }
        }
    }
}

/// Endpoints of every dimension, by id.
fn dim_points(project: &Project) -> Vec<(Id, Point, Point)> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dimension(d) => Some((d.id.clone(), d.a, d.b)),
            _ => None,
        })
        .collect()
}

/// These commands carry no element selection, so a dimension that moved under
/// one of them moved because it was attached to a wall.
fn is_wall_edit(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::SetWallEndpoints { .. }
            | Command::SetWallLength { .. }
            | Command::ResizeRoom { .. }
    )
}

/// Returns (applied, rejected, dimensions dragged by a wall edit).
fn run(seed: u64, steps: usize, start: Project) -> (usize, usize, usize) {
    let mut rng = Lcg(seed);
    let mut doc = Document::new(start.clone());
    let (mut ok, mut rejected, mut dragged) = (0, 0, 0);
    for step in 0..steps {
        // Keep the plan small enough to stay fast, and start over now and then.
        if doc.project().elements.len() > 70 {
            doc = Document::new(start.clone());
        }
        let cmd = random_command(&mut rng, doc.project());
        let before_project = doc.project().clone();
        let before_json = serde_json::to_string(&doc.state()).unwrap();
        let before_revision = doc.revision();

        let preview = doc.preview(&cmd);
        assert_eq!(
            serde_json::to_string(&doc.state()).unwrap(),
            before_json,
            "preview changed the document"
        );
        let applied = doc.apply(cmd.clone(), Origin::User);
        let ctx = format!("seed {seed} step {step} command {cmd:?}");

        match (preview, applied) {
            (Ok(pv), Ok(ap)) => {
                ok += 1;
                assert_eq!(
                    pv.state.project.elements, ap.state.project.elements,
                    "{ctx}"
                );
                assert_eq!(
                    pv.state.project.materials, ap.state.project.materials,
                    "{ctx}"
                );
                assert_eq!(pv.state.project.layers, ap.state.project.layers, "{ctx}");
                assert_eq!(pv.state.derived, ap.state.derived, "{ctx}");
                assert_eq!(pv.diff, ap.diff, "{ctx}");
                assert_eq!(ap.state.revision, before_revision + 1, "{ctx}");
                let result = std::panic::catch_unwind(|| assert_invariants(&ap.state));
                if let Err(e) = result {
                    eprintln!("invariant failed at {ctx}");
                    std::panic::resume_unwind(e);
                }
                // Diff is truthful.
                for id in &ap.diff.added {
                    assert!(
                        !before_project.elements.iter().any(|e| e.id() == id),
                        "{ctx}"
                    );
                    assert!(
                        ap.state.project.elements.iter().any(|e| e.id() == id),
                        "{ctx}"
                    );
                }
                for id in &ap.diff.removed {
                    assert!(
                        before_project.elements.iter().any(|e| e.id() == id),
                        "{ctx}"
                    );
                    assert!(
                        !ap.state.project.elements.iter().any(|e| e.id() == id),
                        "{ctx}"
                    );
                }
                // A dimension that followed a wall edit is in the diff.
                if is_wall_edit(&cmd) {
                    let was = dim_points(&before_project);
                    for (id, a, b) in dim_points(&ap.state.project) {
                        if was.iter().any(|(i, oa, ob)| *i == id && (*oa != a || *ob != b)) {
                            dragged += 1;
                            assert!(ap.diff.modified.contains(&id), "{ctx}");
                            assert!(ap.diff.summary.contains("dimension"), "{ctx}");
                        }
                    }
                }
                // Undo is exact, redo is exact.
                let undone = doc.undo().unwrap();
                assert_eq!(undone.project, before_project, "undo is not exact: {ctx}");
                let redone = doc.redo().unwrap();
                assert_eq!(redone.project, ap.state.project, "redo is not exact: {ctx}");
                assert_eq!(redone.derived, ap.state.derived, "{ctx}");
            }
            (Err(_), Err(e)) => {
                rejected += 1;
                assert_eq!(
                    serde_json::to_string(&doc.state()).unwrap(),
                    before_json,
                    "failed command changed the document: {ctx}"
                );
                assert!(!e.to_string().trim().is_empty(), "{ctx}");
            }
            (pv, ap) => panic!(
                "preview and apply disagree ({} vs {}): {ctx}",
                pv.is_ok(),
                ap.is_ok()
            ),
        }
    }
    (ok, rejected, dragged)
}

/// 6 seeds of 400 steps per test by default (4800 steps in total). Set
/// GUHIT_FUZZ_SEEDS for a longer soak, for example GUHIT_FUZZ_SEEDS=100.
fn seed_count() -> u64 {
    std::env::var("GUHIT_FUZZ_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6)
}

#[test]
fn fuzz_from_a_blank_project() {
    let mut project = blank_project();
    project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 2700.0,
    });
    let (mut ok, mut rejected, mut dragged) = (0, 0, 0);
    for seed in 1..=seed_count() {
        let (o, r, d) = run(seed, 400, project.clone());
        ok += o;
        rejected += r;
        dragged += d;
    }
    println!("blank fuzz: {ok} applied, {rejected} rejected, {dragged} dimensions dragged");
    assert!(
        ok > 800,
        "too few commands succeeded to mean anything: {ok}"
    );
    assert!(
        rejected > 200,
        "too few commands were rejected to test atomicity: {rejected}"
    );
}

#[test]
fn fuzz_from_the_sample_bungalow() {
    let (mut ok, mut rejected, mut dragged) = (0, 0, 0);
    for seed in 100..100 + seed_count() {
        let (o, r, d) = run(seed, 400, templates::sample_bungalow());
        ok += o;
        rejected += r;
        dragged += d;
    }
    println!("bungalow fuzz: {ok} applied, {rejected} rejected, {dragged} dimensions dragged");
    assert!(ok > 800, "{ok}");
    assert!(rejected > 200, "{rejected}");
    assert!(
        dragged > 20,
        "the mix barely made a dimension follow a wall: {dragged}"
    );
}

/// Timing check on a plan far larger than a house: a 10 x 10 grid of rooms
/// drawn as 22 long crossing walls, then 100 more edits.
/// Run: cargo test --release -p guhit-core --test invariants -- --ignored --nocapture
#[test]
#[ignore]
fn perf_smoke_large_grid() {
    let mut doc = blank();
    let t0 = std::time::Instant::now();
    for i in 0..=10 {
        let v = i as f64 * 3000.0;
        doc.apply(add_wall(p(v, 0.0), p(v, 30_000.0)), Origin::User)
            .unwrap();
        doc.apply(add_wall(p(0.0, v), p(30_000.0, v)), Origin::User)
            .unwrap();
    }
    let built = t0.elapsed();
    assert_eq!(rooms(doc.project()).len(), 100);
    assert_invariants(&doc.state());
    let t1 = std::time::Instant::now();
    for i in 0..100 {
        let x = 1500.0 + (i % 10) as f64 * 3000.0;
        let y = 1000.0 + (i / 10) as f64 * 3000.0;
        doc.apply(add_wall(p(x, y), p(x + 800.0, y)), Origin::User)
            .unwrap();
    }
    let edits = t1.elapsed();
    println!("perf: grid of 100 rooms built in {built:?}, then 100 applies in {edits:?} ({:?} per apply, {} elements)", edits / 100, doc.project().elements.len());
}
