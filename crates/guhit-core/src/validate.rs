//! Validation rules. Every rule has a machine code and a plain message.

use std::collections::{BTreeMap, BTreeSet};

use guhit_model::*;

use crate::error::CoreError;
use crate::geom::*;

pub const MIN_WALL_LENGTH_MM: f64 = 50.0;
pub const MIN_WALL_THICKNESS_MM: f64 = 50.0;
pub const MAX_WALL_THICKNESS_MM: f64 = 1000.0;
/// Clear distance an opening keeps from each end of its host wall.
pub const OPENING_END_CLEARANCE_MM: f64 = 50.0;
/// Coordinates beyond 10 km from the origin are treated as a mistake.
pub const MAX_COORD_MM: f64 = 1.0e7;

const FIT_EPS: f64 = 1e-6;

pub fn layer_of(kind: ElementKind) -> Option<LayerKey> {
    Some(match kind {
        ElementKind::Wall => LayerKey::Walls,
        ElementKind::Opening => LayerKey::Openings,
        ElementKind::Room => LayerKey::Rooms,
        ElementKind::Column => LayerKey::Columns,
        ElementKind::Stair => LayerKey::Stairs,
        ElementKind::Asset => LayerKey::Assets,
        ElementKind::Annotation => LayerKey::Annotations,
        ElementKind::Dimension => LayerKey::Dimensions,
        ElementKind::Underlay | ElementKind::Linework | ElementKind::ReferenceModel => LayerKey::Underlays,
        ElementKind::Camera => return None,
        // Pipes live on one layer per system: see `element_layer`.
        ElementKind::Pipe => return None,
    })
}

/// The layer an element is on. Pipes use the layer of their system.
pub fn element_layer(el: &Element) -> Option<LayerKey> {
    match el {
        Element::Pipe(p) => Some(p.system.layer()),
        other => layer_of(other.kind()),
    }
}

pub fn layer_name(key: LayerKey) -> &'static str {
    match key {
        LayerKey::Walls => "Walls",
        LayerKey::Openings => "Openings",
        LayerKey::Rooms => "Rooms",
        LayerKey::Columns => "Columns",
        LayerKey::Stairs => "Stairs",
        LayerKey::Assets => "Assets",
        LayerKey::Annotations => "Annotations",
        LayerKey::Dimensions => "Dimensions",
        LayerKey::Underlays => "Underlays",
        LayerKey::ColdWater => "Cold water",
        LayerKey::HotWater => "Hot water",
        LayerKey::Drainage => "Drainage",
        LayerKey::Vent => "Vent",
    }
}

fn locked_key(project: &Project, key: LayerKey) -> Option<LayerKey> {
    project
        .layers
        .iter()
        .find(|l| l.key == key && l.locked)
        .map(|l| l.key)
}

/// The locked layer a new element of `kind` would go on. Pipes have no
/// layer by kind (see `element_layer`), so use `element_locked` for them.
pub fn is_locked(project: &Project, kind: ElementKind) -> Option<LayerKey> {
    locked_key(project, layer_of(kind)?)
}

/// The locked layer `el` is on, if its layer is locked.
pub fn element_locked(project: &Project, el: &Element) -> Option<LayerKey> {
    locked_key(project, element_layer(el)?)
}

fn locked_error(key: LayerKey, ids: Vec<Id>) -> CoreError {
    CoreError::invalid_for(
        "layer_locked",
        format!(
            "The {} layer is locked. Unlock it to change these elements.",
            layer_name(key)
        ),
        ids,
    )
}

/// Reject a new element on a locked layer.
pub fn ensure_unlocked(project: &Project, kind: ElementKind) -> Result<(), CoreError> {
    match is_locked(project, kind) {
        Some(key) => Err(locked_error(key, vec![])),
        None => Ok(()),
    }
}

