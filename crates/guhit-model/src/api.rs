//! Payloads that cross the IPC boundary. CONTRACT FILE - owned by the orchestrator.
//! The IPC command list lives in `docs/CONTRACT.md` and `src/contract/ipc.ts`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::Command;
use crate::derived::*;
use crate::model::*;

/// Full document state. Sent to the frontend after every change. Projects are
/// small (hundreds of elements), so there is no partial sync.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocState {
    pub project: Project,
    pub derived: Derived,
    /// Increments on every apply, undo and redo.
    pub revision: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Diff {
    pub added: Vec<Id>,
    pub modified: Vec<Id>,
    pub removed: Vec<Id>,
    /// One plain sentence, for example "Moved 1 wall, stretched 2 walls".
    pub summary: String,
}

/// Result of `apply` and of `preview`. For a preview, `state` is hypothetical
/// and nothing was committed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApplyResult {
    pub state: DocState,
    pub diff: Diff,
}

/// Error shape for every IPC call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IpcError {
    /// Machine code: "not_found", "invalid", "no_document", "stale",
    /// "io", "ai_not_configured", "ai_failed", "unknown_command", "bad_args".
    pub code: String,
    pub message: String,
    pub element_ids: Vec<Id>,
}

impl IpcError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            element_ids: vec![],
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for IpcError {}

