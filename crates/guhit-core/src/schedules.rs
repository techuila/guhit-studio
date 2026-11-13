//! Door and window, room, finish and area schedules (DECISIONS D53). LANE
//! L1 owns this module.
//!
//! `derive` fills `Derived::opening_schedule`, `room_schedule` and
//! `level_areas`, one source for the schedules, the sheets and the queries
//! (G4 problem 1: query room and level areas equal `Derived`). Areas count
//! exactly as `Totals` does, through the same `SlabAreas` calls: voids come
//! off, balconies and decks are not floor area. Each room row gives its net
//! area (less voids, as `RoomGeometry`) and its floor area (also less
//! balconies and decks); the floor areas add up to `Totals::floor_area_m2`
//! and the level rows to `floor_area_m2` and `gross_area_m2`.

use std::collections::BTreeMap;

use guhit_model::*;
use serde_json::{json, Value};

use crate::opening_types::mark_order;
use crate::rooms::{face, FaceRef};
use crate::slabs::SlabAreas;
use crate::topo::{Analysis, Face};

/// What `derive` gives, one field per `Derived` field.
#[derive(Debug, Clone, Default)]
pub struct Schedules {
    pub openings: OpeningSchedule,
    pub rooms: Vec<RoomRow>,
    pub levels: Vec<LevelAreas>,
}

/// The thickness of a floor slab that a ceiling stays under (DECISIONS D26).
pub const CEILING_SLAB_MM: f64 = 200.0;

/// Areas kept to a millionth of a square meter, as `Totals` are.
fn tidy(v: f64) -> f64 {
    (v * 1.0e6).round() / 1.0e6
}

/// A level's ceiling: its height, or the underside of the next level's
/// 200 mm slab when that is lower (DECISIONS D26).
pub fn level_ceiling_mm(project: &Project, level: &Level) -> f64 {
    let under_next = project
        .levels
        .iter()
        .filter(|l| l.elevation_mm > level.elevation_mm + 1.0)
        .map(|l| l.elevation_mm - level.elevation_mm - CEILING_SLAB_MM)
        .fold(f64::INFINITY, f64::min);
    level.height_mm.min(under_next)
}

/// A room's ceiling height without its outline: its own, or its level's
/// ceiling. The schedules use `room_row_ceiling_mm`, which also looks up
/// through a void.
pub fn room_ceiling_mm(project: &Project, room: &Room) -> f64 {
    if let Some(h) = room.ceiling_height_mm {
        return h;
    }
    project
        .levels
        .iter()
        .find(|l| l.id == room.level_id)
        .map(|l| level_ceiling_mm(project, l))
        .unwrap_or(defaults::DEFAULT_LEVEL_HEIGHT_MM)
}

/// A room's ceiling height as the schedules and the reflected ceiling plan
/// print it (GS P0): its own; else, when a void in the floor of the level
/// right above lies over more than half of the room, the ceiling of that
/// level measured from this floor (a double height room, and on up through
/// voids stacked over it); else its level's ceiling. Above a void the
/// ceiling is the one the rooms of that level have: under the next slab,
/// or the level height under the roof (DECISIONS D26).
pub(crate) fn room_row_ceiling_mm(project: &Project, room: &Room, f: &Face, slab_areas: &SlabAreas) -> f64 {
    if let Some(h) = room.ceiling_height_mm {
        return h;
    }
    let Some(own) = project.levels.iter().find(|l| l.id == room.level_id) else {
        return defaults::DEFAULT_LEVEL_HEIGHT_MM;
    };
    let mut levels: Vec<&Level> = project.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    let mut top = own;
    while let Some(next) = levels.iter().copied().find(|l| l.elevation_mm > top.elevation_mm + 1.0) {
        if slab_areas.void_over(f, &next.id) * 2.0 <= f.area_mm2 {
            break;
        }
        top = next;
    }
    top.elevation_mm - own.elevation_mm + level_ceiling_mm(project, top)
}