/// Reject adding or copying `el` when its own layer is locked. Unlike
/// `ensure_unlocked` this knows the layer of a pipe (its system).
pub fn ensure_element_unlocked(project: &Project, el: &Element) -> Result<(), CoreError> {
    match element_locked(project, el) {
        Some(key) => Err(locked_error(key, vec![])),
        None => Ok(()),
    }
}

fn finite(name: &str, v: f64, ids: &[Id]) -> Result<(), CoreError> {
    if v.is_finite() && v.abs() <= MAX_COORD_MM {
        Ok(())
    } else if v.is_finite() {
        Err(CoreError::invalid_for(
            "out_of_range",
            format!("{name} is too far from the origin ({v:.0} mm). The limit is 10 km."),
            ids.to_vec(),
        ))
    } else {
        Err(CoreError::invalid_for(
            "not_finite",
            format!("{name} is not a valid number."),
            ids.to_vec(),
        ))
    }
}

fn finite_point(name: &str, p: Point, ids: &[Id]) -> Result<(), CoreError> {
    finite(&format!("{name} x"), p.x, ids)?;
    finite(&format!("{name} y"), p.y, ids)
}

fn in_range(
    code: &str,
    name: &str,
    v: f64,
    lo: f64,
    hi: f64,
    unit: &str,
    ids: &[Id],
) -> Result<(), CoreError> {
    if !v.is_finite() {
        return Err(CoreError::invalid_for(
            "not_finite",
            format!("{name} is not a valid number."),
            ids.to_vec(),
        ));
    }
    if v < lo || v > hi {
        return Err(CoreError::invalid_for(
            code,
            format!("{name} must be between {lo} and {hi} {unit}, got {v:.1}."),
            ids.to_vec(),
        ));
    }
    Ok(())
}

/// Pipe sizes outside this range are a slip, not a design.
pub const PIPE_DIAMETER_MM: (f64, f64) = (10.0, 300.0);
/// Two pipe points in a row closer than this are the same point.
pub const PIPE_POINT_EPS_MM: f64 = 1.0;

fn validate_pipe(project: &Project, p: &Pipe) -> Result<(), CoreError> {
    let ids = [p.id.clone()];
    require_level(project, &p.level_id)?;
    in_range(
        "pipe_diameter",
        "Pipe size",
        p.diameter_mm,
        PIPE_DIAMETER_MM.0,
        PIPE_DIAMETER_MM.1,
        "mm",
        &ids,
    )?;
    if p.points.len() < 2 {
        return Err(CoreError::invalid_for(
            "pipe_too_short",
            "A pipe needs at least two points.",
            ids.to_vec(),
        ));
    }
    for v in &p.points {
        finite("Pipe point x", v.x, &ids)?;
        finite("Pipe point y", v.y, &ids)?;
        finite("Pipe point height", v.z, &ids)?;
    }
    for w in p.points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt();
        if d < PIPE_POINT_EPS_MM {
            return Err(CoreError::invalid_for(
                "pipe_zero_segment",
                format!(
                    "Two pipe points in a row are less than {PIPE_POINT_EPS_MM:.0} mm apart. Remove one of them."
                ),
                ids.to_vec(),
            ));
        }
    }
    Ok(())
}

pub fn require_level(project: &Project, level_id: &str) -> Result<(), CoreError> {
    if project.levels.iter().any(|l| l.id == level_id) {
        Ok(())
    } else {
        Err(CoreError::invalid(
            "unknown_level",
            format!("There is no level with id {level_id}."),
        ))
    }
}

pub fn require_material(project: &Project, material_id: &Option<Id>) -> Result<(), CoreError> {
    match material_id {
        Some(id) if !project.materials.iter().any(|m| &m.id == id) => Err(CoreError::invalid(
            "unknown_material",
            format!("There is no material with id {id}."),
        )),
        _ => Ok(()),
    }
}

