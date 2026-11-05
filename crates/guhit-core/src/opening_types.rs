//! Door and window types with marks (DECISIONS D53). LANE L1 owns this
//! module.
//!
//! A type drives its openings, as in Revit: `upsert` writes the type's
//! style, size, sill (when the type has one) and material (when it has
//! one) into every opening of it in the same step, so the schedule always
//! matches the plan. The fit rules (`validate::check_hosted_openings`, run
//! by `post_validate`) still hold, so a type that pushes an opening past its
//! wall is refused.
//!
//! An opening's own edit is never refused (P0-6): `follow`, run after every
//! leaf command, moves a typed opening whose style, size, sill or material
//! the command changed to the type that matches, or to a new type with the
//! next free mark, and the step's summary says so ("Door moved from D1 to
//! D4"). An opening that is given a type (a new `type_id`, or a new typed
//! opening) takes that type's values instead; when they do not fit its
//! wall it keeps its own and goes to the type that matches them.
//!
//! Marks are "D1", "W1": unique (letter case aside), and an empty mark gets
//! the lowest free number of its kind.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::error::CoreError;
use crate::exec::{outcome, Outcome};
use crate::ids::IdGen;
use crate::validate::{check_hosted_openings, finite, in_range, require_level, require_material};

/// Sizes within this are the same size, mm.
const SAME_MM: f64 = 0.5;

/// A typed opening names a type that exists and is of its own kind
/// (validate hub, part of `Element::Opening`).
pub fn check(project: &Project, o: &Opening) -> Result<(), CoreError> {
    let Some(type_id) = &o.type_id else {
        return Ok(());
    };
    match project.opening_type(type_id) {
        None => Err(CoreError::invalid_for(
            "unknown_opening_type",
            format!("There is no door or window type with id {type_id}."),
            vec![o.id.clone()],
        )),
        Some(t) if t.opening_type != o.opening_type => Err(CoreError::invalid_for(
            "opening_type_mismatch",
            format!(
                "{} is a {} type, so a {} cannot use it.",
                t.mark,
                noun(t.opening_type),
                noun(o.opening_type)
            ),
            vec![o.id.clone()],
        )),
        Some(_) => Ok(()),
    }
}

fn noun(t: OpeningType) -> &'static str {
    match t {
        OpeningType::Door => "door",
        OpeningType::Window => "window",
    }
}

fn capital(t: OpeningType) -> &'static str {
    match t {
        OpeningType::Door => "Door",
        OpeningType::Window => "Window",
    }
}

/// The letter a kind's marks start with.
pub fn mark_prefix(t: OpeningType) -> &'static str {
    match t {
        OpeningType::Door => "D",
        OpeningType::Window => "W",
    }
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= SAME_MM
}

fn mark_taken(project: &Project, mark: &str, except: &str) -> Option<String> {
    project
        .opening_types
        .iter()
        .find(|t| t.id != except && t.mark.trim().eq_ignore_ascii_case(mark.trim()))
        .map(|t| t.mark.clone())
}

/// The lowest free "D<n>" or "W<n>".
pub fn next_mark(project: &Project, kind: OpeningType) -> String {
    (1..)
        .map(|n| format!("{}{n}", mark_prefix(kind)))
        .find(|m| mark_taken(project, m, "").is_none())
        .unwrap_or_default()
}

/// Mark order: the prefix letters, then the number as a number (D2 before
/// D10), then the text.
pub fn mark_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn split(m: &str) -> (String, Option<u64>, String) {
        let m = m.trim();
        let head: String = m.chars().take_while(|c| !c.is_ascii_digit()).collect();
        let digits: String = m[head.len()..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        (
            head.to_ascii_uppercase(),
            digits.parse().ok(),
            m.to_string(),
        )
    }
    let (a, b) = (split(a), split(b));
    a.0.cmp(&b.0)
        .then(a.1.unwrap_or(u64::MAX).cmp(&b.1.unwrap_or(u64::MAX)))
        .then(a.2.cmp(&b.2))
}

