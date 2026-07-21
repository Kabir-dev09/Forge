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
    pub panes: bool,
    pub render: bool,
    pub confirm_close: bool,
    pub command_completion_indicator: bool,
    pub statusbar: bool,
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
            panes: true,
            render: true,
            confirm_close: true,
            command_completion_indicator: true,
            statusbar: true,
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
            panes: old.panes != new.panes,
            render: old.render != new.render,
            confirm_close: old.confirm_close != new.confirm_close,
            command_completion_indicator: old.command_completion_indicator
                != new.command_completion_indicator,
            statusbar: old.statusbar != new.statusbar,
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
            || self.panes
            || self.render
            || self.confirm_close
            || self.command_completion_indicator
            || self.statusbar
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

    #[test]
    fn detects_statusbar_and_confirm_close_changes() {
        let old = ForgeConfig::default();

        let mut status_new = old.clone();
        status_new.statusbar.enabled = !status_new.statusbar.enabled;
        let status_changes = ConfigChangeSet::between(&old, &status_new);
        assert!(status_changes.statusbar);
        assert!(status_changes.any());

        let mut confirm_new = old.clone();
        confirm_new.confirm_close.parsed_panel_color.r =
            confirm_new.confirm_close.parsed_panel_color.r.wrapping_add(1);
        let confirm_changes = ConfigChangeSet::between(&old, &confirm_new);
        assert!(confirm_changes.confirm_close);
        assert!(confirm_changes.any());
    }

    #[test]
    fn detects_pane_config_changes() {
        let old = ForgeConfig::default();
        let mut new = old.clone();
        new.panes.mode = forge_core::config_registry::PaneManagerMode::Scrolling;

        let changes = ConfigChangeSet::between(&old, &new);

        assert!(changes.panes);
        assert!(changes.any());
        assert!(!changes.render);
    }

    #[test]
    fn detects_command_completion_indicator_changes() {
        let old = ForgeConfig::default();
        let mut new = old.clone();
        new.command_completion_indicator.minimum_duration_ms = 1;

        let changes = ConfigChangeSet::between(&old, &new);

        assert!(changes.command_completion_indicator);
        assert!(changes.any());
    }
}
