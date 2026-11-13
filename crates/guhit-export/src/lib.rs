//! Plan export. Pure functions from model + derived data to file contents.
//! Depends on `guhit-model` only, never on `guhit-core`.
//!
//! PUBLIC API IS CONTRACT: `guhit-app` calls `plan_svg`, `plan_pdf`,
//! `plan_dxf`, `model_ifc` and `model_dxf3d`, and for the Wave 2 sheet set
//! (plan 1.6) `view_svg`, `sheet_svg`, `sheets_pdf` and `sheets_dxf`.
//!
//! HUB (DECISIONS D54): after C0 only the orchestrator edits this file.
//! Wave 2 owners: `views/` L3; `sheetset`, `titleblock`, `tables` L4;
//! `roofs`, `stairs` and `model3d` L2; `dxf` (plans and `sheets_dxf`),
//! `dxf3d`, `ifc` L8.
//!
//! How it fits together: `plan` turns one level into drawing primitives in
//! model millimeters. `sheet` lays them out on paper as SVG, `pdf` converts
//! that SVG to a vector PDF, `dxf` writes the same primitives as 1:1 DXF
//! 2013. `views` draws every other view (elevations, sections, roof, site,
//! ceiling) as the same primitives.
//!
//! The whole-model exports go the other way: `model3d` turns the project into
//! solids in model millimeters, `ifc` writes them as IFC4 STEP and `dxf3d` as
//! 3DFACE geometry with the 2D linework alongside it.
//!
//! Pipes (`pipes`) have their own styling in every format: system colors, a
//! weight from the pipe size on the sheet, one DXF layer per system, tubes
//! in 3D and IfcPipeSegments grouped into IfcDistributionSystems in IFC.
//! A project without pipes exports exactly as it did before pipes existed.

use std::collections::BTreeMap;

use guhit_model::{Derived, Id, PlanExportOptions, Project, SheetKind, ViewRef};

pub mod dxf;
pub mod dxf3d;
pub mod geom;
pub mod ifc;
pub mod iso;
pub mod model3d;
pub(crate) mod panel;
pub mod pdf;
pub mod pipes;
pub mod plan;
pub mod review;
pub mod roofs;
pub(crate) mod service_sheet;
pub mod services;
pub mod sheet;
pub mod sheetset;
pub mod stairs;
pub mod symbols;
pub mod tables;
pub mod text;
pub mod titleblock;
pub mod views;

pub use sheet::{paper_size_mm, pick_scale, COMMON_SCALES};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("nothing to export: {0}")]
    Empty(String),
    #[error("export failed: {0}")]
    Failed(String),
    /// A Wave 2 drawing whose lane has not built it yet.
    #[error("{0} is not built yet.")]
    NotBuilt(String),
}

/// What sheets and views need besides the model (plan 1.6).
#[derive(Debug, Clone, Default)]
pub struct SheetInputs {
    /// The title block DATE, "YYYY-MM-DD". None: the day the project last
    /// changed, in UTC.
    pub date: Option<String>,
    /// Image bytes by file name or render id: the firm logo, the vicinity
    /// map, perspectives.
    pub images: BTreeMap<String, Vec<u8>>,
    /// Print `Signatory::masked()`: for agents' pictures and anything sent
    /// to an AI provider (DECISIONS D53). The PDF on disk prints in full.
    pub mask_signatory: bool,
}

/// One view alone as SVG at 1:`scale` (None: the largest common scale that
/// fits). Lane L3.
pub fn view_svg(
    p: &Project,
    d: &Derived,
    view: &ViewRef,
    scale: Option<u32>,
    i: &SheetInputs,
) -> Result<PlanOutput<String>, ExportError> {
    views::view_svg(p, d, view, scale, i)
}

/// One sheet of the set as SVG at true paper size. Lane L4.
pub fn sheet_svg(p: &Project, d: &Derived, sheet_id: &str, i: &SheetInputs) -> Result<String, ExportError> {
    sheetset::sheet_svg(p, d, sheet_id, i)
}

