//! Configuration model for DisplayGroups: the `config.json` schema, validation,
//! schema migrations and crash-safe storage.
//!
//! This crate contains no Win32 code so it can be tested anywhere.

mod config;
mod error;
pub mod geometry;
pub mod icons;
pub mod migrate;
pub mod paths;
pub mod store;
pub mod validate;

pub use config::*;
pub use error::ModelError;
