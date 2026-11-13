//! Builds `Derived` from a project: wall outlines, rooms, footprints,
//! totals, pipes, railings, the object schedule, review items and review
//! marks.

use guhit_model::*;

use crate::devices::{device_issues, schedule, Catalog, RoomIndex};
use crate::issues::{apply_review_marks, review};
use crate::pipes::derive_pipes;
use crate::rooms::{assign_by_seed, face};
use crate::slabs::{self, SlabAreas};
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
    // A void takes its area off the rooms under it; balconies and decks are
    // not floor area (DECISIONS D50).
    let slab_areas = SlabAreas::new(project);
    for el in &project.elements {
        match el {
            Element::Room(r) => {
                derived.totals.room_count += 1;
                if let Some(fref) = assigned.get(&r.id) {
                    let f = face(analysis, *fref);
                    derived.totals.floor_area_m2 += slab_areas.room_floor_area(f) / 1.0e6;
                    derived.rooms.push(RoomGeometry {
                        room_id: r.id.clone(),
                        polygon: f.net.clone(),
                        centerline_polygon: f.centerline.clone(),
                        area_mm2: slab_areas.room_area(f),
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
        // With slabs: floor slabs add where they reach past the footprints,
        // voids, balconies and decks come off.
        if let Some(gross) = slab_areas.gross_area(analysis, &level.level_id) {
            let plain: f64 = level.footprints.iter().map(|(_, a)| a).sum();
            derived.totals.gross_area_m2 += (gross - plain) / 1.0e6;
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
    derived.issues.extend(slabs::review(project, analysis));
    derived.issues.extend(pipe_issues);
    derived.railings = slabs::railings(project);

    // Devices: the object schedule and the device review items.
    let catalog = Catalog::new();
    let rooms = RoomIndex::new(project, analysis, &assigned);
    derived
        .issues
        .extend(device_issues(project, analysis, &rooms, &catalog));
    derived.schedule = schedule(project, &rooms, &catalog);

    // Review marks last, over every finding.
    derived.review_resolved = apply_review_marks(&project.review, &mut derived.issues);
    derived
}
