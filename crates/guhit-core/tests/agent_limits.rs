//! Agents' limits and claims (DECISIONS D47) in `guhit_core::scope`: level
//! and kind limits, what a claim reaches, and where an agent works.

use guhit_core::{compute_derived, scope, templates, CoreError};
use guhit_model::*;

// Fixed ids from `templates::sample_bungalow`: the Living / Dining west of a
// partition at x = 5000, the Bedroom east of it.
const GROUND: &str = "00000000-0000-4000-8000-0000000000a1";
const BEDROOM: &str = "00000000-0000-4000-8000-000000000302";
const SOUTH: &str = "00000000-0000-4000-8000-000000000101";
const EAST: &str = "00000000-0000-4000-8000-000000000102";
const BEDROOM_WINDOW: &str = "00000000-0000-4000-8000-000000000204";
const BED: &str = "00000000-0000-4000-8000-000000000401";

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// The bungalow with an empty second level, "upper".
fn house() -> Project {
    let mut project = templates::sample_bungalow();
    let mut upper = project.levels[0].clone();
    upper.id = "upper".into();
    upper.name = "Second Floor".into();
    upper.elevation_mm = 3000.0;
    project.levels.push(upper);
    project
}

fn wall_on(level: &str) -> Command {
    Command::AddWall {
        start: p(1000.0, 1000.0),
        end: p(3000.0, 1000.0),
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: Some(level.into()),
        role: None,
    }
}

fn moved(id: &str) -> Command {
    Command::MoveElements { ids: vec![id.into()], delta: p(0.0, 100.0), stretch_connected: false }
}

fn message(e: CoreError) -> String {
    match e {
        CoreError::Invalid { code, message, .. } => {
            assert_eq!(code, "out_of_scope");
            message
        }
        other => panic!("expected out_of_scope, got {other:?}"),
    }
}

#[test]
fn a_level_limit_keeps_an_agent_on_its_levels() {
    let project = house();
    let derived = compute_derived(&project);
    let limit = AgentLimit { level_ids: vec!["upper".into()], ids: vec![], kinds: vec![] };
    let check = |c: &Command| scope::check_limit(&project, &derived, &project, &limit, c);
    assert!(check(&wall_on("upper")).is_ok());
    assert_eq!(
        message(check(&wall_on(GROUND)).unwrap_err()),
        "The new wall would be on Ground Floor, outside what this agent is limited to."
    );
    assert!(message(check(&moved(SOUTH)).unwrap_err()).starts_with("Wall 8000 mm is on Ground Floor,"));
    // The roof over the top floor stands on the topmost level with a
    // building, the ground floor here, so this agent may not change it.
    let roof = Command::SetRoof { roof: Roof { kind: RoofKind::Hip, ..project.top_roof() } };
    assert_eq!(
        message(check(&roof).unwrap_err()),
        "Roof over the top floor is on Ground Floor, outside what this agent is limited to."
    );
    let settings = Command::SetProjectSettings { settings: project.settings.clone() };
    assert!(message(check(&settings).unwrap_err()).starts_with("Changing the project settings affects the whole project"));
    let level = Command::DeleteLevel { level_id: "upper".into() };
    assert!(message(check(&level).unwrap_err()).starts_with("Deleting a level"));
    // A batch: a wall it adds on an allowed level can be built on.
    let mut staged = project.clone();
    staged.elements.push(Element::Wall(Wall {
        id: "made".into(),
        level_id: "upper".into(),
        start: p(0.0, 0.0),
        end: p(2000.0, 0.0),
        ..match &project.elements.iter().find(|e| e.id() == SOUTH).unwrap() {
            Element::Wall(w) => w.clone(),
            _ => unreachable!(),
        }
    }));
    let batch = Command::Batch { label: "b".into(), commands: vec![wall_on("upper"), moved("made")] };
    assert!(scope::check_limit(&project, &derived, &staged, &limit, &batch).is_ok());
}

