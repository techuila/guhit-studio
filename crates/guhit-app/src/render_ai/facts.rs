//! Model facts for the AI render prompt, from Rust (DECISIONS D53).
//!
//! The image model sees a picture; the engine knows the building. These
//! lines tell the model what it is looking at: storeys, roofs, outside
//! walls, the rooms, doors, windows and furniture in view with their
//! finishes, the time of day of the view, and an explicit "do not move, add
//! or remove walls, doors or windows". What is "in view" comes from the ids
//! map the window drew with the capture (`MapsMeta::legend`); without one the
//! facts cover the whole house and give no counts.
//!
//! Nothing here reads the title block: license numbers never reach a
//! provider (D53). Names are the architect's own room and material names.

use std::collections::{BTreeMap, HashMap};

use guhit_model::*;

use super::drift::{Named, Weight};

/// Share of the ids map an element must cover to count as in view.
const IN_VIEW_SHARE: f64 = 0.0004;
/// At most this many rooms and furniture kinds are named.
const MAX_ROOMS: usize = 6;
const MAX_FURNITURE: usize = 10;

/// The constraint line, always in the facts.
pub const KEEP_LINE: &str = "Do not move, add or remove walls, doors or windows.";

/// What each element is, for the drift check and its note.
pub struct ElementIndex {
    entries: HashMap<Id, Entry>,
}

struct Entry {
    label: String,
    weight: Weight,
    key: bool,
}

fn material_name(project: &Project, id: Option<&Id>, fallback: &str) -> String {
    let wanted = id.map(String::as_str).unwrap_or(fallback);
    project
        .materials
        .iter()
        .find(|m| m.id == wanted)
        .map(|m| m.name.clone())
        .or_else(|| {
            defaults::default_materials()
                .into_iter()
                .find(|m| m.id == wanted)
                .map(|m| m.name)
        })
        .unwrap_or_default()
}

fn level_name(project: &Project, id: Option<&Id>) -> Option<String> {
    let id = id?;
    project
        .levels
        .iter()
        .find(|l| &l.id == id)
        .map(|l| l.name.clone())
}

fn on_level(label: String, level: Option<String>) -> String {
    match level {
        Some(l) if !l.trim().is_empty() => format!("{label} on the {}", l.trim()),
        _ => label,
    }
}

fn opening_kind(o: &Opening) -> &'static str {
    match o.opening_type {
        OpeningType::Door => "door",
        OpeningType::Window => "window",
    }
}

fn style_word(s: OpeningStyle) -> &'static str {
    match s {
        OpeningStyle::SwingSingle => "single swing",
        OpeningStyle::SwingDouble => "double swing",
        OpeningStyle::Sliding => "sliding",
        OpeningStyle::Fixed => "fixed",
        OpeningStyle::Casement => "casement",
        OpeningStyle::Jalousie => "jalousie",
    }
}

fn roof_word(s: RoofShape) -> &'static str {
    match s {
        RoofShape::Flat => "flat",
        RoofShape::Shed => "shed",
        RoofShape::Gable => "gable",
        RoofShape::Hip => "hip",
    }
}