/// The sheets `ids` (empty: every sheet, in set order) as one PDF. Lane L4.
pub fn sheets_pdf(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<Vec<u8>, ExportError> {
    sheetset::sheets_pdf(p, d, ids, i)
}

/// The sheets' views side by side in model space. Lane L3.
pub fn sheets_dxf(p: &Project, d: &Derived, ids: &[Id]) -> Result<String, ExportError> {
    dxf::sheets(p, d, ids)
}

/// Output of a plan export.
pub struct PlanOutput<T> {
    pub data: T,
    /// Scale actually used, 1:N.
    pub scale_denominator: u32,
}

/// Scaled plan sheet as an SVG document. Sheet size is the real paper size in mm.
pub fn plan_svg(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<PlanOutput<String>, ExportError> {
    let (data, scale_denominator) = render_sheet(project, derived, opts)?;
    Ok(PlanOutput {
        data,
        scale_denominator,
    })
}

/// The SVG of the sheet `opts.sheet` asks for.
fn render_sheet(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<(String, u32), ExportError> {
    match opts.sheet {
        SheetKind::Plan => sheet::render(project, derived, opts),
        SheetKind::PlumbingIsometric => iso::render(project, derived, opts),
        SheetKind::ReflectedCeiling => views::rcp::sheet(project, derived, opts),
        _ => service_sheet::render(project, derived, opts),
    }
}

/// Same sheet as `plan_svg`, as a vector PDF at true paper size.
pub fn plan_pdf(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<PlanOutput<Vec<u8>>, ExportError> {
    let (svg, scale_denominator) = render_sheet(project, derived, opts)?;
    let data = if opts.review_page {
        let mut pages = vec![svg];
        pages.extend(review::pages(project, derived, opts));
        pdf::svgs_to_pdf(&pages)?
    } else {
        pdf::svg_to_pdf(&svg)?
    };
    Ok(PlanOutput {
        data,
        scale_denominator,
    })
}

/// Model-space DXF 2013 (AC1027) in mm, 1:1: blocks per door and window type
/// and catalog item, DIMENSION entities, solid hatches, pipes on one layer per
/// system when `opts.show_pipes`.
///
/// Paper and title block options do not apply to model space. The scale only
/// sizes text, ticks, arrows, riser marks and pipe dashes so they plot at the
/// usual paper size at 1:N: `opts.scale_denominator`, else the project scale,
/// else 100.
pub fn plan_dxf(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<PlanOutput<String>, ExportError> {
    match opts.sheet {
        SheetKind::Plan => dxf::plan(project, derived, opts),
        SheetKind::PlumbingIsometric => {
            let (data, scale_denominator) = iso::dxf(project, opts)?;
            Ok(PlanOutput { data, scale_denominator })
        }
        SheetKind::ReflectedCeiling => {
            let (data, scale_denominator) = views::rcp::dxf(project, derived, opts)?;
            Ok(PlanOutput { data, scale_denominator })
        }
        _ => {
            let (data, scale_denominator) = service_sheet::dxf(project, derived, opts)?;
            Ok(PlanOutput { data, scale_denominator })
        }
    }
}

/// The whole model as an IFC4 STEP file (ISO-10303-21), millimeters.
///
/// One IfcBuildingStorey per level, walls with their mitred outlines, openings
/// cut with IfcRelVoidsElement and filled by IfcDoor or IfcWindow, IfcSpace
/// per room, IfcSlab per footprint, a roof, columns, stairs, furnishing,
/// annotations, and one IfcPipeSegment per straight pipe segment grouped into
/// one IfcDistributionSystem per pipe system. Ids are derived from the element
/// ids, so a re-export of an unchanged project produces the same
/// IfcGloballyUniqueIds.
pub fn model_ifc(project: &Project, derived: &Derived) -> Result<String, ExportError> {
    ifc::write(project, derived)
}

/// The whole model as a 3D DXF in millimeters: 3DFACE solids on the 2D layers
/// plus A-ROOF and A-FLOR-SLAB, pipes as closed tubes on their system layers,
/// and the 2D plan linework of every level at z = 0 so one file serves both
/// uses.
pub fn model_dxf3d(project: &Project, derived: &Derived) -> Result<String, ExportError> {
    dxf3d::write(project, derived)
}
