//! Pipes: fittings, penetrations, take-off, the pipe review items, validation,
//! locked pipe layers, migration and the plumbing demo (docs/CONTRACT.md,
//! "Pipes").

mod common;

use common::*;
use guhit_core::{compute_derived, migrate, templates, CoreError, Document};
use guhit_model::*;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

fn pipe(id: &str, system: PipeSystem, d: f64, pts: &[(f64, f64, f64)]) -> Element {
    Element::Pipe(Pipe {
        id: id.into(),
        level_id: LEVEL.into(),
        system,
        material: defaults::pipe_defaults(system).0,
        diameter_mm: d,
        points: pts.iter().map(|(x, y, z)| v(*x, *y, *z)).collect(),
        name: String::new(),
    })
}

fn cold(id: &str, pts: &[(f64, f64, f64)]) -> Element {
    pipe(id, PipeSystem::ColdWater, 20.0, pts)
}

fn drain(id: &str, d: f64, pts: &[(f64, f64, f64)]) -> Element {
    pipe(id, PipeSystem::Drainage, d, pts)
}

fn wall_el(id: &str, a: (f64, f64), b: (f64, f64), t: f64) -> Element {
    Element::Wall(Wall {
        id: id.into(),
        level_id: LEVEL.into(),
        start: p(a.0, a.1),
        end: p(b.0, b.1),
        thickness_mm: t,
        height_mm: None,
        material_id: None,
    })
}

/// A closed 6.0 x 4.0 m box of 150 mm walls on centerlines, level height 3000.
/// Footprint: -75 to 6075 by -75 to 4075.
fn shell() -> Vec<Element> {
    vec![
        wall_el("w-south", (0.0, 0.0), (6000.0, 0.0), 150.0),
        wall_el("w-east", (6000.0, 0.0), (6000.0, 4000.0), 150.0),
        wall_el("w-north", (6000.0, 4000.0), (0.0, 4000.0), 150.0),
        wall_el("w-west", (0.0, 4000.0), (0.0, 0.0), 150.0),
    ]
}

fn door(id: &str, wall: &str, offset: f64, width: f64) -> Element {
    Element::Opening(Opening {
        id: id.into(),
        wall_id: wall.into(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingSingle,
        offset_mm: offset,
        width_mm: width,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side: false,
        flip_hinge: false,
        material_id: None,
    })
}

fn column(id: &str, x: f64, y: f64) -> Element {
    Element::Column(Column {
        id: id.into(),
        level_id: LEVEL.into(),
        center: p(x, y),
        shape: ColumnShape::Rect,
        width_mm: 200.0,
        depth_mm: 200.0,
        rotation_deg: 0.0,
        material_id: None,
    })
}

fn project_with(elements: Vec<Element>) -> Project {
    let mut project = blank_project();
    project.elements = elements;
    project
}

fn derive(elements: Vec<Element>) -> Derived {
    compute_derived(&project_with(elements))
}

fn fittings(d: &Derived, kind: FittingKind) -> Vec<&PipeFitting> {
    d.pipes.fittings.iter().filter(|f| f.kind == kind).collect()
}

fn pens(d: &Derived, kind: PenetrationKind) -> Vec<&PipePenetration> {
    d.pipes
        .penetrations
        .iter()
        .filter(|p| p.kind == kind)
        .collect()
}

fn issues<'a>(d: &'a Derived, code: &str) -> Vec<&'a Issue> {
    d.issues.iter().filter(|i| i.code == code).collect()
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6 && (a.z - b.z).abs() < 1e-6
}

fn code(e: CoreError) -> String {
    e.code().to_string()
}

// ------------------------------------------------------------------ elbows

#[test]
fn an_elbow_at_every_turn_inside_a_run() {
    let d = derive(vec![cold(
        "a",
        &[
            (0.0, 0.0, 300.0),
            (2000.0, 0.0, 300.0),
            (2000.0, 1000.0, 300.0),
            (2000.0, 1000.0, 900.0),
        ],
    )]);
    let elbows = fittings(&d, FittingKind::Elbow);
    assert_eq!(elbows.len(), 2);
    assert!(near(elbows[0].position, v(2000.0, 0.0, 300.0)));
    assert!(near(elbows[1].position, v(2000.0, 1000.0, 300.0)));
    for e in &elbows {
        assert_eq!(e.angle_deg, 90.0);
        assert_eq!(e.pipe_id, "a");
        assert_eq!(e.branch_pipe_id, None);
        assert_eq!(e.diameter_mm, 20.0);
        assert_eq!(e.level_id, LEVEL);
    }
    assert_eq!((d.pipes.elbow_count, d.pipes.tee_count), (2, 0));
}

#[test]
fn a_turn_of_one_degree_or_less_is_no_elbow() {
    // 0.5 degrees: 1000 mm along, 8.7 mm aside.
    let small = (0.5f64).to_radians().tan() * 1000.0;
    let d = derive(vec![cold(
        "a",
        &[
            (0.0, 0.0, 300.0),
            (1000.0, 0.0, 300.0),
            (2000.0, small, 300.0),
        ],
    )]);
    assert_eq!(d.pipes.elbow_count, 0);
    let bigger = (2.0f64).to_radians().tan() * 1000.0;
    let d = derive(vec![cold(
        "a",
        &[
            (0.0, 0.0, 300.0),
            (1000.0, 0.0, 300.0),
            (2000.0, bigger, 300.0),
        ],
    )]);
    assert_eq!(d.pipes.elbow_count, 1);
    assert!((d.pipes.fittings[0].angle_deg - 2.0).abs() < 1e-5);
}

#[test]
fn ends_of_two_runs_meeting_straight_need_no_fitting() {
    let d = derive(vec![
        cold("a", &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)]),
        cold("b", &[(2000.0, 0.0, 300.0), (4000.0, 0.0, 300.0)]),
    ]);
    assert!(d.pipes.fittings.is_empty());
    assert!(
        issues(&d, "pipes_cross").is_empty(),
        "joined runs do not clash"
    );
}

#[test]
fn ends_of_two_runs_meeting_at_an_angle_are_an_elbow() {
    let d = derive(vec![
        cold("a", &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)]),
        cold("b", &[(2000.0, 0.0, 300.0), (2000.0, 2000.0, 300.0)]),
    ]);
    let elbows = fittings(&d, FittingKind::Elbow);
    assert_eq!(elbows.len(), 1);
    assert_eq!(
        elbows[0].pipe_id, "a",
        "the elbow sits on the run that arrives"
    );
    assert!(near(elbows[0].position, v(2000.0, 0.0, 300.0)));
    assert_eq!(elbows[0].angle_deg, 90.0);
    assert_eq!(d.pipes.tee_count, 0);
    assert!(issues(&d, "pipes_cross").is_empty());
}

// -------------------------------------------------------------------- tees

