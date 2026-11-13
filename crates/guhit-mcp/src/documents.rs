//! The `documents` toolset (DECISIONS D53, D54): the PH title block, the
//! sheet set, section and elevation marks, and the lot from its title.
//!
//! Edits run through `stage` like every edit. Only people set the signing
//! architect's name and license numbers: `title_block` reads them masked and
//! never writes them (D53, RA 10173), and every sheet picture an agent gets
//! prints them masked.

use std::collections::BTreeMap;

use base64::Engine as _;
use guhit_app::AppService;
use guhit_export::SheetInputs;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fail::ToolFail;
use crate::geo::{self, pt};
use crate::stage;
use crate::tools::{obj, scope_schema, Output, ToolDef, MM};
use crate::translate::{find, find_level, parse, pick_level, point, positive, positive_opt, Plan, Primary, Then};

// ------------------------------------------------------------- definitions

fn pt_schema() -> Value {
    json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}, "required": ["x", "y"]})
}

fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn e(values: &[&str], desc: &str) -> Value {
    json!({"type": "string", "enum": values, "description": desc})
}

fn def(name: &'static str, description: String, schema: Value) -> ToolDef {
    ToolDef { name, description, schema, read_only: false, open_world: false }
}

const PAPERS: [&str; 5] = ["a0", "a1", "a2", "a3", "a4"];

pub fn definitions() -> Vec<ToolDef> {
    let mut marks = obj(json!({
        "action": e(&["add", "list", "delete"], ""),
        "kind": e(&["section", "elevation"], "add."),
        "label": s("add: \"A\" for a section, \"North\" or \"Front\" for an elevation."),
        "start": pt_schema(), "end": pt_schema(),
        "flip": {"type": "boolean", "description": "false: it looks to the left going start to end."},
        "depth_mm": {"type": "number", "description": "How far beyond is drawn. Default everything."},
        "level": s("The level it is drawn on. Default the first."),
        "ids": {"type": "array", "items": {"type": "string"}, "description": "delete."},
    }), &["action"]);
    marks["properties"]["scope"] = scope_schema();
    vec![
        def(
            "title_block",
            format!("The PH title block every sheet prints (IRR of PD 1096). action get: its fields, the owner and location from set_project settings, and the signing architect MASKED. set: project_title, lot_no, block_no, tct_no, firm_name, firm_address, firm_logo, one undo step. Only people set the architect's name and license numbers (PRC, PTR, IAPOA, TIN): ask the user to fill them in the app. {MM}"),
            obj(json!({
                "action": e(&["get", "set"], ""),
                "project_title": s(""), "lot_no": s(""), "block_no": s(""), "tct_no": s(""),
                "firm_name": s(""), "firm_address": s(""),
                "firm_logo": s("An image already in the project's files."),
            }), &["action"]),
        ),
        def(
            "sheets",
            format!("The permit sheet set. make (paper, replace): adds missing elevation and section marks, then A-1 site and vicinity, plans, elevations, sections, roof plan, reflected ceiling plans, schedules and the services sheets, one undo step. list: sheets with their viewports and revisions. preview (sheet: number or id): the sheet as a picture, license numbers masked. revise (mark, date, description, sheets): a revision on those sheets. export (format pdf, svg or dxf; sheets, default all): one PDF for the whole set. delete (sheets). vicinity: places the vicinity map image on every sheet that frames one (A-1 after make), or on sheets: image (a PNG or JPEG data URL, stored in the project's files) or file_name (an image already there), radius_m (default 500), one undo step. No map service: the user gives the picture. {MM}"),
            obj(json!({
                "action": e(&["make", "list", "preview", "revise", "export", "delete", "vicinity"], ""),
                "paper": e(&PAPERS, "make. Default a1."),
                "replace": {"type": "boolean", "description": "make: drop the sheets there are first."},
                "sheet": s("preview: A-1 or an id."),
                "sheets": {"type": "array", "items": {"type": "string"}, "description": "Numbers or ids."},
                "mark": s("revise: \"1\", \"A\". Default the next."),
                "date": s("revise: YYYY-MM-DD. Default today."),
                "description": s("revise."),
                "format": e(&["pdf", "svg", "dxf"], "export. Default pdf."),
                "width_px": {"type": "integer", "description": "preview: long side, 400 to 1568."},
                "image": s("vicinity: a PNG or JPEG data URL (data:image/png;base64,...). Not in a batch step: there, give file_name."),
                "file_name": s("vicinity: an image already in the project's files."),
                "radius_m": {"type": "number", "description": "vicinity: the radius note, m. Default the one there, else 500."},
            }), &["action"]),
        ),
        def(
            "view_marks",
            format!("Section and elevation marks on the plan: where a section cuts or an elevation looks from. Each one's drawing is worked out on demand: look mode section or elevation with the mark, and sheets puts it on a sheet. action add, list, delete; one undo step per edit. sheets action make adds four elevations facing true north and two sections if missing. {MM}"),
            marks,
        ),
        def(
            "set_lot",
            format!("The lot from its title (TCT), one undo step, replacing the lot there is. description: the technical description as written (\"... thence N. 45 deg. 30' E., 12.00 m. to point 2; ...\"; the tie line to the BLLM before the first \"thence\" is left out), or legs [{{bearing, distance_mm}}], from start (default 0, 0). The result states the misclosure. sides: role road|side|rear, setback_mm (drawn, never a code check), road, road_width_mm, one per leg. {MM}"),
            obj(json!({
                "description": s("The TCT technical description."),
                "legs": {"type": "array", "items": {"type": "object", "properties": {"bearing": {"type": "string"}, "distance_mm": {"type": "number"}}, "required": ["bearing", "distance_mm"]}},
                "start": pt_schema(),
                "sides": {"type": "array", "items": {"type": "object", "properties": {"role": {"type": "string", "enum": ["road", "side", "rear"]}, "setback_mm": {"type": "number"}, "road": {"type": "string"}, "road_width_mm": {"type": "number"}}, "required": ["role"]}},
                "grade_mm": {"type": "number", "description": "Finish grade below the lowest floor. Default 150."},
            }), &[]),
        ),
    ]
}