pub fn find_wall<'a>(project: &'a Project, wall_id: &str) -> Result<&'a Wall, CoreError> {
    match project.elements.iter().find(|e| e.id() == wall_id) {
        Some(Element::Wall(w)) => Ok(w),
        Some(_) => Err(CoreError::invalid_for(
            "not_a_wall",
            format!("Element {wall_id} is not a wall."),
            vec![wall_id.to_string()],
        )),
        None => Err(CoreError::invalid_for(
            "unknown_wall",
            format!("There is no wall with id {wall_id}."),
            vec![wall_id.to_string()],
        )),
    }
}

/// Wall height, falling back to the level's floor-to-floor height.
pub fn wall_height(project: &Project, wall: &Wall) -> f64 {
    wall.height_mm.unwrap_or_else(|| {
        project
            .levels
            .iter()
            .find(|l| l.id == wall.level_id)
            .map(|l| l.height_mm)
            .unwrap_or(defaults::DEFAULT_LEVEL_HEIGHT_MM)
    })
}

pub fn validate_wall(project: &Project, w: &Wall) -> Result<(), CoreError> {
    let ids = [w.id.clone()];
    require_level(project, &w.level_id)?;
    require_material(project, &w.material_id)?;
    finite_point("Wall start", w.start, &ids)?;
    finite_point("Wall end", w.end, &ids)?;
    let len = dist(w.start, w.end);
    if len < MIN_WALL_LENGTH_MM {
        return Err(CoreError::invalid_for(
            "wall_too_short",
            format!("A wall must be at least {MIN_WALL_LENGTH_MM:.0} mm long. This one would be {len:.0} mm."),
            ids.to_vec(),
        ));
    }
    in_range(
        "wall_thickness",
        "Wall thickness",
        w.thickness_mm,
        MIN_WALL_THICKNESS_MM,
        MAX_WALL_THICKNESS_MM,
        "mm",
        &ids,
    )?;
    if let Some(h) = w.height_mm {
        in_range("wall_height", "Wall height", h, 100.0, 20_000.0, "mm", &ids)?;
    }
    Ok(())
}

fn validate_opening_static(project: &Project, o: &Opening) -> Result<(), CoreError> {
    let ids = [o.id.clone()];
    find_wall(project, &o.wall_id)?;
    require_material(project, &o.material_id)?;
    finite("Opening offset", o.offset_mm, &ids)?;
    in_range(
        "opening_size",
        "Opening width",
        o.width_mm,
        300.0,
        10_000.0,
        "mm",
        &ids,
    )?;
    in_range(
        "opening_size",
        "Opening height",
        o.height_mm,
        100.0,
        10_000.0,
        "mm",
        &ids,
    )?;
    in_range(
        "opening_size",
        "Sill height",
        o.sill_mm,
        0.0,
        10_000.0,
        "mm",
        &ids,
    )
}