#[test]
fn a_run_ending_on_the_middle_of_another_is_a_tee() {
    let d = derive(vec![
        cold("main", &[(0.0, 0.0, 300.0), (4000.0, 0.0, 300.0)]),
        cold("branch", &[(2000.0, 1500.0, 300.0), (2000.0, 0.0, 300.0)]),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(tees[0].pipe_id, "main");
    assert_eq!(tees[0].branch_pipe_id.as_deref(), Some("branch"));
    assert!(near(tees[0].position, v(2000.0, 0.0, 300.0)));
    assert_eq!(tees[0].angle_deg, 90.0);
    assert_eq!((d.pipes.elbow_count, d.pipes.tee_count), (0, 1));
    assert!(
        issues(&d, "pipes_cross").is_empty(),
        "a tee is a join, not a clash"
    );
}

#[test]
fn a_branch_within_the_larger_radius_still_joins() {
    // A 100 mm drain and a 50 mm branch ending 40 mm off its centerline.
    let d = derive(vec![
        drain("main", 100.0, &[(0.0, 0.0, -400.0), (4000.0, 0.0, -440.0)]),
        drain(
            "branch",
            50.0,
            &[(2000.0, 0.0, 300.0), (2000.0, 0.0, -380.0)],
        ),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(tees[0].diameter_mm, 100.0);
    assert!(issues(&d, "pipes_cross").is_empty());
}

#[test]
fn a_run_ending_on_an_interior_point_of_another_is_a_tee() {
    // The main run has a point where the branch lands; it does not turn there.
    let d = derive(vec![
        cold(
            "main",
            &[
                (0.0, 0.0, 300.0),
                (2000.0, 0.0, 300.0),
                (4000.0, 0.0, 300.0),
            ],
        ),
        cold("branch", &[(2000.0, 0.0, 300.0), (2000.0, 0.0, 1000.0)]),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(
        (tees[0].pipe_id.as_str(), tees[0].branch_pipe_id.as_deref()),
        ("main", Some("branch"))
    );
    assert!(near(tees[0].position, v(2000.0, 0.0, 300.0)));
    assert_eq!(d.pipes.elbow_count, 0);
}

#[test]
fn a_branch_landing_on_a_bend_is_one_tee_and_no_elbow() {
    // The main run turns 90 degrees where the branch lands: one fitting.
    let d = derive(vec![
        cold(
            "main",
            &[
                (0.0, 0.0, 300.0),
                (2000.0, 0.0, 300.0),
                (2000.0, 2000.0, 300.0),
            ],
        ),
        cold("branch", &[(2000.0, -1500.0, 300.0), (2000.0, 0.0, 300.0)]),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(
        (tees[0].pipe_id.as_str(), tees[0].branch_pipe_id.as_deref()),
        ("main", Some("branch"))
    );
    assert_eq!(d.pipes.elbow_count, 0);
    assert_eq!(d.pipes.tee_count, 1);
}

#[test]
fn three_run_ends_at_one_point_are_a_tee() {
    // A riser ends where a chase and a fixture feed start: one tee. The riser
    // and the feed line up, so the chase is the branch.
    let d = derive(vec![
        pipe(
            "riser",
            PipeSystem::ColdWater,
            25.0,
            &[(0.0, 0.0, -300.0), (0.0, 0.0, 300.0)],
        ),
        cold("chase", &[(0.0, 0.0, 300.0), (3000.0, 0.0, 300.0)]),
        cold("feed", &[(0.0, 0.0, 300.0), (0.0, 0.0, 600.0)]),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(
        tees[0].pipe_id, "riser",
        "the larger of the straight pair carries the tee"
    );
    assert_eq!(tees[0].branch_pipe_id.as_deref(), Some("chase"));
    assert_eq!(tees[0].diameter_mm, 25.0);
    assert_eq!(tees[0].angle_deg, 90.0);
    assert_eq!(d.pipes.elbow_count, 0);
    assert!(issues(&d, "pipes_cross").is_empty());
}

#[test]
fn a_loop_that_closes_where_a_feed_ends_never_tees_into_itself() {
    // A ring main starts and ends where its feed arrives: three ends, one tee.
    // The ring is the larger pipe, so it would carry the tee of its own end.
    let d = derive(vec![
        pipe(
            "ring",
            PipeSystem::ColdWater,
            25.0,
            &[
                (0.0, 0.0, 300.0),
                (2000.0, 0.0, 300.0),
                (2000.0, 2000.0, 300.0),
                (0.0, 2000.0, 300.0),
                (0.0, 0.0, 300.0),
            ],
        ),
        cold("feed", &[(-2000.0, 0.0, 300.0), (0.0, 0.0, 300.0)]),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(
        (tees[0].pipe_id.as_str(), tees[0].branch_pipe_id.as_deref()),
        ("feed", Some("ring"))
    );
    assert_eq!(d.pipes.elbow_count, 3, "the three corners of the ring");
    assert!(issues(&d, "pipes_cross").is_empty());
}

#[test]
fn only_runs_that_can_connect_join() {
    // A hot water end on a cold water pipe is a clash, not a tee.
    let d = derive(vec![
        cold("c", &[(0.0, 0.0, 300.0), (4000.0, 0.0, 300.0)]),
        pipe(
            "h",
            PipeSystem::HotWater,
            20.0,
            &[(2000.0, 1500.0, 300.0), (2000.0, 0.0, 300.0)],
        ),
    ]);
    assert_eq!(d.pipes.tee_count, 0);
    assert_eq!(issues(&d, "pipes_cross").len(), 1);
    // A vent starting on a drain is a tee.
    let d = derive(vec![
        drain("d", 100.0, &[(0.0, 0.0, -400.0), (4000.0, 0.0, -440.0)]),
        pipe(
            "vent",
            PipeSystem::Vent,
            50.0,
            &[(2000.0, 0.0, -420.0), (2000.0, 0.0, 3000.0)],
        ),
    ]);
    let tees = fittings(&d, FittingKind::Tee);
    assert_eq!(tees.len(), 1);
    assert_eq!(
        (tees[0].pipe_id.as_str(), tees[0].branch_pipe_id.as_deref()),
        ("d", Some("vent"))
    );
}

// ------------------------------------------------------------ penetrations

#[test]
fn a_segment_through_the_floor_inside_the_footprint_is_a_slab_penetration() {
    let mut els = shell();
    els.push(drain(
        "wc",
        100.0,
        &[(1000.0, 1000.0, 20.0), (1000.0, 1000.0, -400.0)],
    ));
    let d = derive(els);
    let slab = pens(&d, PenetrationKind::Slab);
    assert_eq!(slab.len(), 1);
    assert_eq!(slab[0].pipe_id, "wc");
    assert_eq!(slab[0].host_id, None);
    // The middle of the 150 mm slab the 3D view draws under the lowest level.
    assert!(near(slab[0].position, v(1000.0, 1000.0, -75.0)));
    assert!(near(slab[0].direction, v(0.0, 0.0, -1.0)));
    assert_eq!(slab[0].diameter_mm, 100.0);
    assert_eq!(d.pipes.sleeve_count, 1);
}

#[test]
fn a_drain_that_starts_exactly_on_the_floor_counts_once() {
    let mut els = shell();
    els.push(drain(
        "fd",
        50.0,
        &[
            (1000.0, 1000.0, 0.0),
            (1000.0, 1000.0, -300.0),
            (3000.0, 1000.0, -340.0),
        ],
    ));
    let d = derive(els);
    assert_eq!(pens(&d, PenetrationKind::Slab).len(), 1);
}

#[test]
fn a_supply_that_stays_above_the_floor_needs_no_sleeve() {
    let mut els = shell();
    els.push(cold(
        "s",
        &[
            (1000.0, 1000.0, 300.0),
            (3000.0, 1000.0, 300.0),
            (3000.0, 1000.0, 0.0),
            (3000.0, 2000.0, 0.0),
        ],
    ));
    let d = derive(els);
    assert!(d.pipes.penetrations.is_empty());
    assert!(issues(&d, "pipe_penetrations").is_empty());
}

#[test]
fn a_crossing_outside_every_footprint_is_no_slab_penetration() {
    let mut els = shell();
    els.push(cold(
        "meter",
        &[(-1500.0, -1500.0, 300.0), (-1500.0, -1500.0, -300.0)],
    ));
    let d = derive(els);
    assert!(pens(&d, PenetrationKind::Slab).is_empty());
    // No footprint at all, no slab.
    let d = derive(vec![cold("s", &[(0.0, 0.0, 300.0), (0.0, 0.0, -300.0)])]);
    assert!(d.pipes.penetrations.is_empty());
}

#[test]
fn a_pipe_across_a_wall_is_a_wall_penetration() {
    let mut els = shell();
    // Out through the east wall at 500 above the floor.
    els.push(cold(
        "out",
        &[(5000.0, 2000.0, 500.0), (7000.0, 2000.0, 500.0)],
    ));
    let d = derive(els);
    let wall = pens(&d, PenetrationKind::Wall);
    assert_eq!(wall.len(), 1);
    assert_eq!(wall[0].host_id.as_deref(), Some("w-east"));
    assert!(near(wall[0].position, v(6000.0, 2000.0, 500.0)));
    assert!(near(wall[0].direction, v(1.0, 0.0, 0.0)));
    let summary = issues(&d, "pipe_penetrations");
    assert_eq!(summary.len(), 1);
    assert_eq!(
        summary[0].message,
        "1 pipe penetration needs a sleeve: 1 through a wall. Set the sleeves before the pour or the blockwork."
    );
}

#[test]
fn a_chase_along_a_wall_and_a_pipe_under_the_floor_do_not_count() {
    let mut els = shell();
    // Inside the north wall, along it, past the corner joint of the east wall.
    els.push(cold(
        "chase",
        &[(1000.0, 4000.0, 300.0), (5500.0, 4000.0, 300.0)],
    ));
    // Under the slab, across the south wall line.
    els.push(drain(
        "out",
        100.0,
        &[(3000.0, 1000.0, -400.0), (3000.0, -2000.0, -460.0)],
    ));
    // Vertical inside a wall.
    els.push(cold(
        "riser",
        &[(3000.0, 4000.0, 300.0), (3000.0, 4000.0, 2000.0)],
    ));
    let d = derive(els);
    assert!(pens(&d, PenetrationKind::Wall).is_empty());
}

#[test]
fn a_chase_passing_a_wall_that_ends_on_its_wall_does_not_cross_it() {
    // A partition meets the north wall at x = 3000. A chase inside the north
    // wall passes that junction without crossing the partition.
    let mut els = shell();
    els.push(wall_el("w-part", (3000.0, 0.0), (3000.0, 4000.0), 100.0));
    els.push(cold(
        "chase",
        &[(1000.0, 4000.0, 300.0), (5000.0, 4000.0, 300.0)],
    ));
    let d = derive(els);
    assert!(pens(&d, PenetrationKind::Wall).is_empty());
    // A pipe through the partition's body does cross it.
    let mut els = shell();
    els.push(wall_el("w-part", (3000.0, 0.0), (3000.0, 4000.0), 100.0));
    els.push(cold(
        "across",
        &[(1000.0, 2000.0, 300.0), (5000.0, 2000.0, 300.0)],
    ));
    let d = derive(els);
    let wall = pens(&d, PenetrationKind::Wall);
    assert_eq!(wall.len(), 1);
    assert_eq!(wall[0].host_id.as_deref(), Some("w-part"));
}

#[test]
fn a_pipe_through_a_door_opening_is_no_penetration_but_a_review_item() {
    let mut els = shell();
    els.push(door("d1", "w-south", 3000.0, 900.0));
    // Through the doorway at 1.50 m, square to the wall.
    els.push(cold(
        "feed",
        &[(3000.0, 1000.0, 1500.0), (3000.0, -1000.0, 1500.0)],
    ));
    let d = derive(els);
    assert!(pens(&d, PenetrationKind::Wall).is_empty());
    let found = issues(&d, "pipe_across_opening");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        found[0].element_ids,
        vec!["feed".to_string(), "d1".to_string()]
    );
    assert_eq!(found[0].id, "pipe_across_opening:feed,d1");
    assert!(near(found[0].location.unwrap(), v(3000.0, 0.0, 1500.0)));
    assert!(
        found[0]
            .message
            .starts_with("Cold water pipe 20 mm crosses "),
        "{}",
        found[0].message
    );
    assert!(found[0]
        .message
        .contains("runs 1.50 m above the floor through the 900 mm opening"));
    assert!(found[0]
        .message
        .ends_with("Route it above the door head at 2.10 m or under the slab."));
}

#[test]
fn a_chase_through_a_door_opening_names_the_door_by_its_room() {
    let project = templates::plumbing_demo();
    let d = compute_derived(&project);
    let found = issues(&d, "pipe_across_opening");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].message,
        "Heater feed crosses the T&B door. The 20 mm cold water line runs 1.50 m above the floor through the 700 mm opening. Route it above the door head at 2.10 m or under the slab."
    );
}

#[test]
fn a_pipe_above_the_door_head_or_below_the_floor_is_clear() {
    let mut els = shell();
    els.push(door("d1", "w-south", 3000.0, 900.0));
    // 20 mm pipe: the body clears the 2100 head at 2111.
    els.push(cold(
        "high",
        &[(1000.0, 0.0, 2111.0), (5000.0, 0.0, 2111.0)],
    ));
    els.push(drain(
        "low",
        100.0,
        &[(1000.0, 0.0, -400.0), (5000.0, 0.0, -440.0)],
    ));
    let d = derive(els);
    assert!(issues(&d, "pipe_across_opening").is_empty());
    // At 2105 the body enters the opening.
    let mut els = shell();
    els.push(door("d1", "w-south", 3000.0, 900.0));
    els.push(cold(
        "high",
        &[(1000.0, 0.0, 2105.0), (5000.0, 0.0, 2105.0)],
    ));
    assert_eq!(issues(&derive(els), "pipe_across_opening").len(), 1);
}

#[test]
fn a_vent_through_a_gable_roof_is_a_roof_penetration() {
    let mut project = project_with({
        let mut els = shell();
        els.push(pipe(
            "vent",
            PipeSystem::Vent,
            50.0,
            &[(3000.0, 3500.0, -300.0), (3000.0, 3500.0, 4000.0)],
        ));
        els
    });
    project.roof = Roof {
        kind: RoofKind::Gable,
        pitch_deg: 20.0,
        overhang_mm: 600.0,
        thickness_mm: 100.0,
        ridge_axis: Axis::X,
        material_id: None,
    };
    let d = compute_derived(&project);
    let roof = pens(&d, PenetrationKind::Roof);
    assert_eq!(roof.len(), 1);
    // The 3D view's gable: footprint y from -75 to 4075, ridge at 2000, the
    // underside at the wall top (3000) on the footprint edge, rising at 20
    // degrees. The sleeve sits in the middle of the 100 mm thick roof.
    let tan = 20f64.to_radians().tan();
    let under = 3000.0 + (2075.0 - 1500.0) * tan;
    let middle = under + 100.0 / 20f64.to_radians().cos() / 2.0;
    assert!(
        (roof[0].position.z - middle).abs() < 1e-3,
        "{} vs {middle}",
        roof[0].position.z
    );
    assert_eq!((roof[0].position.x, roof[0].position.y), (3000.0, 3500.0));
    assert_eq!(pens(&d, PenetrationKind::Slab).len(), 1);
    assert_eq!(
        issues(&d, "pipe_penetrations")[0].message,
        "2 pipe penetrations need sleeves or flashing: 1 through the floor slab, 1 through the roof. Set the sleeves before the pour or the blockwork, and detail the flashing before the roofing."
    );

    // The overhang is roof too; past it there is none.
    let mut eave = project.clone();
    eave.elements.push(pipe(
        "eave",
        PipeSystem::Vent,
        50.0,
        &[(3000.0, 4500.0, 2500.0), (3000.0, 4500.0, 4000.0)],
    ));
    eave.elements.push(pipe(
        "out",
        PipeSystem::Vent,
        50.0,
        &[(3000.0, 5000.0, 2500.0), (3000.0, 5000.0, 4000.0)],
    ));
    let d = compute_derived(&eave);
    let hit: Vec<&str> = pens(&d, PenetrationKind::Roof)
        .iter()
        .map(|p| p.pipe_id.as_str())
        .collect();
    assert_eq!(hit, vec!["vent", "eave"]);

    // Without a roof nothing goes through one.
    project.roof.kind = RoofKind::None;
    let d = compute_derived(&project);
    assert!(pens(&d, PenetrationKind::Roof).is_empty());
}

#[test]
fn a_flat_roof_is_crossed_at_the_wall_top() {
    let mut project = project_with({
        let mut els = shell();
        els.push(pipe(
            "vent",
            PipeSystem::Vent,
            50.0,
            &[(3000.0, 2000.0, 2000.0), (3000.0, 2000.0, 3500.0)],
        ));
        els
    });
    project.roof.kind = RoofKind::Flat;
    project.roof.thickness_mm = 150.0;
    let d = compute_derived(&project);
    let roof = pens(&d, PenetrationKind::Roof);
    assert_eq!(roof.len(), 1);
    assert!(near(roof[0].position, v(3000.0, 2000.0, 3075.0)));
}

// ------------------------------------------------------------------ checks

#[test]
fn a_pipe_through_a_column_is_flagged_at_the_first_hit() {
    let mut els = shell();
    els.push(column("c1", 3000.0, 2000.0));
    els.push(column("c2", 4500.0, 2000.0));
    els.push(cold(
        "run",
        &[(1000.0, 2000.0, 500.0), (5500.0, 2000.0, 500.0)],
    ));
    let d = derive(els);
    let found = issues(&d, "pipe_through_column");
    assert_eq!(found.len(), 2, "one item per pipe and column");
    assert_eq!(
        found[0].element_ids,
        vec!["run".to_string(), "c1".to_string()]
    );
    assert!(near(found[0].location.unwrap(), v(3000.0, 2000.0, 500.0)));
    assert_eq!(
        found[0].message,
        "Cold water pipe 20 mm runs through a column. The 20 mm cold water line passes through the 200 x 200 mm column 0.50 m above the floor. Keep pipes out of columns: route it around the column or under the slab, or ask the structural engineer first."
    );
}

#[test]
fn a_pipe_beside_a_column_or_under_the_slab_is_clear() {
    let mut els = shell();
    els.push(column("c1", 3000.0, 2000.0));
    // 20 mm pipe 111 mm from the center: 1 mm clear of the face.
    els.push(cold(
        "beside",
        &[(1000.0, 2111.0, 500.0), (5000.0, 2111.0, 500.0)],
    ));
    els.push(drain(
        "under",
        100.0,
        &[(1000.0, 2000.0, -400.0), (5000.0, 2000.0, -440.0)],
    ));
    let d = derive(els);
    assert!(issues(&d, "pipe_through_column").is_empty());
    let mut els = shell();
    els.push(column("c1", 3000.0, 2000.0));
    els.push(cold(
        "grazing",
        &[(1000.0, 2105.0, 500.0), (5000.0, 2105.0, 500.0)],
    ));
    assert_eq!(issues(&derive(els), "pipe_through_column").len(), 1);
}

#[test]
fn crossing_pipes_are_flagged_once_in_id_order() {
    let d = derive(vec![
        pipe(
            "z-hot",
            PipeSystem::HotWater,
            20.0,
            &[(2000.0, 0.0, 300.0), (2000.0, 3000.0, 300.0)],
        ),
        cold("a-cold", &[(0.0, 1000.0, 300.0), (4000.0, 1000.0, 300.0)]),
    ]);
    let found = issues(&d, "pipes_cross");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].element_ids,
        vec!["a-cold".to_string(), "z-hot".to_string()]
    );
    assert_eq!(found[0].id, "pipes_cross:a-cold,z-hot");
    assert!(near(found[0].location.unwrap(), v(2000.0, 1000.0, 300.0)));
    assert_eq!(
        found[0].message,
        "Cold water pipe 20 mm crosses the hot water pipe 20 mm. The lines touch 0.30 m above the floor where they are not joined. Move one of them so the lines clear each other."
    );
    // 25 mm apart in height: two 20 mm pipes clear each other.
    let d = derive(vec![
        pipe(
            "h",
            PipeSystem::HotWater,
            20.0,
            &[(2000.0, 0.0, 325.0), (2000.0, 3000.0, 325.0)],
        ),
        cold("c", &[(0.0, 1000.0, 300.0), (4000.0, 1000.0, 300.0)]),
    ]);
    assert!(issues(&d, "pipes_cross").is_empty());
}

#[test]
fn runs_joined_by_a_tee_are_not_flagged_but_a_second_crossing_is() {
    let joined = vec![
        cold("main", &[(0.0, 0.0, 300.0), (4000.0, 0.0, 300.0)]),
        cold("branch", &[(2000.0, 2000.0, 300.0), (2000.0, 0.0, 300.0)]),
    ];
    assert!(issues(&derive(joined), "pipes_cross").is_empty());
    // The same branch also cuts across the main somewhere else.
    let d = derive(vec![
        cold(
            "main",
            &[
                (0.0, 0.0, 300.0),
                (4000.0, 0.0, 300.0),
                (4000.0, 3000.0, 300.0),
            ],
        ),
        cold(
            "branch",
            &[
                (5000.0, 1500.0, 300.0),
                (2000.0, 1500.0, 300.0),
                (2000.0, 0.0, 300.0),
            ],
        ),
    ]);
    assert_eq!(d.pipes.tee_count, 1);
    let found = issues(&d, "pipes_cross");
    assert_eq!(found.len(), 1);
    assert!(near(found[0].location.unwrap(), v(4000.0, 1500.0, 300.0)));
    assert!(found[0]
        .message
        .ends_with("Move one of them, or join them with a fitting if they connect."));
}

#[test]
fn drain_slope_under_the_default_is_flagged() {
    // 50 mm at 1 percent: flagged.
    let d = derive(vec![drain(
        "d",
        50.0,
        &[(0.0, 0.0, -300.0), (3000.0, 0.0, -330.0)],
    )]);
    let found = issues(&d, "drain_slope_low");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].element_ids, vec!["d".to_string()]);
    assert!(near(found[0].location.unwrap(), v(1500.0, 0.0, -315.0)));
    assert_eq!(
        found[0].message,
        "Drainage pipe 50 mm falls 1.0 percent. The 3.00 m run drops 30 mm, under the 2 percent default for 50 mm drainage. Give it more fall: start it higher or connect it lower."
    );
    // 50 mm at 2 percent: fine.
    let d = derive(vec![drain(
        "d",
        50.0,
        &[(0.0, 0.0, -300.0), (3000.0, 0.0, -360.0)],
    )]);
    assert!(issues(&d, "drain_slope_low").is_empty());
    // 100 mm at 1 percent: fine.
    let d = derive(vec![drain(
        "d",
        100.0,
        &[(0.0, 0.0, -300.0), (3000.0, 0.0, -330.0)],
    )]);
    assert!(issues(&d, "drain_slope_low").is_empty());
}

#[test]
fn a_drain_that_runs_uphill_or_flat_is_flagged() {
    let d = derive(vec![drain(
        "d",
        100.0,
        &[(0.0, 0.0, -330.0), (3000.0, 0.0, -300.0)],
    )]);
    let found = issues(&d, "drain_slope_low");
    assert_eq!(found.len(), 1);
    assert!(
        found[0]
            .message
            .starts_with("Drainage pipe 100 mm runs uphill. The 3.00 m run rises 30 mm"),
        "{}",
        found[0].message
    );
    let d = derive(vec![drain(
        "d",
        100.0,
        &[(0.0, 0.0, -300.0), (3000.0, 0.0, -300.0)],
    )]);
    assert!(issues(&d, "drain_slope_low")[0]
        .message
        .starts_with("Drainage pipe 100 mm has no fall."));
}

#[test]
fn stacks_short_stubs_and_supply_lines_are_not_slope_checked() {
    let d = derive(vec![
        // Vertical stack, then a steep drop: steeper than 45 degrees.
        drain(
            "stack",
            100.0,
            &[(0.0, 0.0, 3000.0), (0.0, 0.0, -300.0), (200.0, 0.0, -600.0)],
        ),
        // 200 mm flat stub.
        drain(
            "stub",
            50.0,
            &[(1000.0, 0.0, -300.0), (1200.0, 0.0, -300.0)],
        ),
        // Water lines run level.
        cold("supply", &[(0.0, 2000.0, 300.0), (4000.0, 2000.0, 300.0)]),
    ]);
    assert!(issues(&d, "drain_slope_low").is_empty());
}

#[test]
fn later_low_segments_of_the_same_run_are_counted_in_one_item() {
    let d = derive(vec![drain(
        "d",
        50.0,
        &[
            (0.0, 0.0, -300.0),
            (1000.0, 0.0, -305.0),
            (2000.0, 0.0, -310.0),
            (3000.0, 0.0, -315.0),
        ],
    )]);
    let found = issues(&d, "drain_slope_low");
    assert_eq!(found.len(), 1);
    assert!(found[0]
        .message
        .ends_with("2 other segments of this run are under the default too."));
    assert!(near(found[0].location.unwrap(), v(500.0, 0.0, -302.5)));
}

#[test]
fn a_named_pipe_is_called_by_its_name() {
    let mut e = drain("d", 50.0, &[(0.0, 0.0, -300.0), (3000.0, 0.0, -330.0)]);
    if let Element::Pipe(p) = &mut e {
        p.name = "kitchen sink waste".into();
    }
    let d = derive(vec![e]);
    assert!(issues(&d, "drain_slope_low")[0]
        .message
        .starts_with("Kitchen sink waste falls 1.0 percent."));
}

// ---------------------------------------------------------------- take-off

#[test]
fn takeoff_groups_by_system_material_and_size_and_rounds_to_the_millimeter() {
    let d = derive(vec![
        pipe(
            "h",
            PipeSystem::HotWater,
            20.0,
            &[(0.0, 5000.0, 300.0), (1000.0, 5000.0, 300.0)],
        ),
        cold("c1", &[(0.0, 0.0, 300.0), (1000.4, 0.0, 300.0)]),
        pipe(
            "c25",
            PipeSystem::ColdWater,
            25.0,
            &[(0.0, 1000.0, 300.0), (2000.0, 1000.0, 300.0)],
        ),
        cold("c2", &[(0.0, 2000.0, 300.0), (2000.4, 2000.0, 300.0)]),
        drain(
            "d",
            100.0,
            &[(0.0, 3000.0, -300.0), (3000.0, 3000.0, -340.0)],
        ),
    ]);
    let rows: Vec<(PipeSystem, PipeMaterial, f64, f64, u32)> = d
        .pipes
        .takeoff
        .iter()
        .map(|r| (r.system, r.material, r.diameter_mm, r.length_m, r.run_count))
        .collect();
    assert_eq!(
        rows,
        vec![
            (PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, 3.001, 2),
            (PipeSystem::ColdWater, PipeMaterial::Ppr, 25.0, 2.0, 1),
            (PipeSystem::HotWater, PipeMaterial::Ppr, 20.0, 1.0, 1),
            (PipeSystem::Drainage, PipeMaterial::Upvc, 100.0, 3.0, 1),
        ]
    );
    // sqrt(3000^2 + 40^2) = 3000.27 mm, rounded to 3000.
    assert_eq!(d.pipes.total_length_m, 9.001);
    assert_eq!(d.pipes.sleeve_count, 0);
}

#[test]
fn the_pipe_takeoff_query_answers_from_derived_data() {
    let doc = Document::new(templates::plumbing_demo());
    let v = doc.query(&Query::PipeTakeoff).unwrap();
    assert_eq!(v["total_length_m"], 44.94);
    assert_eq!(v["elbow_count"], 23);
    assert_eq!(v["tee_count"], 9);
    assert_eq!(v["sleeve_count"], 7);
    assert_eq!(v["rows"].as_array().unwrap().len(), 6);
    assert_eq!(v["rows"][0]["system"], "cold_water");
    assert_eq!(v["rows"][0]["material"], "ppr");
    assert_eq!(v["penetrations"].as_array().unwrap().len(), 7);
    assert_eq!(v["penetrations"][6]["kind"], "roof");
    assert!(v["note"].as_str().unwrap().contains("Master Plumber"));
    // Pipes are listed and described like any element.
    let list = doc
        .query(&Query::ListElements {
            kind: ElementKind::Pipe,
        })
        .unwrap();
    assert_eq!(list["count"], 16);
    assert_eq!(list["elements"][0]["label"], "Service line");
    assert_eq!(list["elements"][0]["length_mm"], 10100.0);
    // Review items carry their location.
    let items = doc.query(&Query::Issues).unwrap();
    let across = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "pipe_across_opening")
        .unwrap();
    assert_eq!(across["location_mm"]["x"], 6500.0);
    assert_eq!(across["location_mm"]["z"], 1500.0);
    let summary = doc.query(&Query::ProjectSummary).unwrap();
    assert_eq!(summary["element_counts"]["pipe"], 16);
}

// -------------------------------------------------------------- validation

fn add_pipe(doc: &mut Document, e: Element) -> Result<ApplyResult, CoreError> {
    doc.apply(Command::AddElement { element: e }, Origin::User)
}

#[test]
fn invalid_pipes_are_refused() {
    let mut doc = blank();
    let one_point = cold("", &[(0.0, 0.0, 300.0)]);
    assert_eq!(
        code(add_pipe(&mut doc, one_point).unwrap_err()),
        "pipe_too_short"
    );
    let zero = cold(
        "",
        &[(0.0, 0.0, 300.0), (0.5, 0.0, 300.0), (1000.0, 0.0, 300.0)],
    );
    assert_eq!(
        code(add_pipe(&mut doc, zero).unwrap_err()),
        "pipe_zero_segment"
    );
    let tiny = pipe(
        "",
        PipeSystem::ColdWater,
        5.0,
        &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)],
    );
    assert_eq!(code(add_pipe(&mut doc, tiny).unwrap_err()), "pipe_diameter");
    let huge = pipe(
        "",
        PipeSystem::Drainage,
        400.0,
        &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)],
    );
    assert_eq!(code(add_pipe(&mut doc, huge).unwrap_err()), "pipe_diameter");
    let nan = cold("", &[(0.0, 0.0, 300.0), (f64::NAN, 0.0, 300.0)]);
    assert_eq!(code(add_pipe(&mut doc, nan).unwrap_err()), "not_finite");
    let nan_size = pipe(
        "",
        PipeSystem::ColdWater,
        f64::NAN,
        &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)],
    );
    assert_eq!(
        code(add_pipe(&mut doc, nan_size).unwrap_err()),
        "not_finite"
    );
    let mut lost = cold("", &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]);
    if let Element::Pipe(p) = &mut lost {
        p.level_id = "nowhere".into();
    }
    assert_eq!(code(add_pipe(&mut doc, lost).unwrap_err()), "unknown_level");
    assert_eq!(doc.revision(), 0, "nothing was applied");
    // Editing a pipe into a bad shape is refused the same way.
    let ok = add_pipe(
        &mut doc,
        cold("", &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
    )
    .unwrap();
    let id = ok.diff.added[0].clone();
    let bad = cold(&id, &[(0.0, 0.0, 300.0)]);
    assert_eq!(
        code(
            doc.apply(Command::UpdateElement { element: bad }, Origin::User)
                .unwrap_err()
        ),
        "pipe_too_short"
    );
}