/// The reads of this toolset, which answer on their own; None for the
/// edits, which run through `stage`.
pub(crate) async fn read(app: &AppService, name: &str, args: &Value) -> Option<Result<Output, ToolFail>> {
    let action = args.get("action").and_then(Value::as_str);
    Some(match (name, action) {
        ("title_block", Some("get")) => title_block_get(app, args).await,
        ("sheets", Some("list")) => sheets_list(app, args).await,
        ("sheets", Some("preview")) => sheets_preview(app, args).await,
        ("sheets", Some("export")) => sheets_export(app, args).await,
        ("view_marks", Some("list")) => marks_list(app, args).await,
        _ => return None,
    })
}

// ------------------------------------------------------------- title block

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TitleBlockIn {
    action: String,
    project_title: Option<String>,
    lot_no: Option<String>,
    block_no: Option<String>,
    tct_no: Option<String>,
    firm_name: Option<String>,
    firm_address: Option<String>,
    firm_logo: Option<String>,
}

/// What an agent is told when it reaches for the license numbers.
fn people_only() -> ToolFail {
    ToolFail::new(
        "forbidden",
        "only people set the signing architect's name and license numbers (PRC, PTR, IAPOA, TIN); an agent never writes them",
    )
    .with_hint("Ask the user to fill the signing architect in the title block form of the Sheets panel. title_block action \"get\" shows them masked.")
}

/// The signatory fields, by name, refused before anything else.
const SIGNATORY_FIELDS: [&str; 11] = [
    "signatory", "name", "prc_no", "prc_valid_until", "ptr_no", "ptr_date", "ptr_place", "iapoa_no", "iapoa_or_no", "iapoa_date", "tin",
];

fn refuse_signatory(args: &Value) -> Result<(), ToolFail> {
    match args.as_object() {
        Some(m) if SIGNATORY_FIELDS.iter().any(|k| m.contains_key(*k)) => Err(people_only()),
        _ => Ok(()),
    }
}

pub(crate) fn title_block(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    refuse_signatory(args)?;
    let i: TitleBlockIn = parse(args)?;
    if i.action != "set" {
        return Err(ToolFail::args(format!("action \"{}\" reads the title block: call title_block on its own", i.action)));
    }
    let mut tb = (*p.title_block).clone();
    let mut changed = false;
    for (slot, v) in [
        (&mut tb.project_title, i.project_title),
        (&mut tb.lot_no, i.lot_no),
        (&mut tb.block_no, i.block_no),
        (&mut tb.tct_no, i.tct_no),
        (&mut tb.firm_name, i.firm_name),
        (&mut tb.firm_address, i.firm_address),
    ] {
        if let Some(v) = v {
            *slot = v.trim().to_string();
            changed = true;
        }
    }
    if let Some(logo) = i.firm_logo {
        let logo = logo.trim().to_string();
        tb.firm_logo = (!logo.is_empty()).then_some(logo);
        changed = true;
    }
    if !changed {
        return Err(ToolFail::args("action \"set\" needs a field to change: project_title, lot_no, block_no, tct_no, firm_name, firm_address or firm_logo"));
    }
    // The engine keeps the stored signatory whatever this one says.
    tb.signatory = Signatory::default();
    Ok(Plan::of(vec![Command::SetTitleBlock { title_block: Box::new(tb) }], Primary::None))
}

async fn title_block_get(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    refuse_signatory(args)?;
    let i: TitleBlockIn = parse(args)?;
    let extra = [i.project_title, i.lot_no, i.block_no, i.tct_no, i.firm_name, i.firm_address, i.firm_logo];
    if extra.iter().any(Option::is_some) {
        return Err(ToolFail::args("action \"get\" takes no fields; action \"set\" changes them"));
    }
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let p = doc.project();
    let masked = p.title_block.masked();
    Ok(Output::Json(json!({
        "revision": doc.revision(),
        "title_block": masked,
        "owner": p.settings.client_name,
        "location": p.settings.location,
        "designer": p.settings.designer,
        "signatory_set": !p.title_block.signatory.is_empty(),
        "note": "The license numbers are masked here and on every picture sent to an agent; the PDF on disk prints them in full. Owner and location come from set_project settings client_name and location. Only people set the signing architect.",
    })))
}

