//! Design review items. These are suggestions for the designer, never
//! statements about permits, codes or structure. Severity is info or
//! warning only. Ids are stable for the same finding on the same elements.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::geom::*;
use crate::rooms::{face, FaceRef};
use crate::topo::{Analysis, Face};

/// A wall end this close to another wall, but not joined, is probably a slip.
const NEAR_MISS_MM: f64 = 300.0;
/// Doors narrower than this get a note.
const NARROW_DOOR_MM: f64 = 700.0;
/// Openings closer than this to a corner or junction get a note.
const CORNER_CLEARANCE_MM: f64 = 100.0;

pub(crate) fn issue(
    code: &str,
    severity: Severity,
    message: String,
    element_ids: Vec<Id>,
) -> Issue {
    // The order of `element_ids` is part of the finding (for example which
    // wall has the loose end), so it is part of the id.
    Issue {
        id: format!("{code}:{}", element_ids.join(",")),
        severity,
        code: code.to_string(),
        message,
        element_ids,
        location: None,
    }
}

pub fn usage_label(usage: RoomUsage) -> &'static str {
    match usage {
        RoomUsage::Living => "living room",
        RoomUsage::Dining => "dining room",
        RoomUsage::Kitchen => "kitchen",
        RoomUsage::Bedroom => "bedroom",
        RoomUsage::MasterBedroom => "master bedroom",
        RoomUsage::Bathroom => "bathroom",
        RoomUsage::PowderRoom => "powder room",
        RoomUsage::Laundry => "laundry",
        RoomUsage::Garage => "garage",
        RoomUsage::Porch => "porch",
        RoomUsage::Hallway => "hallway",
        RoomUsage::Storage => "storage room",
        RoomUsage::Office => "office",
        RoomUsage::Other => "room",
    }
}

/// Rooms people stay in, where daylight and air matter most.
fn is_habitable(usage: RoomUsage) -> bool {
    matches!(
        usage,
        RoomUsage::Living
            | RoomUsage::Dining
            | RoomUsage::Kitchen
            | RoomUsage::Bedroom
            | RoomUsage::MasterBedroom
            | RoomUsage::Office
    )
}

/// A comfortable minimum floor area per usage, in m2. A rule of thumb for
/// the designer, not a regulation.
fn small_room_m2(usage: RoomUsage) -> Option<f64> {
    Some(match usage {
        RoomUsage::Living => 9.0,
        RoomUsage::Dining => 6.0,
        RoomUsage::Kitchen => 4.0,
        RoomUsage::Bedroom => 6.0,
        RoomUsage::MasterBedroom => 9.0,
        RoomUsage::Bathroom => 1.8,
        RoomUsage::PowderRoom => 1.2,
        RoomUsage::Laundry => 1.5,
        RoomUsage::Garage => 12.0,
        RoomUsage::Office => 5.0,
        _ => return None,
    })
}

pub fn opening_center(wall: &Wall, o: &Opening) -> Option<Point> {
    unit(sub(wall.end, wall.start)).map(|d| add(wall.start, scale(d, o.offset_mm)))
}

/// Openings that sit on the boundary of `f`, with a flag that tells whether
/// that stretch of wall faces the outside.
pub fn openings_of_face<'a>(project: &'a Project, f: &Face) -> Vec<(&'a Opening, bool)> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        if !f.wall_ids.contains(&o.wall_id) {
            continue;
        }
        let Some(Element::Wall(w)) = project.elements.iter().find(|e| e.id() == &o.wall_id) else {
            continue;
        };
        let Some(c) = opening_center(w, o) else {
            continue;
        };
        for edge in &f.boundary {
            if edge.wall_ids.contains(&o.wall_id)
                && dist_point_segment(c, edge.from, edge.to) <= 2.0 * JOIN_EPS
            {
                out.push((o, edge.exterior));
                break;
            }
        }
    }
    out
}

