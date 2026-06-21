//! Forge core library - shared types, errors, and configuration registry.

pub mod error;
pub mod color;
pub mod geometry;
pub mod cell;
pub mod crash;
pub mod config_registry;
pub mod cache;
pub mod bindings;

pub use error::{ForgeError, Result};