#[test]
fn an_old_file_with_broken_pipes_still_derives() {
    // Validation keeps these out of a project; a hand-edited file may not.
    let d = derive(vec![
        cold("one", &[(0.0, 0.0, 300.0)]),
        cold("nan", &[(0.0, 0.0, 300.0), (f64::NAN, 0.0, 300.0)]),
        cold("same", &[(0.0, 0.0, 300.0), (0.0, 0.0, 300.0)]),
        cold("ok", &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
    ]);
    assert_eq!(d.pipes.takeoff.len(), 1);
    assert_eq!(d.pipes.takeoff[0].run_count, 1);
    assert_eq!(d.pipes.total_length_m, 1.0);
}

#[test]
fn preview_equals_apply_for_a_new_pipe() {
    let mut doc = Document::new(templates::plumbing_demo());
    let cmd = Command::AddElement {
        element: cold(
            "",
            &[
                (1000.0, 1000.0, 300.0),
                (1000.0, 3000.0, 300.0),
                (3000.0, 3000.0, 300.0),
            ],
        ),
    };
    // The demo's level id, not the test level.
    let cmd = match cmd {
        Command::AddElement {
            element: Element::Pipe(mut p),
        } => {
            p.level_id = doc.project().levels[0].id.clone();
            Command::AddElement {
                element: Element::Pipe(p),
            }
        }
        other => other,
    };
    let preview = doc.preview(&cmd).unwrap();
    let applied = doc.apply(cmd, Origin::User).unwrap();
    assert_eq!(preview.diff, applied.diff);
    assert_eq!(
        preview.state.project.elements,
        applied.state.project.elements
    );
    assert_eq!(preview.state.derived, applied.state.derived);
    assert_eq!(applied.diff.added.len(), 1);
    assert_eq!(applied.diff.summary, "Add pipe: added 1 pipe");
    assert_eq!(applied.state.derived.pipes.takeoff[0].length_m, 15.85);
}

#[test]
fn move_rotate_and_duplicate_carry_pipes_along() {
    let mut doc = blank();
    let added = add_pipe(
        &mut doc,
        cold("", &[(0.0, 0.0, 300.0), (1000.0, 0.0, 600.0)]),
    )
    .unwrap();
    let id = added.diff.added[0].clone();
    let pipe_of =
        |doc: &Document, id: &str| match doc.project().elements.iter().find(|e| e.id() == id) {
            Some(Element::Pipe(p)) => p.clone(),
            _ => panic!("no pipe {id}"),
        };
    doc.apply(
        Command::MoveElements {
            ids: vec![id.clone()],
            delta: p(500.0, 200.0),
            stretch_connected: true,
        },
        Origin::User,
    )
    .unwrap();
    assert_eq!(
        pipe_of(&doc, &id).points,
        vec![v(500.0, 200.0, 300.0), v(1500.0, 200.0, 600.0)]
    );
    doc.apply(
        Command::RotateElements {
            ids: vec![id.clone()],
            pivot: p(500.0, 200.0),
            angle_deg: 90.0,
        },
        Origin::User,
    )
    .unwrap();
    let rotated = pipe_of(&doc, &id);
    assert!(near(rotated.points[1], v(500.0, 1200.0, 600.0)));
    let dup = doc
        .apply(
            Command::DuplicateElements {
                ids: vec![id.clone()],
                delta: p(0.0, 1000.0),
            },
            Origin::User,
        )
        .unwrap();
    assert_eq!(dup.diff.added.len(), 1);
    assert!(near(
        pipe_of(&doc, &dup.diff.added[0]).points[0],
        v(500.0, 1200.0, 300.0)
    ));
}

// ------------------------------------------------------------ locked layers

fn lock(doc: &mut Document, key: LayerKey, locked: bool) {
    doc.apply(
        Command::SetLayer {
            layer: Layer {
                key,
                visible: true,
                locked,
            },
        },
        Origin::User,
    )
    .unwrap();
}

#[test]
fn a_locked_pipe_layer_blocks_edits_of_its_pipes() {
    let mut doc = blank();
    let added = add_pipe(
        &mut doc,
        cold("", &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
    )
    .unwrap();
    let id = added.diff.added[0].clone();
    lock(&mut doc, LayerKey::ColdWater, true);
    let revision = doc.revision();

    let err = add_pipe(
        &mut doc,
        cold("", &[(0.0, 500.0, 300.0), (1000.0, 500.0, 300.0)]),
    )
    .unwrap_err();
    assert_eq!(code(err.clone()), "layer_locked");
    assert!(
        err.to_string().contains("Cold water layer is locked"),
        "{err}"
    );
    let moved = doc.apply(
        Command::MoveElements {
            ids: vec![id.clone()],
            delta: p(100.0, 0.0),
            stretch_connected: true,
        },
        Origin::User,
    );
    assert_eq!(code(moved.unwrap_err()), "layer_locked");
    let rotated = doc.apply(
        Command::RotateElements {
            ids: vec![id.clone()],
            pivot: p(0.0, 0.0),
            angle_deg: 90.0,
        },
        Origin::User,
    );
    assert_eq!(code(rotated.unwrap_err()), "layer_locked");
    let copied = doc.apply(
        Command::DuplicateElements {
            ids: vec![id.clone()],
            delta: p(0.0, 500.0),
        },
        Origin::User,
    );
    assert_eq!(code(copied.unwrap_err()), "layer_locked");
    let deleted = doc.apply(
        Command::DeleteElements {
            ids: vec![id.clone()],
        },
        Origin::User,
    );
    assert_eq!(code(deleted.unwrap_err()), "layer_locked");
    // Moving the pipe to an unlocked system edits the locked layer too.
    let mut hot = cold(&id, &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]);
    if let Element::Pipe(p) = &mut hot {
        p.system = PipeSystem::HotWater;
    }
    let changed = doc.apply(
        Command::UpdateElement {
            element: hot.clone(),
        },
        Origin::User,
    );
    assert_eq!(code(changed.unwrap_err()), "layer_locked");
    // A batch that fails part way changes nothing.
    let batch = doc.apply(
        Command::Batch {
            label: "AI: tidy pipes".into(),
            commands: vec![
                Command::AddElement {
                    element: pipe(
                        "",
                        PipeSystem::HotWater,
                        20.0,
                        &[(0.0, 900.0, 300.0), (1000.0, 900.0, 300.0)],
                    ),
                },
                Command::DeleteElements {
                    ids: vec![id.clone()],
                },
            ],
        },
        Origin::Ai,
    );
    assert_eq!(code(batch.unwrap_err()), "layer_locked");
    assert_eq!(doc.revision(), revision, "nothing was applied");

    // Other pipe layers stay open, and unlocking opens this one again.
    add_pipe(
        &mut doc,
        pipe(
            "",
            PipeSystem::HotWater,
            20.0,
            &[(0.0, 900.0, 300.0), (1000.0, 900.0, 300.0)],
        ),
    )
    .unwrap();
    lock(&mut doc, LayerKey::ColdWater, false);
    doc.apply(Command::UpdateElement { element: hot }, Origin::User)
        .unwrap();
    // Now hot water: locking hot water blocks a move back to cold water.
    lock(&mut doc, LayerKey::HotWater, true);
    let back = cold(&id, &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]);
    assert_eq!(
        code(
            doc.apply(Command::UpdateElement { element: back }, Origin::User)
                .unwrap_err()
        ),
        "layer_locked"
    );
}

