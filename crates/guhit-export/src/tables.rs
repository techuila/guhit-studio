//! Schedules as sheet tables: doors and windows by mark, rooms, finishes and
//! areas, from `Derived::opening_schedule`, `room_schedule` and
//! `level_areas`, never counted again here. LANE L4 owns this module.
//!
//! A table takes its natural width from its text. When it is taller than the
//! room it has, it continues in a second (third...) column to the right,
//! with the header repeated; rows are never dropped.

use guhit_model::{Derived, Id, OpeningStyle, OpeningType, Project, RoomUsage, ScheduleKind};

use crate::geom::v;
use crate::plan::{Cat, HAlign, Item, Pen, Prim};
use crate::sheet::{num, INK, MUTED};
use crate::text::{clean, est_width, xml_escape};

/// Light rules between rows and columns.
const RULE: &str = "#b9c2cd";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone)]
pub struct Col {
    /// A newline splits it into two lines.
    pub head: String,
    pub align: Align,
}

/// A schedule ready to print.
#[derive(Debug, Clone)]
pub struct SheetTable {
    /// "SCHEDULE OF DOORS AND WINDOWS": the viewport title.
    pub title: String,
    pub columns: Vec<Col>,
    pub rows: Vec<Vec<String>>,
    pub total: Option<Vec<String>>,
    /// Lines under the table.
    pub notes: Vec<String>,
    /// Printed in place of the table when there are no rows.
    pub empty: String,
}

/// A table drawn at the origin, and its size in paper mm.
pub struct Rendered {
    pub svg: String,
    pub w: f64,
    pub h: f64,
}

/// The title a schedule viewport gets.
pub fn schedule_title(kind: ScheduleKind) -> &'static str {
    match kind {
        ScheduleKind::DoorsWindows => "SCHEDULE OF DOORS AND WINDOWS",
        ScheduleKind::Rooms => "ROOM SCHEDULE",
        ScheduleKind::Finishes => "SCHEDULE OF FINISHES",
        ScheduleKind::Areas => "FLOOR AREAS",
    }
}

fn col(head: &str, align: Align) -> Col {
    Col { head: head.into(), align }
}

fn level_name(p: &Project, id: &Id) -> String {
    p.levels.iter().find(|l| &l.id == id).map(|l| clean(&l.name)).unwrap_or_default()
}

fn level_order(p: &Project, id: &Id) -> usize {
    p.levels.iter().position(|l| &l.id == id).unwrap_or(usize::MAX)
}

fn material(p: &Project, id: &Option<Id>) -> String {
    id.as_ref()
        .and_then(|id| p.materials.iter().find(|m| &m.id == id))
        .map(|m| clean(&m.name))
        .unwrap_or_default()
}

fn mm(v: f64) -> String {
    format!("{}", v.round() as i64)
}

/// Two decimals; a value that rounds to zero prints "0.00", never "-0.00".
fn m2(v: f64) -> String {
    let v = if v.abs() < 0.005 || !v.is_finite() { 0.0 } else { v };
    format!("{v:.2}")
}

/// "Door, single swing".
pub fn opening_kind(t: OpeningType, s: OpeningStyle) -> String {
    let what = match t {
        OpeningType::Door => "Door",
        OpeningType::Window => "Window",
    };
    let style = match s {
        OpeningStyle::SwingSingle => "single swing",
        OpeningStyle::SwingDouble => "double swing",
        OpeningStyle::Sliding => "sliding",
        OpeningStyle::Fixed => "fixed",
        OpeningStyle::Casement => "casement",
        OpeningStyle::Jalousie => "jalousie",
    };
    format!("{what}, {style}")
}

pub fn usage_name(u: RoomUsage) -> &'static str {
    match u {
        RoomUsage::Living => "Living",
        RoomUsage::Dining => "Dining",
        RoomUsage::Kitchen => "Kitchen",
        RoomUsage::Bedroom => "Bedroom",
        RoomUsage::MasterBedroom => "Master bedroom",
        RoomUsage::Bathroom => "Toilet and bath",
        RoomUsage::PowderRoom => "Powder room",
        RoomUsage::Laundry => "Laundry",
        RoomUsage::Garage => "Garage",
        RoomUsage::Porch => "Porch",
        RoomUsage::Hallway => "Hallway",
        RoomUsage::Storage => "Storage",
        RoomUsage::Office => "Office",
        RoomUsage::Other => "Other",
    }
}