/// Rules of one element that do not depend on other elements, plus
/// references to levels, materials and host walls.
pub fn validate_element(project: &Project, el: &Element) -> Result<(), CoreError> {
    let ids = [el.id().clone()];
    match el {
        Element::Wall(w) => validate_wall(project, w),
        Element::Opening(o) => validate_opening_static(project, o),
        Element::Room(r) => {
            require_level(project, &r.level_id)?;
            require_material(project, &r.floor_material_id)?;
            finite_point("Room seed", r.seed, &ids)?;
            if r.name.trim().is_empty() {
                return Err(CoreError::invalid_for(
                    "bad_name",
                    "A room needs a name.",
                    ids.to_vec(),
                ));
            }
            Ok(())
        }
        Element::Column(c) => {
            require_level(project, &c.level_id)?;
            require_material(project, &c.material_id)?;
            finite_point("Column center", c.center, &ids)?;
            finite("Column rotation", c.rotation_deg, &ids)?;
            in_range(
                "column_size",
                "Column width",
                c.width_mm,
                50.0,
                5000.0,
                "mm",
                &ids,
            )?;
            in_range(
                "column_size",
                "Column depth",
                c.depth_mm,
                50.0,
                5000.0,
                "mm",
                &ids,
            )
        }
        Element::Stair(s) => {
            require_level(project, &s.level_id)?;
            finite_point("Stair origin", s.origin, &ids)?;
            finite("Stair rotation", s.rotation_deg, &ids)?;
            in_range(
                "stair_size",
                "Stair width",
                s.width_mm,
                300.0,
                10_000.0,
                "mm",
                &ids,
            )?;
            in_range(
                "stair_size",
                "Stair run",
                s.run_mm,
                300.0,
                30_000.0,
                "mm",
                &ids,
            )?;
            if s.riser_count < 1 || s.riser_count > 100 {
                return Err(CoreError::invalid_for(
                    "stair_size",
                    "A stair needs between 1 and 100 risers.",
                    ids.to_vec(),
                ));
            }
            Ok(())
        }
        Element::Asset(a) => {
            require_level(project, &a.level_id)?;
            finite_point("Object position", a.position, &ids)?;
            finite("Object rotation", a.rotation_deg, &ids)?;
            in_range(
                "asset_size",
                "Object width",
                a.width_mm,
                1.0,
                50_000.0,
                "mm",
                &ids,
            )?;
            in_range(
                "asset_size",
                "Object depth",
                a.depth_mm,
                1.0,
                50_000.0,
                "mm",
                &ids,
            )?;
            in_range(
                "asset_size",
                "Object height",
                a.height_mm,
                1.0,
                50_000.0,
                "mm",
                &ids,
            )?;
            in_range(
                "asset_size",
                "Object elevation",
                a.elevation_mm,
                -50_000.0,
                50_000.0,
                "mm",
                &ids,
            )
        }
        Element::Annotation(a) => {
            require_level(project, &a.level_id)?;
            finite_point("Note position", a.position, &ids)?;
            finite("Note rotation", a.rotation_deg, &ids)?;
            in_range(
                "annotation_size",
                "Text size",
                a.size_mm,
                1.0,
                100_000.0,
                "mm",
                &ids,
            )
        }
        Element::Dimension(d) => {
            require_level(project, &d.level_id)?;
            finite_point("Dimension point a", d.a, &ids)?;
            finite_point("Dimension point b", d.b, &ids)?;
            finite("Dimension offset", d.offset_mm, &ids)?;
            if dist(d.a, d.b) < JOIN_EPS {
                return Err(CoreError::invalid_for(
                    "dimension_too_short",
                    "A dimension needs two different points.",
                    ids.to_vec(),
                ));
            }
            Ok(())
        }
        Element::Camera(c) => {
            for (n, v) in [("Camera position", c.position), ("Camera target", c.target)] {
                finite(&format!("{n} x"), v.x, &ids)?;
                finite(&format!("{n} y"), v.y, &ids)?;
                finite(&format!("{n} z"), v.z, &ids)?;
            }
            in_range(
                "camera_fov",
                "Field of view",
                c.fov_deg,
                1.0,
                179.0,
                "degrees",
                &ids,
            )?;
            let d = ((c.position.x - c.target.x).powi(2)
                + (c.position.y - c.target.y).powi(2)
                + (c.position.z - c.target.z).powi(2))
            .sqrt();
            if d < JOIN_EPS {
                return Err(CoreError::invalid_for(
                    "camera_target",
                    "The camera position and target must be different points.",
                    ids.to_vec(),
                ));
            }
            Ok(())
        }
        Element::Linework(l) => {
            require_level(project, &l.level_id)?;
            if l.polylines.iter().all(|pl| pl.len() < 2) {
                return Err(CoreError::invalid_for("empty_linework", "Linework has no lines.", ids.to_vec()));
            }
            for pl in &l.polylines {
                for p in pl {
                    finite_point("Linework point", *p, &ids)?;
                }
            }
            Ok(())
        }
        Element::Pipe(p) => validate_pipe(project, p),
        Element::ReferenceModel(m) => {
            require_level(project, &m.level_id)?;
            finite_point("Model position", m.position, &ids)?;
            finite("Model rotation", m.rotation_deg, &ids)?;
            finite("Model elevation", m.elevation_mm, &ids)?;
            in_range("model_scale", "Model scale", m.scale_to_mm, 1e-6, 1e6, "mm per unit", &ids)?;
            if m.file_name.trim().is_empty() {
                return Err(CoreError::invalid_for("bad_file", "A reference model needs a file.", ids.to_vec()));
            }
            Ok(())
        }
        Element::Underlay(u) => {
            require_level(project, &u.level_id)?;
            finite_point("Underlay position", u.position, &ids)?;
            finite("Underlay rotation", u.rotation_deg, &ids)?;
            in_range(
                "underlay_scale",
                "Underlay scale",
                u.mm_per_px,
                1e-6,
                1e6,
                "mm per pixel",
                &ids,
            )?;
            in_range(
                "underlay_opacity",
                "Underlay opacity",
                u.opacity,
                0.0,
                1.0,
                "",
                &ids,
            )?;
            if u.width_px == 0 || u.height_px == 0 {
                return Err(CoreError::invalid_for(
                    "underlay_size",
                    "The underlay image has no size.",
                    ids.to_vec(),
                ));
            }
            Ok(())
        }
    }
}

