//! Grid lines: datums on every level (DECISIONS D54). LANE L1 owns this
//! module.
//!
//! Written in C0: `validate` (finite ends, a length, a unique label) and
//! `describe`. Stub with its final signature: `add_system` (`not_built`).

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::exec::Outcome;
use crate::geom::*;
use crate::ids::IdGen;
use crate::validate::finite_point;

/// The shortest grid line, mm.
pub const MIN_GRID_LENGTH_MM: f64 = 100.0;

/// The rules of one grid line (validate hub, `Element::Grid`).
pub fn validate(project: &Project, g: &GridLine) -> Result<(), CoreError> {
    let ids = [g.id.clone()];
    finite_point("Grid line start", g.start, &ids)?;
    finite_point("Grid line end", g.end, &ids)?;
    if dist(g.start, g.end) < MIN_GRID_LENGTH_MM {
        return Err(CoreError::invalid_for(
            "grid_too_short",
            format!("A grid line must be at least {MIN_GRID_LENGTH_MM:.0} mm long."),
            ids.to_vec(),
        ));
    }
    let label = g.label.trim();
    if label.is_empty() {
        return Err(CoreError::invalid_for(
            "bad_name",
            "A grid line needs a label, such as A or 1.",
            ids.to_vec(),
        ));
    }
    if let Some(other) = project
        .elements
        .iter()
        .find(|e| matches!(e, Element::Grid(o) if o.id != g.id && o.label.trim() == label))
    {
        return Err(CoreError::invalid_for(
            "grid_label_taken",
            format!("Grid line {label} already exists. Give this one another label."),
            vec![g.id.clone(), other.id().clone()],
        ));
    }
    Ok(())
}

/// How far grid lines reach past the outermost lines of the other way, mm:
/// their bubbles clear the automatic exterior dimensions (2400 mm off the
/// outer face, `dims_auto::TIER_OFFSET_MM`).
pub const GRID_OVERSHOOT_MM: f64 = 3600.0;

/// Letters for grid lines, as drawings use them: no I and no O (they read
/// as 1 and 0). After Z: AA, AB.
pub fn grid_letter(i: usize) -> String {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let n = LETTERS.len();
    if i < n {
        (LETTERS[i] as char).to_string()
    } else {
        format!("{}{}", grid_letter(i / n - 1), LETTERS[i % n] as char)
    }
}

/// The index of a letter label (`grid_letter`), if it is one.
fn letter_index(label: &str) -> Option<usize> {
    (0..24 * 25).find(|i| grid_letter(*i).eq_ignore_ascii_case(label.trim()))
}