/// The table of one schedule, from `Derived`.
pub fn schedule(p: &Project, d: &Derived, kind: ScheduleKind) -> SheetTable {
    let title = schedule_title(kind).to_string();
    match kind {
        ScheduleKind::DoorsWindows => {
            // Levels that have any opening, in project order.
            let mut levels: Vec<Id> = Vec::new();
            for l in &p.levels {
                if d.opening_schedule.rows.iter().any(|r| r.per_level.iter().any(|c| c.level_id == l.id && c.count > 0)) {
                    levels.push(l.id.clone());
                }
            }
            let mut columns = vec![
                col("MARK", Align::Center),
                col("TYPE", Align::Left),
                col("WIDTH\n(mm)", Align::Right),
                col("HEIGHT\n(mm)", Align::Right),
                col("SILL\n(mm)", Align::Right),
                col("MATERIAL", Align::Left),
                col("DESCRIPTION", Align::Left),
            ];
            for l in &levels {
                columns.push(col(&format!("{}\n(SETS)", level_name(p, l).to_uppercase()), Align::Right));
            }
            columns.push(col("TOTAL\n(SETS)", Align::Right));
            let rows: Vec<Vec<String>> = d
                .opening_schedule
                .rows
                .iter()
                .map(|r| {
                    let sill = match (r.opening_type, r.sill_mm) {
                        (OpeningType::Door, Some(s)) if s.abs() < 0.5 => "-".to_string(),
                        (_, Some(s)) => mm(s),
                        (_, None) => "VARIES".into(),
                    };
                    let mut row = vec![
                        if r.mark.trim().is_empty() { "-".into() } else { clean(&r.mark) },
                        opening_kind(r.opening_type, r.style),
                        mm(r.width_mm),
                        mm(r.height_mm),
                        sill,
                        material(p, &r.material_id),
                        clean(&r.description),
                    ];
                    for l in &levels {
                        let n: u32 = r.per_level.iter().filter(|c| &c.level_id == l).map(|c| c.count).sum();
                        row.push(if n == 0 { String::new() } else { n.to_string() });
                    }
                    row.push(r.total.to_string());
                    row
                })
                .collect();
            let total = (!rows.is_empty()).then(|| {
                let mut t = vec![String::new(); columns.len()];
                t[1] = "TOTAL".into();
                for (i, l) in levels.iter().enumerate() {
                    let n: u32 = d
                        .opening_schedule
                        .rows
                        .iter()
                        .flat_map(|r| r.per_level.iter())
                        .filter(|c| &c.level_id == l)
                        .map(|c| c.count)
                        .sum();
                    t[7 + i] = n.to_string();
                }
                let all: u32 = d.opening_schedule.rows.iter().map(|r| r.total).sum();
                *t.last_mut().expect("a total column") = all.to_string();
                t
            });
            let mut notes = Vec::new();
            if d.opening_schedule.rows.iter().any(|r| r.mark.trim().is_empty()) {
                notes.push("Rows marked - are openings without a type. Assign types to give them marks.".into());
            }
            SheetTable {
                title,
                columns,
                rows,
                total,
                notes,
                empty: "No doors or windows in the model yet.".into(),
            }
        }
        ScheduleKind::Rooms => {
            let mut list: Vec<_> = d.room_schedule.iter().collect();
            list.sort_by_key(|r| level_order(p, &r.level_id));
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|r| {
                    vec![
                        level_name(p, &r.level_id),
                        clean(&r.name),
                        usage_name(r.usage).into(),
                        m2(r.net_area_m2),
                        m2(r.floor_area_m2),
                        m2(r.perimeter_m),
                        mm(r.ceiling_height_mm),
                    ]
                })
                .collect();
            // Each column adds up: the floor areas to the floor area total.
            let total = (!rows.is_empty()).then(|| {
                let net: f64 = list.iter().map(|r| r.net_area_m2).sum();
                let floor: f64 = list.iter().map(|r| r.floor_area_m2).sum();
                vec![String::new(), "TOTAL".into(), String::new(), m2(net), m2(floor), String::new(), String::new()]
            });
            SheetTable {
                title,
                columns: vec![
                    col("LEVEL", Align::Left),
                    col("ROOM", Align::Left),
                    col("USE", Align::Left),
                    col("NET AREA\n(m\u{00b2})", Align::Right),
                    col("FLOOR AREA\n(m\u{00b2})", Align::Right),
                    col("PERIMETER\n(m)", Align::Right),
                    col("CEILING HT.\n(mm)", Align::Right),
                ],
                rows,
                total,
                notes: vec!["Net area is inside the walls, less voids. Floor area also leaves out balconies and decks, as the floor areas count it.".into()],
                empty: "No rooms in the model yet.".into(),
            }
        }
        ScheduleKind::Finishes => {
            let mut list: Vec<_> = d.room_schedule.iter().collect();
            list.sort_by_key(|r| level_order(p, &r.level_id));
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|r| {
                    vec![
                        level_name(p, &r.level_id),
                        clean(&r.name),
                        material(p, &r.floor_material_id),
                        material(p, &r.finishes.base_material_id),
                        material(p, &r.finishes.wall_material_id),
                        material(p, &r.finishes.ceiling_material_id),
                        mm(r.ceiling_height_mm),
                        clean(&r.finishes.remarks),
                    ]
                })
                .collect();
            SheetTable {
                title,
                columns: vec![
                    col("LEVEL", Align::Left),
                    col("ROOM", Align::Left),
                    col("FLOOR", Align::Left),
                    col("BASE", Align::Left),
                    col("WALLS", Align::Left),
                    col("CEILING", Align::Left),
                    col("CEILING HT.\n(mm)", Align::Right),
                    col("REMARKS", Align::Left),
                ],
                rows,
                total: None,
                notes: vec![],
                empty: "No rooms in the model yet.".into(),
            }
        }
        ScheduleKind::Areas => {
            let mut list: Vec<_> = d.level_areas.iter().collect();
            list.sort_by_key(|r| level_order(p, &r.level_id));
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|r| vec![level_name(p, &r.level_id), m2(r.net_m2), m2(r.gross_m2), m2(r.open_m2), m2(r.void_m2)])
                .collect();
            let total = (!rows.is_empty()).then(|| {
                let sum = |f: fn(&guhit_model::LevelAreas) -> f64| m2(list.iter().map(|r| f(r)).sum());
                vec!["TOTAL".into(), sum(|r| r.net_m2), sum(|r| r.gross_m2), sum(|r| r.open_m2), sum(|r| r.void_m2)]
            });
            let mut notes = vec!["Balconies and decks are open floor, not floor area. Voids are taken off.".to_string()];
            if let Some(site) = &d.site {
                notes.push(format!(
                    "Lot area {} m\u{00b2}; the ground floor covers {:.1}% of the lot.",
                    m2(site.area_m2),
                    site.coverage_pct
                ));
            }
            SheetTable {
                title,
                columns: vec![
                    col("LEVEL", Align::Left),
                    col("NET FLOOR AREA\n(m\u{00b2})", Align::Right),
                    col("GROSS FLOOR AREA\n(m\u{00b2})", Align::Right),
                    col("BALCONIES, DECKS\n(m\u{00b2})", Align::Right),
                    col("VOIDS\n(m\u{00b2})", Align::Right),
                ],
                rows,
                total,
                notes,
                empty: "No levels with floor area yet.".into(),
            }
        }
    }
}