#[test]
fn a_kind_limit_keeps_an_agent_to_its_kinds() {
    let project = house();
    let derived = compute_derived(&project);
    let limit = AgentLimit { level_ids: vec![], ids: vec![], kinds: vec![ElementKind::Opening, ElementKind::Asset] };
    let check = |c: &Command| scope::check_limit(&project, &derived, &project, &limit, c);
    assert!(check(&moved(BED)).is_ok());
    assert!(check(&moved(BEDROOM_WINDOW)).is_ok());
    assert_eq!(
        message(check(&wall_on(GROUND)).unwrap_err()),
        "The new wall is a wall, outside what this agent is limited to."
    );
    assert!(message(check(&moved(EAST)).unwrap_err()).starts_with("Wall 6000 mm is a wall,"));
    // Resizing a room moves its walls.
    let resize = Command::ResizeRoom { room_id: BEDROOM.into(), side: Side::East, delta_mm: 300.0 };
    assert!(check(&resize).is_err());
    // The roof over the top floor is a roof (P1-3): refused here, allowed
    // to a roofs-only agent.
    let roof = Command::SetRoof { roof: Roof { kind: RoofKind::Hip, ..project.top_roof() } };
    assert_eq!(
        message(check(&roof).unwrap_err()),
        "Roof over the top floor is a roof, outside what this agent is limited to."
    );
    let roofs = AgentLimit { level_ids: vec![], ids: vec![], kinds: vec![ElementKind::Roof] };
    assert!(scope::check_limit(&project, &derived, &project, &roofs, &roof).is_ok());
    // An element limit words D30's refusal for the agent.
    let limit = AgentLimit { level_ids: vec![], ids: vec![BEDROOM.into()], kinds: vec![] };
    let e = scope::check_limit(&project, &derived, &project, &limit, &wall_on("upper")).unwrap_err();
    assert!(message(e).ends_with("outside the elements this agent is limited to."));
}

#[test]
fn a_claim_reaches_its_levels_and_its_rooms() {
    let project = house();
    let derived = compute_derived(&project);
    let reach = |levels: &[&str], ids: &[&str], c: &Command| {
        let levels: Vec<Id> = levels.iter().map(|s| s.to_string()).collect();
        let ids: Vec<Id> = ids.iter().map(|s| s.to_string()).collect();
        scope::claim_reach(&project, &derived, &project, &levels, &ids, c)
    };
    assert_eq!(reach(&["upper"], &[], &wall_on("upper")).map(|r| r.0), Some("Second Floor".to_string()));
    assert_eq!(reach(&["upper"], &[], &wall_on(GROUND)), None);
    assert_eq!(reach(&[GROUND], &[], &moved(SOUTH)).map(|r| r.1), Some(vec![SOUTH.to_string()]));
    // A claimed room reaches its walls and what stands in it.
    assert!(reach(&[], &[BEDROOM], &moved(EAST)).is_some());
    assert!(reach(&[], &[BEDROOM], &moved(BED)).is_some());
    assert!(reach(&[], &[BEDROOM], &moved(BEDROOM_WINDOW)).is_some());
    // The roof and settings are nobody's claim.
    let roof = Command::SetRoof { roof: project.top_roof() };
    assert_eq!(reach(&[GROUND, "upper"], &[], &roof), None);
    let level = Command::DeleteLevel { level_id: "upper".into() };
    assert!(reach(&["upper"], &[], &level).is_some());
}

#[test]
fn an_agent_works_where_most_of_what_it_touched_is() {
    let project = house();
    let area = scope::work_area(&project, &[SOUTH.into(), EAST.into(), "nothing".into()]).unwrap();
    assert_eq!(area.level_id, GROUND);
    assert_eq!((area.min, area.max), (p(0.0, 0.0), p(8000.0, 6000.0)));
    assert_eq!(area.center, p(4000.0, 3000.0));
    assert_eq!(scope::work_area(&project, &["nothing".into()]), None);
}

#[test]
fn claims_overlap_by_reach_not_by_id() {
    let project = house();
    let derived = compute_derived(&project);
    let ids = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<Id>>();
    let meet = |a: (&[Id], &[Id]), b: (&[Id], &[Id])| scope::claims_overlap(&project, &derived, a, b);
    let (ground, upper, none) = (ids(&[GROUND]), ids(&["upper"]), ids(&[]));
    assert_eq!(meet((&ground, &none), (&ground, &none)), Some("Ground Floor".to_string()));
    assert_eq!(meet((&upper, &none), (&ground, &none)), None);
    // An element on the other's level, either way round.
    assert!(meet((&none, &ids(&[BED])), (&ground, &none)).is_some());
    assert!(meet((&ground, &none), (&none, &ids(&[BEDROOM]))).unwrap().starts_with("Room Bedroom"));
    // A room and one of its walls meet; the room and the upper floor do not.
    assert!(meet((&none, &ids(&[EAST])), (&none, &ids(&[BEDROOM]))).unwrap().starts_with("Wall"));
    assert_eq!(meet((&upper, &none), (&none, &ids(&[BEDROOM]))), None);
}

