//! Plan export. Pure functions from model + derived data to file contents.
//! Depends on `guhit-model` only, never on `guhit-core`.
//!
//! PUBLIC API IS CONTRACT: `guhit-app` calls `plan_svg`, `plan_pdf`,
//! `plan_dxf`, `model_ifc` and `model_dxf3d`.
//!
//! How it fits together: `plan` turns one level into drawing primitives in
//! model millimeters. `sheet` lays them out on paper as SVG, `pdf` converts
//! that SVG to a vector PDF, `dxf` writes the same primitives as 1:1 DXF.
//!
//! The whole-model exports go the other way: `model3d` turns the project into
//! solids in model millimeters, `ifc` writes them as IFC4 STEP and `dxf3d` as
//! 3DFACE geometry with the 2D linework alongside it.
//!
//! Pipes (`pipes`) have their own styling in every format: system colors, a
//! weight from the pipe size on the sheet, one DXF layer per system, tubes
//! in 3D and IfcPipeSegments grouped into IfcDistributionSystems in IFC.
//! A project without pipes exports exactly as it did before pipes existed.

use guhit_model::{Derived, PlanExportOptions, Project};

pub mod dxf;
pub mod dxf3d;
pub mod geom;
pub mod ifc;
pub mod model3d;
pub mod pdf;
pub mod pipes;
pub mod plan;
pub mod sheet;
pub mod text;

pub use sheet::{paper_size_mm, pick_scale, COMMON_SCALES};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("nothing to export: {0}")]
    Empty(String),
    #[error("export failed: {0}")]
    Failed(String),
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
    let (data, scale_denominator) = sheet::render(project, derived, opts)?;
    Ok(PlanOutput {
        data,
        scale_denominator,
    })
}

/// Same sheet as `plan_svg`, as a vector PDF at true paper size.
pub fn plan_pdf(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<PlanOutput<Vec<u8>>, ExportError> {
    let (svg, scale_denominator) = sheet::render(project, derived, opts)?;
    let data = pdf::svg_to_pdf(&svg)?;
    Ok(PlanOutput {
        data,
        scale_denominator,
    })
}

/// Model-space DXF in mm, 1:1, layered by element category, pipes on one
/// layer per system when `opts.show_pipes`.
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
    let level = plan::resolve_level(project, opts.level_id.as_ref())?;
    let scale = opts
        .scale_denominator
        .filter(|n| *n > 0)
        .or(Some(project.settings.scale_denominator).filter(|n| *n > 0))
        .unwrap_or(100);
    let items = plan::build_items(
        project,
        derived,
        level,
        &plan::PlanOptions {
            scale: scale as f64,
            show_dimensions: opts.show_dimensions,
            show_room_labels: opts.show_room_labels,
            show_assets: opts.show_assets,
            unicode: false,
        },
    );
    let pipe_list = if opts.show_pipes {
        pipes::plan_pipes(project, level)
    } else {
        Vec::new()
    };
    if items.is_empty() && pipe_list.is_empty() {
        return Err(plan::empty_level(level));
    }
    Ok(PlanOutput {
        data: dxf::write(&items, &pipe_list, scale),
        scale_denominator: scale,
    })
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
