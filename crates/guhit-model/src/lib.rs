//! Guhit Studio contract types.
//!
//! This crate is the single source of truth for everything shared between the
//! Rust crates and the TypeScript frontend. TypeScript bindings are generated
//! from it with `pnpm gen:types`. It holds types and built-in data only, no
//! behavior. It is owned by the orchestrator: propose changes, do not edit.

pub mod api;
pub mod command;
pub mod defaults;
pub mod derived;
pub mod live;
pub mod model;

pub use api::*;
pub use command::*;
pub use derived::*;
pub use live::*;
pub use model::*;