/// Does the opening already carry everything type `t` drives?
pub fn matches(t: &OpeningSpec, o: &Opening) -> bool {
    t.opening_type == o.opening_type
        && t.style == o.style
        && same(t.width_mm, o.width_mm)
        && same(t.height_mm, o.height_mm)
        && t.sill_mm.map(|s| same(s, o.sill_mm)).unwrap_or(true)
        && t.material_id
            .as_ref()
            .map(|m| o.material_id.as_ref() == Some(m))
            .unwrap_or(true)
}

/// Write type `t` into the opening.
fn apply(t: &OpeningSpec, o: &mut Opening) {
    o.type_id = Some(t.id.clone());
    o.style = t.style;
    o.width_mm = t.width_mm;
    o.height_mm = t.height_mm;
    if let Some(s) = t.sill_mm {
        o.sill_mm = s;
    }
    if t.material_id.is_some() {
        o.material_id = t.material_id.clone();
    }
}

/// "Single swing door", "Sliding window": a type's first description.
pub fn default_description(kind: OpeningType, style: OpeningStyle) -> String {
    let s = match style {
        OpeningStyle::SwingSingle => "Single swing",
        OpeningStyle::SwingDouble => "Double swing",
        OpeningStyle::Sliding => "Sliding",
        OpeningStyle::Fixed => "Fixed",
        OpeningStyle::Casement => "Casement",
        OpeningStyle::Jalousie => "Jalousie",
    };
    format!("{s} {}", noun(kind))
}

/// A new type that `o` matches: a window keeps its sill in the type, a door
/// leaves the sill to each opening.
fn spec_of(o: &Opening, id: Id, mark: String) -> OpeningSpec {
    OpeningSpec {
        id,
        mark,
        opening_type: o.opening_type,
        style: o.style,
        width_mm: o.width_mm,
        height_mm: o.height_mm,
        sill_mm: match o.opening_type {
            OpeningType::Window => Some(o.sill_mm),
            OpeningType::Door => None,
        },
        material_id: o.material_id.clone(),
        description: default_description(o.opening_type, o.style),
        remarks: String::new(),
    }
}

/// The rules of one type on its own.
fn validate_spec(project: &Project, t: &OpeningSpec) -> Result<(), CoreError> {
    let ids: [Id; 0] = [];
    for (name, v) in [("Type width", t.width_mm), ("Type height", t.height_mm)] {
        finite(name, v, &ids)?;
    }
    in_range(
        "opening_size",
        "Type width",
        t.width_mm,
        300.0,
        10_000.0,
        "mm",
        &ids,
    )?;
    in_range(
        "opening_size",
        "Type height",
        t.height_mm,
        100.0,
        10_000.0,
        "mm",
        &ids,
    )?;
    if let Some(s) = t.sill_mm {
        in_range(
            "opening_size",
            "Type sill height",
            s,
            0.0,
            10_000.0,
            "mm",
            &ids,
        )?;
    }
    require_material(project, &t.material_id)?;
    if t.mark.trim().is_empty() {
        return Err(CoreError::invalid(
            "bad_name",
            "A door or window type needs a mark, such as D1 or W1.",
        ));
    }
    if let Some(other) = mark_taken(project, &t.mark, &t.id) {
        return Err(CoreError::invalid(
            "mark_taken",
            format!(
                "There is already a type {other}. Leave the mark empty to get the next free one ({}), or edit {other} instead.",
                next_mark(project, t.opening_type)
            ),
        ));
    }
    Ok(())
}

fn openings_of<'a>(
    project: &'a Project,
    type_id: &'a str,
) -> impl Iterator<Item = &'a Opening> + 'a {
    project.elements.iter().filter_map(move |e| match e {
        Element::Opening(o) if o.type_id.as_deref() == Some(type_id) => Some(o),
        _ => None,
    })
}

