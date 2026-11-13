//! What an accept would do (`swarm_check`), and the app's gate for what an
//! agent bound to a swarm part reads (`AppService::read_view`) and may not
//! ask for (`AppService::refuse_draft_view`).
//!
//! Lane L1 owns this file.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use guhit_core::Document;
use guhit_model::*;
use tokio::sync::MutexGuard;

use super::draft::{self, Draft};
use crate::{AppService, Session};

/// What one read of an agent sees: the model, or the draft it draws in.
pub enum ReadView<'a> {
    /// The session, locked for as long as this lives.
    Main(MutexGuard<'a, Session>),
    /// The draft view: no lock held.
    Draft(Arc<Document>),
}

impl ReadView<'_> {
    /// The document, or None while the hub shows.
    pub fn doc(&self) -> Option<&Document> {
        match self {
            ReadView::Main(s) => s.doc.as_ref(),
            ReadView::Draft(d) => Some(d),
        }
    }

    /// The document; `no_document` while the hub shows.
    pub fn open_doc(&self) -> Result<&Document, IpcError> {
        self.doc().ok_or_else(crate::no_document)
    }

    pub fn is_draft(&self) -> bool {
        matches!(self, ReadView::Draft(_))
    }
}

/// What the window draws or what is written from the model: it shows the
/// model, never the draft, so an agent drawing a part is refused it
/// (`draft_view`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOutput {
    /// A render, a 3D capture.
    Render,
    /// An AI visualization.
    RenderAi,
    /// A tour video or its frames.
    TourExport,
    /// A file export: plan, sheets, DXF, IFC, a bundle.
    Export,
    /// A saved version.
    SaveVersion,
    /// A look the window draws (its plan, a tour frame).
    WindowLook,
    /// The 3D view.
    View3d,
}

impl WindowOutput {
    fn what(self) -> &'static str {
        match self {
            WindowOutput::Render => "A render",
            WindowOutput::RenderAi => "An AI visualization",
            WindowOutput::TourExport => "A tour video",
            WindowOutput::Export => "An export",
            WindowOutput::SaveVersion => "A saved version",
            WindowOutput::WindowLook => "A look the window draws",
            WindowOutput::View3d => "The 3D view",
        }
    }
}

/// The refusal for `what`.
pub fn draft_view(what: WindowOutput) -> IpcError {
    IpcError::new(
        "draft_view",
        format!(
            "{} shows the model, not the swarm draft you draw in. Check your work with look plan, measure and check; it works once a person accepts the draft.",
            what.what()
        ),
    )
}

impl AppService {
    /// What `agent` reads (None: a person or an in-process caller): the
    /// draft it draws in when it is bound to a swarm part, else the model.
    /// Every read of a bound agent goes through here (D57).
    pub async fn read_view(&self, agent: Option<&str>) -> ReadView<'_> {
        match agent.and_then(|a| self.swarm.view_for(a)) {
            Some(view) => ReadView::Draft(view),
            None => ReadView::Main(self.session.lock().await),
        }
    }

    /// Refuse `what` to an agent bound to a swarm part (`draft_view`).
    /// Everyone else may: renders, exports and versions of the model work
    /// while a draft is open.
    pub fn refuse_draft_view(&self, agent: Option<&str>, what: WindowOutput) -> Result<(), IpcError> {
        match agent {
            Some(a) if self.swarm.part_of(a).is_some() => Err(draft_view(what)),
            _ => Ok(()),
        }
    }

    /// What accepting `parts` (None: all) with the parts they depend on
    /// would do, changing nothing: the parts that would commit, in order
    /// (`applied`, revision 0, the summary each step would have), the steps
    /// that would stay out (`skipped`: the accept is refused
    /// `steps_skipped` unless `accept_rest`), the parts that depend on
    /// `parts` and that discarding them would take too (`dependents`), and
    /// the elements the model would get differently from the draft view
    /// (`differs`).
    pub async fn swarm_check(&self, parts: Option<&[String]>) -> Result<SwarmAcceptResult, IpcError> {
        let main = {
            let s = self.session.lock().await;
            s.open_doc()?.project().clone()
        };
        let (snap, closure, view, dependents, names) = {
            let g = self.swarm.lock();
            let d = g.as_ref().ok_or_else(|| IpcError::new("not_found", "No swarm draft is open."))?;
            let closure = closure_of(d, parts)?;
            let dependents = parts.map(|keys| d.dependents(keys)).unwrap_or_default();
            let names: BTreeMap<String, String> = d.parts.iter().map(|p| (p.part.key.clone(), p.part.name.clone())).collect();
            (d.snapshot(), closure, d.view.clone(), dependents, names)
        };
        let plan = draft::plan(&snap, &main, &closure);
        // Only the parts that change the model: an accept commits no other.
        let applied =
            plan.commits.iter().map(|c| SwarmApplied { part: c.part.clone(), revision: 0, summary: summary_of(c) }).collect();
        let differs = differs(&snap, &closure, &main, &plan.main, view.project(), &names);
        Ok(SwarmAcceptResult { applied, skipped: plan.skipped, state: None, dependents, differs })
    }
}

