//! Interchange import. Pure functions from file bytes to an `ImportInspection`
//! (what is in the file) and an `ImportPlan` (typed commands that put it in the
//! model). Nothing here touches the file system or the network: `guhit-app`
//! reads the bytes, converts DWG when needed, and commits the plan.
//!
//! Depends on `guhit-model` only, never on `guhit-core`.
//!
//! Two passes:
//! 1. `read` turns DXF entities into flat polylines in drawing units, one
//!    layer list, a bounding box and the declared unit.
//! 2. `walls` scales those to millimeters and, in `ImportMode::Walls`, pairs
//!    parallel overlapping segments into wall centerlines with a thickness.
//!    Whatever is left over becomes locked `Linework`.

use guhit_model::*;

pub mod read;
pub mod walls;

/// Largest file accepted. A floor plan DXF is normally well under 5 MB.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("the file is {0} MB. Import accepts files up to 10 MB")]
    TooLarge(u64),
    #[error("{0}")]
    NotDxf(String),
    #[error("the DXF file could not be read: {0}")]
    Parse(String),
    #[error("{0}")]
    Empty(String),
}

impl ImportError {
    /// IPC error code for this failure. Every import failure is the file's
    /// fault, never the app's, so they are all `invalid`.
    pub fn code(&self) -> &'static str {
        "invalid"
    }
}

/// Commands that put an inspected file into the model, plus what they do.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    /// Leaf commands, in order. Wrap them with `batch` before applying.
    pub commands: Vec<Command>,
    pub walls: u32,
    pub linework: u32,
    /// Plain lines about what did not come across, for the result dialog.
    pub skipped: Vec<String>,
}

impl ImportPlan {
    /// The one undo step an import makes.
    pub fn batch(&self, file_name: &str) -> Command {
        Command::Batch {
            label: format!("Import {file_name}"),
            commands: self.commands.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

fn check_size(bytes: &[u8]) -> Result<(), ImportError> {
    if bytes.len() > MAX_BYTES {
        return Err(ImportError::TooLarge((bytes.len() / (1024 * 1024)) as u64));
    }
    Ok(())
}

/// What a DXF file contains, before the user decides how to import it.
/// `detected_walls` per layer is a dry run of the recognizer at the suggested
/// unit, so the dialog can show which layers are worth importing.
pub fn inspect(file_name: &str, bytes: &[u8]) -> Result<ImportInspection, ImportError> {
    check_size(bytes)?;
    let raw = read::read(bytes)?;
    let mm = raw.suggested_mm_per_unit();
    let layers = raw
        .layers
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let segs = walls::segments_of_layer(&raw, i, mm, Point::default());
            ImportLayer {
                name: layer.name.clone(),
                entity_count: raw.paths.iter().filter(|p| p.layer == i).count() as u32,
                detected_walls: walls::recognize(&segs).len() as u32,
                color: layer.color.clone(),
            }
        })
        .filter(|l| l.entity_count > 0)
        .collect();
    Ok(ImportInspection {
        file_name: file_name.to_string(),
        declared_unit: raw.declared_unit.clone(),
        suggested_mm_per_unit: mm,
        layers,
        min: raw.min,
        max: raw.max,
    })
}

/// Typed commands for one import. Every element lands on `options.level_id`
/// and is offset by `options.offset` after scaling.
pub fn plan(file_name: &str, bytes: &[u8], options: &ImportOptions) -> Result<ImportPlan, ImportError> {
    check_size(bytes)?;
    let raw = read::read(bytes)?;
    let mm = if options.mm_per_unit.is_finite() && options.mm_per_unit > 0.0 {
        options.mm_per_unit
    } else {
        raw.suggested_mm_per_unit()
    };
    Ok(walls::build_plan(&raw, file_name, mm, options))
}

#[cfg(test)]
mod tests;
