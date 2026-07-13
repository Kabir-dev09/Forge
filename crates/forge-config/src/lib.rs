//! Forge config library - TOML parser and configuration actor.

pub mod actor;
pub mod extractor;
pub mod imports;
pub mod types;

pub use types::ConfigUpdate;
#[cfg(test)]
mod config_test;
