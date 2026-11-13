//! The `tours` toolset (DECISIONS D42, D54): make a video tour of the house,
//! change its shots, and export it as a video into Visuals.
//!
//! A tour is a view like a saved camera: on no level, allowed in any scope
//! or agent limit. Its frames are worked out by the engine
//! (`Query::TourFrames`), so a preview, the export and the checks agree.
//! The export runs in the window (D31) as a long job: `window_start` hands
//! back a job id at once and `window_wait` waits for it.

use std::time::Duration;

use guhit_app::AppService;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fail::ToolFail;
use crate::tools::{obj, Output, ToolDef, MM};
use crate::translate::{find_level, parse, positive, Plan, Primary};

fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn def(name: &'static str, description: String, schema: Value) -> ToolDef {
    ToolDef { name, description, schema, read_only: false, open_world: false }
}

const SIZES: [&str; 4] = ["hd", "qhd", "4k", "square"];
/// An export waits this long by default before it hands back a job id.
const WAIT_DEFAULT_S: f64 = 30.0;
/// Longest wait a caller may ask for, under the usual MCP client timeouts.
const WAIT_MAX_S: f64 = 110.0;

pub fn definitions() -> Vec<ToolDef> {
    let shot = json!({"type": "object", "properties": {
        "shot": {"description": "Its id, name or number from 1.", "type": ["string", "integer"]},
        "name": {"type": "string"}, "seconds": {"type": "number"},
        "ease": {"type": "string", "enum": ["linear", "in_out"]},
        "transition": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["cut", "dissolve", "dip_black", "dip_white", "whip_pan", "plan_lift"]}, "seconds": {"type": "number"}}},
    }, "required": ["shot"]});
    vec![
        def(
            "make_tour",
            format!("Make a video tour of the house, one undo step: fly in and orbit the outside, through the front door, the rooms in door order with holds, up the stairs, from day to dusk. The engine plans the path through doors; look mode tour shows a frame and query what tour lists them. Export it with export_tour. {MM}"),
            obj(json!({
                "name": s("Default \"House tour\"."),
                "exterior": {"type": "boolean", "description": "Fly in and orbit first. Default true."},
                "day_to_dusk": {"type": "boolean", "description": "Default true."},
                "pace_mps": {"type": "number", "description": "Walking pace, m/s. Default 0.8."},
                "levels": {"type": "array", "items": {"type": "string"}, "description": "Levels to visit, in order. Default every level with rooms."},
            }), &[]),
        ),
        def(
            "edit_tour",
            format!("Change a tour, one undo step: its name and fps (24, 30 or 60), each shot's name, seconds, ease and the transition into it, remove shots, or put them in a new order. {MM}"),
            obj(json!({
                "tour": s("Its id or name."),
                "name": s(""),
                "fps": {"type": "integer", "enum": [24, 30, 60]},
                "shots": {"type": "array", "items": shot},
                "remove": {"type": "array", "items": {"type": ["string", "integer"]}, "description": "Shots by id, name or number."},
                "order": {"type": "array", "items": {"type": ["string", "integer"]}, "description": "Every shot, in the new order."},
            }), &["tour"]),
        ),
        def(
            "export_tour",
            format!("Export a tour as a video into Visuals, rendered in the Guhit Studio window (keep it open and visible). A long job: it answers with the video when done within wait_seconds, else a job_id; call again with job_id to wait for it. H.264 MP4, else VP9, else a PNG frame folder, and it says which. {MM}"),
            obj(json!({
                "tour": s("Its id or name."),
                "size": {"type": "string", "enum": SIZES, "description": "hd 1920 x 1080 (default), qhd, 4k, square."},
                "job_id": s("A job from an earlier call, to wait for it."),
                "wait_seconds": {"type": "number", "description": "1 to 110. Default 30."},
            }), &[]),
        ),
    ]
}

/// A tour by id or name (case and spaces ignored).
pub(crate) fn find_tour<'a>(p: &'a Project, wanted: &str) -> Result<&'a Tour, ToolFail> {
    let w = wanted.trim();
    let tours: Vec<&Tour> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Tour(t) => Some(t),
            _ => None,
        })
        .collect();
    if let Some(t) = tours.iter().find(|t| t.id == w) {
        return Ok(t);
    }
    let named: Vec<&&Tour> = tours.iter().filter(|t| t.name.trim().eq_ignore_ascii_case(w)).collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => Err(ToolFail::new(
            "not_found",
            if tours.is_empty() {
                format!("no tour is `{w}`; the project has no tours yet")
            } else {
                format!("no tour is `{w}`. Tours: {}", tours.iter().map(|t| format!("\"{}\" ({})", t.name, t.id)).collect::<Vec<_>>().join(", "))
            },
        )
        .with_hint("make_tour makes one; query what \"elements\" kind \"tour\" lists them.")),
        _ => Err(ToolFail::args(format!("more than one tour is called `{w}`; give its id"))),
    }
}

