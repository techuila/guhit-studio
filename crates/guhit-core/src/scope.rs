//! AI edit scope (DECISIONS D30): does a command stay inside the part of the
//! plan the user selected?
//!
//! What a selection reaches is derived from the project being checked, never
//! stored, so it follows every staged step:
//! - a room reaches itself, its bounding walls, the doors and windows on its
//!   part of those walls, and on its level every wall, column, stair, object,
//!   note, dimension (both ends), pipe (every point and segment) and camera
//!   inside its centerline polygon;
//! - a wall reaches itself and its doors and windows;
//! - anything else reaches itself;
//! - what earlier steps of the turn made (`check_staged`) reaches itself, and
//!   a wall its doors and windows.
//!
//! New elements must land in the selection's area, on its level: each
//! selected room's centerline polygon with `ROOM_TOLERANCE_MM`, and the
//! bounding box of every other selected element grown by `AREA_MARGIN_MM`.
//! What earlier steps made never widens it. Roof, levels, layers, settings
//! and materials are outside every scope.
//! What the engine changes as a consequence of an allowed command (connected
//! walls stretching, joined pipe runs following, dimensions following, links
//! removed, rooms appearing in closed faces) is allowed.

use std::collections::BTreeSet;

use guhit_model::*;

use crate::derive::compute_derived;
use crate::error::CoreError;
use crate::exec::{self, count_noun, diff_noun, resolve_level};
use crate::geom::*;
use crate::pipes::name_start;
use crate::validate::require_level;

/// A point this close to a selected room's centerline polygon is inside it,
/// so a wall that ends on a bounding wall's centerline is inside.
pub const ROOM_TOLERANCE_MM: f64 = 50.0;

/// How far past its bounding box a selected element other than a room lets
/// new elements go.
pub const AREA_MARGIN_MM: f64 = 500.0;

/// New walls and pipe runs are checked at points at most this far apart.
const SAMPLE_STEP_MM: f64 = 25.0;

/// Cap on the points checked on one segment.
const MAX_SAMPLES: usize = 4000;

/// `describe` names at most this many selected elements.
const MAX_PARTS: usize = 6;

/// The end of every refusal.
const OUTSIDE: &str = "outside the selection this edit is limited to.";

/// Check a scope before a turn: at least one id, and every id in the plan.
pub fn validate(project: &Project, scope_ids: &[Id]) -> Result<(), CoreError> {
    if scope_ids.is_empty() {
        return Err(CoreError::invalid(
            "out_of_scope",
            "The selection this edit is limited to is empty, so nothing may change.",
        ));
    }
    match scope_ids.iter().find(|id| find(project, id).is_none()) {
        Some(id) => Err(CoreError::NotFound(id.clone())),
        None => Ok(()),
    }
}

/// Ok when `command` stays inside the selection `scope_ids` (docs/CONTRACT.md,
/// "AI edit scope"). `derived` must be `compute_derived(project)`.
///
/// A batch is checked step by step on the project as its earlier steps leave
/// it, and what those steps made is in reach for the steps after them.
///
/// Errors: `out_of_scope` naming the element, `not_found` for a scope id or a
/// target that is not in the plan, or the engine's own error when an earlier
/// step of a batch cannot run, since the steps after it cannot be checked.
pub fn check(
    project: &Project,
    derived: &Derived,
    scope_ids: &[Id],
    command: &Command,
) -> Result<(), CoreError> {
    validate(project, scope_ids)?;
    check_staged(project, derived, scope_ids, &[], command)
}

/// `check` for a command staged after others in the same turn. `project` and
/// `derived` are the plan with the earlier steps applied. `made_ids` are the
/// elements those steps made: they are in reach, so a later step can build
/// on them (a door on a wall the turn added), but they never widen the area
/// for new elements. Selected ids that an earlier step removed are skipped.
pub fn check_staged(
    project: &Project,
    derived: &Derived,
    scope_ids: &[Id],
    made_ids: &[Id],
    command: &Command,
) -> Result<(), CoreError> {
    match command {
        Command::Batch { commands, .. } => steps(project, derived, scope_ids, made_ids, commands),
        leaf => Scope::new(project, derived, scope_ids, made_ids)?.leaf(leaf),
    }
}

/// One short sentence of what the selection holds, for the model: "Room
/// Bedroom with its 4 walls, 1 door, 1 window and 3 objects, on Ground
/// Floor". Ids that are not in the plan are left out.
pub fn describe(project: &Project, derived: &Derived, scope_ids: &[Id]) -> String {
    let mut seen: BTreeSet<&Id> = BTreeSet::new();
    let selected: Vec<&Element> = scope_ids
        .iter()
        .filter(|id| seen.insert(*id))
        .filter_map(|id| find(project, id))
        .collect();
    if selected.is_empty() {
        return "nothing".to_string();
    }
    let shown = if selected.len() > MAX_PARTS {
        MAX_PARTS - 1
    } else {
        selected.len()
    };
    let mut parts: Vec<String> = selected[..shown]
        .iter()
        .map(|el| part(project, derived, el))
        .collect();
    if shown < selected.len() {
        parts.push(count_noun(selected.len() - shown, "more element"));
    }
    let mut sentence = parts.join("; ");
    let levels: Vec<&str> = project
        .levels
        .iter()
        .filter(|l| {
            selected
                .iter()
                .any(|el| level_of(project, el) == Some(&l.id))
        })
        .map(|l| l.name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    if !levels.is_empty() {
        sentence.push_str(&format!(", on {}", join_and(&levels)));
    }
    sentence
}

// ---------------------------------------------------------------- batches

/// A batch runs all or nothing, one step after the other, so each step is
/// checked on the project as the steps before it leave it.
fn steps(
    project: &Project,
    derived: &Derived,
    scope_ids: &[Id],
    made_ids: &[Id],
    commands: &[Command],
) -> Result<(), CoreError> {
    let n = commands.len();
    let mut made: Vec<Id> = made_ids.to_vec();
    let mut staged: Option<(Project, Derived)> = None;
    for (i, command) in commands.iter().enumerate() {
        let (p, d) = match &staged {
            Some((p, d)) => (p, d),
            None => (project, derived),
        };
        check_staged(p, d, scope_ids, &made, command).map_err(|e| step_error(e, i, n))?;
        if i + 1 == n {
            break;
        }
        let mut next = p.clone();
        exec::execute(&mut next, command).map_err(|e| step_error(e, i, n))?;
        let had: BTreeSet<&Id> = p.elements.iter().map(Element::id).collect();
        let new: Vec<Id> = next
            .elements
            .iter()
            .map(Element::id)
            .filter(|id| !had.contains(id))
            .cloned()
            .collect();
        made.retain(|id| find(&next, id).is_some());
        made.extend(new);
        let d = compute_derived(&next);
        staged = Some((next, d));
    }
    Ok(())
}

/// "Step 2 of 3: ...", as the engine words the errors of a batch.
fn step_error(e: CoreError, i: usize, n: usize) -> CoreError {
    match e {
        CoreError::Invalid {
            code,
            message,
            element_ids,
        } => CoreError::Invalid {
            code,
            message: format!("Step {} of {n}: {message}", i + 1),
            element_ids,
        },
        other => other,
    }
}

// ------------------------------------------------------------------ scope

/// A selection resolved on one project: what it reaches and where new
/// elements may go.
struct Scope<'a> {
    project: &'a Project,
    derived: &'a Derived,
    selected: &'a [Id],
    made: &'a [Id],
    reach: BTreeSet<Id>,
    areas: Vec<Area>,
}