/// Rows `from..to` of a table, for a sheet that prints part of it. The
/// total and the notes go with the last part; a part the table continues
/// after says on which sheet.
pub fn part(t: &SheetTable, from: usize, to: usize, continued_on: Option<&str>) -> SheetTable {
    let to = to.min(t.rows.len());
    let from = from.min(to);
    let last = to >= t.rows.len();
    SheetTable {
        title: t.title.clone(),
        columns: t.columns.clone(),
        rows: t.rows[from..to].to_vec(),
        total: if last { t.total.clone() } else { None },
        notes: match (last, continued_on) {
            (true, _) => t.notes.clone(),
            (false, Some(next)) => vec![format!("Continued on sheet {}.", clean(next))],
            (false, None) => vec![],
        },
        empty: t.empty.clone(),
    }
}

/// Type sizes of a table at type scale `k`.
struct Metrics {
    font: f64,
    head_font: f64,
    cpad: f64,
    row_h: f64,
    line_h: f64,
    header_h: f64,
}

fn metrics(t: &SheetTable, k: f64) -> Metrics {
    let font = 1.8 * k;
    let head_font = 1.4 * k;
    let lines = t.columns.iter().map(|c| c.head.lines().count()).max().unwrap_or(1).max(1);
    let line_h = head_font * 1.3;
    Metrics {
        font,
        head_font,
        cpad: 1.2 * k,
        row_h: 4.4 * k,
        line_h,
        header_h: lines as f64 * line_h + 2.0 * k,
    }
}