pub fn review(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
) -> Vec<Issue> {
    let mut issues = vec![];
    let walls: Vec<&Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(w),
            _ => None,
        })
        .collect();

    // Rooms.
    for el in &project.elements {
        let Element::Room(r) = el else { continue };
        let Some(fref) = assigned.get(&r.id) else {
            continue;
        };
        let f = face(analysis, *fref);
        let openings = openings_of_face(project, f);
        let area_m2 = f.area_mm2 / 1.0e6;

        if is_habitable(r.usage)
            && !openings
                .iter()
                .any(|(o, ext)| *ext && o.opening_type == OpeningType::Window)
        {
            let has_outside_wall = f.boundary.iter().any(|e| e.exterior);
            let message = if has_outside_wall {
                format!("{} has no window on an outside wall. A window there would bring in daylight and air.", r.name)
            } else {
                format!("{} has no outside wall, so it cannot get a window. Consider how it gets daylight and air.", r.name)
            };
            issues.push(issue(
                "room_no_window",
                Severity::Warning,
                message,
                vec![r.id.clone()],
            ));
        }

        if r.usage != RoomUsage::Porch
            && !openings
                .iter()
                .any(|(o, _)| o.opening_type == OpeningType::Door)
        {
            let severity = if r.usage == RoomUsage::Other {
                Severity::Info
            } else {
                Severity::Warning
            };
            issues.push(issue(
                "room_no_door",
                severity,
                format!("{} has no door yet, so there is no way in.", r.name),
                vec![r.id.clone()],
            ));
        }

        if let Some(min) = small_room_m2(r.usage) {
            if area_m2 < min {
                issues.push(issue(
                    "room_small",
                    Severity::Info,
                    format!(
                        "{} is {:.2} m2, which is tight for a {}. Around {:.1} m2 or more is more comfortable.",
                        r.name,
                        area_m2,
                        usage_label(r.usage),
                        min
                    ),
                    vec![r.id.clone()],
                ));
            }
        }
    }

    // Openings.
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        if o.opening_type == OpeningType::Door && o.width_mm < NARROW_DOOR_MM {
            issues.push(issue(
                "door_narrow",
                Severity::Warning,
                format!(
                    "This door is {:.0} mm wide. Doors under {:.0} mm are hard to walk through and to move furniture through.",
                    o.width_mm, NARROW_DOOR_MM
                ),
                vec![o.id.clone()],
            ));
        }
        let Some(topo) = analysis.wall(&o.wall_id) else {
            continue;
        };
        let noun = match o.opening_type {
            OpeningType::Door => "door",
            OpeningType::Window => "window",
        };
        let (lo, hi) = (
            o.offset_mm - o.width_mm / 2.0,
            o.offset_mm + o.width_mm / 2.0,
        );
        let mut blocked_by: Option<&Id> = None;
        let mut near = false;
        for (at, other) in &topo.junctions_mm {
            let half = walls
                .iter()
                .find(|w| &w.id == other)
                .map(|w| w.thickness_mm / 2.0)
                .unwrap_or(0.0);
            if *at + half > lo && *at - half < hi {
                blocked_by = Some(other);
            } else if *at + half + CORNER_CLEARANCE_MM > lo && *at - half - CORNER_CLEARANCE_MM < hi
            {
                near = true;
            }
        }
        let joined_start = !topo.joined_at_start.is_empty();
        let joined_end = !topo.joined_at_end.is_empty();
        if (joined_start && lo < CORNER_CLEARANCE_MM)
            || (joined_end && hi > topo.length_mm - CORNER_CLEARANCE_MM)
        {
            near = true;
        }
        if let Some(other) = blocked_by {
            issues.push(issue(
                "opening_blocked",
                Severity::Warning,
                format!("Another wall meets this wall right where the {noun} is. Move the {noun} or the wall."),
                vec![o.id.clone(), other.clone()],
            ));
        } else if near {
            issues.push(issue(
                "opening_near_corner",
                Severity::Info,
                format!(
                    "This {noun} is less than {:.0} mm from a corner or wall junction. Leave some room for the frame.",
                    CORNER_CLEARANCE_MM
                ),
                vec![o.id.clone()],
            ));
        }
    }

    // Walls: loose ends and overlaps.
    for (wi, w) in walls.iter().enumerate() {
        let Some(topo) = analysis.wall(&w.id) else {
            continue;
        };
        if topo.outline.is_empty() {
            continue;
        }
        let level_has_rooms = analysis
            .level(&w.level_id)
            .map(|l| !l.faces.is_empty())
            .unwrap_or(false);
        for (end_name, p, joined) in [
            ("start", w.start, !topo.joined_at_start.is_empty()),
            ("end", w.end, !topo.joined_at_end.is_empty()),
        ] {
            if joined {
                continue;
            }
            let mut nearest: Option<(f64, &Id)> = None;
            for (oi, o) in walls.iter().enumerate() {
                // Walls already joined to this one are not a near miss.
                if oi == wi
                    || o.level_id != w.level_id
                    || topo.joined_at_start.contains(&o.id)
                    || topo.joined_at_end.contains(&o.id)
                {
                    continue;
                }
                let d = dist_point_segment(p, o.start, o.end);
                if nearest.map(|(nd, _)| d < nd).unwrap_or(true) {
                    nearest = Some((d, &o.id));
                }
            }
            match nearest {
                Some((d, other)) if d <= NEAR_MISS_MM => issues.push(issue(
                    &format!("wall_end_gap_{end_name}"),
                    Severity::Warning,
                    format!(
                        "This wall stops {:.0} mm short of another wall. If they should meet, drag the end onto it.",
                        d
                    ),
                    vec![w.id.clone(), other.clone()],
                )),
                _ if level_has_rooms => issues.push(issue(
                    &format!("wall_dangling_{end_name}"),
                    Severity::Info,
                    format!("The {end_name} of this wall is not joined to anything."),
                    vec![w.id.clone()],
                )),
                _ => {}
            }
        }
    }
    let mut seen_pairs: Vec<(Id, Id)> = vec![];
    for level in &analysis.levels {
        for e in &level.edges {
            for i in 0..e.wall_ids.len() {
                for j in (i + 1)..e.wall_ids.len() {
                    let (a, b) = (e.wall_ids[i].clone(), e.wall_ids[j].clone());
                    let key = if a < b { (a, b) } else { (b, a) };
                    if !seen_pairs.contains(&key) {
                        seen_pairs.push(key);
                    }
                }
            }
        }
    }
    for (a, b) in seen_pairs {
        issues.push(issue(
            "wall_overlap",
            Severity::Warning,
            "Two walls lie on top of each other here. Delete or shorten one of them.".to_string(),
            vec![a, b],
        ));
    }

    issues
}