/// Part of the area for new elements, on one level.
struct Area {
    level_id: Id,
    shape: Shape,
}

enum Shape {
    /// A selected room's centerline polygon, with `ROOM_TOLERANCE_MM`.
    Room(Vec<Point>),
    /// A bounding box, already grown by `AREA_MARGIN_MM`.
    Box(Point, Point),
}

impl Shape {
    fn contains(&self, p: Point) -> bool {
        match self {
            Shape::Room(poly) => near_polygon(p, poly),
            Shape::Box(lo, hi) => p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y,
        }
    }
}

impl<'a> Scope<'a> {
    fn new(
        project: &'a Project,
        derived: &'a Derived,
        selected: &'a [Id],
        made: &'a [Id],
    ) -> Result<Self, CoreError> {
        let mut reach = BTreeSet::new();
        let mut areas = vec![];
        for el in selected.iter().filter_map(|id| find(project, id)) {
            match el {
                Element::Room(room) => room_reach(project, derived, room, &mut reach),
                Element::Wall(w) => wall_reach(project, &w.id, &mut reach),
                other => {
                    reach.insert(other.id().clone());
                }
            }
            areas.extend(area_of(project, derived, el));
        }
        // What earlier steps made reaches itself, and a wall its doors and
        // windows (a split moves some onto the new half). Never a room's
        // contents: a room the engine closed off may lie outside the area.
        for el in made.iter().filter_map(|id| find(project, id)) {
            match el {
                Element::Wall(w) => wall_reach(project, &w.id, &mut reach),
                other => {
                    reach.insert(other.id().clone());
                }
            }
        }
        if reach.is_empty() {
            return Err(CoreError::invalid(
                "out_of_scope",
                "Nothing of the selection this edit is limited to is left in the plan, so nothing may change. To replace a selected element, add the new one before removing the old.",
            ));
        }
        Ok(Self {
            project,
            derived,
            selected,
            made,
            reach,
            areas,
        })
    }