/// Natural column widths.
fn widths(t: &SheetTable, m: &Metrics) -> Vec<f64> {
    t.columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let head = c.head.lines().map(|l| est_width(l, m.head_font, true)).fold(0.0, f64::max);
            let cells = t
                .rows
                .iter()
                .map(|r| r.get(i).map(|x| est_width(x, m.font, false)).unwrap_or(0.0))
                .chain(t.total.iter().map(|r| r.get(i).map(|x| est_width(x, m.font, true)).unwrap_or(0.0)))
                .fold(0.0, f64::max);
            head.max(cells) + 2.0 * m.cpad
        })
        .collect()
}

/// The size the table takes at type scale `k` when it may be `max_h` tall.
pub fn size(t: &SheetTable, k: f64, max_h: f64) -> (f64, f64) {
    let r = render(t, k, max_h);
    (r.w, r.h)
}

/// Draw the table with its top left corner at the origin. Rows that do not
/// fit under `max_h` continue in another column to the right.
pub fn render(t: &SheetTable, k: f64, max_h: f64) -> Rendered {
    let m = metrics(t, k);
    let mut s = String::from("<g class=\"schedule-table\">\n");
    if t.rows.is_empty() {
        let w = est_width(&t.empty, m.font, false) + 4.0 * m.cpad;
        let h = m.row_h * 2.0;
        s.push_str(&rect(0.0, 0.0, w, h, 0.18, INK));
        s.push_str(&text(2.0 * m.cpad, h / 2.0 + 0.36 * m.font, m.font, "start", false, MUTED, &t.empty));
        s.push_str("</g>\n");
        return Rendered { svg: s, w, h };
    }
    let widths = widths(t, &m);
    let table_w: f64 = widths.iter().sum();
    let note_size = 1.5 * k;
    let notes_h = if t.notes.is_empty() { 0.0 } else { t.notes.len() as f64 * 2.4 * k + 1.0 * k };
    let total_h = if t.total.is_some() { m.row_h } else { 0.0 };
    // Rows per column block.
    let room = (max_h - m.header_h - total_h - notes_h).max(m.row_h);
    let per = ((room / m.row_h).floor() as usize).max(1);
    let chunks: Vec<&[Vec<String>]> = t.rows.chunks(per).collect();
    let gap = 6.0 * k;
    let mut height: f64 = 0.0;
    for (ci, chunk) in chunks.iter().enumerate() {
        let x0 = ci as f64 * (table_w + gap);
        let last = ci + 1 == chunks.len();
        let x_of = |i: usize| x0 + widths[..i].iter().sum::<f64>();
        // Header.
        let head_lines = t.columns.iter().map(|c| c.head.lines().count()).max().unwrap_or(1);
        for (i, c) in t.columns.iter().enumerate() {
            let lines: Vec<&str> = c.head.lines().collect();
            for (j, l) in lines.iter().enumerate() {
                let ly = 1.0 * k + (j + 1 + head_lines - lines.len()) as f64 * m.line_h - 0.2 * m.head_font;
                let (tx, anchor) = anchor_at(c.align, x_of(i), widths[i], m.cpad);
                s.push_str(&text(tx, ly, m.head_font, anchor, true, INK, l));
            }
        }
        let mut y = m.header_h;
        s.push_str(&line((x0, y), (x0 + table_w, y), 0.25, INK));
        for (ri, r) in chunk.iter().enumerate() {
            for (i, c) in t.columns.iter().enumerate() {
                let cell = r.get(i).map(|x| x.as_str()).unwrap_or("");
                let (tx, anchor) = anchor_at(c.align, x_of(i), widths[i], m.cpad);
                s.push_str(&text(tx, y + m.row_h / 2.0 + 0.36 * m.font, m.font, anchor, false, INK, cell));
            }
            y += m.row_h;
            if ri + 1 < chunk.len() {
                s.push_str(&line((x0, y), (x0 + table_w, y), 0.1, RULE));
            }
        }
        if last {
            if let Some(total) = &t.total {
                s.push_str(&line((x0, y), (x0 + table_w, y), 0.25, INK));
                for (i, c) in t.columns.iter().enumerate() {
                    let cell = total.get(i).map(|x| x.as_str()).unwrap_or("");
                    let (tx, anchor) = anchor_at(c.align, x_of(i), widths[i], m.cpad);
                    s.push_str(&text(tx, y + m.row_h / 2.0 + 0.36 * m.font, m.font, anchor, true, INK, cell));
                }
                y += m.row_h;
            }
        }
        for i in 1..t.columns.len() {
            s.push_str(&line((x_of(i), 0.0), (x_of(i), y), 0.1, RULE));
        }
        s.push_str(&rect(x0, 0.0, table_w, y, 0.25, INK));
        height = height.max(y);
    }
    let mut h = height;
    if !t.notes.is_empty() {
        h += 1.0 * k;
        for n in &t.notes {
            h += 2.4 * k;
            s.push_str(&text(0.0, h - 0.5 * k, note_size, "start", false, MUTED, n));
        }
    }
    let notes_w = t.notes.iter().map(|n| est_width(n, note_size, false)).fold(0.0, f64::max);
    let w = (chunks.len() as f64 * (table_w + gap) - gap).max(notes_w);
    s.push_str("</g>\n");
    Rendered { svg: s, w, h }
}

