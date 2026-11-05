//! Payloads that cross the IPC boundary. CONTRACT FILE - owned by the orchestrator.
//! The IPC command list lives in `docs/CONTRACT.md` and `src/contract/ipc.ts`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{Command, Origin};
use crate::derived::*;
use crate::live::*;
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
    /// Who made the step an undo would take back: a live session
    /// participant id. None outside a live session, and for steps made on the
    /// host before the session started.
    #[serde(default)]
    pub undo_by: Option<Id>,
    /// Who made the step a redo would bring back.
    #[serde(default)]
    pub redo_by: Option<Id>,
}

/// One step of the shared undo history, as `history_list` returns it
/// (DECISIONS D38). Kept in memory with the undo step: at most 200, gone
/// when the project closes. Versions stay the lasting record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HistoryStep {
    /// The step's name, as the undo button shows it: "Move wall".
    pub label: String,
    /// What changed, in one sentence (`Diff::summary`).
    pub summary: String,
    pub origin: Origin,
    /// The live session participant who made it. None for steps made
    /// outside a live session, which are this computer's own.
    pub author_id: Option<Id>,
    /// Their name and color at the time, kept after they leave, like chat
    /// messages keep theirs. None with `author_id` None.
    pub author_name: Option<String>,
    pub color: Option<u8>,
    /// When it was made, RFC 3339. Empty for steps made before this version.
    pub at: String,
    /// Elements the step added or changed, for "show me" (at most 200).
    pub ids: Vec<Id>,
    /// Elements the step removed.
    pub removed: Vec<Id>,
    /// For an agent's step: the person it worked for, and their name at the
    /// time. None for steps people made themselves.
    #[serde(default)]
    pub on_behalf_of: Option<Id>,
    #[serde(default)]
    pub on_behalf_name: Option<String>,
}

/// The undo and redo stacks, newest first: `undo[0]` is what an undo takes
/// back, `redo[0]` what a redo brings back.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HistoryList {
    pub undo: Vec<HistoryStep>,
    pub redo: Vec<HistoryStep>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Diff {
    pub added: Vec<Id>,
    pub modified: Vec<Id>,
    pub removed: Vec<Id>,
    /// One plain sentence, for example "Moved 1 wall, stretched 2 walls;
    /// also moved: Wall 4150 mm, Wall 3000 mm".
    pub summary: String,
    /// Walls the command reshaped although it did not name them: walls
    /// joined to a moved wall that stretched or shifted to follow it. Every
    /// edit result names them, so nobody misses a wall that moved on the far
    /// side of a joint. Also in `modified`. Left out of the JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_moved: Vec<Id>,
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
    /// "io", "ai_not_configured", "ai_failed", "unknown_command", "bad_args",
    /// "forbidden", "other_author" (undo or redo of someone else's step in a
    /// live session without `force`), "not_live", "host_only",
    /// "live_refused", "live_unreachable", "live_pin" (the host's certificate
    /// does not match the invite), "live_lost", "no_window".
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
    /// The title block DATE, "YYYY-MM-DD" on the computer's own calendar.
    /// The app fills it in with today's local date when it is None, so a
    /// sheet exported at 02:48 on 4 Nov in Manila reads 2026-11-04. Without
    /// it (an export outside the app) the sheet shows the day the project
    /// last changed, in UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
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
    /// What the export left out and why ("P-2 PLUMBING ISOMETRIC DIAGRAMS:
    /// the model has no water, drainage or vent runs"). A printed sheet never
    /// carries such a note itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

// --------------------------------------------------------- sheets (D53, D54)

/// `export_sheets`: the permit set, or some of its sheets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SheetExportOptions {
    /// PDF writes one file for the whole set; SVG one file per sheet; DXF
    /// the views side by side in model space.
    pub format: PlanFormat,
    /// Empty: every sheet, in set order.
    #[serde(default)]
    pub sheet_ids: Vec<Id>,
    /// Absolute path to write. None: the app asks or picks one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The title block DATE, "YYYY-MM-DD"; None: today, on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// A drawing as SVG: `sheet_preview` and `view_svg`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SvgDrawing {
    pub svg: String,
    /// The scale used, 1:N. None for a sheet, whose viewports each have one.
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
    /// Limits every edit of this turn to the selection (DECISIONS D30). None:
    /// the copilot may edit anything the user asks for.
    #[serde(default)]
    pub scope: Option<EditScope>,
    /// A task the copilot runs instead of a free chat turn. None: chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<AiTask>,
}