    /// The command table of docs/CONTRACT.md, "AI edit scope".
    fn leaf(&self, command: &Command) -> Result<(), CoreError> {
        match command {
            Command::Batch { commands, .. } => steps(
                self.project,
                self.derived,
                self.selected,
                self.made,
                commands,
            ),
            Command::AddWall {
                start,
                end,
                level_id,
                ..
            } => {
                let level = resolve_level(self.project, level_id)?;
                self.new_path(&level, &[*start, *end], "The new wall")
            }
            Command::AddWallChain {
                points,
                closed,
                level_id,
                ..
            } => {
                let level = resolve_level(self.project, level_id)?;
                let mut path = points.clone();
                if *closed && points.len() > 2 {
                    path.push(points[0]);
                }
                self.new_path(&level, &path, "The new walls")
            }
            Command::AddRectRoom {
                origin,
                width_mm,
                depth_mm,
                level_id,
                ..
            } => {
                let level = resolve_level(self.project, level_id)?;
                let (x0, y0) = (origin.x, origin.y);
                let (x1, y1) = (x0 + width_mm, y0 + depth_mm);
                let corners = [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1), pt(x0, y0)];
                self.new_path(&level, &corners, "The new room")
            }
            Command::SetWallEndpoints { wall_id, .. }
            | Command::SetWallLength { wall_id, .. }
            | Command::SplitWall { wall_id, .. } => self.targets(std::slice::from_ref(wall_id)),
            Command::AddOpening {
                wall_id,
                opening_type,
                offset_mm,
                ..
            } => self.new_opening(
                wall_id,
                *offset_mm,
                &format!("The new {}", opening_noun(*opening_type)),
            ),
            Command::ResizeRoom { room_id, .. } => self.targets(std::slice::from_ref(room_id)),
            Command::AddElement { element } => self.new_element(element),
            Command::UpdateElement { element } => self.update(element),
            Command::DeleteElements { ids }
            | Command::MoveElements { ids, .. }
            | Command::RotateElements { ids, .. }
            | Command::SetMaterial { ids, .. } => self.targets(ids),
            Command::DuplicateElements { ids, delta } => {
                self.targets(ids)?;
                self.copies(ids, *delta)
            }
            Command::SetReviewMark { target, note } => self.review_mark(target, note.is_some()),
            // Pipe editing (DECISIONS D39): every run the command edits must
            // be in reach. Joined runs that follow with `keep_joins` are a
            // consequence, like the walls `SetWallLength` stretches.
            Command::MovePipePoints { targets, .. } => {
                let ids: Vec<Id> = targets.iter().map(|t| t.pipe_id.clone()).collect();
                self.targets(&ids)
            }
            Command::SetPipeSegmentLength { pipe_id, .. }
            | Command::SetPipeSlope { pipe_id, .. }
            | Command::InsertPipePoint { pipe_id, .. }
            | Command::RemovePipePoint { pipe_id, .. } => self.targets(std::slice::from_ref(pipe_id)),
            Command::SetRoof { .. } => Err(whole("Changing the roof")),
            Command::SetProjectSettings { .. } => Err(whole("Changing the project settings")),
            Command::UpdateLevel { .. } => Err(whole("Changing a level")),
            Command::AddLevel { .. } => Err(whole("Adding a level")),
            Command::DeleteLevel { .. } => Err(whole("Deleting a level")),
            Command::SetLayer { .. } => Err(whole("Changing a layer")),
            Command::UpsertMaterial { .. } => Err(whole("Adding or changing a material")),
            // Wave 2 (D54). Roof posts stand under the roof they name.
            Command::AddRoofPosts { roof_id, .. } => self.targets(std::slice::from_ref(roof_id)),
            // Tours are views, like cameras: always allowed.
            Command::MakeTour { .. } => Ok(()),
            // Types, grids, automatic dimensions, the lot, the title block,
            // the signatory and sheets act like settings.
            Command::UpsertOpeningSpec { .. }
            | Command::DeleteOpeningSpecs { .. }
            | Command::AssignOpeningTypes { .. } => Err(whole("Changing door and window types")),
            Command::AddGridSystem { .. } => Err(whole("Adding grid lines")),
            Command::AutoDimension { .. } => Err(whole("Adding automatic dimensions")),
            Command::SetLotFromSurvey { .. } => Err(whole("Setting the lot")),
            Command::SetTitleBlock { .. } | Command::SetSignatory { .. } => Err(whole("Changing the title block")),
            Command::UpsertSheet { .. }
            | Command::DeleteSheets { .. }
            | Command::MakeSheetSet { .. }
            | Command::UpsertRevision { .. }
            | Command::DeleteRevision { .. } => Err(whole("Changing the sheets")),
        }
    }

    fn element(&self, id: &Id) -> Result<&'a Element, CoreError> {
        find(self.project, id).ok_or_else(|| CoreError::NotFound(id.clone()))
    }

    fn in_area(&self, level_id: &str, p: Point) -> bool {
        is_finite(p)
            && self
                .areas
                .iter()
                .any(|a| a.level_id == level_id && a.shape.contains(p))
    }

    /// Every target must be in reach. Names the first one that is not and
    /// lists them all in the error.
    fn targets(&self, ids: &[Id]) -> Result<(), CoreError> {
        let mut outside: Vec<&Element> = vec![];
        for id in ids {
            let el = self.element(id)?;
            // The roof over the top floor and the lot act like settings,
            // selected or not (plan 1.8).
            if let Some(what) = settings_like(el) {
                return Err(whole(what));
            }
            if !self.reach.contains(id) && !outside.iter().any(|o| o.id() == id) {
                outside.push(el);
            }
        }
        let ids: Vec<Id> = outside.iter().map(|el| el.id().clone()).collect();
        match outside.as_slice() {
            [] => Ok(()),
            [one] => Err(out(format!("{} is {OUTSIDE}", label(one)), ids)),
            [first, rest @ ..] => Err(out(
                format!(
                    "{} and {} are {OUTSIDE}",
                    label(first),
                    count_noun(rest.len(), "other element")
                ),
                ids,
            )),
        }
    }

    /// New walls: every point and every segment between them in the area.
    fn new_path(&self, level_id: &str, points: &[Point], what: &str) -> Result<(), CoreError> {
        if along(points, |p| self.in_area(level_id, p)) {
            Ok(())
        } else {
            Err(out(format!("{what} would be {OUTSIDE}"), vec![]))
        }
    }

    /// A new door or window: its wall in reach, and its center in the area,
    /// so it goes on the selection's part of a wall the next room shares.
    fn new_opening(&self, wall_id: &Id, offset_mm: f64, what: &str) -> Result<(), CoreError> {
        let el = self.element(wall_id)?;
        let Element::Wall(wall) = el else {
            // The engine refuses an opening on anything but a wall.
            return Ok(());
        };
        let host = label(el);
        if !self.reach.contains(wall_id) {
            return Err(out(
                format!("{what} would be on {host}, which is {OUTSIDE}"),
                vec![wall_id.clone()],
            ));
        }
        match on_wall(wall, offset_mm) {
            Some(center) if !self.in_area(&wall.level_id, center) => Err(out(
                format!(
                    "{what} would be on a part of {host} that is {OUTSIDE}{}",
                    self.covered(wall)
                ),
                vec![wall_id.clone()],
            )),
            _ => Ok(()),
        }
    }

    /// " The selection covers it from 4950 to 8000 mm from its start.": where
    /// along a wall the center of a door or window may go, so the model can
    /// correct the offset. Empty when no part of the wall is in the area.
    fn covered(&self, wall: &Wall) -> String {
        let Some(d) = unit(sub(wall.end, wall.start)) else {
            return String::new();
        };
        let len = dist(wall.start, wall.end);
        let inside = |t: f64| self.in_area(&wall.level_id, add(wall.start, scale(d, t)));
        // The edge between two samples on either side of it, to half a mm:
        // the last point inside, or the first.
        let edge = |mut lo: f64, mut hi: f64| {
            let lo_in = inside(lo);
            while hi - lo > 0.5 {
                let mid = (lo + hi) / 2.0;
                if inside(mid) == lo_in {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if lo_in {
                lo
            } else {
                hi
            }
        };
        let n = ((len / SAMPLE_STEP_MM).ceil() as usize).clamp(1, MAX_SAMPLES);
        let mut parts: Vec<(f64, f64)> = vec![];
        let (mut start, mut prev) = (None, 0.0);
        for k in 0..=n {
            let t = len * k as f64 / n as f64;
            match (inside(t), start) {
                (true, None) => start = Some(if k == 0 { 0.0 } else { edge(prev, t) }),
                (false, Some(s)) => {
                    parts.push((s, edge(prev, t)));
                    start = None;
                }
                _ => {}
            }
            prev = t;
        }
        if let Some(s) = start {
            parts.push((s, len));
        }
        if parts.is_empty() {
            return String::new();
        }
        let spans: Vec<String> = parts
            .iter()
            .map(|(a, b)| format!("{a:.0} to {b:.0}"))
            .collect();
        format!(
            " The selection covers it from {} mm from its start.",
            spans.join(" and from ")
        )
    }

    fn new_element(&self, element: &Element) -> Result<(), CoreError> {
        if let Some(what) = settings_like(element) {
            return Err(whole(what));
        }
        match element {
            // A view changes no part of the plan.
            el if el.is_view() => Ok(()),
            // A grid line holds on every level.
            Element::Grid(_) => Err(whole("Adding a grid line")),
            Element::Opening(o) => self.new_opening(
                &o.wall_id,
                o.offset_mm,
                &format!("The new {}", opening_noun(o.opening_type)),
            ),
            other => self.lands(
                other,
                pt(0.0, 0.0),
                &format!("The new {}", noun(other)),
                vec![],
            ),
        }
    }

    /// A new or copied element, `delta` away from `el`, must land in the area
    /// on its level. Underlays, linework and reference models never do: they
    /// are tracing references for the whole plan.
    fn lands(&self, el: &Element, delta: Point, what: &str, ids: Vec<Id>) -> Result<(), CoreError> {
        let refused = || Err(out(format!("{what} would be {OUTSIDE}"), ids.clone()));
        let (Some(level_id), Some(anchor)) = (level_of(self.project, el), anchor(el)) else {
            return refused();
        };
        require_level(self.project, level_id)?;
        if anchor.shifted(delta).within(|p| self.in_area(level_id, p)) {
            Ok(())
        } else {
            refused()
        }
    }

    /// The element must be in reach. A door or window moved to another wall
    /// needs that wall in reach too, like a new one.
    fn update(&self, element: &Element) -> Result<(), CoreError> {
        let current = self.element(element.id())?;
        self.targets(std::slice::from_ref(element.id()))?;
        match (element, current) {
            (Element::Opening(new), Element::Opening(old)) if new.wall_id != old.wall_id => {
                self.new_opening(&new.wall_id, new.offset_mm, &label(current))
            }
            _ => Ok(()),
        }
    }

    /// Copies land `delta` away from their originals, and must land in the
    /// area like new elements. A door or window copied without its wall
    /// stays on that wall.
    fn copies(&self, ids: &[Id], delta: Point) -> Result<(), CoreError> {
        for el in self
            .project
            .elements
            .iter()
            .filter(|e| ids.contains(e.id()))
        {
            let what = format!("The copy of {}", label(el));
            match el {
                el if el.is_view() => {}
                Element::Opening(o) => {
                    if ids.contains(&o.wall_id) {
                        continue;
                    }
                    let along = host(self.project, o)
                        .and_then(|w| unit(sub(w.end, w.start)))
                        .map(|d| dot(delta, d))
                        .unwrap_or(0.0);
                    self.new_opening(&o.wall_id, o.offset_mm + along, &what)?;
                }
                other => self.lands(other, delta, &what, vec![other.id().clone()])?,
            }
        }
        Ok(())
    }

    /// A mark on one element or one finding changes nothing outside what it
    /// names. A mark on a whole check covers the whole project.
    fn review_mark(&self, target: &ReviewTarget, set_aside: bool) -> Result<(), CoreError> {
        match target {
            ReviewTarget::Check { .. } => Err(whole(if set_aside {
                "Setting a whole review check aside"
            } else {
                "Reopening a whole review check"
            })),
            ReviewTarget::Element { element_id, .. } => {
                self.targets(&[element_id.trim().to_string()])
            }
            ReviewTarget::Issue { id } => {
                let id = id.trim();
                let ids: Vec<Id> = match self.derived.issues.iter().find(|i| i.id == id) {
                    Some(issue) => issue.element_ids.clone(),
                    // A finding the checks no longer make: its id is
                    // "code:element ids".
                    None => id
                        .split_once(':')
                        .map(|(_, rest)| {
                            rest.split(',')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                if ids.is_empty() {
                    return Err(out(
                        format!("That review item names no element, so it is {OUTSIDE}"),
                        vec![],
                    ));
                }
                for element_id in &ids {
                    match find(self.project, element_id) {
                        Some(_) if self.reach.contains(element_id) => {}
                        Some(el) => {
                            return Err(out(
                                format!("That review item involves {}, which is {OUTSIDE}", label(el)),
                                vec![element_id.clone()],
                            ))
                        }
                        None => {
                            return Err(out(
                                format!(
                                    "That review item involves an element that is no longer in the plan, so it is {OUTSIDE}"
                                ),
                                vec![element_id.clone()],
                            ))
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

fn out(message: String, element_ids: Vec<Id>) -> CoreError {
    CoreError::invalid_for("out_of_scope", message, element_ids)
}

/// The roof over the top floor and the lot belong to the whole project:
/// what changing one is called. None for any other element.
fn settings_like(el: &Element) -> Option<&'static str> {
    match el {
        Element::Roof(r) if r.outline == RoofOutline::TopFootprint => Some("Changing the roof over the top floor"),
        Element::Lot(_) => Some("Changing the lot"),
        _ => None,
    }
}

fn whole(what: &str) -> CoreError {
    CoreError::invalid(
        "out_of_scope",
        format!("{what} affects the whole project, which is {OUTSIDE}"),
    )
}

// ---------------------------------------------------------- agents (D47)

/// The end of a refusal by an agent's level or kind limit.
const AGENT_OUTSIDE: &str = "outside what this agent is limited to.";

/// D30's ending, as a refusal by an agent's element limit words it.
const AGENT_IDS_OUTSIDE: &str = "outside the elements this agent is limited to.";

/// Ok when `command` stays inside an agent's limit (DECISIONS D47): only the
/// levels, the elements (with the reach of D30) and the kinds `limit` names.
/// An empty list limits nothing. Under a level or kind limit, levels,
/// layers, settings, materials, the lot, sheets, the title block and
/// revisions are refused, as D30 refuses them; cameras and tours are views
/// and never refused. Grid lines are on no level: they pass level limits
/// and kind limits decide (P1-3). The roof over the top floor is a roof on
/// the topmost level with a building (`top_floor`), so a level limit lets
/// an agent edit it only when it includes that level, and a kinds-only
/// limit with roofs always does. A door or window type change touches each
/// opening it rewrites, so an openings-only agent can define one.
///
/// `base` is the project the command runs on, `derived` its derived data,
/// and `staged` the project after the command (`base` again for a check
/// before running). What an earlier step of a batch made, elements and
/// levels, is looked up in `staged`.
///
/// Errors: `out_of_scope` naming the element and its level or kind.
pub fn check_limit(
    base: &Project,
    derived: &Derived,
    staged: &Project,
    limit: &AgentLimit,
    command: &Command,
) -> Result<(), CoreError> {
    if !limit.ids.is_empty() {
        check(base, derived, &limit.ids, command).map_err(|e| reword(e, OUTSIDE, AGENT_IDS_OUTSIDE))?;
    }
    if limit.level_ids.is_empty() && limit.kinds.is_empty() {
        return Ok(());
    }
    let mut touches = vec![];
    touches_of(base, staged, command, &mut touches);
    let levels = |level: &Id| limit.level_ids.is_empty() || limit.level_ids.contains(level);
    let kinds = |kind: ElementKind| limit.kinds.is_empty() || limit.kinds.contains(&kind);
    // The roof over the top floor, and posts under it, reach this level.
    let top = top_floor(base, derived);
    for touch in &touches {
        match touch {
            Touch::Target { el, kind, level } => {
                let ids = vec![el.id().clone()];
                let level = level.or(if is_top_roof(el) { top } else { None });
                if let Some(level) = level.filter(|l| !levels(l)) {
                    let message = format!("{} is on {}, {AGENT_OUTSIDE}", label(el), level_name(staged, level));
                    return Err(out(message, ids));
                }
                if !kinds(*kind) {
                    let message = format!("{} is {}, {AGENT_OUTSIDE}", label(el), a_kind(*kind, el));
                    return Err(out(message, ids));
                }
            }
            Touch::New { level, kind, what, top_floor } => {
                let level = level.as_ref().or(if *top_floor { top } else { None });
                if let Some(level) = level.filter(|l| !levels(l)) {
                    let message = format!("{what} would be on {}, {AGENT_OUTSIDE}", level_name(staged, level));
                    return Err(out(message, vec![]));
                }
                if !kinds(*kind) {
                    return Err(out(format!("{what} is {}, {AGENT_OUTSIDE}", a_kind_noun(*kind)), vec![]));
                }
            }
            Touch::Whole(what) | Touch::Level { what, .. } => {
                return Err(out(format!("{what} affects the whole project, which is {AGENT_OUTSIDE}"), vec![]));
            }
        }
    }
    Ok(())
}

/// What of a claim `command` reaches, for a `claimed` refusal (DECISIONS
/// D47): a claimed level's name ("Second Floor") or an element's label
/// ("Wall 3000 mm"), with the element ids. None when it stays clear.
///
/// A claim reaches its levels and everything on them, and its elements with
/// the reach of D30 on `base`: a room its bounding walls, their doors and
/// windows on its side and what stands in it; a wall its doors and windows.
/// A new element reaches a claim when it lands on a claimed level. Changing
/// or deleting a claimed level reaches it too; roof, settings and the other
/// whole project changes never do. `base`, `derived` and `staged` are as in
/// `check_limit`.
pub fn claim_reach(
    base: &Project,
    derived: &Derived,
    staged: &Project,
    level_ids: &[Id],
    ids: &[Id],
    command: &Command,
) -> Option<(String, Vec<Id>)> {
    let mut reach = BTreeSet::new();
    for el in ids.iter().filter_map(|id| find(base, id)) {
        match el {
            Element::Room(room) => room_reach(base, derived, room, &mut reach),
            Element::Wall(w) => wall_reach(base, &w.id, &mut reach),
            other => {
                reach.insert(other.id().clone());
            }
        }
    }
    let mut touches = vec![];
    touches_of(base, staged, command, &mut touches);
    for touch in &touches {
        match touch {
            Touch::Target { el, level, .. } => {
                if let Some(level) = level.filter(|l| level_ids.contains(l)) {
                    return Some((level_name(staged, level), vec![el.id().clone()]));
                }
                if reach.contains(el.id()) {
                    return Some((label(el), vec![el.id().clone()]));
                }
            }
            Touch::New { level: Some(level), .. } | Touch::Level { level_id: Some(level), .. }
                if level_ids.contains(level) =>
            {
                return Some((level_name(staged, level), vec![]));
            }
            _ => {}
        }
    }
    None
}

/// Where two claims meet (DECISIONS D47), so one agent cannot claim part of
/// what another holds: a level both claim, an element of one (with D30's
/// reach on `project`) standing on a level the other claims, or an element
/// both reach. The level's name or the element's label; None when they are
/// apart. Each claim is its level ids and element ids.
pub fn claims_overlap(
    project: &Project,
    derived: &Derived,
    a: (&[Id], &[Id]),
    b: (&[Id], &[Id]),
) -> Option<String> {
    if let Some(level) = a.0.iter().find(|l| b.0.contains(l)) {
        return Some(level_name(project, level));
    }
    let reach_of = |ids: &[Id]| {
        let mut reach = BTreeSet::new();
        for el in ids.iter().filter_map(|id| find(project, id)) {
            match el {
                Element::Room(room) => room_reach(project, derived, room, &mut reach),
                Element::Wall(w) => wall_reach(project, &w.id, &mut reach),
                other => {
                    reach.insert(other.id().clone());
                }
            }
        }
        reach
    };
    let (reach_a, reach_b) = (reach_of(a.1), reach_of(b.1));
    // The claimed elements themselves are named before what they reach.
    let on = |ids: &[Id], reach: &BTreeSet<Id>, levels: &[Id]| {
        let mut els = ids.iter().chain(reach.iter()).filter_map(|id| find(project, id));
        els.find(|el| level_of(project, el).is_some_and(|l| levels.contains(l))).map(label)
    };
    let both = |ids: &[Id], other: &BTreeSet<Id>, mine: &BTreeSet<Id>| {
        ids.iter()
            .chain(mine.iter())
            .find(|id| other.contains(*id))
            .and_then(|id| find(project, id))
            .map(label)
    };
    on(a.1, &reach_a, b.0)
        .or_else(|| on(b.1, &reach_b, a.0))
        .or_else(|| both(b.1, &reach_a, &reach_b))
}

/// Where an agent works (DECISIONS D47): the box around what it touched on
/// one level, for its pointer, its 3D pointer and its work area.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkArea {
    pub level_id: Id,
    pub center: Point,
    pub min: Point,
    pub max: Point,
}

/// The box around the elements `ids` on the level most of them stand on
/// (the first such level on a tie). None when none of them is in `project`
/// with a level and a place on the plan. Cameras are views and count for
/// nothing.
pub fn work_area(project: &Project, ids: &[Id]) -> Option<WorkArea> {
    let mut levels: Vec<(&Id, usize, Vec<Point>)> = vec![];
    let mut seen: BTreeSet<&Id> = BTreeSet::new();
    for el in ids.iter().filter(|id| seen.insert(*id)).filter_map(|id| find(project, id)) {
        if el.is_view() {
            continue;
        }
        let Some(level) = level_of(project, el) else { continue };
        let points: Vec<Point> = extent(project, el).into_iter().filter(|p| is_finite(*p)).collect();
        if points.is_empty() {
            continue;
        }
        match levels.iter_mut().find(|(l, _, _)| *l == level) {
            Some((_, n, all)) => {
                *n += 1;
                all.extend(points);
            }
            None => levels.push((level, 1, points)),
        }
    }
    let most = levels.iter().map(|(_, n, _)| *n).max()?;
    let (level, _, points) = levels.into_iter().find(|(_, n, _)| *n == most)?;
    let (min, max) = bbox(&points);
    Some(WorkArea {
        level_id: level.clone(),
        center: pt((min.x + max.x) / 2.0, (min.y + max.y) / 2.0),
        min,
        max,
    })
}

/// What one command touches, for agents' limits and claims.
enum Touch<'a> {
    /// An element it changes, moves or removes, its kind for a kind limit,
    /// and its level (None for a grid line and for the roof over the top
    /// floor, which a level limit places with `top_floor`).
    Target {
        el: &'a Element,
        kind: ElementKind,
        level: Option<&'a Id>,
    },
    /// An element it makes: its level when known, kind and words for it.
    /// `top_floor`: on no level, but a level limit places it on the topmost
    /// level with a building, as the roof over the top floor and its posts.
    New {
        level: Option<Id>,
        kind: ElementKind,
        what: String,
        top_floor: bool,
    },
    /// A change to the whole project.
    Whole(&'static str),
    /// A change to the levels: adding one (no id), changing or deleting one.
    Level {
        level_id: Option<Id>,
        what: &'static str,
    },
}

/// Every touch of `command`, steps of batches included, in order.
fn touches_of<'a>(base: &'a Project, staged: &'a Project, command: &Command, out: &mut Vec<Touch<'a>>) {
    // Elements an earlier step made are only in `staged`.
    let get = |id: &str| find(base, id).map(|el| (el, base)).or_else(|| find(staged, id).map(|el| (el, staged)));
    let target = |id: &str, out: &mut Vec<Touch<'a>>| {
        if let Some((el, project)) = get(id) {
            if matches!(el, Element::Lot(_)) {
                out.push(Touch::Whole("Changing the lot"));
            } else if !el.is_view() {
                out.push(Touch::Target { el, kind: el.kind(), level: level_of(project, el) });
            }
        }
    };
    // Every opening of these types, each on its wall's level.
    let typed = |type_ids: &[&Id], out: &mut Vec<Touch<'a>>| {
        for el in &base.elements {
            if let Element::Opening(o) = el {
                if o.type_id.as_ref().is_some_and(|t| type_ids.contains(&t)) {
                    out.push(Touch::Target { el, kind: ElementKind::Opening, level: level_of(base, el) });
                }
            }
        }
    };
    let wall_level = |wall_id: &str| get(wall_id).and_then(|(el, project)| level_of(project, el)).cloned();
    let new_on = |level_id: &Option<Id>, kind: ElementKind, what: &str| Touch::New {
        level: resolve_level(staged, level_id).ok(),
        kind,
        what: what.to_string(),
        top_floor: false,
    };
    match command {
        Command::Batch { commands, .. } => {
            for c in commands {
                touches_of(base, staged, c, out);
            }
        }
        Command::AddWall { level_id, .. } => out.push(new_on(level_id, ElementKind::Wall, "The new wall")),
        Command::AddWallChain { level_id, .. } => out.push(new_on(level_id, ElementKind::Wall, "The new walls")),
        Command::AddRectRoom { level_id, .. } => {
            out.push(new_on(level_id, ElementKind::Wall, "The new room's walls"));
            out.push(new_on(level_id, ElementKind::Room, "The new room"));
        }
        Command::SetWallEndpoints { wall_id, .. }
        | Command::SetWallLength { wall_id, .. }
        | Command::SplitWall { wall_id, .. } => target(wall_id, out),
        Command::AddOpening { wall_id, opening_type, .. } => out.push(Touch::New {
            level: wall_level(wall_id),
            kind: ElementKind::Opening,
            what: format!("The new {}", opening_noun(*opening_type)),
            top_floor: false,
        }),
        // A resize moves the room's walls: a kind limit needs walls.
        Command::ResizeRoom { room_id, .. } => {
            if let Some((el, project)) = get(room_id) {
                out.push(Touch::Target { el, kind: ElementKind::Wall, level: level_of(project, el) });
            }
        }
        Command::AddElement { element } => match element {
            el if el.is_view() => {}
            Element::Lot(_) => out.push(Touch::Whole("Setting the lot")),
            Element::Opening(o) => out.push(Touch::New {
                level: wall_level(&o.wall_id),
                kind: ElementKind::Opening,
                what: format!("The new {}", opening_noun(o.opening_type)),
                top_floor: false,
            }),
            other => out.push(Touch::New {
                level: level_of(staged, other).cloned(),
                kind: other.kind(),
                what: format!("The new {}", noun(other)),
                top_floor: is_top_roof(other),
            }),
        },
        Command::UpdateElement { element } => {
            target(element.id(), out);
            // Moved to another level or wall: where it goes counts too.
            let level = match element {
                Element::Opening(o) => wall_level(&o.wall_id),
                el if el.is_view() => None,
                other => level_of(staged, other).cloned(),
            };
            if let Some((current, _)) = get(element.id()) {
                if level.is_some() {
                    out.push(Touch::New { level, kind: element.kind(), what: label(current), top_floor: false });
                }
            }
        }
        Command::DeleteElements { ids }
        | Command::MoveElements { ids, .. }
        | Command::RotateElements { ids, .. }
        | Command::SetMaterial { ids, .. }
        | Command::DuplicateElements { ids, .. } => {
            for id in ids {
                target(id, out);
            }
        }
        Command::SetReviewMark { target: mark, note } => match mark {
            ReviewTarget::Check { .. } => out.push(Touch::Whole(if note.is_some() {
                "Setting a whole review check aside"
            } else {
                "Reopening a whole review check"
            })),
            ReviewTarget::Element { element_id, .. } => target(element_id.trim(), out),
            // "code:element ids", as the checks name their findings.
            ReviewTarget::Issue { id } => {
                if let Some((_, rest)) = id.trim().split_once(':') {
                    for element_id in rest.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        target(element_id, out);
                    }
                }
            }
        },
        Command::MovePipePoints { targets, .. } => {
            for t in targets {
                target(&t.pipe_id, out);
            }
        }
        Command::SetPipeSegmentLength { pipe_id, .. }
        | Command::SetPipeSlope { pipe_id, .. }
        | Command::InsertPipePoint { pipe_id, .. }
        | Command::RemovePipePoint { pipe_id, .. } => target(pipe_id, out),
        // The roof over the top floor is a roof on no level that reaches the
        // topmost level with a building (P1-3, `top_floor`).
        Command::SetRoof { .. } => match base.elements.iter().find(|e| is_top_roof(e)) {
            Some(el) => out.push(Touch::Target { el, kind: ElementKind::Roof, level: None }),
            None => out.push(Touch::New {
                level: None,
                kind: ElementKind::Roof,
                what: "The new roof over the top floor".into(),
                top_floor: true,
            }),
        },
        Command::SetProjectSettings { .. } => out.push(Touch::Whole("Changing the project settings")),
        Command::SetLayer { .. } => out.push(Touch::Whole("Changing a layer")),
        Command::UpsertMaterial { .. } => out.push(Touch::Whole("Adding or changing a material")),
        Command::UpdateLevel { level } => {
            out.push(Touch::Level { level_id: Some(level.id.clone()), what: "Changing a level" })
        }
        Command::AddLevel { .. } => out.push(Touch::Level { level_id: None, what: "Adding a level" }),
        Command::DeleteLevel { level_id } => {
            out.push(Touch::Level { level_id: Some(level_id.clone()), what: "Deleting a level" })
        }
        // Wave 2 (D54, P1-3).
        Command::UpsertOpeningSpec { spec } => typed(&[&spec.id], out),
        Command::DeleteOpeningSpecs { ids } => typed(&ids.iter().collect::<Vec<_>>(), out),
        Command::AssignOpeningTypes { level_id } => {
            for el in &base.elements {
                if let Element::Opening(o) = el {
                    let level = level_of(base, el);
                    if o.type_id.is_none() && (level_id.is_none() || level == level_id.as_ref()) {
                        out.push(Touch::Target { el, kind: ElementKind::Opening, level });
                    }
                }
            }
        }
        Command::AddGridSystem { .. } => out.push(Touch::New {
            level: None,
            kind: ElementKind::Grid,
            what: "The new grid lines".into(),
            top_floor: false,
        }),
        Command::AutoDimension { level_id, .. } => out.push(Touch::New {
            level: Some(level_id.clone()),
            kind: ElementKind::Dimension,
            what: "The new dimensions".into(),
            top_floor: false,
        }),
        Command::AddRoofPosts { roof_id, .. } => {
            target(roof_id, out);
            let roof = get(roof_id);
            out.push(Touch::New {
                level: roof.and_then(|(el, project)| level_of(project, el)).cloned(),
                kind: ElementKind::Column,
                what: "The new roof posts".into(),
                top_floor: roof.is_some_and(|(el, _)| is_top_roof(el)),
            });
        }
        Command::MakeTour { .. } => {}
        Command::SetLotFromSurvey { .. } => out.push(Touch::Whole("Setting the lot")),
        Command::SetTitleBlock { .. } | Command::SetSignatory { .. } => out.push(Touch::Whole("Changing the title block")),
        Command::UpsertSheet { .. }
        | Command::DeleteSheets { .. }
        | Command::MakeSheetSet { .. }
        | Command::UpsertRevision { .. }
        | Command::DeleteRevision { .. } => out.push(Touch::Whole("Changing the sheets")),
    }
}

/// A level's name for a sentence, or "another level" when it has none.
fn level_name(project: &Project, level_id: &str) -> String {
    project
        .levels
        .iter()
        .find(|l| l.id == level_id)
        .map(|l| l.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("another level")
        .to_string()
}

/// The roof over the top floor (`RoofOutline::TopFootprint`).
fn is_top_roof(el: &Element) -> bool {
    matches!(el, Element::Roof(r) if r.outline == RoofOutline::TopFootprint)
}

/// The level the roof over the top floor reaches under a level limit: the
/// topmost level with a building, where `roofs::derived_outlines` puts it,
/// or the highest level while no level has a building yet. `derived` is
/// `compute_derived(project)`.
fn top_floor<'a>(project: &'a Project, derived: &Derived) -> Option<&'a Id> {
    let built = |l: &&Level| derived.footprints.iter().any(|f| f.level_id == l.id && !f.polygon.is_empty());
    let highest = |a: &&Level, b: &&Level| a.elevation_mm.total_cmp(&b.elevation_mm);
    project
        .levels
        .iter()
        .filter(built)
        .max_by(highest)
        .or_else(|| project.levels.iter().max_by(highest))
        .map(|l| &l.id)
}

/// "a door", "an object": what `el` is, for a kind refusal.
fn a_kind(kind: ElementKind, el: &Element) -> String {
    match el {
        Element::Opening(o) => with_article(opening_noun(o.opening_type)),
        _ => a_kind_noun(kind),
    }
}

fn a_kind_noun(kind: ElementKind) -> String {
    match kind {
        ElementKind::Opening => "a door or window".to_string(),
        ElementKind::Linework => "linework".to_string(),
        other => with_article(exec::kind_noun(other)),
    }
}

fn with_article(noun: &str) -> String {
    let vowel = noun.chars().next().is_some_and(|c| "aeiou".contains(c));
    format!("{} {noun}", if vowel { "an" } else { "a" })
}

/// A refusal with one phrase of its message replaced.
fn reword(e: CoreError, from: &str, to: &str) -> CoreError {
    match e {
        CoreError::Invalid { code, message, element_ids } => CoreError::Invalid {
            code,
            message: message.replace(from, to),
            element_ids,
        },
        other => other,
    }
}

// ------------------------------------------------------------------ reach

/// Everything a selected room reaches, on the project as it is now.
fn room_reach(project: &Project, derived: &Derived, room: &Room, reach: &mut BTreeSet<Id>) {
    reach.insert(room.id.clone());
    let Some(geo) = derived.rooms.iter().find(|g| g.room_id == room.id) else {
        return;
    };
    let level = &room.level_id;
    let inside = |p: Point| near_polygon(p, &geo.centerline_polygon);
    // Bounding walls, and walls standing inside the room.
    let walls: BTreeSet<&Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w)
                if geo.wall_ids.contains(&w.id)
                    || (&w.level_id == level && along(&[w.start, w.end], inside)) =>
            {
                Some(&w.id)
            }
            _ => None,
        })
        .collect();
    for el in &project.elements {
        let hit = match el {
            Element::Wall(w) => walls.contains(&w.id),
            // Only the doors and windows on this room's part of a wall: a
            // long wall can bound the next room too.
            Element::Opening(o) => {
                walls.contains(&o.wall_id)
                    && host(project, o)
                        .and_then(|w| on_wall(w, o.offset_mm))
                        .is_some_and(inside)
            }
            Element::Camera(c) => {
                camera_level(project, c) == Some(level) && inside(pt(c.position.x, c.position.y))
            }
            Element::Room(_)
            | Element::Underlay(_)
            | Element::Linework(_)
            | Element::ReferenceModel(_)
            | Element::Tour(_)
            | Element::Grid(_)
            | Element::Lot(_) => false,
            other => {
                level_of(project, other) == Some(level)
                    && anchor(other).is_some_and(|a| a.within(inside))
            }
        };
        if hit {
            reach.insert(el.id().clone());
        }
    }
}

/// A wall and its doors and windows.
fn wall_reach(project: &Project, wall_id: &Id, reach: &mut BTreeSet<Id>) {
    reach.insert(wall_id.clone());
    for el in &project.elements {
        if let Element::Opening(o) = el {
            if &o.wall_id == wall_id {
                reach.insert(o.id.clone());
            }
        }
    }
}

// ------------------------------------------------------------------- area

/// Where a selected element lets new elements go: a room's centerline
/// polygon, or the bounding box of anything else grown by `AREA_MARGIN_MM`.
/// Cameras have no level, so they give no area.
fn area_of(project: &Project, derived: &Derived, el: &Element) -> Option<Area> {
    if let Element::Room(r) = el {
        if let Some(geo) = derived.rooms.iter().find(|g| g.room_id == r.id) {
            return Some(Area {
                level_id: r.level_id.clone(),
                shape: Shape::Room(geo.centerline_polygon.clone()),
            });
        }
    }
    let level_id = level_of(project, el)?;
    let points: Vec<Point> = extent(project, el)
        .into_iter()
        .filter(|p| is_finite(*p))
        .collect();
    if points.is_empty() {
        return None;
    }
    let (lo, hi) = bbox(&points);
    let margin = pt(AREA_MARGIN_MM, AREA_MARGIN_MM);
    Some(Area {
        level_id: level_id.clone(),
        shape: Shape::Box(sub(lo, margin), add(hi, margin)),
    })
}

/// The plan points an element covers, for its bounding box.
fn extent(project: &Project, el: &Element) -> Vec<Point> {
    match el {
        Element::Wall(w) => vec![w.start, w.end],
        Element::Opening(o) => host(project, o)
            .and_then(|w| {
                let d = unit(sub(w.end, w.start))?;
                let center = add(w.start, scale(d, o.offset_mm));
                let half = scale(d, o.width_mm / 2.0);
                Some(vec![sub(center, half), add(center, half)])
            })
            .unwrap_or_default(),
        Element::Room(r) => vec![r.seed],
        Element::Column(c) => match c.shape {
            ColumnShape::Round => footprint(c.center, c.width_mm, c.width_mm, 0.0),
            ColumnShape::Rect => footprint(c.center, c.width_mm, c.depth_mm, c.rotation_deg),
        },
        Element::Stair(s) => {
            let w = s.width_mm / 2.0;
            [pt(-w, 0.0), pt(w, 0.0), pt(w, s.run_mm), pt(-w, s.run_mm)]
                .iter()
                .map(|q| rotate_about(add(s.origin, *q), s.origin, s.rotation_deg))
                .collect()
        }
        Element::Asset(a) => footprint(a.position, a.width_mm, a.depth_mm, a.rotation_deg),
        Element::Slab(s) => s.polygon.clone(),
        Element::Annotation(a) => vec![a.position],
        Element::Dimension(d) => vec![d.a, d.b],
        Element::Camera(_) => vec![],
        Element::Underlay(u) => {
            let (w, h) = (
                u.width_px as f64 * u.mm_per_px,
                u.height_px as f64 * u.mm_per_px,
            );
            [pt(0.0, 0.0), pt(w, 0.0), pt(w, h), pt(0.0, h)]
                .iter()
                .map(|q| rotate_about(add(u.position, *q), u.position, u.rotation_deg))
                .collect()
        }
        Element::Linework(l) => l.polylines.iter().flatten().copied().collect(),
        Element::ReferenceModel(m) => vec![m.position],
        Element::Pipe(p) => p.points.iter().map(|v| pt(v.x, v.y)).collect(),
        Element::Roof(r) => match &r.outline {
            RoofOutline::Polygon { points } => points.clone(),
            _ => vec![],
        },
        Element::Grid(g) => vec![g.start, g.end],
        Element::ViewMark(m) => vec![m.start, m.end],
        Element::Lot(l) => l.corners.clone(),
        Element::Tour(_) => vec![],
    }
}

/// Corners of a `w` x `d` rectangle centered on `center`, turned by
/// `rotation_deg`.
fn footprint(center: Point, w: f64, d: f64, rotation_deg: f64) -> Vec<Point> {
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(sx, sy)| {
            rotate_about(
                add(center, pt(sx * w / 2.0, sy * d / 2.0)),
                center,
                rotation_deg,
            )
        })
        .collect()
}