/// The roof over the top floor stands on the topmost level with a building
/// (D54). A level limit lets an agent change it, or put posts under it, only
/// when the limit names that level; a kinds-only limit with roofs always
/// does. With no building yet it stands on the highest level.
#[test]
fn the_roof_over_the_top_floor_reaches_the_topmost_level_with_a_building() {
    let limit = |levels: &[&str], kinds: &[ElementKind]| AgentLimit {
        level_ids: levels.iter().map(|s| s.to_string()).collect(),
        ids: vec![],
        kinds: kinds.to_vec(),
    };
    let check = |p: &Project, l: &AgentLimit, c: &Command| scope::check_limit(p, &compute_derived(p), p, l, c);
    let refused = |p: &Project, l: &AgentLimit, c: &Command| message(check(p, l, c).unwrap_err());
    let edits = |p: &Project| {
        let roof = p.top_roof_area().expect("the bungalow has a roof over the top floor").clone();
        vec![
            Command::SetRoof { roof: Roof { kind: RoofKind::Hip, ..p.top_roof() } },
            Command::UpdateElement { element: Element::Roof(RoofArea { pitch_deg: 30.0, ..roof.clone() }) },
            Command::DeleteElements { ids: vec![roof.id.clone()] },
            Command::AddRoofPosts { roof_id: roof.id, size_mm: 200.0, max_spacing_mm: 3000.0 },
        ]
    };
    let on = |level: &str| format!("Roof over the top floor is on {level}, outside what this agent is limited to.");

    // Only the ground floor has a building, so it carries the roof.
    let house = house();
    for c in &edits(&house) {
        assert!(check(&house, &limit(&[GROUND], &[]), c).is_ok(), "{c:?}");
        assert!(check(&house, &limit(&[GROUND, "upper"], &[]), c).is_ok(), "{c:?}");
        assert_eq!(refused(&house, &limit(&["upper"], &[]), c), on("Ground Floor"), "{c:?}");
    }

    // A kinds-only limit with roofs reaches it on any level; posts are
    // columns too. A level limit still applies beside a kind limit.
    let [set, update, delete, posts] = <[Command; 4]>::try_from(edits(&house)).unwrap();
    for c in [&set, &update, &delete] {
        assert!(check(&house, &limit(&[], &[ElementKind::Roof]), c).is_ok(), "{c:?}");
        assert_eq!(refused(&house, &limit(&["upper"], &[ElementKind::Roof]), c), on("Ground Floor"));
        assert_eq!(
            refused(&house, &limit(&[], &[ElementKind::Wall]), c),
            "Roof over the top floor is a roof, outside what this agent is limited to."
        );
    }
    assert!(refused(&house, &limit(&[], &[ElementKind::Roof]), &posts).contains("a column"));
    assert!(check(&house, &limit(&[], &[ElementKind::Roof, ElementKind::Column]), &posts).is_ok());

    // Build on the second floor: the roof moves up with the top floor.
    let mut two = house.clone();
    let upper_walls: Vec<Element> = two
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if w.level_id == GROUND => {
                Some(Element::Wall(Wall { id: format!("upper-{}", w.id), level_id: "upper".into(), ..w.clone() }))
            }
            _ => None,
        })
        .collect();
    two.elements.extend(upper_walls);
    for c in &edits(&two) {
        assert!(check(&two, &limit(&["upper"], &[]), c).is_ok(), "{c:?}");
        assert_eq!(refused(&two, &limit(&[GROUND], &[]), c), on("Second Floor"), "{c:?}");
    }

    // No building at all: the highest level. A new roof is worded as new.
    let mut bare = house.clone();
    bare.elements.retain(|e| matches!(e, Element::Roof(_)));
    for c in &edits(&bare) {
        assert!(check(&bare, &limit(&["upper"], &[]), c).is_ok(), "{c:?}");
        assert_eq!(refused(&bare, &limit(&[GROUND], &[]), c), on("Second Floor"), "{c:?}");
    }
    bare.elements.clear();
    let new_roof = Command::SetRoof { roof: Roof { kind: RoofKind::Gable, ..house.top_roof() } };
    assert!(check(&bare, &limit(&["upper"], &[]), &new_roof).is_ok());
    assert_eq!(
        refused(&bare, &limit(&[GROUND], &[]), &new_roof),
        "The new roof over the top floor would be on Second Floor, outside what this agent is limited to."
    );
}