/// A copilot task with its own pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum AiTask {
    /// Furnish rooms: one proposal per room (`AiTurn::proposals`), each
    /// accepted or rejected on its own (D8).
    Furnish { room_ids: Vec<Id>, style: String },
}

/// Limits an AI edit to part of the plan (DECISIONS D30). The engine checks
/// every command the AI stages or commits against it (`guhit_core::scope`)
/// and refuses one that reaches outside, naming the element: the model reads
/// a tool error that starts with `out_of_scope:`. The copilot and MCP
/// clients are held to it alike.
///
/// What the selection reaches is derived, never stored: a room reaches its
/// bounding walls, the doors and windows on its part of them, and what
/// stands inside it; a wall reaches its doors and windows; any other element
/// reaches itself. New elements must land inside the area of the selection
/// on its level. Project-wide changes (roof, levels, layers, settings) are
/// outside every scope. Changes the engine makes as a consequence (connected
/// walls stretching, dimensions following, links removed) are allowed.
/// docs/CONTRACT.md, "AI edit scope", has the full rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EditScope {
    /// The selected elements. At least one.
    pub ids: Vec<Id>,
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
    /// A task with one proposal per room (`AiTask::Furnish`): each is
    /// resolved on its own with `ai_resolve`, and `proposal` is None. Left
    /// out of the JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<AiProposal>,
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
    /// A tour video (`RenderRecord::video`); the image is its poster frame.
    Video,
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
    /// The roof and the cutaway the image shows. None: not known (older
    /// records, tour videos). Left out of the JSON when None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<ViewVisibility>,
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
    /// Conditioning maps saved with a model view, for AI renders. The new
    /// record fields are left out of the JSON at their defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<RenderMap>,
    /// The project look: AI renders' style reference. At most one record.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub look: bool,
    /// For an AI visualization: how far it strayed from the model's lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift: Option<DriftReport>,
    /// A tour video; `image_path` is its poster frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoInfo>,
}

/// A conditioning map saved with a model view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderMap {
    pub kind: MapKind,
    /// Absolute path of the PNG inside the project folder.
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MapKind {
    Depth,
    Normal,
    /// Feature lines of the model.
    Lines,
    /// One flat color per element.
    Ids,
    /// Untextured, evenly lit.
    Clay,
}

/// What a model view's maps were drawn with, kept in
/// `renders/<id>.maps.json` (AI render v2, lane L6).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapsMeta {
    /// Size of every map, pixels.
    pub width: u32,
    pub height: u32,
    /// The roof and the cutaway the maps show. None: not known.
    #[serde(default)]
    pub visibility: Option<ViewVisibility>,
    /// The colors of the ids map, one per element in view.
    #[serde(default)]
    pub legend: Vec<LegendEntry>,
}

/// One color of the ids map.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LegendEntry {
    /// "#rrggbb" as drawn.
    pub color: String,
    /// The element, or "" for a part of the model that belongs to none.
    pub element_id: Id,
    /// How many pixels of the map it covers.
    #[serde(default)]
    #[ts(type = "number")]
    pub pixels: u64,
}

impl LegendEntry {
    /// The color as bytes. None for anything but "#rrggbb".
    pub fn rgb(&self) -> Option<[u8; 3]> {
        let hex = self.color.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some([byte(0)?, byte(2)?, byte(4)?])
    }
}

