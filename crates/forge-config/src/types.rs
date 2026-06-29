use forge_core::config_registry::ForgeConfig;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigChangeSet {
    pub font: bool,
    pub window: bool,
    pub blur: bool,
    pub cursor: bool,
    pub scrollback: bool,
    pub shell: bool,
    pub theme: bool,
    pub behavior: bool,
    pub render: bool,
    pub keybindings: bool,
}

impl ConfigChangeSet {
    pub fn all() -> Self {
        Self {
            font: true,
            window: true,
            blur: true,
            cursor: true,
            scrollback: true,
            shell: true,
            theme: true,
            behavior: true,
            render: true,
            keybindings: true,
        }
    }

    pub fn between(old: &ForgeConfig, new: &ForgeConfig) -> Self {
        Self {
            font: old.font != new.font,
            window: old.window != new.window,
            blur: old.blur != new.blur,
            cursor: old.cursor != new.cursor,
            scrollback: old.scrollback != new.scrollback,
            shell: old.shell != new.shell,
            theme: old.theme != new.theme,
            behavior: old.behavior != new.behavior,
            render: old.render != new.render,
            keybindings: old.keybindings != new.keybindings,
        }
    }

    pub fn any(self) -> bool {
        self.font
            || self.window
            || self.blur
            || self.cursor
            || self.scrollback
            || self.shell
            || self.theme
            || self.behavior
            || self.render
            || self.keybindings
    }
}

pub struct ConfigUpdate {
    pub config: ForgeConfig,
    pub changes: ConfigChangeSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_blur_changes() {
        let old = ForgeConfig::default();
        let mut new = old.clone();
        new.blur.enabled = true;

        let changes = ConfigChangeSet::between(&old, &new);

        assert!(changes.blur);
        assert!(changes.any());
        assert!(!changes.window);
    }
}