/// Where an element stands, for the area of new and copied elements and for
/// what a room reaches.
enum Anchor {
    /// Every point must be inside.
    Points(Vec<Point>),
    /// Every point, and the segments between them, must be inside.
    Path(Vec<Point>),
}

impl Anchor {
    fn shifted(self, d: Point) -> Self {
        let shift = |ps: Vec<Point>| ps.into_iter().map(|p| add(p, d)).collect();
        match self {
            Anchor::Points(ps) => Anchor::Points(shift(ps)),
            Anchor::Path(ps) => Anchor::Path(shift(ps)),
        }
    }

    fn within(&self, inside: impl Fn(Point) -> bool) -> bool {
        match self {
            Anchor::Points(ps) => !ps.is_empty() && ps.iter().all(|p| inside(*p)),
            Anchor::Path(ps) => along(ps, inside),
        }
    }
}

/// Openings stand on their wall, cameras and tours are views, underlays,
/// linework and reference models are tracing references, and grids, the
/// lot and roofs over a derived outline belong to no area: none has an
/// anchor.
fn anchor(el: &Element) -> Option<Anchor> {
    Some(match el {
        Element::Wall(w) => Anchor::Path(vec![w.start, w.end]),
        Element::Room(r) => Anchor::Points(vec![r.seed]),
        Element::Column(c) => Anchor::Points(vec![c.center]),
        Element::Stair(s) => Anchor::Points(vec![s.origin]),
        Element::Asset(a) => Anchor::Points(vec![a.position]),
        Element::Annotation(a) => Anchor::Points(vec![a.position]),
        Element::Dimension(d) => Anchor::Points(vec![d.a, d.b]),
        Element::Pipe(p) => Anchor::Path(p.points.iter().map(|v| pt(v.x, v.y)).collect()),
        Element::Slab(s) => Anchor::Points(s.polygon.clone()),
        Element::ViewMark(m) => Anchor::Path(vec![m.start, m.end]),
        Element::Roof(r) => match &r.outline {
            RoofOutline::Polygon { points } => Anchor::Points(points.clone()),
            _ => return None,
        },
        Element::Opening(_)
        | Element::Camera(_)
        | Element::Tour(_)
        | Element::Grid(_)
        | Element::Lot(_)
        | Element::Underlay(_)
        | Element::Linework(_)
        | Element::ReferenceModel(_) => return None,
    })
}

