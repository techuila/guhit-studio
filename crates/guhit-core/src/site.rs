//! The lot from its title (DECISIONS D53, D54). LANE L1 owns this module.
//!
//! Written in C0: `validate` (one lot per project, P1-7; corners, sides and
//! setbacks), `query` and `describe`. Stubs with their final signatures:
//! `set_from_survey` (`not_built`), `derive` (None) and `review` (none).
//! Setbacks and coverage are facts drawn on the site plan, never code checks.

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::exec::Outcome;
use crate::geom::*;
use crate::ids::IdGen;
use crate::topo::Analysis;
use crate::validate::{finite, finite_point};

/// The rules of the lot (validate hub, `Element::Lot`).
pub fn validate(project: &Project, lot: &Lot) -> Result<(), CoreError> {
    let ids = [lot.id.clone()];
    if let Some(other) = project
        .elements
        .iter()
        .find(|e| matches!(e, Element::Lot(o) if o.id != lot.id))
    {
        return Err(CoreError::invalid_for(
            "lot_exists",
            "The project already has a lot. Edit that lot, or set it again from the title's technical description.",
            vec![other.id().clone()],
        ));
    }
    if lot.corners.len() < 3 {
        return Err(CoreError::invalid_for(
            "lot_outline",
            "A lot needs at least three corners.",
            ids.to_vec(),
        ));
    }
    for p in &lot.corners {
        finite_point("Lot corner", *p, &ids)?;
    }
    if signed_area(&lot.corners).abs() < 1.0 || !is_simple(&lot.corners) {
        return Err(CoreError::invalid_for(
            "lot_outline",
            "The lot's sides cross or enclose nothing. Check the corners.",
            ids.to_vec(),
        ));
    }
    if lot.sides.len() != lot.corners.len() {
        return Err(CoreError::invalid_for(
            "lot_outline",
            format!(
                "A lot has one side per corner: {} corners, {} sides.",
                lot.corners.len(),
                lot.sides.len()
            ),
            ids.to_vec(),
        ));
    }
    for s in &lot.sides {
        finite("Setback", s.setback_mm, &ids)?;
        if s.setback_mm < 0.0 {
            return Err(CoreError::invalid_for(
                "lot_setback",
                "A setback cannot be negative.",
                ids.to_vec(),
            ));
        }
        if let Some(w) = s.road_width_mm {
            finite("Road width", w, &ids)?;
        }
    }
    finite("Finish grade", lot.grade_mm, &ids)
}

/// A survey closes when its legs end within this share of the perimeter of
/// where they began (1:100). Past it a bearing or a distance is wrong.
pub const MAX_MISCLOSURE: f64 = 0.01;

/// The forms `parse_bearing` reads, for its error messages.
const BEARING_FORMS: &str =
    "Write it as the title does: N or S, the angle, then E or W, for example N 45 deg 30' E, N. 45 deg. 30' E., N 45°30' E, N 45-30 E or S 12°05'30\" W. Due North, Due East and so on also read.";