#[test]
fn locking_one_system_does_not_block_other_layers() {
    let mut doc = blank();
    lock(&mut doc, LayerKey::Drainage, true);
    doc.apply(add_wall(p(0.0, 0.0), p(4000.0, 0.0)), Origin::User)
        .unwrap();
    add_pipe(
        &mut doc,
        cold("", &[(0.0, 500.0, 300.0), (1000.0, 500.0, 300.0)]),
    )
    .unwrap();
    let err = add_pipe(
        &mut doc,
        drain("", 50.0, &[(0.0, 900.0, -300.0), (1000.0, 900.0, -330.0)]),
    )
    .unwrap_err();
    assert_eq!(code(err), "layer_locked");
}

#[test]
fn a_hidden_pipe_layer_does_not_block_edits() {
    let mut doc = blank();
    doc.apply(
        Command::SetLayer {
            layer: Layer {
                key: LayerKey::Vent,
                visible: false,
                locked: false,
            },
        },
        Origin::User,
    )
    .unwrap();
    let added = add_pipe(
        &mut doc,
        pipe(
            "",
            PipeSystem::Vent,
            50.0,
            &[(0.0, 0.0, 300.0), (0.0, 0.0, 3000.0)],
        ),
    )
    .unwrap();
    let id = added.diff.added[0].clone();
    doc.apply(Command::DeleteElements { ids: vec![id] }, Origin::User)
        .unwrap();
}