/// Every point of a path, and every segment between two of them, is inside.
/// Segments are checked at points `SAMPLE_STEP_MM` apart, so a wall between
/// two points of an L-shaped room cannot cut across the room next door.
fn along(points: &[Point], inside: impl Fn(Point) -> bool) -> bool {
    let Some(first) = points.first() else {
        return false;
    };
    if !inside(*first) {
        return false;
    }
    points.windows(2).all(|s| {
        let n = ((dist(s[0], s[1]) / SAMPLE_STEP_MM).ceil() as usize).clamp(1, MAX_SAMPLES);
        (1..=n).all(|k| inside(lerp(s[0], s[1], k as f64 / n as f64)))
    })
}

/// Inside a room's centerline polygon, or within `ROOM_TOLERANCE_MM` of it.
fn near_polygon(p: Point, poly: &[Point]) -> bool {
    is_finite(p)
        && poly.len() >= 3
        && (point_in_polygon(p, poly) || dist_to_boundary(p, poly) <= ROOM_TOLERANCE_MM)
}

// --------------------------------------------------------------- lookups

fn find<'p>(project: &'p Project, id: &str) -> Option<&'p Element> {
    project.elements.iter().find(|e| e.id() == id)
}

fn host<'p>(project: &'p Project, o: &Opening) -> Option<&'p Wall> {
    project.elements.iter().find_map(|e| match e {
        Element::Wall(w) if w.id == o.wall_id => Some(w),
        _ => None,
    })
}

