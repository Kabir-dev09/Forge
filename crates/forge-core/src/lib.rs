//! Forge core library - shared types, errors, and configuration registry.

pub mod bindings;
pub mod cell;
pub mod color;
pub mod config_registry;
mod config_validation;
pub mod crash;
pub mod error;
pub mod geometry;

pub use error::{ForgeError, Result};