/// The table as drawing items in one block, model mm at 1:`n`, its top
/// left corner at the origin and y up: the sheets DXF draws schedules from
/// this. Rules go on `Cat::Symb`, text on `Cat::Text`. A DXF has no paper
/// to fit, so it never continues in a second column; right aligned cells
/// are centered, since CAD fonts measure text their own way.
pub fn items(t: &SheetTable, k: f64, n: f64) -> Vec<Item> {
    let m = metrics(t, k);
    let at = |x: f64, y: f64| v(x * n, -y * n);
    let mut out: Vec<Item> = Vec::new();
    let rule = |out: &mut Vec<Item>, a: (f64, f64), b: (f64, f64), pen: Pen| {
        out.push(Item { cat: Cat::Symb, pen, prim: Prim::Line { a: at(a.0, a.1), b: at(b.0, b.1) } });
    };
    let write = |out: &mut Vec<Item>, x: f64, y: f64, size: f64, align: HAlign, bold: bool, s: &str| {
        let s = clean(s);
        if !s.is_empty() {
            out.push(Item { cat: Cat::Text, pen: Pen::Light, prim: Prim::Text { pos: at(x, y), height: size * n, rot_deg: 0.0, align, text: s, bold } });
        }
    };
    let rect = |out: &mut Vec<Item>, w: f64, h: f64| {
        let c = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        for i in 0..4 {
            rule(out, c[i], c[(i + 1) % 4], Pen::Medium);
        }
    };
    if t.rows.is_empty() {
        let w = est_width(&t.empty, m.font, false) + 4.0 * m.cpad;
        rect(&mut out, w, m.row_h * 2.0);
        write(&mut out, 2.0 * m.cpad, m.row_h + 0.36 * m.font, m.font, HAlign::Start, false, &t.empty);
        return out;
    }
    // CAD fonts run wider than the sheet's: a third more room per column.
    let widths: Vec<f64> = widths(t, &m).iter().map(|w| w * 1.35).collect();
    let table_w: f64 = widths.iter().sum();
    let x_of = |i: usize| widths[..i].iter().sum::<f64>();
    let cell = |out: &mut Vec<Item>, i: usize, y: f64, size: f64, bold: bool, s: &str| match t.columns[i].align {
        Align::Left => write(out, x_of(i) + m.cpad, y, size, HAlign::Start, bold, s),
        Align::Right | Align::Center => write(out, x_of(i) + widths[i] / 2.0, y, size, HAlign::Middle, bold, s),
    };
    let head_lines = t.columns.iter().map(|c| c.head.lines().count()).max().unwrap_or(1);
    for (i, c) in t.columns.iter().enumerate() {
        let lines: Vec<&str> = c.head.lines().collect();
        for (j, l) in lines.iter().enumerate() {
            let ly = 1.0 * k + (j + 1 + head_lines - lines.len()) as f64 * m.line_h - 0.2 * m.head_font;
            cell(&mut out, i, ly, m.head_font, true, l);
        }
    }
    let mut y = m.header_h;
    rule(&mut out, (0.0, y), (table_w, y), Pen::Medium);
    for (ri, r) in t.rows.iter().enumerate() {
        for i in 0..t.columns.len() {
            cell(&mut out, i, y + m.row_h / 2.0 + 0.36 * m.font, m.font, false, r.get(i).map(|x| x.as_str()).unwrap_or(""));
        }
        y += m.row_h;
        if ri + 1 < t.rows.len() {
            rule(&mut out, (0.0, y), (table_w, y), Pen::Fine);
        }
    }
    if let Some(total) = &t.total {
        rule(&mut out, (0.0, y), (table_w, y), Pen::Medium);
        for i in 0..t.columns.len() {
            cell(&mut out, i, y + m.row_h / 2.0 + 0.36 * m.font, m.font, true, total.get(i).map(|x| x.as_str()).unwrap_or(""));
        }
        y += m.row_h;
    }
    for i in 1..t.columns.len() {
        rule(&mut out, (x_of(i), 0.0), (x_of(i), y), Pen::Fine);
    }
    rect(&mut out, table_w, y);
    let mut h = y + 1.0 * k;
    for note in &t.notes {
        h += 2.4 * k;
        write(&mut out, 0.0, h - 0.5 * k, 1.5 * k, HAlign::Start, false, note);
    }
    out
}

