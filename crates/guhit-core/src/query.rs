//! Read-only questions answered from model data. The JSON goes to the AI
//! copilot and to the UI, so keys are self-explanatory: areas are m2 with
//! 2 decimals, lengths are mm, every element carries its id and name.

use std::collections::BTreeMap;

use guhit_model::*;
use serde_json::{json, Value};

use crate::derive::derived_from;
use crate::error::CoreError;
use crate::exec::kind_noun;
use crate::geom::*;
use crate::issues::{opening_center, openings_of_face, usage_label};
use crate::rooms::{assign_by_seed, face, FaceRef};
use crate::topo::{analyze, Analysis};

fn m2(area_mm2: f64) -> f64 {
    (area_mm2 / 1.0e6 * 100.0).round() / 100.0
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn mm(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn point(p: Point) -> Value {
    json!({ "x_mm": mm(p.x), "y_mm": mm(p.y) })
}

fn enum_str<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

struct Ctx<'a> {
    project: &'a Project,
    analysis: Analysis,
    assigned: BTreeMap<Id, FaceRef>,
}

impl<'a> Ctx<'a> {
    fn level_name(&self, id: &str) -> String {
        self.project
            .levels
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "unknown level".into())
    }

    fn material_name(&self, id: &Option<Id>) -> Value {
        match id {
            Some(id) => self
                .project
                .materials
                .iter()
                .find(|m| &m.id == id)
                .map(|m| json!(m.name))
                .unwrap_or(Value::Null),
            None => Value::Null,
        }
    }

    fn rooms_of_wall(&self, wall_id: &str) -> Vec<Value> {
        let mut out = vec![];
        for el in &self.project.elements {
            if let Element::Room(r) = el {
                if let Some(f) = self.assigned.get(&r.id) {
                    if face(&self.analysis, *f)
                        .wall_ids
                        .iter()
                        .any(|w| w == wall_id)
                    {
                        out.push(json!({ "id": r.id, "name": r.name }));
                    }
                }
            }
        }
        out
    }

    fn rooms_of_opening(&self, o: &Opening) -> Vec<Value> {
        let mut out = vec![];
        for el in &self.project.elements {
            if let Element::Room(r) = el {
                if let Some(f) = self.assigned.get(&r.id) {
                    if openings_of_face(self.project, face(&self.analysis, *f))
                        .iter()
                        .any(|(x, _)| x.id == o.id)
                    {
                        out.push(json!({ "id": r.id, "name": r.name }));
                    }
                }
            }
        }
        out
    }

    fn room_json(&self, r: &Room) -> Value {
        let mut v = json!({
            "id": r.id,
            "name": r.name,
            "usage": enum_str(&r.usage),
            "level": self.level_name(&r.level_id),
            "floor_material": self.material_name(&r.floor_material_id),
            "name_is_automatic": r.auto_named,
        });
        let Some(fref) = self.assigned.get(&r.id) else {
            v["enclosed"] = json!(false);
            return v;
        };
        let f = face(&self.analysis, *fref);
        let (lo, hi) = bbox(&f.net);
        let openings = openings_of_face(self.project, f);
        let list = |ty: OpeningType| -> Vec<Value> {
            openings
                .iter()
                .filter(|(o, _)| o.opening_type == ty)
                .map(|(o, ext)| json!({ "id": o.id, "width_mm": mm(o.width_mm), "wall_id": o.wall_id, "on_outside_wall": ext }))
                .collect()
        };
        let walls: Vec<Value> = f
            .wall_ids
            .iter()
            .map(|id| {
                let t = self.analysis.wall(id);
                json!({
                    "id": id,
                    "length_mm": t.map(|t| mm(t.length_mm)),
                    "faces_outside": t.map(|t| t.exterior),
                })
            })
            .collect();
        v["enclosed"] = json!(true);
        v["net_area_m2"] = json!(m2(f.area_mm2));
        v["centerline_area_m2"] = json!(m2(f.centerline_area_mm2));
        v["perimeter_mm"] = json!(mm(f.perimeter_mm));
        v["clear_width_x_mm"] = json!(mm(hi.x - lo.x));
        v["clear_depth_y_mm"] = json!(mm(hi.y - lo.y));
        v["is_rectangular"] = json!(
            f.net.len() == 4 && (signed_area(&f.net) - (hi.x - lo.x) * (hi.y - lo.y)).abs() < 1.0
        );
        v["label_point"] = point(f.label);
        v["bounding_walls"] = json!(walls);
        v["doors"] = json!(list(OpeningType::Door));
        v["windows"] = json!(list(OpeningType::Window));
        v["has_window_on_outside_wall"] = json!(openings
            .iter()
            .any(|(o, ext)| *ext && o.opening_type == OpeningType::Window));
        v
    }

    fn wall_json(&self, w: &Wall) -> Value {
        let t = self.analysis.wall(&w.id);
        let hosted: Vec<Value> = self
            .project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.wall_id == w.id => Some(json!({
                    "id": o.id,
                    "type": enum_str(&o.opening_type),
                    "offset_from_wall_start_mm": mm(o.offset_mm),
                    "width_mm": mm(o.width_mm),
                })),
                _ => None,
            })
            .collect();
        json!({
            "id": w.id,
            "kind": "wall",
            "level": self.level_name(&w.level_id),
            "start": point(w.start),
            "end": point(w.end),
            "length_mm": mm(dist(w.start, w.end)),
            "thickness_mm": mm(w.thickness_mm),
            "height_mm": mm(crate::validate::wall_height(self.project, w)),
            "material": self.material_name(&w.material_id),
            "faces_outside": t.map(|t| t.exterior),
            "joined_at_start_to": t.map(|t| t.joined_at_start.clone()),
            "joined_at_end_to": t.map(|t| t.joined_at_end.clone()),
            "walls_meeting_its_body": t.map(|t| t.junctions_mm.iter().map(|(at, id)| json!({ "wall_id": id, "at_mm_from_start": mm(*at) })).collect::<Vec<_>>()),
            "openings": hosted,
            "rooms": self.rooms_of_wall(&w.id),
        })
    }

    fn opening_json(&self, o: &Opening) -> Value {
        let host = self.project.elements.iter().find_map(|e| match e {
            Element::Wall(w) if w.id == o.wall_id => Some(w),
            _ => None,
        });
        json!({
            "id": o.id,
            "kind": "opening",
            "type": enum_str(&o.opening_type),
            "style": enum_str(&o.style),
            "wall_id": o.wall_id,
            "offset_from_wall_start_mm": mm(o.offset_mm),
            "width_mm": mm(o.width_mm),
            "height_mm": mm(o.height_mm),
            "sill_mm": mm(o.sill_mm),
            "center": host.and_then(|w| opening_center(w, o)).map(point),
            "host_wall_length_mm": host.map(|w| mm(dist(w.start, w.end))),
            "on_outside_wall": self.analysis.wall(&o.wall_id).map(|t| t.exterior),
            "material": self.material_name(&o.material_id),
            "rooms": self.rooms_of_opening(o),
        })
    }

    fn element_json(&self, el: &Element) -> Value {
        match el {
            Element::Wall(w) => self.wall_json(w),
            Element::Opening(o) => self.opening_json(o),
            Element::Room(r) => {
                let mut v = self.room_json(r);
                v["kind"] = json!("room");
                v
            }
            Element::Column(c) => json!({
                "id": c.id, "kind": "column", "level": self.level_name(&c.level_id),
                "center": point(c.center), "shape": enum_str(&c.shape),
                "width_mm": mm(c.width_mm), "depth_mm": mm(c.depth_mm),
                "rotation_deg": r2(c.rotation_deg), "material": self.material_name(&c.material_id),
            }),
            Element::Stair(s) => json!({
                "id": s.id, "kind": "stair", "level": self.level_name(&s.level_id),
                "origin": point(s.origin), "rotation_deg": r2(s.rotation_deg),
                "width_mm": mm(s.width_mm), "run_mm": mm(s.run_mm), "riser_count": s.riser_count,
            }),
            Element::Asset(a) => {
                let room = self
                    .analysis
                    .face_at(&a.level_id, a.position)
                    .and_then(|fi| {
                        let li = self
                            .analysis
                            .levels
                            .iter()
                            .position(|l| l.level_id == a.level_id)?;
                        self.assigned
                            .iter()
                            .find(|(_, f)| **f == (li, fi))
                            .map(|(id, _)| id.clone())
                    })
                    .and_then(|id| {
                        self.project.elements.iter().find_map(|e| match e {
                            Element::Room(r) if r.id == id => {
                                Some(json!({ "id": r.id, "name": r.name }))
                            }
                            _ => None,
                        })
                    });
                json!({
                    "id": a.id, "kind": "object", "name": a.name, "catalog_key": a.catalog_key,
                    "category": enum_str(&a.category), "level": self.level_name(&a.level_id),
                    "position": point(a.position), "rotation_deg": r2(a.rotation_deg),
                    "width_mm": mm(a.width_mm), "depth_mm": mm(a.depth_mm), "height_mm": mm(a.height_mm),
                    "in_room": room,
                })
            }
            Element::Annotation(a) => json!({
                "id": a.id, "kind": "note", "level": self.level_name(&a.level_id),
                "text": a.text, "position": point(a.position), "text_size_mm": mm(a.size_mm),
            }),
            Element::Dimension(d) => json!({
                "id": d.id, "kind": "dimension", "level": self.level_name(&d.level_id),
                "a": point(d.a), "b": point(d.b), "measured_mm": mm(dist(d.a, d.b)),
                "text_override": d.text_override,
            }),
            Element::Camera(c) => json!({
                "id": c.id, "kind": "camera", "name": c.name, "preset": enum_str(&c.preset),
                "position_mm": { "x": mm(c.position.x), "y": mm(c.position.y), "z": mm(c.position.z) },
                "target_mm": { "x": mm(c.target.x), "y": mm(c.target.y), "z": mm(c.target.z) },
                "fov_deg": r2(c.fov_deg),
            }),
            Element::Underlay(u) => json!({
                "id": u.id, "kind": "underlay", "level": self.level_name(&u.level_id),
                "file_name": u.file_name, "position": point(u.position),
                "width_mm": mm(u.width_px as f64 * u.mm_per_px), "height_mm": mm(u.height_px as f64 * u.mm_per_px),
                "scale_confirmed": u.scale_confirmed, "locked": u.locked,
            }),
            Element::Linework(l) => json!({
                "id": l.id, "kind": "linework", "level": self.level_name(&l.level_id),
                "name": l.name, "polyline_count": l.polylines.len(), "locked": l.locked,
            }),
            Element::ReferenceModel(m) => json!({
                "id": m.id, "kind": "reference_model", "level": self.level_name(&m.level_id),
                "name": m.name, "file_name": m.file_name, "position": point(m.position),
                "rotation_deg": r2(m.rotation_deg), "locked": m.locked,
            }),
        }
    }
}