/// The schedules of `project`. `derived` holds everything the derive hub
/// worked out before schedules (walls, rooms, footprints, totals, stairs,
/// railings, roofs, pipes).
pub fn derive(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
    _derived: &Derived,
) -> Schedules {
    let slab_areas = SlabAreas::new(project);
    Schedules {
        openings: opening_schedule(project),
        rooms: room_rows(project, analysis, assigned, &slab_areas),
        levels: level_rows(project, analysis, assigned, &slab_areas),
    }
}

/// The level of each opening: its wall's.
fn opening_level<'a>(walls: &BTreeMap<&'a Id, &'a Id>, o: &Opening) -> Option<&'a Id> {
    walls.get(&o.wall_id).copied()
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.5
}

/// The door and window schedule: one row per type in mark order (doors,
/// then windows; a type no opening uses yet shows a total of 0), then the
/// untyped openings grouped by kind, style, size, sill and material with an
/// empty mark.
pub fn opening_schedule(project: &Project) -> OpeningSchedule {
    let walls: BTreeMap<&Id, &Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((&w.id, &w.level_id)),
            _ => None,
        })
        .collect();
    let openings: Vec<&Opening> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) => Some(o),
            _ => None,
        })
        .collect();
    let per_level = |members: &[&Opening]| -> Vec<LevelCount> {
        project
            .levels
            .iter()
            .map(|l| LevelCount {
                level_id: l.id.clone(),
                count: members
                    .iter()
                    .filter(|o| opening_level(&walls, o) == Some(&l.id))
                    .count() as u32,
            })
            .collect()
    };
    // One value when every member shares it.
    let common_sill = |members: &[&Opening]| -> Option<f64> {
        let first = members.first()?.sill_mm;
        members
            .iter()
            .all(|o| same(o.sill_mm, first))
            .then_some(first)
    };
    let common_material = |members: &[&Opening]| -> Option<Id> {
        let first = members.first()?.material_id.clone();
        members
            .iter()
            .all(|o| o.material_id == first)
            .then_some(first)
            .flatten()
    };

    let kind_rank = |t: OpeningType| matches!(t, OpeningType::Window) as u8;
    let mut types: Vec<&OpeningSpec> = project.opening_types.iter().collect();
    types.sort_by(|a, b| {
        kind_rank(a.opening_type)
            .cmp(&kind_rank(b.opening_type))
            .then(mark_order(&a.mark, &b.mark))
    });

    let mut rows = vec![];
    let mut marks = vec![];
    for t in types {
        let members: Vec<&Opening> = openings
            .iter()
            .copied()
            .filter(|o| o.type_id.as_deref() == Some(t.id.as_str()))
            .collect();
        for o in &members {
            marks.push(OpeningMark {
                opening_id: o.id.clone(),
                mark: t.mark.clone(),
            });
        }
        rows.push(OpeningRow {
            mark: t.mark.clone(),
            type_id: Some(t.id.clone()),
            opening_type: t.opening_type,
            style: t.style,
            width_mm: t.width_mm,
            height_mm: t.height_mm,
            sill_mm: t.sill_mm.or_else(|| common_sill(&members)),
            material_id: t.material_id.clone().or_else(|| common_material(&members)),
            description: t.description.clone(),
            per_level: per_level(&members),
            total: members.len() as u32,
        });
    }

    // Untyped: grouped, doors first, widest first.
    let mut groups: Vec<Vec<&Opening>> = vec![];
    for o in openings.iter().copied().filter(|o| o.type_id.is_none()) {
        let key = |x: &Opening| {
            x.opening_type == o.opening_type
                && x.style == o.style
                && same(x.width_mm, o.width_mm)
                && same(x.height_mm, o.height_mm)
                && same(x.sill_mm, o.sill_mm)
                && x.material_id == o.material_id
        };
        match groups.iter_mut().find(|g| key(g[0])) {
            Some(g) => g.push(o),
            None => groups.push(vec![o]),
        }
    }
    groups.sort_by(|a, b| {
        let (a, b) = (a[0], b[0]);
        kind_rank(a.opening_type)
            .cmp(&kind_rank(b.opening_type))
            .then(b.width_mm.total_cmp(&a.width_mm))
            .then(b.height_mm.total_cmp(&a.height_mm))
            .then(a.sill_mm.total_cmp(&b.sill_mm))
            .then(a.id.cmp(&b.id))
    });
    for g in groups {
        let o = g[0];
        rows.push(OpeningRow {
            mark: String::new(),
            type_id: None,
            opening_type: o.opening_type,
            style: o.style,
            width_mm: o.width_mm,
            height_mm: o.height_mm,
            sill_mm: Some(o.sill_mm),
            material_id: o.material_id.clone(),
            description: crate::opening_types::default_description(o.opening_type, o.style),
            per_level: per_level(&g),
            total: g.len() as u32,
        });
    }
    OpeningSchedule { rows, marks }
}