fn count_noun(n: usize, kind: OpeningType) -> String {
    crate::exec::count_noun(n, noun(kind))
}

/// `Command::UpsertOpeningSpec`: add or replace a type, every opening of it
/// following in the same step.
pub fn upsert(
    project: &mut Project,
    ids: &mut IdGen,
    spec: &OpeningSpec,
) -> Result<Outcome, CoreError> {
    let mut t = spec.clone();
    t.mark = t.mark.trim().to_string();
    t.description = t.description.trim().to_string();
    t.remarks = t.remarks.trim().to_string();
    if t.id.trim().is_empty() {
        t.id = ids.next_id();
    } else {
        t.id = t.id.trim().to_string();
        ids.reserve(&t.id);
    }
    let at = project.opening_types.iter().position(|x| x.id == t.id);
    if let Some(i) = at {
        let old = &project.opening_types[i];
        let using = openings_of(project, &t.id).count();
        if old.opening_type != t.opening_type && using > 0 {
            return Err(CoreError::invalid(
                "opening_type_mismatch",
                format!(
                    "{} is used by {}, so it stays a {} type. Make a new {} type instead.",
                    old.mark,
                    count_noun(using, old.opening_type),
                    noun(old.opening_type),
                    noun(t.opening_type)
                ),
            ));
        }
    }
    if t.mark.is_empty() {
        t.mark = next_mark(project, t.opening_type);
    }
    if t.description.is_empty() {
        t.description = default_description(t.opening_type, t.style);
    }
    validate_spec(project, &t)?;

    let mut n = 0;
    for el in project.elements.iter_mut() {
        if let Element::Opening(o) = el {
            if o.type_id.as_deref() == Some(t.id.as_str()) {
                apply(&t, o);
                n += 1;
            }
        }
    }
    let kind = t.opening_type;
    let summary = match at {
        Some(_) if n > 0 => format!("Changed type {}: {} follow", t.mark, count_noun(n, kind)),
        Some(_) => format!("Changed type {}", t.mark),
        None if n > 0 => format!("Added type {}: {} follow", t.mark, count_noun(n, kind)),
        None => format!("Added {} type {}", noun(kind), t.mark),
    };
    let label = match (at.is_some(), kind) {
        (true, OpeningType::Door) => "Edit door type",
        (true, OpeningType::Window) => "Edit window type",
        (false, OpeningType::Door) => "Add door type",
        (false, OpeningType::Window) => "Add window type",
    };
    match at {
        Some(i) => project.opening_types[i] = t,
        None => project.opening_types.push(t),
    }
    Ok(Outcome {
        summary: Some(summary),
        ..outcome(label)
    })
}

/// `Command::DeleteOpeningSpecs`: their openings become untyped at their
/// size.
pub fn delete(project: &mut Project, type_ids: &[Id]) -> Result<Outcome, CoreError> {
    if type_ids.is_empty() {
        return Err(CoreError::invalid(
            "empty_selection",
            "No door or window types were given.",
        ));
    }
    let mut marks = vec![];
    for id in type_ids {
        match project.opening_type(id) {
            Some(t) => marks.push(t.mark.clone()),
            None => {
                return Err(CoreError::invalid(
                    "unknown_opening_type",
                    format!("There is no door or window type with id {id}."),
                ))
            }
        }
    }
    let mut n = 0;
    for el in project.elements.iter_mut() {
        if let Element::Opening(o) = el {
            if o.type_id
                .as_ref()
                .map(|t| type_ids.contains(t))
                .unwrap_or(false)
            {
                o.type_id = None;
                n += 1;
            }
        }
    }
    project.opening_types.retain(|t| !type_ids.contains(&t.id));
    let what = if marks.len() == 1 {
        format!("type {}", marks[0])
    } else {
        format!("types {}", marks.join(", "))
    };
    let summary = match n {
        0 => format!("Deleted {what}"),
        1 => format!("Deleted {what}; 1 opening is untyped now"),
        n => format!("Deleted {what}; {n} openings are untyped now"),
    };
    Ok(Outcome {
        summary: Some(summary),
        ..outcome(if marks.len() == 1 {
            "Delete type"
        } else {
            "Delete types"
        })
    })
}