/// The point `offset_mm` along a wall's centerline from its start.
fn on_wall(w: &Wall, offset_mm: f64) -> Option<Point> {
    unit(sub(w.end, w.start)).map(|d| add(w.start, scale(d, offset_mm)))
}

/// The level an element stands on (`Element::level_id`), and an opening's
/// wall's level. Cameras, tours, grids, the lot and the roof over the top
/// floor have none.
fn level_of<'a>(project: &'a Project, el: &'a Element) -> Option<&'a Id> {
    match el {
        Element::Opening(o) => host(project, o).map(|w| &w.level_id),
        other => other.level_id(),
    }
}

/// A camera stands in the storey that holds its eye height.
fn camera_level<'p>(project: &'p Project, c: &Camera) -> Option<&'p Id> {
    let z = c.position.z;
    project
        .levels
        .iter()
        .find(|l| z >= l.elevation_mm && z < l.elevation_mm + l.height_mm)
        .map(|l| &l.id)
}

// ----------------------------------------------------------------- words

/// A short name for an element, as the copilot names it: "Wall 3000 mm",
/// "Door 900 x 2100", "Room Bedroom", "Double bed". No ids: those go in the
/// error's element ids.
fn label(el: &Element) -> String {
    match el {
        Element::Wall(w) => format!("Wall {:.0} mm", dist(w.start, w.end)),
        Element::Opening(o) => format!(
            "{} {:.0} x {:.0}",
            match o.opening_type {
                OpeningType::Door => "Door",
                OpeningType::Window => "Window",
            },
            o.width_mm,
            o.height_mm
        ),
        Element::Room(r) => format!("Room {}", r.name.trim()),
        Element::Column(c) => match c.shape {
            ColumnShape::Round => format!("Round column {:.0}", c.width_mm),
            ColumnShape::Rect => format!("Column {:.0} x {:.0}", c.width_mm, c.depth_mm),
        },
        Element::Stair(_) => "Stair".to_string(),
        Element::Asset(a) if !a.name.trim().is_empty() => a.name.trim().to_string(),
        Element::Asset(_) => "Object".to_string(),
        Element::Annotation(a) => {
            let line = a.text.lines().next().unwrap_or_default().trim();
            let short: String = line.chars().take(30).collect();
            if line.chars().count() > 30 {
                format!("Note \"{}...\"", short.trim_end())
            } else {
                format!("Note \"{short}\"")
            }
        }
        Element::Dimension(d) => format!("Dimension {:.0} mm", dist(d.a, d.b)),
        Element::Camera(c) => format!("Camera {}", c.name.trim()),
        Element::Underlay(u) => format!("Underlay {}", u.file_name),
        Element::Linework(l) => format!("Linework {}", l.name),
        Element::ReferenceModel(m) => format!("Reference model {}", m.name),
        Element::Pipe(p) => name_start(p),
        Element::Slab(s) => match s.kind {
            SlabKind::Floor => "Floor slab",
            SlabKind::Balcony => "Balcony",
            SlabKind::Deck => "Deck",
            SlabKind::Void => "Floor void",
        }
        .to_string(),
        Element::Roof(r) if r.outline == RoofOutline::TopFootprint => "Roof over the top floor".to_string(),
        Element::Roof(_) => "Roof".to_string(),
        Element::Grid(g) => format!("Grid line {}", g.label.trim()),
        Element::ViewMark(m) => format!(
            "{} mark {}",
            match m.mark_kind {
                MarkKind::Section => "Section",
                MarkKind::Elevation => "Elevation",
            },
            m.label.trim()
        ),
        Element::Lot(_) => "Lot".to_string(),
        Element::Tour(t) => format!("Tour {}", t.name.trim()),
    }
}