impl ElementIndex {
    pub fn new(project: &Project, derived: &Derived) -> Self {
        let marks: HashMap<&Id, &str> = derived
            .opening_schedule
            .marks
            .iter()
            .map(|m| (&m.opening_id, m.mark.as_str()))
            .collect();
        let walls: HashMap<&Id, &Wall> = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Wall(w) => Some((&w.id, w)),
                _ => None,
            })
            .collect();
        let mut entries = HashMap::new();
        for e in &project.elements {
            let level = || level_name(project, e.level_id());
            let w = |priority: u8| Weight {
                priority,
                scored: true,
            };
            let entry = match e {
                Element::Opening(o) => {
                    let mark = marks
                        .get(&o.id)
                        .map(|m| format!(" {m}"))
                        .unwrap_or_default();
                    let lvl = walls
                        .get(&o.wall_id)
                        .and_then(|w| level_name(project, Some(&w.level_id)));
                    Entry {
                        label: on_level(format!("{}{mark}", opening_kind(o)), lvl),
                        weight: w(6),
                        key: true,
                    }
                }
                Element::Asset(a) => Entry {
                    label: a.name.to_lowercase(),
                    weight: Weight {
                        priority: 5,
                        scored: false,
                    },
                    key: false,
                },
                Element::Stair(_) => Entry {
                    label: on_level("the stair".into(), level()),
                    weight: w(4),
                    key: true,
                },
                Element::Column(_) => Entry {
                    label: on_level("a column".into(), level()),
                    weight: w(4),
                    key: true,
                },
                Element::Slab(s) => {
                    let what = match s.kind {
                        SlabKind::Balcony => "the balcony",
                        SlabKind::Deck => "the deck",
                        SlabKind::Void => "the void",
                        SlabKind::Floor => "a floor slab",
                    };
                    Entry {
                        label: on_level(what.into(), level()),
                        weight: w(3),
                        key: false,
                    }
                }
                Element::Roof(_) => Entry {
                    label: "the roof".into(),
                    weight: w(2),
                    key: false,
                },
                Element::Wall(wall) if wall.is_site() => Entry {
                    label: "the fence".into(),
                    weight: w(1),
                    key: false,
                },
                Element::Wall(_) => Entry {
                    label: on_level("a wall".into(), level()),
                    weight: w(1),
                    key: false,
                },
                Element::Room(r) => Entry {
                    label: format!("the {} floor", r.name.to_lowercase()),
                    weight: w(1),
                    key: false,
                },
                Element::Pipe(_)
                | Element::Annotation(_)
                | Element::Dimension(_)
                | Element::ReferenceModel(_) => Entry {
                    label: String::new(),
                    weight: Weight {
                        priority: 5,
                        scored: false,
                    },
                    key: false,
                },
                _ => continue,
            };
            entries.insert(e.id().clone(), entry);
        }
        Self { entries }
    }

    /// How the drift check counts an element.
    pub fn weight(&self, id: &str) -> Option<Weight> {
        self.entries.get(id).map(|e| e.weight)
    }

    /// How the drift note names an element.
    pub fn named(&self, id: &str) -> Option<Named> {
        self.entries
            .get(id)
            .filter(|e| !e.label.is_empty())
            .map(|e| Named {
                label: e.label.clone(),
                key: e.key,
            })
    }
}

/// What the facts are built from.
pub struct FactsInput<'a> {
    pub project: &'a Project,
    pub derived: &'a Derived,
    pub camera: &'a Camera,
    /// Element ids with the pixels they cover in the ids map, and the map's
    /// pixel count. None: what is in view is not known.
    pub in_view: Option<(&'a [(Id, u64)], u64)>,
    /// Add the "do not move" line. Off only when the architect turned
    /// "Keep geometry" off.
    pub keep_geometry: bool,
}

/// "4:30 pm" from minutes after midnight.
fn clock(minutes: u16) -> String {
    let (h, m) = ((minutes / 60) % 24, minutes % 60);
    let (h12, half) = match h {
        0 => (12, "am"),
        1..=11 => (h, "am"),
        12 => (12, "pm"),
        _ => (h - 12, "pm"),
    };
    format!("{h12}:{m:02} {half}")
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn light_line(light: &ViewLight) -> String {
    let month = MONTHS
        .get(light.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("March");
    let sky = match light.sky {
        SkyKind::Clear | SkyKind::Photo => "clear sky",
        SkyKind::Cloudy => "overcast sky",
    };
    let night = !(360..=1080).contains(&light.minutes);
    let lamps = match light.lamps {
        LampMode::On => "lamps lit",
        LampMode::Off => "lamps off",
        LampMode::Auto if night => "lamps lit",
        LampMode::Auto => "lamps off",
    };
    format!(
        "- Light: {} on {} {month}, {sky}, {lamps}.",
        clock(light.minutes),
        light.day
    )
}

fn storeys(project: &Project) -> usize {
    let with_walls: std::collections::BTreeSet<&Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if !w.is_site() => Some(&w.level_id),
            _ => None,
        })
        .collect();
    with_walls.len().max(1)
}

fn storey_words(n: usize) -> String {
    match n {
        1 => "single storey".to_string(),
        2 => "two storey".to_string(),
        3 => "three storey".to_string(),
        n => format!("{n} storey"),
    }
}

/// "1 sofa", "4 dining chairs".
fn counted(n: usize, name: &str) -> String {
    if n == 1 {
        format!("1 {name}")
    } else if name.ends_with('s') {
        format!("{n} {name}")
    } else {
        format!("{n} {name}s")
    }
}

fn is_furniture(c: AssetCategory) -> bool {
    matches!(
        c,
        AssetCategory::Furniture
            | AssetCategory::Sanitary
            | AssetCategory::Kitchen
            | AssetCategory::Appliance
            | AssetCategory::Plant
            | AssetCategory::Vehicle
            | AssetCategory::Lighting
            | AssetCategory::Site
    )
}