/// The level each opening stands on: its wall's.
fn opening_levels(project: &Project) -> BTreeMap<Id, Id> {
    let walls: BTreeMap<&Id, &Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((&w.id, &w.level_id)),
            _ => None,
        })
        .collect();
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) => walls.get(&o.wall_id).map(|l| (o.id.clone(), (*l).clone())),
            _ => None,
        })
        .collect()
}

/// The order new types get their marks in: doors, then windows; the widest
/// first (the main door is D1), then the tallest, then by style, then the
/// lowest sill.
fn style_rank(s: OpeningStyle) -> u8 {
    match s {
        OpeningStyle::SwingDouble => 0,
        OpeningStyle::SwingSingle => 1,
        OpeningStyle::Sliding => 2,
        OpeningStyle::Casement => 3,
        OpeningStyle::Jalousie => 4,
        OpeningStyle::Fixed => 5,
    }
}

/// `Command::AssignOpeningTypes`: every untyped opening (on `level_id`, or
/// everywhere) gets a type. Openings of one kind, style, size, material and
/// (windows) sill share one; an existing type that matches is reused, else
/// a new one gets the next free mark, in the order of `style_rank`.
pub fn assign(
    project: &mut Project,
    ids: &mut IdGen,
    level_id: &Option<Id>,
) -> Result<Outcome, CoreError> {
    if let Some(l) = level_id {
        require_level(project, l)?;
    }
    let levels = opening_levels(project);
    let untyped: Vec<Opening> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) if o.type_id.is_none() => Some(o.clone()),
            _ => None,
        })
        .filter(|o| {
            level_id
                .as_ref()
                .map(|l| levels.get(&o.id) == Some(l))
                .unwrap_or(true)
        })
        .collect();
    if untyped.is_empty() {
        return Err(CoreError::invalid(
            "nothing_to_type",
            if level_id.is_some() {
                "Every door and window on this level already has a type."
            } else {
                "Every door and window already has a type."
            },
        ));
    }

    // Openings that no existing type matches, in mark order.
    let mut fresh: Vec<&Opening> = untyped
        .iter()
        .filter(|o| !project.opening_types.iter().any(|t| matches(t, o)))
        .collect();
    fresh.sort_by(|a, b| {
        let kind = |o: &Opening| matches!(o.opening_type, OpeningType::Window) as u8;
        kind(a)
            .cmp(&kind(b))
            .then(b.width_mm.total_cmp(&a.width_mm))
            .then(b.height_mm.total_cmp(&a.height_mm))
            .then(style_rank(a.style).cmp(&style_rank(b.style)))
            .then(a.sill_mm.total_cmp(&b.sill_mm))
            .then(a.material_id.cmp(&b.material_id))
            .then(a.id.cmp(&b.id))
    });
    let mut made: Vec<String> = vec![];
    for o in fresh {
        if project.opening_types.iter().any(|t| matches(t, o)) {
            continue;
        }
        let mark = next_mark(project, o.opening_type);
        let spec = spec_of(o, ids.next_id(), mark.clone());
        project.opening_types.push(spec);
        made.push(mark);
    }

    let types = project.opening_types.clone();
    let mut n = 0;
    for el in project.elements.iter_mut() {
        let Element::Opening(o) = el else { continue };
        if !untyped.iter().any(|u| u.id == o.id) {
            continue;
        }
        if let Some(t) = types.iter().find(|t| matches(t, o)) {
            o.type_id = Some(t.id.clone());
            n += 1;
        }
    }
    let typed = crate::exec::count_noun(n, "opening");
    let summary = match made.len() {
        0 => format!("Typed {typed} with the existing types"),
        1 => format!("Typed {typed}: new type {}", made[0]),
        _ => format!("Typed {typed}: new types {}", made.join(", ")),
    };
    Ok(Outcome {
        summary: Some(summary),
        ..outcome("Assign types")
    })
}

