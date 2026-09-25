//! The review page of a PDF export ("Design review (suggestions)"): open
//! review items grouped by level, then the items set aside with their notes,
//! then set-aside findings the checks no longer produce. Same paper and title
//! block as the sheet; long lists continue on more pages.
//!
//! Review items are suggestions (DECISIONS D24): nothing on the page reads
//! as an approval, a certification or a compliance result.

use guhit_model::{Derived, Element, Issue, IssueStatus, OpeningType, PlanExportOptions, Project, ReviewTarget, Severity};

use crate::panel::wrap;
use crate::pipes;
use crate::sheet::{num, title_block, Layout, Svg, FONT_FAMILY, INK, MUTED};
use crate::text::{clean, est_width, xml_escape};

pub const TITLE: &str = "Design review (suggestions)";

fn severity_label(s: Severity) -> (&'static str, &'static str) {
    match s {
        Severity::Error => ("ERROR", "#a3261d"),
        Severity::Warning => ("WARNING", "#9a5a00"),
        Severity::Info => ("INFO", "#2c5d9a"),
    }
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

/// What a check looks for, as a plural noun phrase: "Narrow doors". Mirrors
/// `CHECK_LABEL` in src/shell/review.ts, so the page reads like the app's
/// review list. A code this build does not know reads as its words.
pub fn check_label(code: &str) -> String {
    let known = match code {
        "room_no_window" => "Rooms without a window",
        "room_no_door" => "Rooms without a door",
        "room_small" => "Small rooms",
        "door_narrow" => "Narrow doors",
        "opening_blocked" => "Openings where a wall meets",
        "opening_near_corner" => "Openings near a corner",
        "wall_end_gap_start" => "Walls stopping short at the start",
        "wall_end_gap_end" => "Walls stopping short at the end",
        "wall_dangling_start" => "Wall starts joined to nothing",
        "wall_dangling_end" => "Wall ends joined to nothing",
        "wall_overlap" => "Overlapping walls",
        "pipe_through_column" => "Pipes through a column",
        "pipe_across_opening" => "Pipes across a door or window",
        "pipes_cross" => "Crossing pipes",
        "drain_slope_low" => "Drains that fall too little",
        "pipe_penetrations" => "Sleeves and flashings",
        "light_no_switch" => "Lights with no switch",
        "switch_no_load" => "Switches that control nothing",
        "switch_behind_door" => "Switches behind a door",
        "aircon_no_outlet" => "Aircon units with no outlet",
        "lineset_long" => "Line sets over the maximum length",
        "lineset_rise" => "Line sets over the maximum rise",
        "lineset_short" => "Line sets under 3 m",
        "lineset_extra" => "Line set length past the standard install",
        "condensate_slope_low" => "Condensate drains that fall too little",
        "condensate_open_end" => "Condensate drains ending away from a drain",
        "indoor_unit_clearance" => "Indoor unit clearances",
        "outdoor_unit_clearance" => "Outdoor unit clearances",
        "outdoor_unit_unsupported" => "Outdoor units without support",
        "unit_near_tv" => "Aircon units near a TV",
        _ => "",
    };
    if known.is_empty() {
        words(code)
    } else {
        known.to_string()
    }
}

/// "roof_leak_risk" -> "Roof leak risk".
fn words(code: &str) -> String {
    let words = code.replace('_', " ");
    let mut cs = words.trim().chars();
    match cs.next() {
        Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
        None => "A check".to_string(),
    }
}

/// A short name for an element in a sentence: its own name when it has one.
fn element_name(project: &Project, id: &str) -> String {
    let named = |name: &str, fallback: &str| {
        let n = clean(name);
        if n.is_empty() {
            fallback.to_string()
        } else {
            n
        }
    };
    match project.elements.iter().find(|e| e.id() == id) {
        Some(Element::Asset(a)) => named(&a.name, "an object"),
        Some(Element::Room(r)) => named(&r.name, "a room"),
        Some(Element::Pipe(p)) => named(&p.name, &format!("a {} run", pipes::label(p.system).to_lowercase())),
        Some(Element::Wall(_)) => "a wall".into(),
        Some(Element::Opening(o)) => match o.opening_type {
            OpeningType::Door => "a door".into(),
            OpeningType::Window => "a window".into(),
        },
        Some(Element::Column(_)) => "a column".into(),
        Some(Element::Stair(_)) => "a stair".into(),
        Some(_) => "an object".into(),
        None => "an object that is gone".into(),
    }
}

/// "A", "A and B", "A, B and C".
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What a set-aside finding that the checks no longer produce was, from its
/// mark, in the words of the app's set-aside list: "Pipes through a column,
/// on Second floor WC drain", "Narrow doors, everywhere".
fn resolved_title(project: &Project, target: &ReviewTarget) -> String {
    match target {
        // Engine ids read "<code>:<element ids>".
        ReviewTarget::Issue { id } => match id.split_once(':') {
            Some((code, ids)) if !code.trim().is_empty() => {
                let names: Vec<String> = ids.split(',').filter(|e| !e.trim().is_empty()).map(|e| element_name(project, e.trim())).collect();
                if names.is_empty() {
                    check_label(code)
                } else {
                    format!("{}, on {}", check_label(code), join_names(&names))
                }
            }
            _ => "An item the checks no longer find".to_string(),
        },
        ReviewTarget::Check { code } => format!("{}, everywhere", check_label(code)),
        ReviewTarget::Element { code, element_id } => format!("{}, on {}", check_label(code), element_name(project, element_id)),
    }
}

/// The level an issue belongs to: the level of its first element, through
/// the host wall for openings.
fn issue_level(project: &Project, issue: &Issue) -> Option<String> {
    let find = |id: &str| project.elements.iter().find(|e| e.id() == id);
    for id in &issue.element_ids {
        let level_id = match find(id) {
            Some(Element::Opening(o)) => match find(&o.wall_id) {
                Some(Element::Wall(w)) => Some(w.level_id.clone()),
                _ => None,
            },
            Some(Element::Wall(e)) => Some(e.level_id.clone()),
            Some(Element::Room(e)) => Some(e.level_id.clone()),
            Some(Element::Column(e)) => Some(e.level_id.clone()),
            Some(Element::Stair(e)) => Some(e.level_id.clone()),
            Some(Element::Asset(e)) => Some(e.level_id.clone()),
            Some(Element::Annotation(e)) => Some(e.level_id.clone()),
            Some(Element::Dimension(e)) => Some(e.level_id.clone()),
            Some(Element::Underlay(e)) => Some(e.level_id.clone()),
            Some(Element::Linework(e)) => Some(e.level_id.clone()),
            Some(Element::ReferenceModel(e)) => Some(e.level_id.clone()),
            Some(Element::Pipe(e)) => Some(e.level_id.clone()),
            _ => None,
        };
        if level_id.is_some() {
            return level_id;
        }
    }
    None
}

/// One line of the page, in paper mm relative to the text column.
enum Line {
    /// Section heading with its count.
    Section(String),
    /// Level name inside a section.
    Level(String),
    /// First line of an item: badge, then text.
    Item { badge: (&'static str, &'static str), text: String },
    /// A following line of an item's text.
    More(String),
    /// A note line under a set-aside item.
    Note(String),
    /// First line of a finding that is no longer found: no badge, since its
    /// severity went with it.
    Gone(String),
    Plain(String),
    Gap,
}

fn lines(project: &Project, derived: &Derived, width: f64, k: f64) -> Vec<Line> {
    let size = 2.0 * k;
    let text_w = width - 20.0 * k;
    let mut out = Vec::new();
    let push_item = |out: &mut Vec<Line>, issue: &Issue, note: Option<&str>| {
        // The engine's message is the item, as in the app's review list.
        let text = clean(&issue.message);
        let wrapped = wrap(&text, size, text_w);
        for (i, l) in wrapped.into_iter().enumerate() {
            if i == 0 {
                out.push(Line::Item { badge: severity_label(issue.severity), text: l });
            } else {
                out.push(Line::More(l));
            }
        }
        // An empty note prints nothing.
        if let Some(n) = note.map(clean).filter(|n| !n.is_empty()) {
            for l in wrap(&format!("Note: {n}"), 1.8 * k, text_w) {
                out.push(Line::Note(l));
            }
        }
    };
    let level_name = |id: Option<&String>| -> String {
        id.and_then(|id| project.levels.iter().find(|l| &l.id == id))
            .map(|l| clean(&l.name))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Whole project".into())
    };

    let open: Vec<&Issue> = derived.issues.iter().filter(|i| i.status == IssueStatus::Open).collect();
    let aside: Vec<&Issue> = derived.issues.iter().filter(|i| i.status == IssueStatus::Ignored).collect();
    out.push(Line::Section(format!("OPEN ({})", open.len())));
    if open.is_empty() {
        out.push(Line::Plain("No open items.".into()));
    }
    // Levels in project order, then items without a level.
    let mut groups: Vec<(Option<String>, Vec<&Issue>)> = Vec::new();
    for i in &open {
        let l = issue_level(project, i);
        match groups.iter_mut().find(|g| g.0 == l) {
            Some(g) => g.1.push(i),
            None => groups.push((l, vec![i])),
        }
    }
    groups.sort_by_key(|g| {
        g.0.as_ref()
            .and_then(|id| project.levels.iter().position(|l| &l.id == id))
            .unwrap_or(usize::MAX)
    });
    for (level, mut items) in groups {
        items.sort_by_key(|i| severity_rank(i.severity));
        out.push(Line::Level(format!("{} ({})", level_name(level.as_ref()), items.len())));
        for i in items {
            push_item(&mut out, i, None);
        }
    }
    out.push(Line::Gap);
    out.push(Line::Section(format!("SET ASIDE ({})", aside.len())));
    if aside.is_empty() {
        out.push(Line::Plain("No items are set aside.".into()));
    }
    for i in aside {
        push_item(&mut out, i, Some(&i.note));
    }
    if !derived.review_resolved.is_empty() {
        out.push(Line::Gap);
        out.push(Line::Section(format!("NO LONGER FOUND ({})", derived.review_resolved.len())));
        out.push(Line::Plain("Findings set aside earlier that the checks no longer produce.".into()));
        for m in &derived.review_resolved {
            for (i, l) in wrap(&resolved_title(project, &m.target), size, text_w).into_iter().enumerate() {
                out.push(if i == 0 { Line::Gone(l) } else { Line::More(l) });
            }
            let n = clean(&m.note);
            if !n.is_empty() {
                for l in wrap(&format!("Note: {n}"), 1.8 * k, text_w) {
                    out.push(Line::Note(l));
                }
            }
        }
    }
    out
}

fn line_height(l: &Line, k: f64) -> f64 {
    match l {
        Line::Section(_) => 7.0 * k,
        Line::Level(_) => 5.2 * k,
        Line::Item { .. } | Line::Gone(_) => 4.4 * k,
        Line::More(_) | Line::Note(_) => 3.1 * k,
        Line::Plain(_) => 3.6 * k,
        Line::Gap => 3.0 * k,
    }
}

/// The review pages as SVG documents, same paper as the sheet.
pub fn pages(project: &Project, derived: &Derived, opts: &PlanExportOptions) -> Vec<String> {
    let l = Layout::new(opts.paper, opts.orientation, true);
    let k = l.k;
    let m = l.margin;
    let pad = 8.0 * k;
    let x0 = m + pad;
    let width = l.width - 2.0 * (m + pad);
    let top = m + pad;
    let bottom = l.height - m - l.title_h - 6.0 * k;
    let all = lines(project, derived, width, k);

    // Split into pages. A page starts with its heading.
    let head_h = 17.0 * k;
    let mut pages: Vec<Vec<&Line>> = vec![Vec::new()];
    let mut y = top + head_h;
    for line in &all {
        let h = line_height(line, k);
        // Keep a section or level heading with the line after it.
        let need = match line {
            Line::Section(_) | Line::Level(_) => h + 3.6 * k,
            _ => h,
        };
        if y + need > bottom && !pages.last().expect("a page").is_empty() {
            pages.push(Vec::new());
            y = top + head_h;
            if matches!(line, Line::Gap) {
                continue;
            }
        }
        pages.last_mut().expect("a page").push(line);
        y += h;
    }

    let total = pages.len();
    pages
        .iter()
        .enumerate()
        .map(|(pi, page)| {
            let mut svg = Svg { s: String::new() };
            svg.s.push_str(&format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"{FONT_FAMILY}\">\n",
                num(l.width),
                num(l.height),
                num(l.width),
                num(l.height)
            ));
            svg.s.push_str(&format!("<title>{}</title>\n", xml_escape(&format!("{} - {TITLE}", clean(&project.name)))));
            svg.s.push_str(&format!(
                "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
                num(l.width),
                num(l.height)
            ));
            svg.s.push_str("<g id=\"review\">\n");
            let heading = if pi == 0 {
                TITLE.to_string()
            } else {
                format!("{TITLE}, continued")
            };
            svg.text(x0, top + 5.0 * k, 5.0 * k, "start", true, INK, &heading);
            let sub = if total > 1 {
                format!(
                    "{}. Suggestions from the design checks in Guhit Studio; they approve or certify nothing. Page {} of {}.",
                    clean(&project.name),
                    pi + 1,
                    total
                )
            } else {
                format!(
                    "{}. Suggestions from the design checks in Guhit Studio; they approve or certify nothing.",
                    clean(&project.name)
                )
            };
            svg.text(x0, top + 10.5 * k, 2.2 * k, "start", false, MUTED, &sub);
            svg.line((x0, top + 13.0 * k), (x0 + width, top + 13.0 * k), 0.35);
            let mut y = top + head_h;
            let size = 2.0 * k;
            let text_x = x0 + 20.0 * k;
            for line in page {
                let h = line_height(line, k);
                let base = y + h - 1.0 * k;
                match line {
                    Line::Section(t) => {
                        svg.text(x0, base - 1.0 * k, 3.0 * k, "start", true, INK, t);
                    }
                    Line::Level(t) => {
                        svg.text(x0, base, 2.4 * k, "start", true, INK, t);
                        svg.line((x0, base + 1.0 * k), (x0 + width, base + 1.0 * k), 0.18);
                    }
                    Line::Item { badge, text } => {
                        let bw = est_width(badge.0, 1.5 * k, true) + 2.0 * k;
                        svg.s.push_str(&format!(
                            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"0.6\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.25\"/>\n",
                            num(x0),
                            num(base - 2.3 * k),
                            num(bw),
                            num(2.9 * k),
                            badge.1
                        ));
                        svg.text(x0 + bw / 2.0, base - 0.25 * k, 1.5 * k, "middle", true, badge.1, badge.0);
                        svg.text(text_x, base, size, "start", false, INK, text);
                    }
                    Line::More(t) | Line::Gone(t) => svg.text(text_x, base, size, "start", false, INK, t),
                    Line::Note(t) => svg.text(text_x, base, 1.8 * k, "start", false, MUTED, t),
                    Line::Plain(t) => svg.text(x0, base, size, "start", false, MUTED, t),
                    Line::Gap => {}
                }
                y += h;
            }
            svg.s.push_str("</g>\n<g id=\"sheet\">\n");
            svg.rect(m, m, l.width - 2.0 * m, l.height - 2.0 * m, 0.35, "none");
            title_block(&mut svg, project, TITLE, opts, &l, "-");
            svg.s.push_str("</g>\n</svg>\n");
            svg.s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_read_like_the_review_list() {
        assert_eq!(check_label("pipe_through_column"), "Pipes through a column");
        assert_eq!(check_label("door_narrow"), "Narrow doors");
        assert_eq!(check_label("roof_leak_risk"), "Roof leak risk");
        assert_eq!(check_label(" "), "A check");
        assert_eq!(join_names(&["A".into(), "B".into(), "C".into()]), "A, B and C");
    }

    #[test]
    fn every_engine_check_has_a_label() {
        // Keep in step with guhit_core::issues::REVIEW_CODES (not a
        // dependency of this crate): a missing label reads as raw words.
        for code in [
            "room_no_window", "room_no_door", "room_small", "door_narrow", "opening_blocked", "opening_near_corner",
            "wall_end_gap_start", "wall_end_gap_end", "wall_dangling_start", "wall_dangling_end", "wall_overlap",
            "pipe_through_column", "pipe_across_opening", "pipes_cross", "drain_slope_low", "pipe_penetrations",
            "condensate_slope_low", "condensate_open_end", "light_no_switch", "switch_no_load", "switch_behind_door",
            "aircon_no_outlet", "lineset_long", "lineset_rise", "lineset_short", "lineset_extra",
            "indoor_unit_clearance", "outdoor_unit_clearance", "outdoor_unit_unsupported", "unit_near_tv",
        ] {
            assert_ne!(check_label(code), words(code), "{code} has no label");
        }
    }
}
