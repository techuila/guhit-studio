//! Builds `Derived` from a project: wall outlines, rooms, footprints,
//! totals, pipes and review items.

use guhit_model::*;

use crate::issues::review;
use crate::pipes::derive_pipes;
use crate::rooms::{assign_by_seed, face};
use crate::topo::{analyze, Analysis};

/// Recompute everything in `Derived` from scratch.
pub fn compute_derived(project: &Project) -> Derived {
    let analysis = analyze(project);
    derived_from(project, &analysis)
}

pub fn derived_from(project: &Project, analysis: &Analysis) -> Derived {
    let mut derived = Derived::default();

    for w in &analysis.walls {
        derived.totals.wall_length_m += w.length_mm / 1000.0;
        derived.walls.push(WallGeometry {
            wall_id: w.wall_id.clone(),
            length_mm: w.length_mm,
            outline: w.outline.clone(),
            exterior: w.exterior,
            joined_at_start: w.joined_at_start.clone(),
            joined_at_end: w.joined_at_end.clone(),
        });
    }

    let assigned = assign_by_seed(project, analysis);
    for el in &project.elements {
        match el {
            Element::Room(r) => {
                derived.totals.room_count += 1;
                if let Some(fref) = assigned.get(&r.id) {
                    let f = face(analysis, *fref);
                    derived.totals.floor_area_m2 += f.area_mm2 / 1.0e6;
                    derived.rooms.push(RoomGeometry {
                        room_id: r.id.clone(),
                        polygon: f.net.clone(),
                        centerline_polygon: f.centerline.clone(),
                        area_mm2: f.area_mm2,
                        perimeter_mm: f.perimeter_mm,
                        label_point: f.label,
                        wall_ids: f.wall_ids.clone(),
                    });
                }
            }
            Element::Opening(o) => match o.opening_type {
                OpeningType::Door => derived.totals.door_count += 1,
                OpeningType::Window => derived.totals.window_count += 1,
            },
            _ => {}
        }
    }

    // One entry per closed building on a level, largest first. A level with
    // no closed building gets one entry with an empty polygon.
    for level in &analysis.levels {
        if level.footprints.is_empty() {
            derived.footprints.push(Footprint {
                level_id: level.level_id.clone(),
                polygon: vec![],
                area_mm2: 0.0,
            });
        }
        for (polygon, area) in &level.footprints {
            derived.totals.gross_area_m2 += area / 1.0e6;
            derived.footprints.push(Footprint {
                level_id: level.level_id.clone(),
                polygon: polygon.clone(),
                area_mm2: *area,
            });
        }
    }

    // Sums of many floats pick up dust like 45.337500000000006. Totals are
    // kept to a millionth of a unit (1 mm2, 0.001 mm of wall).
    let tidy = |v: f64| (v * 1.0e6).round() / 1.0e6;
    derived.totals.floor_area_m2 = tidy(derived.totals.floor_area_m2);
    derived.totals.gross_area_m2 = tidy(derived.totals.gross_area_m2);
    derived.totals.wall_length_m = tidy(derived.totals.wall_length_m);

    let (pipes, pipe_issues) = derive_pipes(project, analysis, &assigned);
    derived.pipes = pipes;
    derived.issues = review(project, analysis, &assigned);
    derived.issues.extend(pipe_issues);
    derived
}
