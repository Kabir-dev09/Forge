//! Forge config library - Lua VM and configuration actor.

pub mod actor;
pub mod extractor;
pub mod types;
pub mod watcher;

pub use types::ConfigUpdate;
#[cfg(test)]
mod config_test;
