use forge_core::config_registry::ForgeConfig;

/// A diff of changed configuration values.
/// Only Some fields have changed. None means "no change, keep previous value."
pub struct ConfigUpdate {
    pub config: ForgeConfig,
    // In a future step this will become a delta struct.
    // For now, shipping the entire config is acceptable.
}