/// How far an AI image strayed from the model it was conditioned on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DriftReport {
    /// The share of model edges, 0 to 1, with no match in the AI image.
    pub missing: f64,
    /// Above the warning threshold: the image may show a different building.
    pub warn: bool,
    /// One sentence for the user.
    pub note: String,
}

/// A tour video file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VideoInfo {
    /// Absolute path inside the project folder (`renders/<id>.mp4`), or the
    /// frame folder for codec "png".
    pub path: String,
    pub tour_id: Id,
    pub seconds: f64,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    /// "avc", "vp9", or "png" for a folder of frames.
    pub codec: String,
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
    /// Send the capture's conditioning maps too. Left out: yes.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub use_maps: bool,
    /// The style reference image, a render id. None: the project look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_reference_id: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenderAiResult {
    pub record: RenderRecord,
    /// Seconds the provider took.
    pub seconds: f64,
}

/// Agents' AI images in this project today, against the cap (D53).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentUsage {
    /// This computer's date, "YYYY-MM-DD": the cap resets at local midnight.
    pub day: String,
    /// Agents' AI images in this project today.
    pub used: u32,
    /// Images agents may make per project per day (settings.json).
    pub cap: u32,
    pub default_cap: u32,
}

/// What one AI render will cost and send, shown before every call
/// (`render_ai_estimate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Estimate {
    /// Exact model id, "gemini-3.1-flash-image".
    pub model: String,
    /// "Gemini Flash", "Gemini Pro".
    pub model_label: String,
    /// "0.5K", "1K", "2K", "4K".
    pub image_size: String,
    /// Google list price per image, "0.10".
    pub price_usd: String,
    /// Why this model: "High quality", "the project look goes with it".
    pub why: String,
    pub pro: bool,
    /// The style reference that goes with it: the project look, or the one
    /// the request named. None: no reference.
    pub style_reference_id: Option<Id>,
    /// The maps that go with the capture.
    pub maps: Vec<MapKind>,
    /// Maps were asked for and the capture has none yet: the window draws
    /// them first.
    pub maps_missing: bool,
    /// The whole text the provider receives.
    pub prompt: String,
    pub agents: AgentUsage,
}

/// `render_ai_draft`'s answer: the image, checked, not saved yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Draft {
    /// Keep it with `render_ai_keep`, or drop it with `render_ai_discard`.
    pub pending_id: Id,
    /// The image, as a data URL.
    pub image: String,
    /// None: the capture had no line map, so it was not checked.
    pub drift: Option<DriftReport>,
    /// Model edges with no match, red on transparent, as a PNG data URL.
    pub overlay: Option<String>,
    pub estimate: Estimate,
    /// Seconds the provider took.
    pub seconds: f64,
    /// The model that made it, "gemini/gemini-3.1-flash-image".
    pub provider: String,
}

// ------------------------------------------------------------- app events

/// Pushed to the window: the Tauri event `app_event` in the desktop app,
/// `GET /events` (server-sent events) on the dev bridge. Document changes keep
/// their own `doc_changed` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum AppEvent {
    /// The live session changed: it started or ended, someone joined or
    /// left, the connection dropped or came back.
    Live { status: LiveStatus },
    /// Someone else's pointer, selection, level or cursor chat changed. None:
    /// they left, or their presence is gone.
    Presence {
        participant_id: Id,
        presence: Option<Presence>,
    },
    /// A chat message was sent in the live session, by anyone, this computer
    /// included.
    Chat { message: ChatMessage },
    /// The backend needs the window to do something only the window can:
    /// render or capture. Answer with `window_reply`.
    WindowRequest { request: WindowRequest },
    /// The agents working on this computer's project changed: one connected,
    /// moved, changed its status, was paused or stopped (DECISIONS D47). The
    /// full list, newest first.
    Agents { agents: Vec<AgentSummary> },
}

// ---------------------------------------------------------- window requests

