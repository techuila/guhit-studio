//! Engine errors. PUBLIC API IS CONTRACT: `guhit-app` depends on `CoreError`
//! and on its conversion to `IpcError`.

use guhit_model::*;

#[derive(Debug, Clone, thiserror::Error)]
pub enum CoreError {
    #[error("element not found: {0}")]
    NotFound(Id),
    #[error("{message}")]
    Invalid {
        /// Machine code, for example "wall_too_short", "opening_outside_wall".
        code: String,
        message: String,
        element_ids: Vec<Id>,
    },
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}

impl CoreError {
    pub fn invalid(code: &str, message: impl Into<String>) -> Self {
        CoreError::Invalid {
            code: code.to_string(),
            message: message.into(),
            element_ids: vec![],
        }
    }

    /// Same as `invalid`, naming the elements the message is about.
    pub fn invalid_for(code: &str, message: impl Into<String>, element_ids: Vec<Id>) -> Self {
        CoreError::Invalid {
            code: code.to_string(),
            message: message.into(),
            element_ids,
        }
    }

    /// The machine code of an `Invalid` error, or a fixed code for the rest.
    pub fn code(&self) -> &str {
        match self {
            CoreError::NotFound(_) => "not_found",
            CoreError::Invalid { code, .. } => code,
            CoreError::NothingToUndo => "nothing_to_undo",
            CoreError::NothingToRedo => "nothing_to_redo",
        }
    }
}

impl From<CoreError> for IpcError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotFound(id) => IpcError {
                code: "not_found".into(),
                message: format!("element not found: {id}"),
                element_ids: vec![id],
            },
            CoreError::Invalid {
                message,
                element_ids,
                ..
            } => IpcError {
                code: "invalid".into(),
                message,
                element_ids,
            },
            CoreError::NothingToUndo => IpcError::new("invalid", "nothing to undo"),
            CoreError::NothingToRedo => IpcError::new("invalid", "nothing to redo"),
        }
    }
}