pub fn run_query(project: &Project, query: &Query) -> Result<Value, CoreError> {
    let analysis = analyze(project);
    let assigned = assign_by_seed(project, &analysis);
    let derived = derived_from(project, &analysis);
    let ctx = Ctx {
        project,
        analysis,
        assigned,
    };
    let rooms = || {
        project.elements.iter().filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        })
    };
    Ok(match query {
        Query::ProjectSummary => {
            let mut counts: BTreeMap<&'static str, u32> = BTreeMap::new();
            for el in &project.elements {
                *counts.entry(kind_noun(el.kind())).or_default() += 1;
            }
            let levels: Vec<Value> = project
                .levels
                .iter()
                .map(|l| {
                    let room_area: f64 = rooms()
                        .filter(|r| r.level_id == l.id)
                        .filter_map(|r| ctx.assigned.get(&r.id))
                        .map(|f| face(&ctx.analysis, *f).area_mm2)
                        .sum();
                    let gross: f64 = derived
                        .footprints
                        .iter()
                        .filter(|f| f.level_id == l.id)
                        .map(|f| f.area_mm2)
                        .sum();
                    json!({
                        "id": l.id,
                        "name": l.name,
                        "elevation_mm": mm(l.elevation_mm),
                        "floor_to_floor_height_mm": mm(l.height_mm),
                        "room_count": rooms().filter(|r| r.level_id == l.id).count(),
                        "net_floor_area_m2": m2(room_area),
                        "gross_footprint_area_m2": m2(gross),
                    })
                })
                .collect();
            json!({
                "project_name": project.name,
                "client": project.settings.client_name,
                "location": project.settings.location,
                "designer": project.settings.designer,
                "drawing_scale": format!("1:{}", project.settings.scale_denominator),
                "units_note": "All lengths are millimeters, all areas are square meters. Plan +x is east, +y is north.",
                "totals": {
                    "net_floor_area_m2": r2(derived.totals.floor_area_m2),
                    "gross_footprint_area_m2": r2(derived.totals.gross_area_m2),
                    "total_wall_length_m": r2(derived.totals.wall_length_m),
                    "rooms": derived.totals.room_count,
                    "doors": derived.totals.door_count,
                    "windows": derived.totals.window_count,
                },
                "levels": levels,
                "element_counts": counts,
                "rooms": rooms().map(|r| json!({
                    "id": r.id,
                    "name": r.name,
                    "net_area_m2": ctx.assigned.get(&r.id).map(|f| m2(face(&ctx.analysis, *f).area_mm2)),
                })).collect::<Vec<_>>(),
                "roof": { "kind": enum_str(&project.roof.kind), "pitch_deg": r2(project.roof.pitch_deg), "overhang_mm": mm(project.roof.overhang_mm) },
                "default_wall_thickness_mm": mm(project.settings.default_wall_thickness_mm),
                "review_item_count": derived.issues.len(),
            })
        }
        Query::RoomList => {
            let list: Vec<Value> = rooms().map(|r| ctx.room_json(r)).collect();
            json!({ "room_count": list.len(), "total_net_area_m2": r2(derived.totals.floor_area_m2), "rooms": list })
        }
        Query::RoomsWithoutExteriorWindow => {
            let mut list = vec![];
            for r in rooms() {
                let Some(fref) = ctx.assigned.get(&r.id) else {
                    continue;
                };
                let f = face(&ctx.analysis, *fref);
                let has = openings_of_face(project, f)
                    .iter()
                    .any(|(o, ext)| *ext && o.opening_type == OpeningType::Window);
                if has {
                    continue;
                }
                let mut outside: Vec<Id> = vec![];
                for e in f.boundary.iter().filter(|e| e.exterior) {
                    for id in &e.wall_ids {
                        if !outside.contains(id) {
                            outside.push(id.clone());
                        }
                    }
                }
                list.push(json!({
                    "id": r.id,
                    "name": r.name,
                    "usage": enum_str(&r.usage),
                    "usage_label": usage_label(r.usage),
                    "net_area_m2": m2(f.area_mm2),
                    "outside_wall_ids": outside,
                    "can_get_a_window": !outside.is_empty(),
                }));
            }
            json!({
                "count": list.len(),
                "checked_room_count": rooms().count(),
                "rooms": list,
                "note": "A room is listed when none of its windows sits on a wall stretch that faces the outside.",
            })
        }
        Query::Describe { ids } => {
            let mut found = vec![];
            let mut missing = vec![];
            for id in ids {
                match project.elements.iter().find(|e| e.id() == id) {
                    Some(el) => found.push(ctx.element_json(el)),
                    None => missing.push(id.clone()),
                }
            }
            json!({ "elements": found, "not_found": missing })
        }
        Query::ListElements { kind } => {
            let list: Vec<Value> = project
                .elements
                .iter()
                .filter(|e| e.kind() == *kind)
                .map(|e| ctx.element_json(e))
                .collect();
            json!({ "kind": kind_noun(*kind), "count": list.len(), "elements": list })
        }
        Query::Issues => {
            let list: Vec<Value> = derived
                .issues
                .iter()
                .map(|i| {
                    json!({
                        "id": i.id,
                        "severity": enum_str(&i.severity),
                        "code": i.code,
                        "message": i.message,
                        "element_ids": i.element_ids,
                    })
                })
                .collect();
            json!({
                "count": list.len(),
                "note": "These are design suggestions, not code compliance or permit checks.",
                "items": list,
            })
        }
    })
}