// ------------------------------------------------------------------ sheets

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetsIn {
    action: String,
    paper: Option<String>,
    replace: Option<bool>,
    sheet: Option<String>,
    sheets: Option<Vec<String>>,
    mark: Option<String>,
    date: Option<String>,
    description: Option<String>,
    format: Option<String>,
    width_px: Option<u32>,
    image: Option<String>,
    file_name: Option<String>,
    radius_m: Option<f64>,
}

fn paper_of(name: &str) -> Result<PaperSize, ToolFail> {
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "a0" => PaperSize::A0,
        "a1" => PaperSize::A1,
        "a2" => PaperSize::A2,
        "a3" => PaperSize::A3,
        "a4" => PaperSize::A4,
        other => return Err(ToolFail::args(format!("unknown paper `{other}`. Use {}", PAPERS.join(", ")))),
    })
}

/// A sheet by number ("A-1", case and spaces ignored) or id.
pub(crate) fn find_sheet<'a>(p: &'a Project, wanted: &str) -> Result<&'a Sheet, ToolFail> {
    let w = wanted.trim();
    let norm = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    p.sheet_set
        .sheets
        .iter()
        .find(|s| s.id == w)
        .or_else(|| p.sheet_set.sheets.iter().find(|s| norm(&s.number) == norm(w)))
        .ok_or_else(|| {
            let known: Vec<&str> = p.sheet_set.sheets.iter().map(|s| s.number.as_str()).collect();
            ToolFail::new(
                "not_found",
                if known.is_empty() {
                    format!("no sheet is `{w}`; the project has no sheets yet")
                } else {
                    format!("no sheet is `{w}`. Sheets: {}", known.join(", "))
                },
            )
            .with_hint("sheets action \"make\" makes the set; action \"list\" shows it.")
        })
}

fn sheet_ids(p: &Project, names: &[String]) -> Result<Vec<Id>, ToolFail> {
    names.iter().map(|n| find_sheet(p, n).map(|s| s.id.clone())).collect()
}

/// Today on this computer, as the title block writes a date.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    // Philippine time (UTC+8), the app's home; the title block date is a day.
    let days = (secs + 8 * 3600).div_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

pub(crate) fn sheets(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: SheetsIn = parse(args)?;
    match i.action.as_str() {
        "make" => {
            let paper = paper_of(i.paper.as_deref().unwrap_or("a1"))?;
            let mut plan = Plan::of(vec![Command::MakeSheetSet { paper, replace: i.replace.unwrap_or(false) }], Primary::None);
            plan.info = Some(json!({"note": "sheets action \"list\" shows the set; \"preview\" shows a sheet; \"export\" writes one PDF."}));
            Ok(plan)
        }
        "revise" => {
            let names = i.sheets.as_ref().filter(|n| !n.is_empty()).ok_or_else(|| ToolFail::args("action \"revise\" needs sheets: the sheet numbers it changes"))?;
            let description = i.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).ok_or_else(|| ToolFail::args("action \"revise\" needs a description of the change"))?;
            let revision = SheetRevision {
                id: String::new(),
                mark: i.mark.map(|m| m.trim().to_string()).unwrap_or_else(|| (p.sheet_set.revisions.len() + 1).to_string()),
                date: i.date.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).unwrap_or_else(today),
                description: description.to_string(),
            };
            Ok(Plan::of(vec![Command::UpsertRevision { revision, sheet_ids: sheet_ids(p, names)? }], Primary::None))
        }
        "delete" => {
            let names = i.sheets.as_ref().filter(|n| !n.is_empty()).ok_or_else(|| ToolFail::args("action \"delete\" needs sheets"))?;
            Ok(Plan::of(vec![Command::DeleteSheets { ids: sheet_ids(p, names)? }], Primary::None))
        }
        "vicinity" => vicinity(p, &i),
        other => Err(ToolFail::args(format!(
            "action \"{other}\" {}",
            if matches!(other, "list" | "preview" | "export") { "reads the set: call sheets on its own, not as a step of batch" } else { "is not one. Use make, list, preview, revise, export, delete or vicinity" }
        ))),
    }
}