// --------------------------------------------------------------- migration

#[test]
fn a_version_1_project_opens_with_all_layers_and_schema_2() {
    let mut v1 = serde_json::to_value(templates::sample_bungalow()).unwrap();
    v1["schema_version"] = serde_json::json!(1);
    let layers = v1["layers"].as_array_mut().unwrap();
    layers.truncate(9);
    layers[2]["locked"] = serde_json::json!(true);
    let old: Project = serde_json::from_value(v1).unwrap();
    assert_eq!(old.layers.len(), 9);

    let doc = Document::new(old.clone());
    let project = doc.project();
    assert_eq!(project.schema_version, SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 2);
    let keys: Vec<LayerKey> = project.layers.iter().map(|l| l.key).collect();
    let expected: Vec<LayerKey> = defaults::default_layers().iter().map(|l| l.key).collect();
    assert_eq!(keys, expected);
    assert_eq!(keys.len(), 13);
    assert!(project.layers[2].locked, "existing layers keep their state");
    for l in &project.layers[9..] {
        assert!(l.visible && !l.locked);
    }
    assert!(doc.derived().pipes.fittings.is_empty());

    // Also on a restore, and running it again changes nothing.
    let restored = Document::with_revision(old.clone(), 7);
    assert_eq!(restored.project().layers.len(), 13);
    let mut twice = old;
    migrate(&mut twice);
    let once = twice.clone();
    migrate(&mut twice);
    assert_eq!(once, twice);
}

