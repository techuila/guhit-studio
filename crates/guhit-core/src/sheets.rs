//! The title block, sheets, revisions and view marks (DECISIONS D53, D54).
//! LANE L1 owns this module.
//!
//! Written in C0: `set_title_block` (never touches the signatory, P1-4),
//! `set_signatory`, `upsert_sheet` and `delete_sheets` (ids from `IdGen`,
//! P0-5), `upsert_revision`, `delete_revision`, `prune_viewports` (P1-8)
//! and `validate_mark`. Stub with its final signature: `make_sheet_set`
//! (`not_built`).

use std::collections::BTreeSet;

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::exec::{outcome, Outcome};
use crate::geom::*;
use crate::ids::IdGen;
use crate::validate::{finite_point, require_level};

/// `Command::SetTitleBlock`: everything but the signatory, which only
/// `SetSignatory` changes.
pub fn set_title_block(
    project: &mut Project,
    title_block: &TitleBlock,
) -> Result<Outcome, CoreError> {
    let signatory = project.title_block.signatory.clone();
    project.title_block = Box::new(TitleBlock {
        signatory,
        ..title_block.clone()
    });
    Ok(outcome("Edit title block"))
}

/// `Command::SetSignatory`. The app refuses it from agents and the copilot
/// before it gets here (DECISIONS D53).
pub fn set_signatory(project: &mut Project, signatory: &Signatory) -> Result<Outcome, CoreError> {
    let trim = |s: &String| s.trim().to_string();
    project.title_block.signatory = Signatory {
        name: trim(&signatory.name),
        prc_no: trim(&signatory.prc_no),
        prc_valid_until: trim(&signatory.prc_valid_until),
        ptr_no: trim(&signatory.ptr_no),
        ptr_date: trim(&signatory.ptr_date),
        ptr_place: trim(&signatory.ptr_place),
        iapoa_no: trim(&signatory.iapoa_no),
        iapoa_or_no: trim(&signatory.iapoa_or_no),
        iapoa_date: trim(&signatory.iapoa_date),
        tin: trim(&signatory.tin),
    };
    Ok(outcome("Edit signing architect"))
}

/// "A-3": the first "A-n" no sheet has.
fn next_sheet_number(project: &Project) -> String {
    (1..)
        .map(|n| format!("A-{n}"))
        .find(|n| {
            !project
                .sheet_set
                .sheets
                .iter()
                .any(|s| s.number.trim() == n)
        })
        .unwrap_or_default()
}

/// A viewport's view must name what is there: a level, a view mark of the
/// right kind.
fn check_view(project: &Project, view: &ViewRef) -> Result<(), CoreError> {
    let mark = |id: &Id, kind: MarkKind| match project.elements.iter().find(|e| e.id() == id) {
        Some(Element::ViewMark(m)) if m.mark_kind == kind => Ok(()),
        _ => Err(CoreError::invalid(
            "bad_viewport",
            format!(
                "There is no {} mark with id {id}.",
                if kind == MarkKind::Section {
                    "section"
                } else {
                    "elevation"
                }
            ),
        )),
    };
    match view {
        ViewRef::Plan { level_id, .. } => require_level(project, level_id),
        ViewRef::Elevation { mark_id } => mark(mark_id, MarkKind::Elevation),
        ViewRef::Section { mark_id } => mark(mark_id, MarkKind::Section),
        ViewRef::VicinityMap { radius_m, .. } if !radius_m.is_finite() || *radius_m <= 0.0 => {
            Err(CoreError::invalid(
                "bad_viewport",
                "A vicinity map's radius must be more than 0 m.",
            ))
        }
        _ => Ok(()),
    }
}