fn opening_noun(o: &Opening) -> &'static str {
    match o.opening_type {
        OpeningType::Door => "door",
        OpeningType::Window => "window",
    }
}

/// Every opening on `wall_id` must fit the wall, keep clear of the wall
/// ends, not overlap its neighbors and stay below the wall top.
///
/// `moving` holds the openings this command added or changed. An overlap
/// message names one of them first, so dragging a door onto a window reads
/// "The door would overlap a window", not the other way round.
pub fn check_hosted_openings(
    project: &Project,
    wall_id: &str,
    moving: &BTreeSet<Id>,
) -> Result<(), CoreError> {
    let Some(Element::Wall(wall)) = project.elements.iter().find(|e| e.id() == wall_id) else {
        return Ok(());
    };
    let len = dist(wall.start, wall.end);
    let height = wall_height(project, wall);
    let mut spans: Vec<(f64, f64, &Opening)> = vec![];
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        if o.wall_id != wall_id {
            continue;
        }
        let (lo, hi) = (
            o.offset_mm - o.width_mm / 2.0,
            o.offset_mm + o.width_mm / 2.0,
        );
        if !(lo.is_finite() && hi.is_finite())
            || lo < OPENING_END_CLEARANCE_MM - FIT_EPS
            || hi > len - OPENING_END_CLEARANCE_MM + FIT_EPS
        {
            return Err(CoreError::invalid_for(
                "opening_outside_wall",
                format!(
                    "The {} ({:.0} mm wide, centered {:.0} mm from the wall start) does not fit on its wall, which is {:.0} mm long. Openings keep {:.0} mm clear of each wall end.",
                    opening_noun(o),
                    o.width_mm,
                    o.offset_mm,
                    len,
                    OPENING_END_CLEARANCE_MM
                ),
                vec![o.id.clone(), wall.id.clone()],
            ));
        }
        if o.sill_mm + o.height_mm > height + FIT_EPS {
            return Err(CoreError::invalid_for(
                "opening_too_tall",
                format!(
                    "The top of the {} (sill {:.0} + height {:.0} = {:.0} mm) is above the wall, which is {:.0} mm high.",
                    opening_noun(o),
                    o.sill_mm,
                    o.height_mm,
                    o.sill_mm + o.height_mm,
                    height
                ),
                vec![o.id.clone(), wall.id.clone()],
            ));
        }
        spans.push((lo, hi, o));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.id.cmp(&b.2.id)));
    for pair in spans.windows(2) {
        if pair[1].0 < pair[0].1 - FIT_EPS {
            // Name the element the user is moving or adding first. The spans
            // are sorted by position, which says nothing about which one moved.
            let (first, second) = if moving.contains(&pair[0].2.id) && !moving.contains(&pair[1].2.id) {
                (pair[0].2, pair[1].2)
            } else {
                (pair[1].2, pair[0].2)
            };
            return Err(CoreError::invalid_for(
                "opening_overlap",
                format!(
                    "The {} would overlap a {} on the same wall.",
                    opening_noun(first),
                    opening_noun(second)
                ),
                vec![first.id.clone(), second.id.clone()],
            ));
        }
    }
    Ok(())
}