// ---------------------------------------------------------------- project hub

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectMeta {
    pub id: Id,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub floor_area_m2: f64,
    pub room_count: u32,
    /// PNG data URL, or None when no thumbnail was saved yet.
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SnapshotMeta {
    pub id: Id,
    pub label: String,
    pub created_at: String,
    pub revision: u32,
    /// True for automatic snapshots, false for user-named versions.
    pub auto: bool,
}

// --------------------------------------------------------------------- export

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PlanFormat {
    Pdf,
    Svg,
    Dxf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Orientation {
    Landscape,
    Portrait,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanExportOptions {
    pub level_id: Option<Id>,
    pub paper: PaperSize,
    pub orientation: Orientation,
    /// 1:N. None picks the largest common scale that fits the sheet.
    pub scale_denominator: Option<u32>,
    pub show_dimensions: bool,
    pub show_room_labels: bool,
    pub show_assets: bool,
    pub title_block: bool,
    /// Draw pipes on visible pipe layers, with a legend. DXF puts each system
    /// on its own layer. Older callers that omit it get true.
    #[serde(default = "default_true")]
    pub show_pipes: bool,
    /// Which sheet to make. Older callers that omit it get the plan.
    #[serde(default)]
    pub sheet: SheetKind,
    /// Add a page listing open and set-aside review items with their notes
    /// (PDF only). Wording stays "suggestion", never approval.
    #[serde(default)]
    pub review_page: bool,
}

/// The plan sheets a PH permit and hand-off set asks for. Every sheet keeps
/// the title block; the signing professional's fields stay blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SheetKind {
    /// The architectural plan. Pipes follow `show_pipes`.
    #[default]
    Plan,
    /// Lighting layout: fixtures, switches and their links, a legend and counts.
    Lighting,
    /// Power layout: outlets, special purpose outlets, panelboard, conduit,
    /// a legend, counts and a schedule of loads with blank ratings for the PEE.
    Power,
    /// Plumbing layout: water, drainage, vent and storm, legend, fixture table.
    Plumbing,
    /// Water and sanitary isometric diagrams, not to scale, with a legend.
    PlumbingIsometric,
    /// Aircon layout: units, line sets, condensate, core holes, a legend.
    Aircon,
}

fn default_true() -> bool {
    true
}

/// Whole-model exports produced by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ModelFormat {
    /// IFC4 STEP file: walls, openings, doors, windows, spaces, slabs, roof,
    /// pipe segments grouped into distribution systems.
    Ifc,
    /// 3D DXF with 3DFACE entities, layered like the 2D DXF. Pipes are
    /// tubes on their system layer.
    Dxf3d,
    /// DWG through the ODA File Converter, when configured.
    Dwg,
}

// --------------------------------------------------------------- interchange

/// What a DXF or DWG file contains, before the user decides how to import it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportInspection {
    pub file_name: String,
    /// Unit declared by the file ($INSUNITS), or None when it declares none.
    pub declared_unit: Option<String>,
    /// mm per drawing unit the app suggests. The user must confirm it.
    pub suggested_mm_per_unit: f64,
    pub layers: Vec<ImportLayer>,
    /// Bounding box in drawing units.
    pub min: Point,
    pub max: Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportLayer {
    pub name: String,
    pub entity_count: u32,
    /// Walls the recognizer would produce from this layer at the suggested unit.
    pub detected_walls: u32,
    /// "#rrggbb" from the layer color, for the preview.
    pub color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ImportMode {
    /// Recognize parallel line pairs as walls with thickness; unmatched lines become linework.
    Walls,
    /// Everything becomes locked linework to trace over.
    Linework,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportOptions {
    pub mm_per_unit: f64,
    /// Layers to bring in. Empty means all.
    pub layers: Vec<String>,
    pub mode: ImportMode,
    /// Plan offset applied after scaling, so a drawing far from its origin lands near 0,0.
    pub offset: Point,
    pub level_id: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportResult {
    pub state: DocState,
    pub walls_added: u32,
    pub linework_added: u32,
    pub skipped: Vec<String>,
}

/// State of the optional ODA File Converter used for DWG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DwgConverterStatus {
    pub configured: bool,
    pub path: Option<String>,
    pub works: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExportResult {
    /// Absolute path of the written file.
    pub path: String,
    /// Scale that was actually used, 1:N.
    pub scale_denominator: Option<u32>,
}

// ------------------------------------------------------------------------- AI

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiSettings {
    /// The key itself never crosses IPC toward the frontend.
    pub has_api_key: bool,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AiRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiMessage {
    pub role: AiRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiRequest {
    pub message: String,
    pub selection_ids: Vec<Id>,
    /// Level the user is working on. New elements go here. None means the first level.
    #[serde(default)]
    pub active_level_id: Option<Id>,
    /// Prior turns of this conversation, oldest first.
    pub history: Vec<AiMessage>,
}

/// A change the copilot wants to make. Nothing is committed until the user
/// accepts it with `ai_resolve`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiProposal {
    pub id: Id,
    /// Always a `Command::Batch`.
    pub command: Command,
    /// Dry run against the revision the proposal was made on.
    pub preview: ApplyResult,
    pub base_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiTurn {
    pub reply: String,
    pub proposal: Option<AiProposal>,
    /// Names of the tools the model called, for transparency.
    pub tools_used: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiResolveResult {
    /// Present when the proposal was accepted and applied.
    pub applied: Option<ApplyResult>,
}

// -------------------------------------------------------------------- renders

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderStyle {
    pub key: String,
    pub name: String,
    pub description: String,
    /// Prompt fragment sent to the image provider.
    pub prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RenderSource {
    /// Tier 1: deterministic capture of the live 3D view.
    ModelView,
    /// Tier 2: generative image conditioned on a model view.
    AiVisualization,
}

/// How a model view image was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RenderKind {
    /// The live 3D view as it was on screen.
    Capture,
    /// A path traced render.
    PathTraced,
    /// The live view refined with jittered samples, when the path tracer
    /// cannot run on this computer.
    Enhanced,
    /// A contact sheet of sun positions.
    ShadowStudy,
}

/// The path tracer's time and sample budget: Quick aims at a minute at HD,
/// Final at five.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TraceQuality {
    Quick,
    Final,
}

/// How a model view image was made, kept with its record so every computer
/// that opens the project can say so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderInfo {
    pub kind: RenderKind,
    /// Image size in pixels.
    pub width: u32,
    pub height: u32,
    /// Samples per pixel of a path traced render, else 0.
    #[serde(default)]
    pub samples: u32,
    /// How long it took, seconds.
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub quality: Option<TraceQuality>,
    /// The graphics adapter it ran on, as the browser names it. Empty when
    /// unknown.
    #[serde(default)]
    pub gpu: String,
}

/// A saved visual, always tied to the model revision and camera it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderRecord {
    pub id: Id,
    pub created_at: String,
    pub source: RenderSource,
    pub revision: u32,
    pub camera: Camera,
    pub style_key: Option<String>,
    pub prompt: String,
    /// Absolute path of the PNG inside the project folder.
    pub image_path: String,
    /// For an AI visualization: the model-view capture it was made from, so
    /// the UI can show the two side by side.
    #[serde(default)]
    pub source_render_id: Option<Id>,
    /// For an AI visualization: provider and model, for example "gemini/gemini-3.1-flash-image".
    #[serde(default)]
    pub provider: Option<String>,
    /// For a model view: how it was made. None for older records and AI
    /// visualizations.
    #[serde(default)]
    pub info: Option<RenderInfo>,
}

// ------------------------------------------------------- AI visualization

/// Which hosted image model renders visualizations. Keys never reach the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderAiSettings {
    /// "gemini" for now; the abstraction allows others later.
    pub provider: String,
    pub has_api_key: bool,
    pub model: String,
    /// Plain wording of the cost per image at the current model, for the UI.
    pub cost_hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RenderQuality {
    /// About 0.5K, cheapest, for quick looks.
    Draft,
    /// 1K to 2K, for client images.
    Standard,
    /// 4K where the model allows it.
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderAiRequest {
    /// The Tier 1 capture (source ModelView) to condition on.
    pub source_render_id: Id,
    pub style_key: Option<String>,
    /// Free text: building type, materials, mood, time of day.
    pub prompt: String,
    pub quality: RenderQuality,
    /// True keeps geometry, camera and composition strictly (the default).
    pub keep_geometry: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderAiResult {
    pub record: RenderRecord,
    /// Seconds the provider took.
    pub seconds: f64,
}