/// `Command::UpsertSheet`: add or replace a sheet by id. An empty id, empty
/// viewport ids and an empty number are assigned; numbers are unique.
pub fn upsert_sheet(
    project: &mut Project,
    ids: &mut IdGen,
    sheet: &Sheet,
    index: Option<u32>,
) -> Result<Outcome, CoreError> {
    let mut sheet = sheet.clone();
    sheet.number = sheet.number.trim().to_string();
    sheet.title = sheet.title.trim().to_string();
    if sheet.id.is_empty() {
        sheet.id = ids.next_id();
    } else {
        ids.reserve(&sheet.id);
    }
    if sheet.number.is_empty() {
        sheet.number = next_sheet_number(project);
    }
    if let Some(other) = project
        .sheet_set
        .sheets
        .iter()
        .find(|s| s.id != sheet.id && s.number.trim() == sheet.number)
    {
        return Err(CoreError::invalid(
            "sheet_number_taken",
            format!(
                "Sheet {} already exists ({}). Give this sheet another number.",
                other.number, other.title
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    for vp in sheet.viewports.iter_mut() {
        if vp.id.is_empty() {
            vp.id = ids.next_id();
        } else {
            ids.reserve(&vp.id);
        }
        if !seen.insert(vp.id.clone()) {
            return Err(CoreError::invalid(
                "duplicate_id",
                "Two viewports on the sheet share one id.",
            ));
        }
        if vp.scale_denominator == Some(0) {
            return Err(CoreError::invalid(
                "bad_viewport",
                "A viewport's scale must be 1:1 or smaller.",
            ));
        }
        if let Some(p) = vp.position {
            finite_point("Viewport position", p, &[])?;
        }
        check_view(project, &vp.view)?;
    }
    for r in &sheet.revision_ids {
        if !project.sheet_set.revisions.iter().any(|x| &x.id == r) {
            return Err(CoreError::invalid(
                "bad_revision",
                format!("There is no revision with id {r}."),
            ));
        }
    }
    let sheets = &mut project.sheet_set.sheets;
    let at = sheets.iter().position(|s| s.id == sheet.id);
    let label = if at.is_some() {
        "Edit sheet"
    } else {
        "Add sheet"
    };
    let summary = format!(
        "{} {} {}",
        if at.is_some() {
            "Changed sheet"
        } else {
            "Added sheet"
        },
        sheet.number,
        sheet.title
    );
    if let Some(i) = at {
        sheets.remove(i);
    }
    let to = index
        .map(|i| i as usize)
        .or(at)
        .unwrap_or(sheets.len())
        .min(sheets.len());
    sheets.insert(to, sheet);
    Ok(Outcome {
        summary: Some(summary.trim_end().to_string()),
        ..outcome(label)
    })
}

/// `Command::DeleteSheets`.
pub fn delete_sheets(project: &mut Project, ids: &[Id]) -> Result<Outcome, CoreError> {
    if ids.is_empty() {
        return Err(CoreError::invalid(
            "empty_selection",
            "No sheets were given.",
        ));
    }
    for id in ids {
        if !project.sheet_set.sheets.iter().any(|s| &s.id == id) {
            return Err(CoreError::invalid(
                "unknown_sheet",
                format!("There is no sheet with id {id}."),
            ));
        }
    }
    project.sheet_set.sheets.retain(|s| !ids.contains(&s.id));
    Ok(outcome(if ids.len() == 1 {
        "Delete sheet"
    } else {
        "Delete sheets"
    }))
}

/// How far marks stand off the buildings, mm.
const MARK_MARGIN_MM: f64 = 5000.0;
/// How far sections run past the buildings, mm: their heads stand outside
/// the automatic exterior dimensions (2400 mm) and short of the elevation
/// marks (5000 mm).
const SECTION_OVERRUN_MM: f64 = 3500.0;

/// The first free "<prefix>-n".
fn next_number(taken: &BTreeSet<String>, prefix: &str) -> String {
    (1..)
        .map(|n| format!("{prefix}-{n}"))
        .find(|n| !taken.contains(n))
        .unwrap_or_default()
}

/// The levels that have each service, for the E, P and M sheets (D19, GA
/// P1-1): a service sheet shows only the levels with something of its trade
/// on them, and there is none when no level has. Electrical and aircon:
/// runs of the trade, or objects the schedule counts under it. Plumbing
/// layouts: only once the model has plumbing runs, on the levels with runs
/// or fixtures; isometrics on the levels with water, drainage or vent runs
/// (fixtures alone draw nothing there).
struct Trades {
    electrical: BTreeSet<Id>,
    plumbing: BTreeSet<Id>,
    isometric: BTreeSet<Id>,
    aircon: BTreeSet<Id>,
}

fn trades(project: &Project, derived: &Derived) -> Trades {
    let piped = |systems: &[PipeSystem]| -> BTreeSet<Id> {
        project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Pipe(p) if systems.contains(&p.system) => Some(p.level_id.clone()),
                _ => None,
            })
            .collect()
    };
    let counted = |g: ScheduleGroup| -> BTreeSet<Id> {
        derived
            .schedule
            .iter()
            .filter(|r| r.group == g && r.count > 0)
            .map(|r| r.level_id.clone())
            .collect()
    };
    let sanitary = piped(&[PipeSystem::ColdWater, PipeSystem::HotWater, PipeSystem::Drainage, PipeSystem::Vent]);
    let runs: BTreeSet<Id> = sanitary.union(&piped(&[PipeSystem::Storm])).cloned().collect();
    let plumbing = if runs.is_empty() {
        BTreeSet::new()
    } else {
        runs.union(&counted(ScheduleGroup::Plumbing)).cloned().collect()
    };
    Trades {
        electrical: piped(&[PipeSystem::Conduit]).union(&counted(ScheduleGroup::Electrical)).cloned().collect(),
        plumbing,
        isometric: sanitary,
        aircon: piped(&[PipeSystem::Refrigerant, PipeSystem::Condensate]).union(&counted(ScheduleGroup::Aircon)).cloned().collect(),
    }
}

/// Levels in elevation order that have building walls (all levels when none
/// has).
fn building_levels(project: &Project) -> Vec<Level> {
    let mut levels: Vec<Level> = project.levels.clone();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    let built: Vec<Level> = levels
        .iter()
        .filter(|l| {
            project.elements.iter().any(
                |e| matches!(e, Element::Wall(w) if w.level_id == l.id && w.role.is_building()),
            )
        })
        .cloned()
        .collect();
    if built.is_empty() {
        levels
    } else {
        built
    }
}

/// "A", "B": the first free section labels, never a grid line's label, so
/// a section mark is never read as a grid (grids A to C give sections D, E).
fn next_section_label(project: &Project, made: &[ViewMark]) -> String {
    (0..)
        .map(crate::grids::grid_letter)
        .find(|l| {
            !project.elements.iter().any(|e| match e {
                Element::ViewMark(m) => m.mark_kind == MarkKind::Section && m.label.trim().eq_ignore_ascii_case(l),
                Element::Grid(g) => g.label.trim().eq_ignore_ascii_case(l),
                _ => false,
            }) && !made.iter().any(|m| m.mark_kind == MarkKind::Section && m.label.eq_ignore_ascii_case(l))
        })
        .unwrap_or_default()
}

/// The marks the set needs that the project lacks: an elevation looking at
/// each compass face (North, East, South, West: the face it shows), and two
/// sections, through the stair along its run and through the longest room
/// along its length (or across it when there is no stair).
fn missing_marks(
    project: &Project,
    derived: &Derived,
    ids: &mut IdGen,
    level_id: &Id,
) -> Vec<ViewMark> {
    let pts: Vec<Point> = {
        let fp: Vec<Point> = derived
            .footprints
            .iter()
            .flat_map(|f| f.polygon.iter().copied())
            .collect();
        if fp.is_empty() {
            derived
                .walls
                .iter()
                .flat_map(|w| w.outline.iter().copied())
                .collect()
        } else {
            fp
        }
    };
    let mut out: Vec<ViewMark> = vec![];
    if pts.is_empty() {
        return out;
    }
    let extent = |d: Point| {
        let v: Vec<f64> = pts.iter().map(|p| dot(*p, d)).collect();
        (
            v.iter().cloned().fold(f64::INFINITY, f64::min),
            v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let existing: Vec<&ViewMark> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::ViewMark(m) => Some(m),
            _ => None,
        })
        .collect();
    // The way a mark looks: left of start to end, or right when flipped.
    let looks = |m: &ViewMark| {
        unit(sub(m.end, m.start)).map(|d| {
            if m.flip {
                scale(perp_left(d), -1.0)
            } else {
                perp_left(d)
            }
        })
    };

    for (name, az) in [
        ("North", 0.0),
        ("East", 90.0),
        ("South", 180.0),
        ("West", 270.0),
    ] {
        let face = crate::site::direction_of(project, az);
        let taken = existing.iter().any(|m| {
            m.mark_kind == MarkKind::Elevation
                && (m.label.trim().eq_ignore_ascii_case(name)
                    || looks(m).map(|l| dot(l, face) < -0.7).unwrap_or(false))
        });
        if taken {
            continue;
        }
        // It stands off this face and looks back at it: left of start to
        // end is toward the building.
        let d = perp_left(face);
        let (_, reach) = extent(face);
        let (w0, w1) = extent(d);
        let at = |w: f64| add(scale(face, reach + MARK_MARGIN_MM), scale(d, w));
        out.push(ViewMark {
            id: ids.next_id(),
            level_id: level_id.clone(),
            mark_kind: MarkKind::Elevation,
            label: name.to_string(),
            start: at(w0 - MARK_MARGIN_MM / 2.0),
            end: at(w1 + MARK_MARGIN_MM / 2.0),
            flip: false,
            depth_mm: None,
        });
    }

    let sections = existing
        .iter()
        .filter(|m| m.mark_kind == MarkKind::Section)
        .count();
    let mut cuts: Vec<(Point, Point)> = vec![];
    if let Some(flight) = derived.stairs.first().and_then(|s| s.flights.first()) {
        if let Some(u) = unit(flight.up) {
            let c = scale(
                flight.outline.iter().fold(pt(0.0, 0.0), |a, p| add(a, *p)),
                1.0 / flight.outline.len().max(1) as f64,
            );
            cuts.push((c, u));
        }
    }
    // The longest room, by its longest side.
    let longest = derived
        .rooms
        .iter()
        .filter_map(|r| {
            let n = r.polygon.len();
            let (i, len) = (0..n)
                .map(|i| (i, dist(r.polygon[i], r.polygon[(i + 1) % n])))
                .max_by(|a, b| a.1.total_cmp(&b.1))?;
            let u = unit(sub(r.polygon[(i + 1) % n], r.polygon[i]))?;
            let (lo, hi) = {
                let v: Vec<f64> = r.polygon.iter().map(|p| dot(*p, u)).collect();
                (
                    v.iter().cloned().fold(f64::INFINITY, f64::min),
                    v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                )
            };
            Some((r.label_point, u, (hi - lo).max(len), r.area_mm2))
        })
        .max_by(|a, b| a.2.total_cmp(&b.2).then(a.3.total_cmp(&b.3)));
    if let Some((c, u, _, _)) = longest {
        cuts.push((c, u));
        if cuts.len() < 2 {
            cuts.push((c, perp_left(u)));
        }
    }
    for (c, u) in cuts.into_iter().take(2usize.saturating_sub(sections)) {
        // Across the whole building, standing off both ends.
        let (lo, hi) = extent(u);
        let t = dot(c, u);
        let base = sub(c, scale(u, t));
        let label = next_section_label(project, &out);
        out.push(ViewMark {
            id: ids.next_id(),
            level_id: level_id.clone(),
            mark_kind: MarkKind::Section,
            label,
            start: add(base, scale(u, lo - SECTION_OVERRUN_MM)),
            end: add(base, scale(u, hi + SECTION_OVERRUN_MM)),
            flip: false,
            depth_mm: None,
        });
    }
    out
}

/// The views already on some sheet.
fn placed(project: &Project) -> Vec<ViewRef> {
    project
        .sheet_set
        .sheets
        .iter()
        .flat_map(|s| s.viewports.iter().map(|v| v.view.clone()))
        .collect()
}

/// `Command::MakeSheetSet`: the permit set. First the marks it lacks (four
/// elevations facing the compass, two sections), then in order: A-1 site
/// and vicinity; a plan per level; the elevations, two to a sheet; the
/// sections; the roof plan and each level's reflected ceiling plan; the
/// schedules; then E sheets (lighting, power), P sheets (plumbing layout,
/// isometric) and M sheets (aircon) only for the services the project has,
/// on the levels that have them (D19; their professional blocks print
/// blank). `replace` makes the set new: a new sheet showing views of a
/// replaced one keeps its number, id and revision marks, in the old order,
/// and sheets with views new to the set come after, numbered after them;
/// the revisions stay. Otherwise only views on no sheet yet are added,
/// numbered after the sheets there are. Scales are left to fit.
pub fn make_sheet_set(
    project: &mut Project,
    ids: &mut IdGen,
    paper: PaperSize,
    replace: bool,
) -> Result<Outcome, CoreError> {
    let derived = crate::compute_derived(project);
    let levels = building_levels(project);
    let has_building = derived.walls.iter().any(|w| {
        project
            .elements
            .iter()
            .any(|e| matches!(e, Element::Wall(x) if x.id == w.wall_id && x.role.is_building()))
    });
    if !has_building {
        return Err(CoreError::invalid(
            "nothing_to_draw",
            "The project has no building walls yet, so there is nothing to put on the sheets. Draw the walls first.",
        ));
    }
    let ground = levels[0].id.clone();
    let marks = missing_marks(project, &derived, ids, &ground);
    let mark_count = (
        marks
            .iter()
            .filter(|m| m.mark_kind == MarkKind::Elevation)
            .count(),
        marks
            .iter()
            .filter(|m| m.mark_kind == MarkKind::Section)
            .count(),
    );
    for m in marks {
        project.elements.push(Element::ViewMark(m));
    }
    // Replaced sheets hand their number, id and revision marks on to the new
    // sheet that shows the most of their views (GA P1-7).
    let old_sheets: Vec<Sheet> = if replace {
        std::mem::take(&mut project.sheet_set.sheets)
    } else {
        vec![]
    };
    let dropped = old_sheets.len();
    let on_sheets = placed(project);
    let mut taken: BTreeSet<String> = project
        .sheet_set
        .sheets
        .iter()
        .map(|s| s.number.trim().to_string())
        .collect();
    let view_marks: Vec<ViewMark> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::ViewMark(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    let trades = trades(project, &derived);

    // (prefix, title, views)
    let mut plan: Vec<(&str, String, Vec<ViewRef>)> = vec![];
    plan.push((
        "A",
        "Site Development Plan and Vicinity Map".into(),
        vec![
            ViewRef::SitePlan,
            ViewRef::VicinityMap {
                file_name: None,
                radius_m: 500.0,
            },
        ],
    ));
    for l in &levels {
        plan.push((
            "A",
            format!("{} Plan", l.name.trim()),
            vec![ViewRef::Plan {
                level_id: l.id.clone(),
                sheet: SheetKind::Plan,
            }],
        ));
    }
    let elevations: Vec<&ViewMark> = {
        let order = ["north", "south", "east", "west"];
        let mut e: Vec<&ViewMark> = view_marks
            .iter()
            .filter(|m| {
                m.mark_kind == MarkKind::Elevation
                    && !on_sheets.contains(&ViewRef::Elevation {
                        mark_id: m.id.clone(),
                    })
            })
            .collect();
        e.sort_by_key(|m| {
            order
                .iter()
                .position(|o| m.label.trim().eq_ignore_ascii_case(o))
                .unwrap_or(order.len())
        });
        e
    };
    for pair in elevations.chunks(2) {
        let names: Vec<String> = pair.iter().map(|m| m.label.trim().to_string()).collect();
        plan.push((
            "A",
            format!("{} Elevations", names.join(" and ")),
            pair.iter()
                .map(|m| ViewRef::Elevation {
                    mark_id: m.id.clone(),
                })
                .collect(),
        ));
    }
    let sections: Vec<&ViewMark> = view_marks
        .iter()
        .filter(|m| {
            m.mark_kind == MarkKind::Section
                && !on_sheets.contains(&ViewRef::Section {
                    mark_id: m.id.clone(),
                })
        })
        .collect();
    for pair in sections.chunks(2) {
        let names: Vec<String> = pair.iter().map(|m| m.label.trim().to_string()).collect();
        plan.push((
            "A",
            format!("Sections {}", names.join(" and ")),
            pair.iter()
                .map(|m| ViewRef::Section {
                    mark_id: m.id.clone(),
                })
                .collect(),
        ));
    }
    let mut roof = vec![ViewRef::RoofPlan];
    roof.extend(levels.iter().map(|l| ViewRef::Plan {
        level_id: l.id.clone(),
        sheet: SheetKind::ReflectedCeiling,
    }));
    plan.push(("A", "Roof Plan and Reflected Ceiling Plans".into(), roof));
    plan.push((
        "A",
        "Schedules of Doors, Windows, Rooms, Finishes and Areas".into(),
        [
            ScheduleKind::DoorsWindows,
            ScheduleKind::Rooms,
            ScheduleKind::Finishes,
            ScheduleKind::Areas,
        ]
        .into_iter()
        .map(|schedule| ViewRef::Schedule { schedule })
        .collect(),
    ));
    // The levels of a trade, in elevation order; an empty list adds no sheet.
    let per_level = |with: &BTreeSet<Id>, sheet: SheetKind| -> Vec<ViewRef> {
        levels
            .iter()
            .filter(|l| with.contains(&l.id))
            .map(|l| ViewRef::Plan {
                level_id: l.id.clone(),
                sheet,
            })
            .collect()
    };
    for (prefix, title, with, sheet) in [
        ("E", "Lighting Layout", &trades.electrical, SheetKind::Lighting),
        ("E", "Power Layout", &trades.electrical, SheetKind::Power),
        ("P", "Plumbing Layout", &trades.plumbing, SheetKind::Plumbing),
        ("P", "Plumbing Isometric Diagrams", &trades.isometric, SheetKind::PlumbingIsometric),
        ("M", "Air Conditioning Layout", &trades.aircon, SheetKind::Aircon),
    ] {
        plan.push((prefix, title.into(), per_level(with, sheet)));
    }

    let plan: Vec<(&str, String, Vec<ViewRef>)> = plan
        .into_iter()
        .map(|(prefix, title, views)| (prefix, title, views.into_iter().filter(|v| !on_sheets.contains(v)).collect::<Vec<_>>()))
        .filter(|(_, _, views)| !views.is_empty())
        .collect();
    // The replaced sheet each new one takes after, in plan order: the one
    // sharing the most views (the earlier on a tie), each at most once.
    let mut claimed = vec![false; old_sheets.len()];
    let heirs: Vec<Option<usize>> = plan
        .iter()
        .map(|(_, _, views)| {
            let best = old_sheets
                .iter()
                .enumerate()
                .filter(|(i, _)| !claimed[*i])
                .map(|(i, s)| (i, s.viewports.iter().filter(|v| views.contains(&v.view)).count()))
                .filter(|(_, shared)| *shared > 0)
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(i, _)| i);
            if let Some(i) = best {
                claimed[i] = true;
                taken.insert(old_sheets[i].number.trim().to_string());
            }
            best
        })
        .collect();
    // Sheets that came back stay in their old order; new ones follow,
    // numbered after them.
    let mut made_sheets: Vec<((usize, usize), Sheet)> = vec![];
    for (k, ((prefix, title, views), heir)) in plan.into_iter().zip(heirs).enumerate() {
        let viewports = views
            .into_iter()
            .map(|view| Viewport {
                id: ids.next_id(),
                view,
                scale_denominator: None,
                position: None,
                title: None,
            })
            .collect();
        let (order, id, number, revision_ids) = match heir {
            Some(i) => {
                let old = &old_sheets[i];
                ((0, i), old.id.clone(), old.number.trim().to_string(), old.revision_ids.clone())
            }
            None => {
                let number = next_number(&taken, prefix);
                taken.insert(number.clone());
                ((1, k), ids.next_id(), number, vec![])
            }
        };
        made_sheets.push((
            order,
            Sheet {
                id,
                number,
                title,
                paper,
                orientation: Orientation::Landscape,
                viewports,
                revision_ids,
            },
        ));
    }
    made_sheets.sort_by_key(|(order, _)| *order);
    let mut made: Vec<String> = vec![];
    for (_, sheet) in made_sheets {
        made.push(sheet.number.clone());
        project.sheet_set.sheets.push(sheet);
    }
    if made.is_empty() && mark_count == (0, 0) {
        return Err(CoreError::invalid(
            "nothing_to_add",
            "Every view of the set is on a sheet already. Make the set again with replace to start over.",
        ));
    }
    let mut summary = match made.len() {
        0 => "Added no sheets".to_string(),
        1 => format!("Made sheet {}", made[0]),
        n => format!("Made {n} sheets: {}", made.join(", ")),
    };
    if dropped > 0 {
        summary.push_str(&format!(
            " in place of {}",
            crate::exec::count_noun(dropped, "sheet")
        ));
    }
    let mut added = vec![];
    if mark_count.0 > 0 {
        added.push(crate::exec::count_noun(mark_count.0, "elevation mark"));
    }
    if mark_count.1 > 0 {
        added.push(crate::exec::count_noun(mark_count.1, "section mark"));
    }
    if !added.is_empty() {
        summary.push_str(&format!(", with {}", added.join(" and ")));
    }
    Ok(Outcome {
        summary: Some(summary),
        ..outcome("Make permit set")
    })
}

/// `Command::UpsertRevision`: add or replace a revision by id (empty:
/// assigned) and mark it on `sheet_ids`.
pub fn upsert_revision(
    project: &mut Project,
    ids: &mut IdGen,
    revision: &SheetRevision,
    sheet_ids: &[Id],
) -> Result<Outcome, CoreError> {
    let mut rev = revision.clone();
    rev.mark = rev.mark.trim().to_string();
    if rev.mark.is_empty() {
        return Err(CoreError::invalid(
            "bad_name",
            "A revision needs a mark, such as 1 or A.",
        ));
    }
    if rev.id.is_empty() {
        rev.id = ids.next_id();
    } else {
        ids.reserve(&rev.id);
    }
    for id in sheet_ids {
        if !project.sheet_set.sheets.iter().any(|s| &s.id == id) {
            return Err(CoreError::invalid(
                "unknown_sheet",
                format!("There is no sheet with id {id}."),
            ));
        }
    }
    let revisions = &mut project.sheet_set.revisions;
    let label = match revisions.iter_mut().find(|r| r.id == rev.id) {
        Some(slot) => {
            *slot = rev.clone();
            "Edit revision"
        }
        None => {
            revisions.push(rev.clone());
            "Add revision"
        }
    };
    for s in project
        .sheet_set
        .sheets
        .iter_mut()
        .filter(|s| sheet_ids.contains(&s.id))
    {
        if !s.revision_ids.contains(&rev.id) {
            s.revision_ids.push(rev.id.clone());
        }
    }
    Ok(outcome(label))
}

/// `Command::DeleteRevision`: the revision, and its mark on every sheet.
pub fn delete_revision(project: &mut Project, id: &Id) -> Result<Outcome, CoreError> {
    if !project.sheet_set.revisions.iter().any(|r| &r.id == id) {
        return Err(CoreError::invalid(
            "bad_revision",
            format!("There is no revision with id {id}."),
        ));
    }
    project.sheet_set.revisions.retain(|r| &r.id != id);
    for s in project.sheet_set.sheets.iter_mut() {
        s.revision_ids.retain(|r| r != id);
    }
    Ok(outcome("Delete revision"))
}

/// P1-8: every viewport whose level or view mark is gone leaves its sheet,
/// in the step that removed it. Returns a phrase per sheet for the step's
/// summary: "removed 2 viewports from A-3".
pub fn prune_viewports(project: &mut Project) -> Vec<String> {
    let levels: BTreeSet<Id> = project.levels.iter().map(|l| l.id.clone()).collect();
    let marks: BTreeSet<Id> = project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::ViewMark(_)))
        .map(|e| e.id().clone())
        .collect();
    let mut notes = vec![];
    for s in project.sheet_set.sheets.iter_mut() {
        let before = s.viewports.len();
        s.viewports.retain(|vp| match &vp.view {
            ViewRef::Plan { level_id, .. } => levels.contains(level_id),
            ViewRef::Elevation { mark_id } | ViewRef::Section { mark_id } => {
                marks.contains(mark_id)
            }
            _ => true,
        });
        let gone = before - s.viewports.len();
        if gone > 0 {
            let what = if gone == 1 {
                "1 viewport".to_string()
            } else {
                format!("{gone} viewports")
            };
            notes.push(format!("removed {what} from sheet {}", s.number));
        }
    }
    notes
}