/// A new opening from `AddOpening` with a `type_id`: its type's style, size
/// and sill win over the command's (a sill None on the type keeps the
/// opening's), and its material when it has one.
pub fn apply_to_new(project: &Project, o: &mut Opening) -> Result<(), CoreError> {
    if o.type_id.is_none() {
        return Ok(());
    }
    check(project, o)?;
    if let Some(t) = o.type_id.as_ref().and_then(|id| project.opening_type(id)) {
        apply(&t.clone(), o);
    }
    Ok(())
}

/// Runs after every leaf command, after `post_validate` (exec hub): typed
/// openings that no longer carry their type's values move (P0-6). Returns a
/// phrase per move for the step's summary.
///
/// - An opening that just got its type (new, or a new `type_id`) takes the
///   type's values, when they fit its wall; else it keeps its own and moves.
/// - An opening whose own style, size, sill or material changed goes to the
///   type that matches, or to a new type with the next free mark.
pub fn follow(before: &Project, project: &mut Project, ids: &mut IdGen) -> Vec<String> {
    if project.opening_types.is_empty() {
        return vec![];
    }
    let old: BTreeMap<&Id, &Opening> = before
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) => Some((&o.id, o)),
            _ => None,
        })
        .collect();
    let mut notes = vec![];
    for i in 0..project.elements.len() {
        let Element::Opening(o) = &project.elements[i] else {
            continue;
        };
        let Some(tid) = o.type_id.clone() else {
            continue;
        };
        let Some(t) = project.opening_type(&tid).cloned() else {
            continue;
        };
        if matches(&t, o) {
            continue;
        }
        let prior = old.get(&o.id).copied();
        let newly_typed = prior
            .map(|p| p.type_id.as_deref() != Some(tid.as_str()))
            .unwrap_or(true);
        if newly_typed {
            let own = o.clone();
            let wall = own.wall_id.clone();
            if let Element::Opening(o) = &mut project.elements[i] {
                apply(&t, o);
            }
            let fits =
                check_hosted_openings(project, &wall, &[own.id.clone()].into_iter().collect())
                    .is_ok();
            if fits {
                continue;
            }
            project.elements[i] = Element::Opening(own);
        }
        let Element::Opening(o) = &project.elements[i] else {
            continue;
        };
        let o = o.clone();
        let (to, new) = match project
            .opening_types
            .iter()
            .find(|x| x.id != tid && matches(x, &o))
        {
            Some(x) => (x.clone(), false),
            None => {
                let spec = OpeningSpec {
                    description: if t.style == o.style && t.opening_type == o.opening_type {
                        t.description.clone()
                    } else {
                        default_description(o.opening_type, o.style)
                    },
                    ..spec_of(&o, ids.next_id(), next_mark(project, o.opening_type))
                };
                project.opening_types.push(spec.clone());
                (spec, true)
            }
        };
        if let Element::Opening(x) = &mut project.elements[i] {
            x.type_id = Some(to.id.clone());
        }
        let kind = capital(o.opening_type);
        notes.push(match (newly_typed, new) {
            (true, true) => format!(
                "{kind} kept its size, which {} does not fit; new type {}",
                t.mark, to.mark
            ),
            (true, false) => format!(
                "{kind} kept its size, which {} does not fit; type {}",
                t.mark, to.mark
            ),
            (false, true) => format!("{kind} moved from {} to new type {}", t.mark, to.mark),
            (false, false) => format!("{kind} moved from {} to {}", t.mark, to.mark),
        });
    }
    notes
}