/// The facts, one line each, starting with a heading line.
pub fn describe(input: &FactsInput) -> String {
    let p = input.project;
    let mut lines = vec!["Model facts (from the architect's model, follow them):".to_string()];

    // The building and its roofs.
    let roofs: Vec<String> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Roof(r) => Some(format!(
                "a {} roof in {}",
                roof_word(r.shape),
                material_name(p, r.material_id.as_ref(), defaults::MAT_ROOF_DEFAULT).to_lowercase()
            )),
            _ => None,
        })
        .collect();
    let mut building = format!("- A {} house", storey_words(storeys(p)));
    if !roofs.is_empty() {
        let mut unique = roofs.clone();
        unique.dedup();
        building.push_str(&format!(" with {}", unique.join(" and ")));
    }
    building.push('.');
    lines.push(building);

    // Outside walls: the most used building wall finish.
    let mut wall_use: BTreeMap<String, usize> = BTreeMap::new();
    for e in &p.elements {
        if let Element::Wall(w) = e {
            if !w.is_site() {
                *wall_use
                    .entry(material_name(
                        p,
                        w.material_id.as_ref(),
                        defaults::MAT_WALL_DEFAULT,
                    ))
                    .or_default() += 1;
            }
        }
    }
    if let Some((name, _)) = wall_use.iter().max_by_key(|(_, n)| **n) {
        if !name.is_empty() {
            lines.push(format!("- Walls: {}.", name.to_lowercase()));
        }
    }

    // What is in view, when the ids map says.
    let in_view: Option<HashMap<&Id, u64>> = input.in_view.map(|(entries, total)| {
        let min = ((total as f64) * IN_VIEW_SHARE).max(1.0) as u64;
        entries
            .iter()
            .filter(|(_, px)| *px >= min)
            .map(|(id, px)| (id, *px))
            .collect()
    });
    let seen = |id: &Id| in_view.as_ref().is_none_or(|v| v.contains_key(id));

    let (mut windows, mut doors) = (vec![], vec![]);
    let mut furniture: BTreeMap<String, usize> = BTreeMap::new();
    let mut rooms = vec![];
    for e in &p.elements {
        match e {
            Element::Opening(o) if seen(&o.id) => match o.opening_type {
                OpeningType::Window => windows.push(o.style),
                OpeningType::Door => doors.push(o.style),
            },
            Element::Asset(a)
                if is_furniture(a.category)
                    && in_view.as_ref().is_some_and(|v| v.contains_key(&a.id)) =>
            {
                *furniture.entry(a.name.to_lowercase()).or_default() += 1;
            }
            Element::Room(r) if seen(&r.id) => rooms.push(r),
            _ => {}
        }
    }
    let styles = |list: &[OpeningStyle]| {
        let mut words: Vec<&str> = list.iter().map(|s| style_word(*s)).collect();
        words.sort_unstable();
        words.dedup();
        words.join(", ")
    };
    if in_view.is_some() {
        let mut parts = vec![];
        if !windows.is_empty() {
            parts.push(format!(
                "{} ({})",
                counted(windows.len(), "window"),
                styles(&windows)
            ));
        }
        if !doors.is_empty() {
            parts.push(format!(
                "{} ({})",
                counted(doors.len(), "door"),
                styles(&doors)
            ));
        }
        if !parts.is_empty() {
            lines.push(format!("- In view: {}.", parts.join(" and ")));
        }
    }
    if !rooms.is_empty() {
        let named: Vec<String> = rooms
            .iter()
            .take(MAX_ROOMS)
            .map(|r| {
                let floor =
                    material_name(p, r.floor_material_id.as_ref(), defaults::MAT_FLOOR_DEFAULT)
                        .to_lowercase();
                match r.finishes.wall_material_id.as_ref() {
                    Some(w) => format!(
                        "{} (floor {floor}, walls {})",
                        r.name,
                        material_name(p, Some(w), "").to_lowercase()
                    ),
                    None => format!("{} (floor {floor})", r.name),
                }
            })
            .collect();
        let heading = if in_view.is_some() {
            "Rooms in view"
        } else {
            "Rooms"
        };
        lines.push(format!("- {heading}: {}.", named.join(", ")));
    }
    if !furniture.is_empty() {
        let mut items: Vec<(String, usize)> = furniture.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let named: Vec<String> = items
            .iter()
            .take(MAX_FURNITURE)
            .map(|(n, c)| counted(*c, n))
            .collect();
        lines.push(format!("- Furniture in view: {}.", named.join(", ")));
    }
    if let Some(light) = &input.camera.light {
        lines.push(light_line(light));
    }

    // The constraint, with the counts when they are known.
    if !input.keep_geometry {
        return lines.join("\n");
    }
    let mut keep = format!("- {KEEP_LINE}");
    if in_view.is_some() && (!windows.is_empty() || !doors.is_empty()) {
        let mut counts = vec![];
        if !windows.is_empty() {
            counts.push(counted(windows.len(), "window"));
        }
        if !doors.is_empty() {
            counts.push(counted(doors.len(), "door"));
        }
        keep.push_str(&format!(" Keep exactly {} in view.", counts.join(" and ")));
    }
    lines.push(keep);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(light: Option<ViewLight>) -> Camera {
        Camera {
            id: "c".into(),
            name: "Front".into(),
            preset: CameraPreset::ExteriorCorner,
            position: Vec3 {
                x: 0.0,
                y: -9000.0,
                z: 1600.0,
            },
            target: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1600.0,
            },
            fov_deg: 45.0,
            light,
        }
    }

    fn bungalow() -> (Project, Derived) {
        let p = guhit_core::templates::sample_bungalow();
        let doc = guhit_core::Document::new(p);
        (doc.project().clone(), doc.derived().clone())
    }

    #[test]
    fn the_whole_house_without_an_ids_map() {
        let (p, d) = bungalow();
        let text = describe(&FactsInput {
            project: &p,
            derived: &d,
            camera: &camera(None),
            in_view: None,
            keep_geometry: true,
        });
        assert!(text.starts_with("Model facts"));
        assert!(text.contains("single storey house"), "{text}");
        assert!(text.contains("- Rooms:"), "{text}");
        assert!(text.contains(KEEP_LINE));
        assert!(
            !text.contains("Keep exactly"),
            "no counts without the ids map"
        );
        assert!(!text.contains("In view"));
    }

    #[test]
    fn what_is_in_view_is_counted() {
        let (p, d) = bungalow();
        let windows: Vec<Id> = p
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.opening_type == OpeningType::Window => Some(o.id.clone()),
                _ => None,
            })
            .take(2)
            .collect();
        assert_eq!(windows.len(), 2, "the bungalow has windows");
        let seen: Vec<(Id, u64)> = windows
            .iter()
            .map(|w| (w.clone(), 5000))
            .chain([("nothing".to_string(), 9)])
            .collect();
        let light = ViewLight {
            month: 3,
            day: 21,
            minutes: 16 * 60 + 30,
            sky: SkyKind::Clear,
            exposure_ev: None,
            lamps: LampMode::Auto,
        };
        let text = describe(&FactsInput {
            project: &p,
            derived: &d,
            camera: &camera(Some(light)),
            in_view: Some((&seen, 1_000_000)),
            keep_geometry: true,
        });
        assert!(text.contains("- In view: 2 windows"), "{text}");
        assert!(text.contains("Keep exactly 2 windows in view."), "{text}");
        assert!(
            text.contains("- Light: 4:30 pm on 21 March, clear sky, lamps off."),
            "{text}"
        );
    }

    #[test]
    fn the_title_block_never_reaches_the_prompt() {
        let (mut p, d) = bungalow();
        p.title_block.signatory.prc_no = "0123456".into();
        p.title_block.signatory.tin = "999-888-777".into();
        let text = describe(&FactsInput {
            project: &p,
            derived: &d,
            camera: &camera(None),
            in_view: None,
            keep_geometry: true,
        });
        assert!(!text.contains("0123456") && !text.contains("999-888-777"));
    }

    #[test]
    fn openings_outrank_walls_and_furniture_does_not_count() {
        let (p, d) = bungalow();
        let index = ElementIndex::new(&p, &d);
        let opening = p
            .elements
            .iter()
            .find(|e| matches!(e, Element::Opening(_)))
            .expect("an opening");
        let wall = p
            .elements
            .iter()
            .find(|e| matches!(e, Element::Wall(_)))
            .expect("a wall");
        let (o, w) = (
            index.weight(opening.id()).expect("o"),
            index.weight(wall.id()).expect("w"),
        );
        assert!(o.priority > w.priority && o.scored && w.scored);
        let named = index.named(opening.id()).expect("named");
        assert!(
            named.key && (named.label.starts_with("window") || named.label.starts_with("door")),
            "{}",
            named.label
        );
        if let Some(asset) = p.elements.iter().find(|e| matches!(e, Element::Asset(_))) {
            assert!(!index.weight(asset.id()).expect("asset").scored);
        }
    }

    #[test]
    fn the_clock_reads_like_a_person_says_it() {
        assert_eq!(clock(0), "12:00 am");
        assert_eq!(clock(9 * 60 + 5), "9:05 am");
        assert_eq!(clock(12 * 60), "12:00 pm");
        assert_eq!(clock(18 * 60 + 45), "6:45 pm");
    }
}