/// The rules of one section or elevation mark (validate hub,
/// `Element::ViewMark`).
pub fn validate_mark(project: &Project, m: &ViewMark) -> Result<(), CoreError> {
    let ids = [m.id.clone()];
    require_level(project, &m.level_id)?;
    finite_point("Mark start", m.start, &ids)?;
    finite_point("Mark end", m.end, &ids)?;
    if dist(m.start, m.end) < JOIN_EPS {
        return Err(CoreError::invalid_for(
            "mark_too_short",
            "A section or elevation mark needs two different points.",
            ids.to_vec(),
        ));
    }
    if m.label.trim().is_empty() {
        return Err(CoreError::invalid_for(
            "bad_name",
            "A section or elevation mark needs a label.",
            ids.to_vec(),
        ));
    }
    if let Some(d) = m.depth_mm {
        if !d.is_finite() || d <= 0.0 {
            return Err(CoreError::invalid_for(
                "bad_mark",
                "A mark's depth must be more than 0 mm.",
                ids.to_vec(),
            ));
        }
    }
    Ok(())
}

/// A section or elevation mark for `Query::Describe` and `ListElements`.
pub fn describe_mark(project: &Project, m: &ViewMark) -> Value {
    json!({
        "id": m.id,
        "kind": "view_mark",
        "mark_kind": m.mark_kind,
        "label": m.label,
        "level": project.levels.iter().find(|l| l.id == m.level_id).map(|l| l.name.clone()),
        "start": { "x": m.start.x, "y": m.start.y },
        "end": { "x": m.end.x, "y": m.end.y },
        "looks": if m.flip { "right of start to end" } else { "left of start to end" },
        "depth_mm": m.depth_mm,
    })
}