/// The parts a check of `parts` (None: all) covers, as an accept takes them.
fn closure_of(d: &Draft, parts: Option<&[String]>) -> Result<BTreeSet<String>, IpcError> {
    let keys: Vec<String> = match parts {
        None => d.parts.iter().map(|p| p.part.key.clone()).collect(),
        Some(keys) => {
            for k in keys {
                if d.part(k).is_none() {
                    return Err(IpcError::new("not_found", format!("The swarm has no part {k}.")));
                }
            }
            keys.to_vec()
        }
    };
    Ok(d.closure(&keys).into_iter().filter(|k| d.part(k).is_some_and(|p| p.part.state != SwarmPartState::Accepted)).collect())
}

/// The summary the part's undo step would have.
fn summary_of(c: &draft::PartCommit) -> String {
    let mut doc = Document::with_revision(c.base.clone(), 0);
    let replayed = guhit_core::Replayed { base: c.base.clone(), project: c.project.clone(), label: format!("Swarm: {}", c.name) };
    doc.apply_replayed(replayed, Origin::Ai, None, "").map(|r| r.diff.summary).unwrap_or_else(|_| format!("Swarm: {}", c.name))
}

/// Elements the closure's steps touch that `after` (the model after the
/// accept) holds differently from `view`, and elements it would add that
/// the view does not have. One `differs` entry each, under the part whose
/// step touched it last.
fn differs(
    snap: &draft::Snapshot,
    closure: &BTreeSet<String>,
    main: &Project,
    after: &Project,
    view: &Project,
    names: &BTreeMap<String, String>,
) -> Vec<SwarmConflict> {
    let mut last: BTreeMap<&Id, &draft::RecordedStep> = BTreeMap::new();
    for s in snap.steps.iter().filter(|s| closure.contains(&s.part)) {
        for id in s.added.iter().chain(&s.modified) {
            last.insert(id, s);
        }
    }
    let in_view: BTreeMap<&Id, &Element> = view.elements.iter().map(|e| (e.id(), e)).collect();
    let in_main: BTreeSet<&Id> = main.elements.iter().map(|e| e.id()).collect();
    let mut out = vec![];
    for el in &after.elements {
        let id = el.id();
        match (in_view.get(id), last.get(id)) {
            (Some(v), Some(step)) if *v != el => {
                let fields = fields_apart(v, el);
                let who = names.get(&step.part).cloned().unwrap_or_else(|| step.part.clone());
                let message = match (v, el) {
                    // Auto rooms are named by count, so the name moves.
                    (Element::Room(d), Element::Room(m)) if d.name != m.name && fields == "name" => {
                        format!("{} by {who} would be named \"{}\" in the model.", noun(v), m.name)
                    }
                    _ => format!("{} by {who} would come out differently from the draft ({fields}).", noun(v)),
                };
                out.push(SwarmConflict {
                    part: step.part.clone(),
                    step: step.seq,
                    label: step.label.clone(),
                    code: "differs".into(),
                    message,
                    element_ids: vec![id.clone()],
                });
            }
            (None, _) if !in_main.contains(id) => out.push(SwarmConflict {
                part: String::new(),
                step: 0,
                label: String::new(),
                code: "differs".into(),
                message: format!("The model would get {} the draft does not show.", with_article(&noun(el).to_lowercase())),
                element_ids: vec![id.clone()],
            }),
            _ => {}
        }
    }
    out
}

/// The top-level fields two versions of an element differ in: "name, seed".
fn fields_apart(a: &Element, b: &Element) -> String {
    let (a, b) = (serde_json::to_value(a).unwrap_or_default(), serde_json::to_value(b).unwrap_or_default());
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else { return "its shape".into() };
    let keys: Vec<&str> =
        a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).map(|k| k.as_str()).collect::<BTreeSet<_>>().into_iter().collect();
    if keys.is_empty() {
        "its shape".into()
    } else {
        keys.iter().map(|k| k.replace('_', " ")).collect::<Vec<_>>().join(", ")
    }
}

/// "Room \"Kitchen\"", "Wall", "Door".
fn noun(el: &Element) -> String {
    let kind = serde_json::to_value(el.kind()).ok().and_then(|v| v.as_str().map(|s| s.replace('_', " "))).unwrap_or_default();
    let mut kind = {
        let mut c = kind.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    if let Element::Room(r) = el {
        kind = format!("Room \"{}\"", r.name);
    }
    kind
}

fn with_article(noun: &str) -> String {
    let an = noun.chars().next().is_some_and(|c| "aeiou".contains(c));
    format!("{} {noun}", if an { "an" } else { "a" })
}