// ---------------------------------------------------------- plumbing demo

#[test]
fn the_plumbing_demo_known_answers() {
    let doc = Document::new(templates::plumbing_demo());
    let state = doc.state();
    let d = &state.derived;
    let project = &state.project;

    assert_eq!(project.name, "Bungalow with plumbing");
    let count = |kind: ElementKind| project.elements.iter().filter(|e| e.kind() == kind).count();
    assert_eq!(count(ElementKind::Wall), 7);
    assert_eq!(count(ElementKind::Opening), 8);
    assert_eq!(count(ElementKind::Room), 3);
    assert_eq!(count(ElementKind::Column), 6);
    assert_eq!(count(ElementKind::Asset), 7);
    assert_eq!(count(ElementKind::Pipe), 16);
    assert_eq!(d.rooms.len(), 3, "every room has its face");
    assert_eq!(
        (
            d.totals.room_count,
            d.totals.door_count,
            d.totals.window_count
        ),
        (3, 3, 5)
    );

    // Take-off, as the concept counted it.
    let rows: Vec<(PipeSystem, f64, f64, u32)> = d
        .pipes
        .takeoff
        .iter()
        .map(|r| (r.system, r.diameter_mm, r.length_m, r.run_count))
        .collect();
    assert_eq!(
        rows,
        vec![
            (PipeSystem::ColdWater, 20.0, 11.85, 6),
            (PipeSystem::ColdWater, 25.0, 10.1, 1),
            (PipeSystem::HotWater, 20.0, 7.47, 2),
            (PipeSystem::Drainage, 50.0, 7.312, 3),
            (PipeSystem::Drainage, 100.0, 3.951, 3),
            (PipeSystem::Vent, 50.0, 4.257, 1),
        ]
    );
    assert_eq!(d.pipes.total_length_m, 44.94);

    // Fittings. 22 turns inside runs, plus the chase end meeting the shower
    // riser at 90 degrees. 7 ends on another run's body, plus two points
    // where three run ends meet (service line, chase and sink supply; the
    // building drain start with the lavatory and sink wastes).
    assert_eq!(d.pipes.elbow_count, 23);
    assert_eq!(d.pipes.tee_count, 9);
    let end_elbow = d
        .pipes
        .fittings
        .iter()
        .find(|f| f.kind == FittingKind::Elbow && near(f.position, v(7450.0, 6000.0, 300.0)))
        .expect("an elbow where the chase meets the shower riser");
    assert_eq!(end_elbow.angle_deg, 90.0);
    let three_way = d
        .pipes
        .fittings
        .iter()
        .find(|f| f.kind == FittingKind::Tee && near(f.position, v(2000.0, 6000.0, 300.0)))
        .expect("a tee where three cold water runs meet");
    assert_eq!(three_way.diameter_mm, 25.0);

    // Penetrations: 6 through the slab, 1 through the roof, none through walls.
    assert_eq!(pens(d, PenetrationKind::Slab).len(), 6);
    assert_eq!(pens(d, PenetrationKind::Wall).len(), 0);
    let roof = pens(d, PenetrationKind::Roof);
    assert_eq!(roof.len(), 1);
    let vent = project.elements.iter().find_map(|e| match e {
        Element::Pipe(p) if p.name == "Vent stack" => Some(p.id.clone()),
        _ => None,
    });
    assert_eq!(Some(roof[0].pipe_id.clone()), vent);
    assert_eq!(d.pipes.sleeve_count, 7);

    // Review items: exactly the concept's four, and nothing from rooms or
    // openings.
    let codes: Vec<&str> = d.issues.iter().map(|i| i.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            "pipe_across_opening",
            "pipe_through_column",
            "drain_slope_low",
            "pipe_penetrations"
        ]
    );
    let messages: Vec<&str> = d.issues.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "Heater feed crosses the T&B door. The 20 mm cold water line runs 1.50 m above the floor through the 700 mm opening. Route it above the door head at 2.10 m or under the slab.",
            "Cold water chase runs through a column. The 20 mm cold water line passes through the 200 x 200 mm column 0.30 m above the floor. Keep pipes out of columns: route it around the column or under the slab, or ask the structural engineer first.",
            "Kitchen sink waste falls 0.8 percent. The 4.30 m run drops 34 mm, under the 2 percent default for 50 mm drainage. Give it more fall: start it higher or connect it lower.",
            "7 pipe penetrations need sleeves or flashing: 6 through the floor slab, 1 through the roof. Set the sleeves before the pour or the blockwork, and detail the flashing before the roofing.",
        ]
    );
    // Every item but the summary is located; none reads like an approval.
    for i in &d.issues {
        assert_eq!(
            i.location.is_some(),
            i.code != "pipe_penetrations",
            "{}",
            i.code
        );
        let lower = i.message.to_lowercase();
        for word in ["violat", "non-compliant", "approved", "complies"] {
            assert!(!lower.contains(word), "{}", i.message);
        }
        assert!(!i.message.contains('\u{2014}') && !i.message.contains('\u{2013}'));
    }
    let column = d
        .issues
        .iter()
        .find(|i| i.code == "pipe_through_column")
        .unwrap();
    assert!(near(column.location.unwrap(), v(5000.0, 6000.0, 300.0)));
    let slope = d
        .issues
        .iter()
        .find(|i| i.code == "drain_slope_low")
        .unwrap();
    assert!(near(slope.location.unwrap(), v(4150.0, 5700.0, -317.0)));

    // Derived data is deterministic.
    assert_eq!(compute_derived(project), *d);
}

