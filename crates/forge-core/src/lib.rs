//! Forge core library - shared types, errors, and configuration registry.

pub mod bindings;
pub mod cache;
pub mod cell;
pub mod color;
pub mod config_registry;
pub mod crash;
pub mod error;
pub mod geometry;

pub use error::{ForgeError, Result};