// --------------------------------------------------------------- make_tour

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MakeIn {
    name: Option<String>,
    exterior: Option<bool>,
    day_to_dusk: Option<bool>,
    pace_mps: Option<f64>,
    levels: Option<Vec<String>>,
}

pub(crate) fn make_tour(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: MakeIn = parse(args)?;
    let levels = i.levels.unwrap_or_default().iter().map(|l| find_level(p, l).map(|l| l.id.clone())).collect::<Result<Vec<_>, _>>()?;
    let pace = positive(i.pace_mps.unwrap_or(0.8), "pace_mps")?;
    if !(0.3..=3.0).contains(&pace) {
        return Err(ToolFail::args("pace_mps is a walking pace: 0.3 to 3 m/s"));
    }
    let options = AutoTour { exterior: i.exterior.unwrap_or(true), day_to_dusk: i.day_to_dusk.unwrap_or(true), pace_mps: pace, levels };
    let name = i.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let mut plan = Plan::of(vec![Command::MakeTour { name, options }], Primary::Kind(ElementKind::Tour));
    plan.info = Some(json!({"note": "look mode tour with the tour and at_s shows a frame; export_tour writes the video."}));
    Ok(plan)
}

// --------------------------------------------------------------- edit_tour

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionIn {
    kind: Option<TransitionKind>,
    seconds: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShotIn {
    shot: Value,
    name: Option<String>,
    seconds: Option<f64>,
    ease: Option<Ease>,
    transition: Option<TransitionIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditIn {
    tour: String,
    name: Option<String>,
    fps: Option<u32>,
    shots: Option<Vec<ShotIn>>,
    remove: Option<Vec<Value>>,
    order: Option<Vec<Value>>,
}

/// The index of a shot named by id, name or number from 1.
fn shot_index(t: &Tour, v: &Value) -> Result<usize, ToolFail> {
    let found = match v {
        Value::Number(n) => n.as_u64().filter(|n| *n >= 1 && (*n as usize) <= t.shots.len()).map(|n| n as usize - 1),
        Value::String(s) => {
            let s = s.trim();
            t.shots
                .iter()
                .position(|x| x.id == s)
                .or_else(|| t.shots.iter().position(|x| x.name.trim().eq_ignore_ascii_case(s)))
                .or_else(|| s.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= t.shots.len()).map(|n| n - 1))
        }
        _ => None,
    };
    found.ok_or_else(|| {
        ToolFail::new("not_found", format!("tour \"{}\" has no shot {v}; it has {} shots", t.name, t.shots.len()))
            .with_hint("query what \"describe\" with the tour id lists its shots.")
    })
}

pub(crate) fn edit_tour(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: EditIn = parse(args)?;
    let mut t = find_tour(p, &i.tour)?.clone();
    let before = t.clone();
    if let Some(n) = i.name {
        let n = n.trim().to_string();
        if n.is_empty() {
            return Err(ToolFail::args("name is empty"));
        }
        t.name = n;
    }
    if let Some(f) = i.fps {
        t.fps = f;
    }
    for (k, s) in i.shots.unwrap_or_default().into_iter().enumerate() {
        let at = shot_index(&before, &s.shot)?;
        let shot = &mut t.shots[at];
        if let Some(n) = s.name {
            shot.name = n.trim().to_string();
        }
        if let Some(sec) = s.seconds {
            shot.seconds = positive(sec, &format!("shots[{k}].seconds"))?;
        }
        if let Some(e) = s.ease {
            shot.ease = e;
        }
        if let Some(tr) = s.transition {
            if let Some(kind) = tr.kind {
                shot.transition.kind = kind;
            }
            if let Some(sec) = tr.seconds {
                if !(sec.is_finite() && sec >= 0.0) {
                    return Err(ToolFail::args(format!("shots[{k}].transition.seconds is 0 or more")));
                }
                shot.transition.seconds = sec;
            }
        }
    }
    if let Some(order) = i.order {
        let idx = order.iter().map(|v| shot_index(&before, v)).collect::<Result<Vec<_>, _>>()?;
        let mut sorted = idx.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != before.shots.len() || idx.len() != before.shots.len() {
            return Err(ToolFail::args(format!("order names every shot once: {} shots", before.shots.len())));
        }
        let shots = t.shots.clone();
        t.shots = idx.into_iter().map(|k| shots[k].clone()).collect();
    }
    if let Some(remove) = i.remove {
        let ids: Vec<Id> = remove.iter().map(|v| shot_index(&before, v).map(|k| before.shots[k].id.clone())).collect::<Result<_, _>>()?;
        t.shots.retain(|s| !ids.contains(&s.id));
        if t.shots.is_empty() {
            return Err(ToolFail::args("that would remove every shot; delete the tour instead"));
        }
    }
    if t == before {
        return Err(ToolFail::args("nothing would change: give name, fps, shots, remove or order"));
    }
    let seconds: f64 = t.shots.iter().map(|s| s.seconds).sum();
    let mut plan = Plan::of(vec![Command::UpdateElement { element: Element::Tour(t) }], Primary::None);
    plan.info = Some(json!({"seconds": (seconds * 10.0).round() / 10.0}));
    Ok(plan)
}

// ------------------------------------------------------------- export_tour

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportIn {
    tour: Option<String>,
    size: Option<String>,
    job_id: Option<String>,
    wait_seconds: Option<f64>,
}

fn size_of(name: &str) -> Result<RenderSize, ToolFail> {
    Ok(match name {
        "hd" => RenderSize::Hd,
        "qhd" => RenderSize::Qhd,
        "4k" => RenderSize::Uhd,
        "square" => RenderSize::Square,
        other => return Err(ToolFail::args(format!("unknown size `{other}`. Use {}", SIZES.join(", ")))),
    })
}

pub(crate) async fn export_tour(app: &AppService, args: &Value, ctx: &crate::agent::CallContext) -> Result<Output, ToolFail> {
    let i: ExportIn = parse(args)?;
    let wait = Duration::from_secs_f64(i.wait_seconds.unwrap_or(WAIT_DEFAULT_S).clamp(1.0, WAIT_MAX_S));
    let (job, tour) = match (&i.job_id, &i.tour) {
        (Some(job), None) => {
            if i.size.is_some() {
                return Err(ToolFail::args("size goes with tour, when the export starts"));
            }
            (job.trim().to_string(), None)
        }
        (None, Some(t)) => {
            let (id, name) = {
                let r = crate::view::read(app, ctx).await;
                let tour = find_tour(r.doc()?.project(), t)?;
                (tour.id.clone(), tour.name.clone())
            };
            let size = size_of(i.size.as_deref().unwrap_or("hd"))?;
            let job = app.window_start(WindowTask::ExportTour { tour_id: id, size })?;
            (job, Some(name))
        }
        _ => return Err(ToolFail::args("give tour to start an export, or job_id to wait for one")),
    };
    match app.window_wait(&job, wait).await.map_err(|e| crate::visuals::window_busy_with(e, app.window_activity().as_ref()))? {
        Some(reply) => {
            let records = crate::visuals::records(app, &reply.render_ids).await;
            Ok(Output::Json(json!({
                "status": "done",
                "job_id": job,
                "videos": records,
                "note": reply.note,
            })))
        }
        None => Ok(Output::Json(json!({
            "status": "running",
            "job_id": job,
            "tour": tour,
            "window": app.window_activity(),
            "note": "The window is rendering and encoding the tour. Call export_tour with this job_id to wait for it; the video also appears in the app's Visuals panel.",
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tour() -> Tour {
        let key = ShotKey { at: 0.0, position: Vec3 { x: 0.0, y: -9000.0, z: 1600.0 }, target: Vec3 { x: 0.0, y: 0.0, z: 1600.0 }, fov_deg: 60.0 };
        let shot = |id: &str, name: &str| Shot {
            id: id.into(),
            name: name.into(),
            kind: ShotMove::Hold,
            keys: vec![key],
            seconds: 3.0,
            ease: Ease::InOut,
            light: None,
            light_end: None,
            transition: Transition { kind: TransitionKind::Cut, seconds: 0.0 },
        };
        Tour { id: "t1".into(), name: "House tour".into(), shots: vec![shot("s1", "Fly in"), shot("s2", "Living"), shot("s3", "Kitchen")], fps: 30 }
    }

    #[test]
    fn a_tour_is_edited_by_shot_name_number_or_id() {
        let mut p = guhit_core::templates::sample_bungalow();
        p.elements.push(Element::Tour(tour()));
        let plan = edit_tour(&p, &json!({"tour": "house TOUR", "fps": 60, "shots": [{"shot": "Living", "seconds": 5, "transition": {"kind": "dissolve", "seconds": 1}}], "remove": [3]})).unwrap();
        let Command::UpdateElement { element: Element::Tour(t) } = &plan.commands[0] else { panic!() };
        assert_eq!((t.fps, t.shots.len(), t.shots[1].seconds, t.shots[1].transition.kind), (60, 2, 5.0, TransitionKind::Dissolve));
        let plan = edit_tour(&p, &json!({"tour": "t1", "order": ["s3", 1, "Living"]})).unwrap();
        let Command::UpdateElement { element: Element::Tour(t) } = &plan.commands[0] else { panic!() };
        assert_eq!(t.shots.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s3", "s1", "s2"]);
        assert_eq!(edit_tour(&p, &json!({"tour": "t1", "order": ["s3"]})).unwrap_err().code, "bad_args");
        assert_eq!(edit_tour(&p, &json!({"tour": "nope", "fps": 24})).unwrap_err().code, "not_found");
        assert!(matches!(make_tour(&p, &json!({"name": "Walkthrough"})).unwrap().commands[0], Command::MakeTour { .. }));
    }
}