#[test]
fn the_plumbing_demo_fixture_matches_the_engine() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/plumbing-demo.docstate.json");
    let golden: DocState =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture exists"))
            .expect("fixture parses");
    let live = Document::new(templates::plumbing_demo()).state();
    assert_eq!(
        golden.project, live.project,
        "fixture project differs from templates::plumbing_demo()"
    );
    assert_eq!(
        golden.derived, live.derived,
        "regenerate: cargo run -p guhit-core --example gen_fixture"
    );
}

// ------------------------------------------------------------------- levels

#[test]
fn upper_levels_use_their_own_slab_and_only_the_top_level_meets_the_roof() {
    let mut project = blank_project();
    project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 2800.0,
    });
    project.roof.kind = RoofKind::Flat;
    let on = |mut e: Element, level: &str| {
        match &mut e {
            Element::Wall(w) => w.level_id = level.into(),
            Element::Pipe(p) => p.level_id = level.into(),
            _ => {}
        }
        e
    };
    for w in shell() {
        project.elements.push(w.clone());
        let mut upper = on(w, "level-2");
        *upper.id_mut() = format!("{}-2", upper.id());
        project.elements.push(upper);
    }
    // A waste through the upper floor, and a vent on the ground floor that
    // rises through the upper floor and the roof.
    project.elements.push(on(
        drain(
            "waste",
            50.0,
            &[(1000.0, 1000.0, 300.0), (1000.0, 1000.0, -400.0)],
        ),
        "level-2",
    ));
    project.elements.push(on(
        pipe(
            "vent",
            PipeSystem::Vent,
            50.0,
            &[(2000.0, 1000.0, 1000.0), (2000.0, 1000.0, 7000.0)],
        ),
        "level-1",
    ));
    let d = compute_derived(&project);
    let slab = pens(&d, PenetrationKind::Slab);
    assert_eq!(
        slab.len(),
        1,
        "a slab crossing counts on the pipe's own level"
    );
    assert_eq!(slab[0].level_id, "level-2");
    // The 200 mm suspended slab the 3D view draws under an upper level.
    assert!(near(slab[0].position, v(1000.0, 1000.0, -100.0)));
    assert!(
        pens(&d, PenetrationKind::Roof).is_empty(),
        "the roof sits on the top level only"
    );

    // The same vent drawn on the top level goes through the flat roof at the
    // top level's wall top (2800) plus half the 150 mm roof.
    project.elements.push(on(
        pipe(
            "vent-2",
            PipeSystem::Vent,
            50.0,
            &[(3000.0, 1000.0, 1000.0), (3000.0, 1000.0, 4000.0)],
        ),
        "level-2",
    ));
    let d = compute_derived(&project);
    let roof = pens(&d, PenetrationKind::Roof);
    assert_eq!(roof.len(), 1);
    assert!(near(roof[0].position, v(3000.0, 1000.0, 2875.0)));
}