/// `sheets` action `vicinity`: the image `file_name` (already in the
/// project's files, see `store_vicinity`) on every vicinity map viewport of
/// the set, or of `sheets`, as one undo step.
fn vicinity(p: &Project, i: &SheetsIn) -> Result<Plan, ToolFail> {
    if i.image.is_some() {
        return Err(ToolFail::args("image is stored when sheets action \"vicinity\" is called on its own; in a batch step give file_name"));
    }
    let file = i.file_name.as_deref().map(str::trim).filter(|f| !f.is_empty()).ok_or_else(|| {
        ToolFail::args("action \"vicinity\" needs image (a PNG or JPEG data URL) or file_name (an image in the project's files)")
    })?;
    let radius = match i.radius_m {
        Some(r) if !(r.is_finite() && r > 0.0) => return Err(ToolFail::args("radius_m is more than 0")),
        r => r,
    };
    let wanted = match i.sheets.as_ref().filter(|n| !n.is_empty()) {
        Some(names) => Some(sheet_ids(p, names)?),
        None => None,
    };
    let mut commands = vec![];
    for sheet in &p.sheet_set.sheets {
        if wanted.as_ref().is_some_and(|w| !w.contains(&sheet.id)) {
            continue;
        }
        let mut changed = sheet.clone();
        let mut any = false;
        for vp in changed.viewports.iter_mut() {
            if let ViewRef::VicinityMap { file_name, radius_m } = &mut vp.view {
                *file_name = Some(file.to_string());
                *radius_m = radius.unwrap_or(*radius_m);
                any = true;
            }
        }
        if any {
            commands.push(Command::UpsertSheet { sheet: Box::new(changed), index: None });
        }
    }
    if commands.is_empty() {
        return Err(ToolFail::new("not_found", "no sheet frames a vicinity map").with_hint("sheets action \"make\" puts one on A-1, beside the site plan."));
    }
    let mut plan = Plan::of(commands, Primary::None);
    plan.info = Some(json!({"file_name": file, "note": "sheets action \"preview\" shows the sheet with the map, the north arrow, the site marked and the radius note."}));
    Ok(plan)
}