/// One row per room with a polygon, in level order, then element order.
fn room_rows(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
    slab_areas: &SlabAreas,
) -> Vec<RoomRow> {
    let mut rows = vec![];
    for level in &project.levels {
        for el in &project.elements {
            let Element::Room(r) = el else { continue };
            if r.level_id != level.id {
                continue;
            }
            let Some(fref) = assigned.get(&r.id) else {
                continue;
            };
            let f = face(analysis, *fref);
            rows.push(RoomRow {
                room_id: r.id.clone(),
                level_id: r.level_id.clone(),
                name: r.name.clone(),
                usage: r.usage,
                net_area_m2: tidy(slab_areas.room_area(f) / 1.0e6),
                floor_area_m2: tidy(slab_areas.room_floor_area(f) / 1.0e6),
                perimeter_m: tidy(f.perimeter_mm / 1000.0),
                ceiling_height_mm: room_row_ceiling_mm(project, r, f, slab_areas),
                floor_material_id: r.floor_material_id.clone(),
                finishes: r.finishes.clone(),
            });
        }
    }
    rows
}

/// One row per level, as `Totals` counts: net is the rooms' floor area,
/// gross the footprints plus floor slabs past them, both less voids,
/// balconies and decks.
fn level_rows(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
    slab_areas: &SlabAreas,
) -> Vec<LevelAreas> {
    project
        .levels
        .iter()
        .map(|level| {
            let net: f64 = project
                .elements
                .iter()
                .filter_map(|e| match e {
                    Element::Room(r) if r.level_id == level.id => assigned.get(&r.id),
                    _ => None,
                })
                .map(|fref| slab_areas.room_floor_area(face(analysis, *fref)))
                .sum();
            let plain: f64 = analysis
                .level(&level.id)
                .map(|l| l.footprints.iter().map(|(_, a)| a).sum())
                .unwrap_or(0.0);
            let gross = slab_areas.gross_area(analysis, &level.id).unwrap_or(plain);
            let slab_area = |kinds: &[SlabKind]| -> f64 {
                project
                    .elements
                    .iter()
                    .filter_map(|e| match e {
                        Element::Slab(s) if s.level_id == level.id && kinds.contains(&s.kind) => {
                            Some(crate::geom::signed_area(&s.polygon).abs())
                        }
                        _ => None,
                    })
                    .sum()
            };
            LevelAreas {
                level_id: level.id.clone(),
                net_m2: tidy(net / 1.0e6),
                gross_m2: tidy(gross / 1.0e6),
                open_m2: tidy(slab_area(&[SlabKind::Balcony, SlabKind::Deck]) / 1.0e6),
                void_m2: tidy(slab_area(&[SlabKind::Void]) / 1.0e6),
            }
        })
        .collect()
}

/// `Query::OpeningSchedule`.
pub fn opening_query(derived: &Derived) -> Value {
    json!({
        "rows": derived.opening_schedule.rows,
        "marks": derived.opening_schedule.marks,
        "note": "Door and window types by mark, with counts per level. Untyped openings are grouped by type, style and size with an empty mark.",
    })
}

/// `Query::RoomSchedule`.
pub fn room_query(derived: &Derived) -> Value {
    json!({
        "rooms": derived.room_schedule,
        "note": "Square meters. net_area_m2 is inside the walls less voids; floor_area_m2 is what the totals count, also less balconies and decks, and the rows' floor areas add up to the total floor area.",
    })
}

/// `Query::LevelAreas`.
pub fn level_areas_query(derived: &Derived) -> Value {
    json!({
        "levels": derived.level_areas,
        "note": "Square meters per level, exactly as the totals count them.",
    })
}