#[test]
fn runs_on_two_levels_meet_at_their_real_height() {
    // A stack on the ground floor ends at 3000, where a second floor run
    // starts on its floor: they join straight, no clash and no fitting.
    let mut project = blank_project();
    project.levels.push(Level {
        id: "level-2".into(),
        name: "2F".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    project.elements.push(pipe(
        "low",
        PipeSystem::Vent,
        50.0,
        &[(0.0, 0.0, 300.0), (0.0, 0.0, 3000.0)],
    ));
    let mut high = pipe(
        "high",
        PipeSystem::Vent,
        50.0,
        &[(0.0, 0.0, 0.0), (0.0, 0.0, 2000.0)],
    );
    if let Element::Pipe(p) = &mut high {
        p.level_id = "level-2".into();
    }
    project.elements.push(high);
    let d = compute_derived(&project);
    assert!(d.pipes.fittings.is_empty());
    assert!(issues(&d, "pipes_cross").is_empty());
}

/// Timing check: a large plumbing layout, far busier than a house.
/// Run: cargo test --release -p guhit-core --test pipes -- --ignored --nocapture
#[test]
#[ignore]
fn perf_smoke_many_pipes() {
    let mut project = templates::plumbing_demo();
    let level = project.levels[0].id.clone();
    let base: Vec<Element> = project.elements.clone();
    // Nine copies of the demo house side by side: 144 runs, 63 walls.
    for k in 1..9 {
        let dx = (k % 3) as f64 * 14_000.0;
        let dy = (k / 3) as f64 * 10_000.0;
        for e in &base {
            let mut c = e.clone();
            *c.id_mut() = format!("{}-{k}", c.id());
            match &mut c {
                Element::Wall(w) => {
                    w.start = p(w.start.x + dx, w.start.y + dy);
                    w.end = p(w.end.x + dx, w.end.y + dy);
                }
                Element::Opening(o) => o.wall_id = format!("{}-{k}", o.wall_id),
                Element::Pipe(pp) => {
                    for q in pp.points.iter_mut() {
                        q.x += dx;
                        q.y += dy;
                    }
                }
                Element::Column(c) => c.center = p(c.center.x + dx, c.center.y + dy),
                _ => continue,
            }
            project.elements.push(c);
        }
    }
    let _ = level;
    let t = std::time::Instant::now();
    let d = compute_derived(&project);
    let took = t.elapsed();
    println!(
        "perf: {} runs, {} fittings, {} penetrations, {} issues in {took:?}",
        project
            .elements
            .iter()
            .filter(|e| e.kind() == ElementKind::Pipe)
            .count(),
        d.pipes.fittings.len(),
        d.pipes.penetrations.len(),
        d.issues.len()
    );
    assert_eq!(d.pipes.tee_count, 9 * 9);
}