/// A quadrant bearing as PH titles write it ("N 45 deg 30' E", "N. 45 deg.
/// 30' E.", "N 45°30' E", "N 45-30 E", "S 12°05'30\" W", "N45d30'E",
/// "Due North") as an azimuth: degrees clockwise from true north, 0 to 360.
/// The error says what is wrong in plain words.
pub fn parse_bearing(text: &str) -> Result<f64, String> {
    let mut s = format!(" {} ", text.trim().to_uppercase());
    if s.trim().is_empty() {
        return Err("it is empty".into());
    }
    for (from, to) in [("NORTH", "N"), ("SOUTH", "S"), ("EAST", "E"), ("WEST", "W")] {
        s = s.replace(from, to);
    }
    if let Some(rest) = s.trim().strip_prefix("DUE") {
        return match rest.trim().trim_end_matches('.') {
            "N" => Ok(0.0),
            "E" => Ok(90.0),
            "S" => Ok(180.0),
            "W" => Ok(270.0),
            other => Err(format!("\"Due {other}\" is not north, south, east or west")),
        };
    }
    for word in [
        "DEGREES", "DEGREE", "DEGS", "DEG", "MINUTES", "MINUTE", "MINS", "MIN", "SECONDS",
        "SECOND", "SECS", "SEC",
    ] {
        s = s.replace(word, " ");
    }
    let letters: Vec<(usize, char)> = s
        .char_indices()
        .filter(|(_, c)| c.is_ascii_alphabetic())
        .collect();
    // A D between numbers is a degree sign ("45D30").
    let letters: Vec<(usize, char)> = letters
        .iter()
        .enumerate()
        .filter(|(k, (_, c))| !(*c == 'D' && *k > 0 && *k + 1 < letters.len()))
        .map(|(_, x)| *x)
        .collect();
    let (Some(&(i0, first)), Some(&(i1, last))) = (letters.first(), letters.last()) else {
        return Err("it has no N or S at the start and no E or W at the end".into());
    };
    // One letter ("N 45", "45 E", "E") is half a bearing.
    if letters.len() < 2 {
        return Err(if first == 'N' || first == 'S' {
            format!("it has {first} at the start but no E or W at the end")
        } else {
            format!("it has {first} but no N or S at the start; a bearing reads N or S, the angle, then E or W")
        });
    }
    if letters.len() != 2 {
        let odd: String = letters[1..letters.len() - 1]
            .iter()
            .map(|(_, c)| *c)
            .collect();
        return Err(format!("it has letters it cannot read ({odd})"));
    }
    if first != 'N' && first != 'S' {
        return Err(format!(
            "it starts with {first}; a bearing starts with N or S"
        ));
    }
    if last != 'E' && last != 'W' {
        return Err(format!("it ends with {last}; a bearing ends with E or W"));
    }
    let middle = &s[i0 + 1..i1];
    let nums: Vec<f64> = middle
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|t| t.trim_matches('.'))
        .filter(|t| !t.is_empty())
        .map(|t| {
            t.parse::<f64>()
                .map_err(|_| format!("\"{t}\" is not a number"))
        })
        .collect::<Result<_, _>>()?;
    let (deg, min, sec) = match nums.as_slice() {
        [] => return Err("it has no angle".into()),
        [d] => (*d, 0.0, 0.0),
        [d, m] => (*d, *m, 0.0),
        [d, m, s] => (*d, *m, *s),
        _ => return Err("it has more numbers than degrees, minutes and seconds".into()),
    };
    if min >= 60.0 || sec >= 60.0 || (nums.len() > 1 && deg.fract() != 0.0) {
        return Err("its minutes and seconds must be under 60, after whole degrees".into());
    }
    let angle = deg + min / 60.0 + sec / 3600.0;
    if angle > 90.0 {
        return Err(format!(
            "its angle is {angle:.2} degrees; a quadrant bearing is 0 to 90"
        ));
    }
    Ok(match (first, last) {
        ('N', 'E') => angle,
        ('S', 'E') => 180.0 - angle,
        ('S', 'W') => 180.0 + angle,
        _ => (360.0 - angle) % 360.0,
    })
}