/// `Command::AddGridSystem`: lines 1, 2, 3 spaced along x (they run along
/// y) and A, B, C spaced along y (they run along x), from `origin`, turned
/// by `rotation_deg` about it. Spacings are between neighbors, so n
/// spacings make n + 1 lines. Numbers and letters continue after the
/// project's highest ones. The lines reach 3600 mm past the outermost lines
/// of the other way; with no lines the other way, past the buildings, or
/// 6000 mm when there are none.
pub fn add_system(
    project: &mut Project,
    ids: &mut IdGen,
    origin: Point,
    rotation_deg: f64,
    x_spacings_mm: &[f64],
    y_spacings_mm: &[f64],
) -> Result<Outcome, CoreError> {
    crate::validate::ensure_unlocked(project, ElementKind::Grid)?;
    finite_point("Grid origin", origin, &[])?;
    crate::validate::finite("Grid rotation", rotation_deg, &[])?;
    if x_spacings_mm.is_empty() && y_spacings_mm.is_empty() {
        return Err(CoreError::invalid(
            "bad_grid",
            "Give the spacings of the grid lines along x, along y, or both: for example 4000, 3500.",
        ));
    }
    for s in x_spacings_mm.iter().chain(y_spacings_mm) {
        if !s.is_finite() || *s < MIN_GRID_LENGTH_MM {
            return Err(CoreError::invalid(
                "bad_grid",
                format!(
                    "Grid spacings must be at least {MIN_GRID_LENGTH_MM:.0} mm. One is {s:.0} mm."
                ),
            ));
        }
    }
    if x_spacings_mm.len() + y_spacings_mm.len() > 100 {
        return Err(CoreError::invalid(
            "bad_grid",
            "A grid system has at most 100 spacings.",
        ));
    }
    let a = rotation_deg.to_radians();
    let (ux, uy) = (pt(a.cos(), a.sin()), pt(-a.sin(), a.cos()));
    let at = |x: f64, y: f64| add(origin, add(scale(ux, x), scale(uy, y)));
    let cum = |sp: &[f64]| -> Vec<f64> {
        let mut out = if sp.is_empty() { vec![] } else { vec![0.0] };
        let mut t = 0.0;
        for s in sp {
            t += s;
            out.push(t);
        }
        out
    };
    let (xs, ys) = (cum(x_spacings_mm), cum(y_spacings_mm));
    // The extent of the buildings in the grid's own axes, for lines with
    // nothing to cross the other way.
    let local: Vec<Point> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if w.role.is_building() => Some([w.start, w.end]),
            _ => None,
        })
        .flatten()
        .map(|p| {
            let d = sub(p, origin);
            pt(dot(d, ux), dot(d, uy))
        })
        .collect();
    let span = |lines: &[f64], other_axis: fn(&Point) -> f64| -> (f64, f64) {
        if let (Some(lo), Some(hi)) = (lines.first(), lines.last()) {
            return (lo - GRID_OVERSHOOT_MM, hi + GRID_OVERSHOOT_MM);
        }
        let vals: Vec<f64> = local.iter().map(other_axis).collect();
        if vals.is_empty() {
            return (-GRID_OVERSHOOT_MM, 6000.0 + GRID_OVERSHOOT_MM);
        }
        let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (lo - GRID_OVERSHOOT_MM, hi + GRID_OVERSHOOT_MM)
    };
    let (y0, y1) = span(&ys, |p| p.y);
    let (x0, x1) = span(&xs, |p| p.x);

    let labels: Vec<String> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Grid(g) => Some(g.label.trim().to_string()),
            _ => None,
        })
        .collect();
    let first_number = labels
        .iter()
        .filter_map(|l| l.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let first_letter = labels
        .iter()
        .filter_map(|l| letter_index(l))
        .max()
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut numbered = vec![];
    for (i, x) in xs.iter().enumerate() {
        numbered.push(GridLine {
            id: ids.next_id(),
            label: (first_number + i as u32).to_string(),
            start: at(*x, y0),
            end: at(*x, y1),
        });
    }
    let mut lettered = vec![];
    for (i, y) in ys.iter().enumerate() {
        lettered.push(GridLine {
            id: ids.next_id(),
            label: grid_letter(first_letter + i),
            start: at(x0, *y),
            end: at(x1, *y),
        });
    }
    let mut parts = vec![];
    for set in [&numbered, &lettered] {
        if let (Some(f), Some(l)) = (set.first(), set.last()) {
            parts.push(format!("{} to {}", f.label, l.label));
        }
    }
    let n = numbered.len() + lettered.len();
    let summary = format!(
        "Added {} ({})",
        crate::exec::count_noun(n, "grid line"),
        parts.join(", ")
    );
    for g in numbered.into_iter().chain(lettered) {
        project.elements.push(Element::Grid(g));
    }
    Ok(Outcome {
        summary: Some(summary),
        ..crate::exec::outcome("Add grid")
    })
}

/// A grid line for `Query::Describe` and `ListElements`.
pub fn describe(g: &GridLine) -> Value {
    json!({
        "id": g.id,
        "kind": "grid",
        "label": g.label,
        "start": { "x": g.start.x, "y": g.start.y },
        "end": { "x": g.end.x, "y": g.end.y },
        "length_mm": dist(g.start, g.end).round(),
    })
}