fn anchor_at(a: Align, x: f64, w: f64, pad: f64) -> (f64, &'static str) {
    match a {
        Align::Left => (x + pad, "start"),
        Align::Right => (x + w - pad, "end"),
        Align::Center => (x + w / 2.0, "middle"),
    }
}

fn text(x: f64, y: f64, size: f64, anchor: &str, bold: bool, color: &str, t: &str) -> String {
    if t.is_empty() {
        return String::new();
    }
    format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"{anchor}\"{} fill=\"{color}\" stroke=\"none\">{}</text>\n",
        num(x),
        num(y),
        num(size),
        if bold { " font-weight=\"bold\"" } else { "" },
        xml_escape(t)
    )
}

fn line(a: (f64, f64), b: (f64, f64), w: f64, color: &str) -> String {
    format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{}\"/>\n",
        num(a.0),
        num(a.1),
        num(b.0),
        num(b.1),
        num(w)
    )
}

fn rect(x: f64, y: f64, w: f64, h: f64, sw: f64, color: &str) -> String {
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\"/>\n",
        num(x),
        num(y),
        num(w),
        num(h),
        num(sw)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: usize) -> SheetTable {
        SheetTable {
            title: "T".into(),
            columns: vec![col("ROOM", Align::Left), col("AREA\n(m2)", Align::Right)],
            rows: (0..rows).map(|i| vec![format!("Room {i}"), "12.00".into()]).collect(),
            total: Some(vec!["TOTAL".into(), "99.00".into()]),
            notes: vec!["A note.".into()],
            empty: "Nothing.".into(),
        }
    }

    #[test]
    fn tall_tables_continue_in_another_column_and_keep_every_row() {
        let short = render(&table(5), 1.0, 200.0);
        let tall = render(&table(80), 1.0, 120.0);
        assert!(tall.h <= 120.0 + 1e-6, "{}", tall.h);
        assert!(tall.w > short.w * 2.0, "{} vs {}", tall.w, short.w);
        for i in 0..80 {
            assert!(tall.svg.contains(&format!(">Room {i}<")), "row {i} missing");
        }
        assert_eq!(tall.svg.matches(">TOTAL<").count(), 1);
    }

    #[test]
    fn an_empty_schedule_says_so() {
        let r = render(&table(0), 1.0, 100.0);
        assert!(r.svg.contains("Nothing."));
    }
}