/// An azimuth as a quadrant bearing to the second: "N 45°30' E",
/// "S 12°05'30\" W".
pub fn format_bearing(azimuth_deg: f64) -> String {
    let mut az = azimuth_deg.rem_euclid(360.0);
    // To the second first, so 44°59'59.9" reads 45°00'.
    az = (az * 3600.0).round() / 3600.0 % 360.0;
    let (ns, angle, ew) = if az <= 90.0 {
        ('N', az, 'E')
    } else if az < 180.0 {
        ('S', 180.0 - az, 'E')
    } else if az <= 270.0 {
        ('S', az - 180.0, 'W')
    } else {
        ('N', 360.0 - az, 'W')
    };
    let total = (angle * 3600.0).round() as i64;
    let (d, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if s == 0 {
        format!("{ns} {d}°{m:02}' {ew}")
    } else {
        format!("{ns} {d}°{m:02}'{s:02}\" {ew}")
    }
}

/// True north and east as plan directions (`ProjectSettings::north_angle_deg`
/// turns true north from +y, counter-clockwise).
fn compass(project: &Project) -> (Point, Point) {
    let n = project.settings.north_angle_deg.to_radians();
    (pt(-n.sin(), n.cos()), pt(n.cos(), n.sin()))
}

/// The plan direction of an azimuth.
pub fn direction_of(project: &Project, azimuth_deg: f64) -> Point {
    let (north, east) = compass(project);
    let a = azimuth_deg.to_radians();
    add(scale(north, a.cos()), scale(east, a.sin()))
}

/// The azimuth of a plan direction.
pub fn azimuth_of(project: &Project, d: Point) -> f64 {
    let (north, east) = compass(project);
    dot(d, east)
        .atan2(dot(d, north))
        .to_degrees()
        .rem_euclid(360.0)
}

/// "1:18,000".
fn ratio(perimeter: f64, miss: f64) -> String {
    let r = (perimeter / miss).round() as u64;
    let digits = r.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("1:{out}")
}

/// `Command::SetLotFromSurvey`: the lot from its title's technical
/// description. Leg i runs from corner i; corner 1 is `start`. A small
/// misclosure is spread over the corners by the compass rule (each corner
/// moves by its share of the distance walked), and the summary states it.
/// The corners are kept in the title's order, clockwise (most titles) or
/// not, so side i is the title's line i to i + 1. One lot per
/// project: an existing lot takes the new corners and keeps its id, grade
/// and, when the count matches, its sides.
pub fn set_from_survey(
    project: &mut Project,
    ids: &mut IdGen,
    start: Point,
    legs: &[SurveyLeg],
) -> Result<Outcome, CoreError> {
    crate::validate::ensure_unlocked(project, ElementKind::Lot)?;
    finite_point("Lot start", start, &[])?;
    if legs.len() < 3 {
        return Err(CoreError::invalid(
            "lot_outline",
            format!(
                "A lot needs at least three legs; this description has {}.",
                legs.len()
            ),
        ));
    }
    let mut corners = vec![start];
    let mut walked = vec![0.0];
    let mut perimeter = 0.0;
    for (i, leg) in legs.iter().enumerate() {
        let az = parse_bearing(&leg.bearing).map_err(|why| {
            CoreError::invalid(
                "bad_bearing",
                format!(
                    "Leg {} reads \"{}\", which is not a bearing: {why}. {BEARING_FORMS}",
                    i + 1,
                    leg.bearing.trim()
                ),
            )
        })?;
        if !leg.distance_mm.is_finite() || leg.distance_mm < 1.0 {
            return Err(CoreError::invalid(
                "bad_distance",
                format!(
                    "Leg {} has a distance of {} mm. Each leg needs a length, in mm.",
                    i + 1,
                    leg.distance_mm
                ),
            ));
        }
        let last = *corners.last().unwrap();
        corners.push(add(last, scale(direction_of(project, az), leg.distance_mm)));
        perimeter += leg.distance_mm;
        walked.push(perimeter);
    }
    let end = corners.pop().unwrap();
    let miss_v = sub(end, start);
    let miss = length(miss_v);
    if miss > perimeter * MAX_MISCLOSURE {
        return Err(CoreError::invalid(
            "lot_not_closed",
            format!(
                "The legs do not close: they end {:.2} m from where they start, on a perimeter of {:.2} m. Check each bearing and distance against the title; N and S or E and W swapped on one leg is the usual slip.",
                miss / 1000.0,
                perimeter / 1000.0
            ),
        ));
    }
    // Compass rule.
    for (i, c) in corners.iter_mut().enumerate() {
        *c = sub(*c, scale(miss_v, walked[i] / perimeter));
    }
    // The corners stay in the title's order, clockwise or not, so corner
    // numbers and side bearings read as the title does (GR1 #3). Readers
    // that need a winding go through `ccw`.
    if !is_simple(&corners) || signed_area(&corners).abs() < 1.0 {
        return Err(CoreError::invalid(
            "lot_outline",
            "The legs cross each other or enclose nothing. Check the bearings against the title.",
        ));
    }
    let n = corners.len();
    let area = signed_area(&corners).abs() / 1.0e6;
    let blank_side = || LotSide {
        role: LotSideRole::Side,
        setback_mm: 0.0,
        road: String::new(),
        road_width_mm: None,
    };
    let existing = project
        .elements
        .iter()
        .position(|e| matches!(e, Element::Lot(_)));
    let (lot, label) = match existing.map(|i| &project.elements[i]) {
        Some(Element::Lot(old)) => (
            Lot {
                id: old.id.clone(),
                corners,
                sides: if old.sides.len() == n {
                    old.sides.clone()
                } else {
                    (0..n).map(|_| blank_side()).collect()
                },
                grade_mm: old.grade_mm,
            },
            "Edit lot",
        ),
        _ => (
            Lot {
                id: ids.next_id(),
                corners,
                sides: (0..n).map(|_| blank_side()).collect(),
                grade_mm: 150.0,
            },
            "Set lot",
        ),
    };
    match existing {
        Some(i) => project.elements[i] = Element::Lot(lot),
        None => project.elements.push(Element::Lot(lot)),
    }
    let closure = if miss < 0.5 {
        "it closes exactly".to_string()
    } else {
        format!(
            "misclosure {:.0} mm ({}), spread over the corners",
            miss,
            ratio(perimeter, miss)
        )
    };
    Ok(Outcome {
        summary: Some(format!(
            "Set the lot from {n} legs: {area:.2} m2, {closure}"
        )),
        ..crate::exec::outcome(label)
    })
}

/// The shortest distance between two segments.
fn segment_gap(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segments_touch(a, b, c, d) {
        return 0.0;
    }
    dist_point_segment(a, c, d)
        .min(dist_point_segment(b, c, d))
        .min(dist_point_segment(c, a, b))
        .min(dist_point_segment(d, a, b))
}

/// The lot's corners counter-clockwise, with each side's index into the
/// stored order.
fn ccw(lot: &Lot) -> (Vec<Point>, Vec<usize>) {
    let n = lot.corners.len();
    if signed_area(&lot.corners) >= 0.0 {
        (lot.corners.clone(), (0..n).collect())
    } else {
        // Reversed: new side k runs from old corner n-1-k to n-2-k, which is
        // old side n-2-k (mod n).
        let corners: Vec<Point> = lot.corners.iter().rev().copied().collect();
        (corners, (0..n).map(|k| (2 * n - 2 - k) % n).collect())
    }
}

/// The lot shrunk by each side's setback. Empty when no side has one.
pub fn setback_outline(lot: &Lot) -> Vec<Point> {
    if lot.sides.iter().all(|s| s.setback_mm <= 0.0) || lot.corners.len() < 3 {
        return vec![];
    }
    let (corners, side_of) = ccw(lot);
    let dists: Vec<f64> = side_of
        .iter()
        .map(|i| lot.sides.get(*i).map(|s| s.setback_mm).unwrap_or(0.0))
        .collect();
    let out = offset_walk_left(&corners, &dists);
    if out.len() >= 3 && signed_area(&out) > 1.0 && is_simple(&out) {
        out
    } else {
        vec![]
    }
}

/// The lowest level that has a building: its footprints carry the coverage.
fn ground_level(project: &Project, derived: &Derived) -> Option<Id> {
    let mut levels: Vec<&Level> = project.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    levels
        .into_iter()
        .find(|l| {
            derived
                .footprints
                .iter()
                .any(|f| f.level_id == l.id && f.polygon.len() >= 3)
        })
        .map(|l| l.id.clone())
}

/// `Derived::site`: area, each side's length, bearing and nearest building
/// face, the setback outline, and the coverage as a fact.
pub fn derive(
    project: &Project,
    _analysis: &Analysis,
    derived: &Derived,
) -> Option<Box<SiteGeometry>> {
    let lot = project.elements.iter().find_map(|e| match e {
        Element::Lot(l) => Some(l),
        _ => None,
    })?;
    let n = lot.corners.len();
    if n < 3 {
        return None;
    }
    let area_mm2 = signed_area(&lot.corners).abs();
    let buildings: Vec<&Vec<Point>> = derived
        .footprints
        .iter()
        .filter(|f| f.polygon.len() >= 3)
        .map(|f| &f.polygon)
        .collect();
    let sides = (0..n)
        .map(|i| {
            let (a, b) = (lot.corners[i], lot.corners[(i + 1) % n]);
            let clearance = buildings
                .iter()
                .flat_map(|poly| {
                    (0..poly.len())
                        .map(move |k| segment_gap(a, b, poly[k], poly[(k + 1) % poly.len()]))
                })
                .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))))
                .map(|d| (d * 10.0).round() / 10.0);
            LotSideGeometry {
                length_mm: (dist(a, b) * 10.0).round() / 10.0,
                bearing: format_bearing(azimuth_of(project, sub(b, a))),
                clearance_mm: clearance,
            }
        })
        .collect();
    let covered: f64 = match ground_level(project, derived) {
        Some(l) => derived
            .footprints
            .iter()
            .filter(|f| f.level_id == l)
            .map(|f| f.area_mm2)
            .sum(),
        None => 0.0,
    };
    Some(Box::new(SiteGeometry {
        lot_id: lot.id.clone(),
        area_m2: (area_mm2 / 1.0e6 * 1.0e6).round() / 1.0e6,
        sides,
        setback_outline: setback_outline(lot),
        coverage_pct: if area_mm2 > 0.0 {
            (covered / area_mm2 * 100.0 * 100.0).round() / 100.0
        } else {
            0.0
        },
    }))
}