fn opening_noun(t: OpeningType) -> &'static str {
    match t {
        OpeningType::Door => "door",
        OpeningType::Window => "window",
    }
}

/// "wall", "switch", "object": a new element, inside a sentence.
fn noun(el: &Element) -> &'static str {
    match el {
        Element::Opening(o) => opening_noun(o.opening_type),
        other => diff_noun(other),
    }
}

/// One selected element for `describe`: a room or a wall with what it
/// holds, anything else by its name.
fn part(project: &Project, derived: &Derived, el: &Element) -> String {
    let mut reach = BTreeSet::new();
    match el {
        Element::Room(r) => room_reach(project, derived, r, &mut reach),
        Element::Wall(w) => wall_reach(project, &w.id, &mut reach),
        _ => return label(el),
    }
    reach.remove(el.id());
    let held = counts(project, &reach);
    if held.is_empty() {
        label(el)
    } else {
        format!("{} with its {}", label(el), join_and(&held))
    }
}

/// "4 walls", "1 door", "3 objects": a set of elements by kind.
fn counts(project: &Project, ids: &BTreeSet<Id>) -> Vec<String> {
    const NOUNS: [&str; 10] = [
        "wall",
        "door",
        "window",
        "object",
        "column",
        "stair",
        "pipe",
        "note",
        "dimension",
        "camera",
    ];
    let mut n = [0usize; 10];
    for el in project.elements.iter().filter(|e| ids.contains(e.id())) {
        let slot = match el {
            Element::Wall(_) => 0,
            Element::Opening(o) if o.opening_type == OpeningType::Door => 1,
            Element::Opening(_) => 2,
            Element::Asset(_) => 3,
            Element::Column(_) => 4,
            Element::Stair(_) => 5,
            Element::Pipe(_) => 6,
            Element::Annotation(_) => 7,
            Element::Dimension(_) => 8,
            Element::Camera(_) => 9,
            _ => continue,
        };
        n[slot] += 1;
    }
    NOUNS
        .iter()
        .zip(n)
        .filter(|(_, count)| *count > 0)
        .map(|(noun, count)| count_noun(count, noun))
        .collect()
}

/// "a, b and c".
fn join_and<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_string(),
        [head @ .., last] => {
            let head: Vec<&str> = head.iter().map(|s| s.as_ref()).collect();
            format!("{} and {}", head.join(", "), last.as_ref())
        }
    }
}