/// Before `sheets` action `vicinity` is staged: an `image` data URL is stored
/// in the project's files (the app's `file_store`, which never overwrites)
/// and named by `file_name` instead; a `file_name` given must be there.
/// Other calls pass as they are.
pub(crate) async fn store_vicinity(app: &AppService, name: &str, args: &Value) -> Result<Value, ToolFail> {
    if name != "sheets" || args.get("action").and_then(Value::as_str) != Some("vicinity") {
        return Ok(args.clone());
    }
    let mut out = args.clone();
    match (args.get("image").and_then(Value::as_str), args.get("file_name").and_then(Value::as_str)) {
        (Some(_), Some(_)) => return Err(ToolFail::args("give image or file_name, not both")),
        (Some(data), None) => {
            let stored = app.handle("file_store", json!({"file_name": "vicinity-map.png", "data": data})).await?;
            let map = out.as_object_mut().ok_or_else(|| ToolFail::args("arguments must be an object"))?;
            map.remove("image");
            map.insert("file_name".into(), stored["file_name"].clone());
        }
        (None, Some(file)) => {
            if let Err(e) = app.handle("file_data", json!({"file_name": file.trim()})).await {
                let fail = ToolFail::from(e);
                let code = if fail.code == "io" { "not_found" } else { fail.code.as_str() };
                return Err(ToolFail::new(code, format!("no image `{}` in the project's files: {}", file.trim(), fail.message))
                    .with_hint("Give image, a PNG or JPEG data URL, and it is stored for you."));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// A view as a contract value for a sheet listing: "plan Ground Floor", "elevation North".
fn view_text(p: &Project, v: &ViewRef) -> String {
    let mark = |id: &str| {
        p.elements
            .iter()
            .find_map(|e| match e {
                Element::ViewMark(m) if m.id == id => Some(m.label.clone()),
                _ => None,
            })
            .unwrap_or_else(|| id.to_string())
    };
    match v {
        ViewRef::Plan { level_id, sheet } => {
            let level = p.levels.iter().find(|l| &l.id == level_id).map(|l| l.name.clone()).unwrap_or_default();
            let kind = serde_json::to_value(sheet).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            format!("{kind} {level}")
        }
        ViewRef::Elevation { mark_id } => format!("elevation {}", mark(mark_id)),
        ViewRef::Section { mark_id } => format!("section {}", mark(mark_id)),
        ViewRef::RoofPlan => "roof plan".into(),
        ViewRef::SitePlan => "site plan".into(),
        ViewRef::VicinityMap { .. } => "vicinity map".into(),
        ViewRef::Perspective { render_id } => format!("perspective {render_id}"),
        ViewRef::Schedule { schedule } => format!("{} schedule", serde_json::to_value(schedule).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()),
        ViewRef::Notes { .. } => "notes".into(),
    }
}

async fn sheets_list(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: SheetsIn = parse(args)?;
    if i.sheet.is_some() || i.paper.is_some() || i.format.is_some() {
        return Err(ToolFail::args("action \"list\" takes no other arguments"));
    }
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let p = doc.project();
    let sheets: Vec<Value> = p
        .sheet_set
        .sheets
        .iter()
        .map(|sh| {
            json!({
                "id": sh.id, "number": sh.number, "title": sh.title, "paper": sh.paper, "orientation": sh.orientation,
                "viewports": sh.viewports.iter().map(|v| json!({"id": v.id, "view": view_text(p, &v.view), "scale": v.scale_denominator.map(|n| format!("1:{n}")), "title": v.title})).collect::<Vec<_>>(),
                "revisions": sh.revision_ids.iter().filter_map(|r| p.sheet_set.revisions.iter().find(|x| &x.id == r)).map(|r| json!({"mark": r.mark, "date": r.date, "description": r.description})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Output::Json(json!({
        "revision": doc.revision(),
        "count": sheets.len(),
        "sheets": sheets,
        "revisions": p.sheet_set.revisions,
        "note": if p.sheet_set.sheets.is_empty() { "No sheets yet: sheets action \"make\" makes the permit set." } else { "sheets action \"preview\" with a sheet number shows it." },
    })))
}

/// The images a sheet prints, by the names `SheetInputs::images` keys them:
/// the firm logo and vicinity maps from the project's files, perspectives
/// from Visuals. One that cannot be read is left out.
async fn sheet_images(app: &AppService, p: &Project, sheet: &Sheet) -> BTreeMap<String, Vec<u8>> {
    let mut names: Vec<(String, bool)> = vec![];
    if let Some(logo) = &p.title_block.firm_logo {
        names.push((logo.clone(), true));
    }
    for v in &sheet.viewports {
        match &v.view {
            ViewRef::VicinityMap { file_name: Some(f), .. } => names.push((f.clone(), true)),
            ViewRef::Perspective { render_id } => names.push((render_id.clone(), false)),
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for (name, is_file) in names {
        let url = if is_file {
            app.handle("file_data", json!({"file_name": name})).await
        } else {
            app.handle("render_data", json!({"id": name})).await
        };
        let bytes = url.ok().and_then(|v| v.as_str().map(str::to_string)).and_then(|u| {
            let data = u.split_once(";base64,").map(|(_, d)| d.to_string())?;
            base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).ok()
        });
        if let Some(b) = bytes {
            out.insert(name, b);
        }
    }
    out
}

/// Belt and braces for D53: a license number printed in full on a picture
/// for an agent is replaced by its masked form.
pub(crate) fn mask_svg(svg: &str, signatory: &Signatory) -> String {
    let masked = signatory.masked();
    let mut out = svg.to_string();
    for (full, hidden) in [
        (&signatory.prc_no, &masked.prc_no),
        (&signatory.ptr_no, &masked.ptr_no),
        (&signatory.iapoa_no, &masked.iapoa_no),
        (&signatory.iapoa_or_no, &masked.iapoa_or_no),
        (&signatory.tin, &masked.tin),
    ] {
        if full.trim().chars().filter(char::is_ascii_alphanumeric).count() >= 3 && full != hidden {
            out = out.replace(full.as_str(), hidden);
        }
    }
    out
}

/// A sheet as SVG for an agent: license numbers masked, images in.
pub(crate) async fn sheet_svg_for_agent(app: &AppService, wanted: &str) -> Result<(String, Value), ToolFail> {
    let (project, derived) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        (doc.project().clone(), doc.derived().clone())
    };
    let sheet = find_sheet(&project, wanted)?.clone();
    let images = sheet_images(app, &project, &sheet).await;
    let inputs = SheetInputs { date: None, images, mask_signatory: true };
    let svg = guhit_export::sheet_svg(&project, &derived, &sheet.id, &inputs).map_err(export_fail)?;
    let svg = mask_svg(&svg, &project.title_block.signatory);
    let meta = json!({"id": sheet.id, "number": sheet.number, "title": sheet.title, "paper": sheet.paper, "orientation": sheet.orientation, "viewports": sheet.viewports.len()});
    Ok((svg, meta))
}

pub(crate) fn export_fail(e: guhit_export::ExportError) -> ToolFail {
    match e {
        guhit_export::ExportError::NotBuilt(_) => ToolFail::new("not_built", e.to_string()),
        guhit_export::ExportError::Empty(_) => ToolFail::new("invalid", e.to_string()),
        guhit_export::ExportError::Failed(_) => ToolFail::new("io", e.to_string()),
    }
}

async fn sheets_preview(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: SheetsIn = parse(args)?;
    let wanted = i.sheet.as_deref().or_else(|| i.sheets.as_ref().and_then(|v| v.first()).map(String::as_str)).ok_or_else(|| ToolFail::args("action \"preview\" needs sheet: its number (A-1) or id"))?;
    let mut args = json!({"mode": "sheet", "sheet": wanted});
    if let Some(w) = i.width_px {
        args["width_px"] = json!(w);
    }
    crate::look::look(app, &args, &crate::agent::CallContext::local()).await
}

async fn sheets_export(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: SheetsIn = parse(args)?;
    let format = match i.format.as_deref().unwrap_or("pdf") {
        "pdf" => PlanFormat::Pdf,
        "svg" => PlanFormat::Svg,
        "dxf" => PlanFormat::Dxf,
        other => return Err(ToolFail::args(format!("unknown format `{other}`. Use pdf, svg or dxf"))),
    };
    let ids = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        let p = doc.project();
        if p.sheet_set.sheets.is_empty() {
            return Err(ToolFail::new("invalid", "the project has no sheets to export").with_hint("sheets action \"make\" makes the permit set first."));
        }
        sheet_ids(p, i.sheets.as_deref().unwrap_or_default())?
    };
    let options = SheetExportOptions { format, sheet_ids: ids, path: None, date: None };
    let result = app.handle("export_sheets", serde_json::to_value(&options).map_err(|e| ToolFail::new("io", e.to_string()))?).await?;
    Ok(Output::Json(json!({
        "ok": true,
        "format": i.format.unwrap_or_else(|| "pdf".into()),
        "result": result,
        "note": "Written to the app's exports folder. The PDF on disk prints the signing architect in full; review items are suggestions, and nothing here is permit approval.",
    })))
}

// -------------------------------------------------------------- view marks

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarksIn {
    action: String,
    kind: Option<MarkKind>,
    label: Option<String>,
    start: Option<Value>,
    end: Option<Value>,
    flip: Option<bool>,
    depth_mm: Option<f64>,
    level: Option<String>,
    ids: Option<Vec<Id>>,
}

pub(crate) fn view_marks(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: MarksIn = parse(args)?;
    match i.action.as_str() {
        "add" => {
            let kind = i.kind.ok_or_else(|| ToolFail::args("action \"add\" needs kind: section or elevation"))?;
            let label = i.label.as_deref().map(str::trim).filter(|l| !l.is_empty()).ok_or_else(|| ToolFail::args("action \"add\" needs a label"))?;
            let (Some(a), Some(b)) = (&i.start, &i.end) else {
                return Err(ToolFail::args("action \"add\" needs start and end: the cut, or the picture plane"));
            };
            let mark = ViewMark {
                id: String::new(),
                level_id: pick_level(p, i.level.as_deref(), default_level)?,
                mark_kind: kind,
                label: label.to_string(),
                start: point(a, "start")?,
                end: point(b, "end")?,
                flip: i.flip.unwrap_or(false),
                depth_mm: positive_opt(i.depth_mm, "depth_mm")?,
            };
            Ok(Plan::of(vec![Command::AddElement { element: Element::ViewMark(mark) }], Primary::Kind(ElementKind::ViewMark)))
        }
        "delete" => {
            let ids = i.ids.filter(|v| !v.is_empty()).ok_or_else(|| ToolFail::args("action \"delete\" needs ids"))?;
            for id in &ids {
                if !matches!(find(p, id)?, Element::ViewMark(_)) {
                    return Err(ToolFail::args(format!("`{id}` is not a section or elevation mark; delete removes other elements")).with_ids(vec![id.clone()]));
                }
            }
            Ok(Plan::of(vec![Command::DeleteElements { ids }], Primary::None))
        }
        "list" => Err(ToolFail::args("action \"list\" reads the marks: call view_marks on its own")),
        other => Err(ToolFail::args(format!("unknown action `{other}`. Use add, list or delete"))),
    }
}

async fn marks_list(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: MarksIn = parse(args)?;
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let p = doc.project();
    let level = match i.level.as_deref() {
        Some(l) => Some(find_level(p, l)?.id.clone()),
        None => None,
    };
    let rows: Vec<Value> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::ViewMark(m) if level.as_ref().is_none_or(|l| &m.level_id == l) => Some(m),
            _ => None,
        })
        .map(|m| {
            json!({
                "id": m.id, "kind": m.mark_kind, "label": m.label,
                "level": p.levels.iter().find(|l| l.id == m.level_id).map(|l| l.name.clone()),
                "start": m.start, "end": m.end, "flip": m.flip, "depth_mm": m.depth_mm,
                "length_mm": geo::r1(geo::dist(m.start, m.end)),
            })
        })
        .collect();
    Ok(Output::Json(json!({"revision": doc.revision(), "count": rows.len(), "marks": rows})))
}

// ---------------------------------------------------------------------- lot

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegIn {
    bearing: String,
    distance_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SideIn {
    role: LotSideRole,
    setback_mm: Option<f64>,
    road: Option<String>,
    road_width_mm: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LotIn {
    description: Option<String>,
    legs: Option<Vec<LegIn>>,
    start: Option<Value>,
    sides: Option<Vec<SideIn>>,
    grade_mm: Option<f64>,
}

pub(crate) fn set_lot(args: &Value) -> Result<Plan, ToolFail> {
    let i: LotIn = parse(args)?;
    let legs = match (&i.description, i.legs) {
        (Some(text), None) => parse_tct(text)?,
        (None, Some(list)) => list
            .into_iter()
            .enumerate()
            .map(|(k, l)| {
                let bearing = l.bearing.trim().to_string();
                if bearing.is_empty() {
                    return Err(ToolFail::args(format!("legs[{k}].bearing is empty")));
                }
                Ok(SurveyLeg { bearing, distance_mm: positive(l.distance_mm, &format!("legs[{k}].distance_mm"))? })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ToolFail::args("give description (the TCT technical description) or legs, not both")),
    };
    if legs.len() < 3 {
        return Err(ToolFail::args(format!("a lot needs at least 3 legs; found {}", legs.len())));
    }
    let start = match &i.start {
        Some(v) => point(v, "start")?,
        None => pt(0.0, 0.0),
    };
    let sides: Vec<LotSide> = i
        .sides
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(k, s)| {
            Ok(LotSide {
                role: s.role,
                setback_mm: match s.setback_mm {
                    Some(v) if !(v.is_finite() && v >= 0.0) => return Err(ToolFail::args(format!("sides[{k}].setback_mm is 0 or more"))),
                    Some(v) => v,
                    None => 0.0,
                },
                road: s.road.unwrap_or_default().trim().to_string(),
                road_width_mm: positive_opt(s.road_width_mm, &format!("sides[{k}].road_width_mm"))?,
            })
        })
        .collect::<Result<Vec<_>, ToolFail>>()?;
    if !sides.is_empty() && sides.len() != legs.len() {
        return Err(ToolFail::args(format!("sides has {} entries for {} legs: one per leg, in order", sides.len(), legs.len())));
    }
    let mut info = json!({"legs": legs.iter().map(|l| json!({"bearing": l.bearing, "distance_mm": l.distance_mm})).collect::<Vec<_>>()});
    // The traverse's misclosure, as the legs read (the engine spreads it over
    // the corners): how far the last leg ends from the start.
    if let Some(dirs) = legs.iter().map(|l| bearing_dir(&l.bearing)).collect::<Option<Vec<_>>>() {
        let (mut x, mut y, mut perimeter) = (0.0, 0.0, 0.0);
        for (d, l) in dirs.iter().zip(&legs) {
            x += d.x * l.distance_mm;
            y += d.y * l.distance_mm;
            perimeter += l.distance_mm;
        }
        let miss = (x * x + y * y).sqrt();
        info["misclosure_mm"] = json!(geo::r1(miss));
        info["closure"] = json!(if miss < 0.5 { "closes exactly".to_string() } else { format!("1 in {:.0}", perimeter / miss) });
    }
    let mut plan = Plan::of(vec![Command::SetLotFromSurvey { start, legs }], Primary::Kind(ElementKind::Lot));
    if !sides.is_empty() || i.grade_mm.is_some() {
        plan.then.push(Then::LotSides { sides, grade_mm: i.grade_mm.map(|g| g.max(0.0)) });
    }
    plan.info = Some(info);
    Ok(plan)
}

/// The legs of a TCT technical description, in order. The tie line to the
/// BLLM before the first "thence" is not a side of the lot and is left
/// out. Bearings come back as "N 45-30 E" (seconds as "N 45-30-15 E"),
/// distances in mm.
pub fn parse_tct(text: &str) -> Result<Vec<SurveyLeg>, ToolFail> {
    let upper = text.to_ascii_uppercase();
    let parts: Vec<&str> = upper.split("THENCE").collect();
    let legs_text: Vec<&str> = if parts.len() > 1 { parts[1..].to_vec() } else { vec![upper.as_str()] };
    let mut legs = vec![];
    for (k, part) in legs_text.iter().enumerate() {
        match leg_in(part) {
            Some(leg) => legs.push(leg),
            None if parts.len() > 1 => {
                return Err(ToolFail::args(format!(
                    "the course after \"thence\" number {} has no bearing and distance I can read: \"{}\"",
                    k + 1,
                    part.trim().chars().take(80).collect::<String>()
                ))
                .with_hint("Give legs [{bearing: \"N 45-30 E\", distance_mm: 12000}] instead, from the title."))
            }
            None => {}
        }
    }
    if legs.is_empty() {
        return Err(ToolFail::args("no bearing and distance found in the description").with_hint("Give legs [{bearing: \"N 45-30 E\", distance_mm: 12000}] instead, from the title."));
    }
    Ok(legs)
}

/// A quadrant bearing ("N 45-30 E", "S 0°48' W", "N 45 30 15 E") as a plan
/// unit vector, +x east, +y north. None when it does not read as one.
fn bearing_dir(bearing: &str) -> Option<Point> {
    let up = bearing.to_ascii_uppercase();
    let ns = up.trim_start().chars().next().filter(|c| *c == 'N' || *c == 'S')?;
    let ew = up.trim_end().chars().last().filter(|c| *c == 'E' || *c == 'W')?;
    let nums: Vec<f64> = up.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
    let deg = nums.first()? + nums.get(1).copied().unwrap_or(0.0) / 60.0 + nums.get(2).copied().unwrap_or(0.0) / 3600.0;
    if !(0.0..=90.0).contains(&deg) {
        return None;
    }
    let (s, c) = deg.to_radians().sin_cos();
    Some(pt(if ew == 'E' { s } else { -s }, if ns == 'N' { c } else { -c }))
}

/// One course: "N. 45 DEG. 30' E., 12.00 M." in any common spelling.
fn leg_in(text: &str) -> Option<SurveyLeg> {
    // Tokens: words and numbers, everything else a break.
    let mut tokens: Vec<String> = vec![];
    let mut cur = String::new();
    for c in text.chars() {
        let part_of_number = c.is_ascii_digit() || (c == '.' && cur.chars().last().is_some_and(|l| l.is_ascii_digit())) || (c == ',' && cur.chars().all(|d| d.is_ascii_digit()) && !cur.is_empty());
        if c.is_ascii_alphabetic() && cur.chars().all(|d| d.is_ascii_alphabetic()) {
            cur.push(c);
        } else if part_of_number && cur.chars().all(|d| d.is_ascii_digit() || d == '.' || d == ',') {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            if c.is_ascii_alphanumeric() {
                cur.push(c);
            }
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    let num = |t: &str| t.trim_end_matches(['.', ',']).replace(',', "").parse::<f64>().ok();
    let start = tokens.iter().position(|t| t == "N" || t == "S")?;
    let ns = tokens[start].clone();
    let mut angle: Vec<f64> = vec![];
    let mut k = start + 1;
    while k < tokens.len() && tokens[k] != "E" && tokens[k] != "W" {
        if let Some(v) = num(&tokens[k]) {
            angle.push(v);
        } else if !matches!(tokens[k].as_str(), "DEG" | "DEGS" | "DEGREES" | "D" | "MIN" | "MINS" | "MINUTES" | "SEC" | "SECS" | "SECONDS") {
            return None;
        }
        k += 1;
        if angle.len() > 3 {
            return None;
        }
    }
    let ew = tokens.get(k)?.clone();
    let distance = tokens[k + 1..].iter().find_map(|t| num(t))?;
    let (deg, min, sec) = (*angle.first()?, angle.get(1).copied().unwrap_or(0.0), angle.get(2).copied().unwrap_or(0.0));
    if !(0.0..=90.0).contains(&deg) || !(0.0..60.0).contains(&min) || !(0.0..60.0).contains(&sec) || distance <= 0.0 {
        return None;
    }
    let bearing = if sec > 0.0 {
        format!("{ns} {deg:.0}-{min:.0}-{sec:.0} {ew}")
    } else {
        format!("{ns} {deg:.0}-{min:.0} {ew}")
    };
    Some(SurveyLeg { bearing, distance_mm: (distance * 1000.0 * 10.0).round() / 10.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tct_description_reads_its_legs_and_skips_the_tie_line() {
        let text = "Beginning at a point marked \"1\" on plan, being N. 45 deg. 30' E., 1,234.56 m. from BLLM No. 1, Cad. 123; \
            thence S. 89 deg. 12' E., 12.00 m. to point 2; thence S. 00 deg. 48' W., 15.00 m. to point 3; \
            thence N. 89 deg. 12' W., 12.00 m. to point 4; thence N. 00 deg. 48' E., 15.00 m. to the point of beginning.";
        let legs = parse_tct(text).unwrap();
        assert_eq!(legs.len(), 4);
        assert_eq!(legs[0].bearing, "S 89-12 E");
        assert_eq!(legs[0].distance_mm, 12_000.0);
        assert_eq!(legs[1].bearing, "S 0-48 W");
        assert_eq!(legs[3].distance_mm, 15_000.0);
        let degrees = parse_tct("thence N 45°30'15\" E 10.5 m; thence S 44-29 E, 20 m; thence S 45 30 W 10.5 m").unwrap();
        assert_eq!(degrees[0].bearing, "N 45-30-15 E");
        assert_eq!(degrees[2].distance_mm, 10_500.0);
        assert_eq!(parse_tct("no bearings here").unwrap_err().code, "bad_args");
        // The result states the misclosure: none for a parallelogram, 1 m
        // when the last leg falls short.
        let closed = set_lot(&json!({"description": text})).unwrap().info.unwrap();
        assert_eq!((closed["misclosure_mm"].clone(), closed["closure"].clone()), (json!(0.0), json!("closes exactly")));
        let short = set_lot(&json!({"legs": [{"bearing": "N 90-0 E", "distance_mm": 12000}, {"bearing": "N 0-0 E", "distance_mm": 15000}, {"bearing": "N 90-0 W", "distance_mm": 12000}, {"bearing": "S 0-0 W", "distance_mm": 14000}]})).unwrap().info.unwrap();
        assert_eq!(short["misclosure_mm"], 1000.0);
        assert_eq!(short["closure"], "1 in 53");
    }

    #[test]
    fn the_signatory_is_never_written_and_reads_masked() {
        let mut p = guhit_core::templates::sample_bungalow();
        p.title_block.signatory.prc_no = "0123456".into();
        let f = title_block(&p, &json!({"action": "set", "prc_no": "999"})).unwrap_err();
        assert_eq!(f.code, "forbidden");
        assert!(f.hint().unwrap().contains("title block form"));
        let plan = title_block(&p, &json!({"action": "set", "tct_no": "T-12345", "lot_no": "7"})).unwrap();
        let Command::SetTitleBlock { title_block } = &plan.commands[0] else { panic!() };
        assert_eq!((title_block.tct_no.as_str(), title_block.signatory.is_empty()), ("T-12345", true));
        let svg = "<text>PRC 0123456</text>";
        assert!(!mask_svg(svg, &p.title_block.signatory).contains("0123456"));
    }

    #[test]
    fn sheets_and_marks_translate() {
        let p = guhit_core::templates::sample_bungalow();
        let level = p.levels[0].id.clone();
        assert!(matches!(sheets(&p, &json!({"action": "make", "paper": "A1"})).unwrap().commands[0], Command::MakeSheetSet { paper: PaperSize::A1, replace: false }));
        assert_eq!(sheets(&p, &json!({"action": "revise", "sheets": ["A-9"], "description": "x"})).unwrap_err().code, "not_found");
        let m = view_marks(&p, &json!({"action": "add", "kind": "section", "label": "A", "start": [-1000, 3000], "end": [9000, 3000]}), &level).unwrap();
        assert!(matches!(&m.commands[0], Command::AddElement { element: Element::ViewMark(v) } if v.label == "A" && v.mark_kind == MarkKind::Section));
        assert_eq!(today().len(), 10);
    }
}