/// `building_over_setback`: a building that reaches past the setback line
/// drawn along a side, one item per side, with how far. Setbacks are what
/// was entered, never a code check.
pub fn review(project: &Project, derived: &Derived) -> Vec<Issue> {
    let Some(lot) = project.elements.iter().find_map(|e| match e {
        Element::Lot(l) => Some(l),
        _ => None,
    }) else {
        return vec![];
    };
    if lot.sides.iter().all(|s| s.setback_mm <= 0.0) {
        return vec![];
    }
    let (corners, side_of) = ccw(lot);
    let n = corners.len();
    let points: Vec<Point> = derived
        .footprints
        .iter()
        .flat_map(|f| f.polygon.iter().copied())
        .collect();
    let mut out = vec![];
    for k in 0..n {
        let i = side_of[k];
        let Some(side) = lot.sides.get(i) else {
            continue;
        };
        if side.setback_mm <= 0.0 {
            continue;
        }
        let (a, b) = (corners[k], corners[(k + 1) % n]);
        let Some(u) = unit(sub(b, a)) else { continue };
        let len = dist(a, b);
        // How far past the setback line, for points beside this side.
        let past = points
            .iter()
            .filter(|p| {
                let t = dot(sub(**p, a), u);
                (-side.setback_mm..=len + side.setback_mm).contains(&t)
            })
            .map(|p| side.setback_mm - cross(u, sub(*p, a)))
            .fold(0.0, f64::max);
        if past < 1.0 {
            continue;
        }
        let role = match side.role {
            LotSideRole::Road if !side.road.trim().is_empty() => {
                format!(", along {}", side.road.trim())
            }
            LotSideRole::Road => ", the road side".to_string(),
            LotSideRole::Rear => ", the rear".to_string(),
            LotSideRole::Side => String::new(),
        };
        out.push(Issue {
            id: format!("building_over_setback:{}:{}", lot.id, i + 1),
            severity: Severity::Warning,
            code: "building_over_setback".into(),
            message: format!(
                "The building reaches {:.0} mm past the {:.2} m setback drawn along lot side {}{role}. The setback is the one entered for this lot; check it against the local rules.",
                past,
                side.setback_mm / 1000.0,
                i + 1
            ),
            element_ids: vec![lot.id.clone()],
            location: None,
            status: IssueStatus::Open,
            note: String::new(),
        });
    }
    out
}

/// `Query::Site`.
pub fn query(derived: &Derived) -> Value {
    json!({
        "site": derived.site,
        "note": "Lot area, sides, setbacks and building coverage from the model. Facts for the site plan, never a zoning or code check.",
    })
}

/// The lot for `Query::Describe` and `ListElements`.
pub fn describe(lot: &Lot) -> Value {
    json!({
        "id": lot.id,
        "kind": "lot",
        "corners": lot.corners.iter().map(|p| json!({ "x": p.x, "y": p.y })).collect::<Vec<_>>(),
        "sides": lot.sides,
        "area_m2": (signed_area(&lot.corners).abs() / 1.0e6 * 100.0).round() / 100.0,
        "grade_mm": lot.grade_mm,
    })
}