fn only_rehosted(before: &Opening, after: &Opening) -> bool {
    let mut b = before.clone();
    b.offset_mm = after.offset_mm;
    b.wall_id = after.wall_id.clone();
    &b == after
}

/// Safety net run after every single command. Checks everything the command
/// added or changed, the openings of every touched wall, and locked layers.
pub fn post_validate(before: &Project, after: &Project) -> Result<(), CoreError> {
    let old: BTreeMap<&Id, &Element> = before.elements.iter().map(|e| (e.id(), e)).collect();
    let new_ids: BTreeSet<&Id> = after.elements.iter().map(|e| e.id()).collect();
    if new_ids.len() != after.elements.len() {
        return Err(CoreError::invalid(
            "duplicate_id",
            "Two elements would share one id.",
        ));
    }

    let mut changed: Vec<&Element> = vec![];
    let mut changed_walls: BTreeSet<Id> = BTreeSet::new();
    for el in &after.elements {
        if old.get(el.id()).map(|b| *b != el).unwrap_or(true) {
            changed.push(el);
            if let Element::Wall(w) = el {
                changed_walls.insert(w.id.clone());
            }
        }
    }

    // Locked layers. Openings that only ride along with an edited host wall
    // are not an edit of the Openings layer.
    let mut locked: Vec<(LayerKey, Id)> = vec![];
    let mut underlay_locked: Vec<Id> = vec![];
    for el in &changed {
        let prior = old.get(el.id());
        if let (Element::Opening(a), Some(Element::Opening(b))) = (el, prior) {
            if only_rehosted(b, a)
                && (changed_walls.contains(&a.wall_id) || changed_walls.contains(&b.wall_id))
            {
                continue;
            }
        }
        if let (Element::Underlay(a), Some(Element::Underlay(b))) = (el, prior) {
            if a.locked && b.locked {
                underlay_locked.push(a.id.clone());
            }
        }
        if let Some(key) = element_locked(after, el) {
            locked.push((key, el.id().clone()));
        } else if let Some(key) = prior.and_then(|b| element_locked(after, b)) {
            // Moving an element off a locked layer edits that layer too: a
            // pipe whose system changes from a locked one.
            locked.push((key, el.id().clone()));
        }
    }
    for el in &before.elements {
        if new_ids.contains(el.id()) {
            continue;
        }
        if let Some(key) = element_locked(after, el) {
            locked.push((key, el.id().clone()));
        }
        if let Element::Underlay(u) = el {
            if u.locked {
                underlay_locked.push(u.id.clone());
            }
        }
    }
    if let Some((key, _)) = locked.first().cloned() {
        let ids = locked
            .into_iter()
            .filter(|(k, _)| *k == key)
            .map(|(_, id)| id)
            .collect();
        return Err(locked_error(key, ids));
    }
    if !underlay_locked.is_empty() {
        return Err(CoreError::invalid_for(
            "underlay_locked",
            "This underlay is locked. Unlock it to move or delete it.",
            underlay_locked,
        ));
    }

    let mut walls_to_check: BTreeSet<Id> = changed_walls.clone();
    let mut moving: BTreeSet<Id> = BTreeSet::new();
    for el in &changed {
        validate_element(after, el)?;
        if let Element::Opening(o) = el {
            walls_to_check.insert(o.wall_id.clone());
            moving.insert(o.id.clone());
        }
    }
    for id in &walls_to_check {
        check_hosted_openings(after, id, &moving)?;
    }
    Ok(())
}