/// A task for the window, because the 3D renderer and the plan canvas live
/// there (DECISIONS D31). MCP render and capture tools use it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WindowRequest {
    pub id: Id,
    pub task: WindowTask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum WindowTask {
    /// Render with the path tracer and save every image to Visuals, as the
    /// Render button does. `views`: camera ids of saved views, rendered one
    /// after the other; empty renders the current 3D view.
    Render {
        views: Vec<Id>,
        quality: TraceQuality,
        size: RenderSize,
        /// What the images show. None: what each saved view stored, else
        /// the model's defaults (roof on, no cutaway); never whatever the
        /// window happens to show (an agent cannot see the window's toggles).
        #[serde(default)]
        visibility: Option<ViewVisibility>,
    },
    /// Save a capture of the live 3D view (Tier 1), seen from a saved view
    /// when `camera_id` is set, else from `camera` when set.
    CaptureView {
        camera_id: Option<Id>,
        /// As for `Render`.
        #[serde(default)]
        visibility: Option<ViewVisibility>,
        /// An unsaved camera: a tour preview, or the `look` camera. Ignored
        /// when `camera_id` is set. Left out of the JSON when None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        camera: Option<Camera>,
    },
    /// Draw the plan of a level (None: the level on screen) on white and
    /// answer it as `WindowReply::image`. Nothing is saved.
    CapturePlan { level_id: Option<Id> },
    /// Render the conditioning maps of a saved model view and attach them to
    /// its record (`RenderRecord::maps`).
    CaptureMaps { render_id: Id },
    /// Export a tour as a video to Visuals: a long job through
    /// `window_start` and `window_wait`. The reply's `render_ids` holds the
    /// video record.
    ExportTour { tour_id: Id, size: RenderSize },
}

/// What a render or capture asked for from outside the window shows
/// (DECISIONS D31): the roof, and the active level's cutaway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ViewVisibility {
    pub roof: bool,
    pub cutaway: bool,
}

/// Render sizes, as the render panel offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RenderSize {
    /// 1920 x 1080.
    Hd,
    /// 2560 x 1440.
    Qhd,
    /// 3840 x 2160.
    #[serde(rename = "4k")]
    #[ts(rename = "4k")]
    Uhd,
    /// 2048 x 2048.
    Square,
}

/// What the window answers to a `WindowRequest`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WindowReply {
    /// Records saved to Visuals (render, capture_view), in order.
    #[serde(default)]
    pub render_ids: Vec<Id>,
    /// PNG or JPEG data URL: the plan (capture_plan), or a preview of the
    /// first saved image at most 1568 px on its long side.
    #[serde(default)]
    pub image: Option<String>,
    /// One sentence for the caller: how the render went, what was saved, or
    /// why it is an enhanced capture instead of a path traced render.
    #[serde(default)]
    pub note: String,
}

// -------------------------------------------------------------------- MCP

/// The MCP server as the Connect agent dialog shows it (DECISIONS D35).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpStatus {
    /// The "Allow agents" switch. Off: every MCP request is refused.
    /// Stored in settings.json as `mcp_enabled`, on by default.
    pub enabled: bool,
    /// True while the MCP listener is bound.
    pub listening: bool,
    /// The port it serves on, or tried to.
    pub port: u16,
    /// `http://127.0.0.1:{port}/mcp`.
    pub url: String,
    /// Absolute path of the desktop executable, to run with `--mcp-stdio`.
    /// None in the dev bridge.
    pub stdio_command: Option<String>,
    /// The agent that used the server last, since the app started.
    pub last_client: Option<McpClientSeen>,
    /// True when `mcp_claude_desktop_bundle` can write the extension file.
    pub claude_desktop_bundle: bool,
    /// The agents working on the project now (DECISIONS D47).
    #[serde(default)]
    pub agents: Vec<AgentSummary>,
}

/// An MCP client as it named itself in `clientInfo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpClientSeen {
    pub name: String,
    pub version: String,
    /// RFC 3339 UTC: its latest request.
    pub at: String,
}
