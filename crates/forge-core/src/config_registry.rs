use crate::color::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub bold_family: Option<String>,
    pub italic_family: Option<String>,
    pub ligatures: LigatureConfig,
    pub nerd_fonts: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LigatureMode {
    Never,
    Always,
    CursorAware,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LigatureConfig {
    pub enabled: bool,
    pub mode: LigatureMode,
    pub features: Vec<String>,
    pub max_token_len: usize,
    pub cache_entries: usize,
}

impl Default for LigatureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: LigatureMode::CursorAware,
            features: vec!["liga".to_string(), "clig".to_string(), "calt".to_string()],
            max_token_len: 64,
            cache_entries: 4096,
        }
    }
}

impl LigatureConfig {
    pub const MIN_TOKEN_LEN: usize = 2;
    pub const MAX_TOKEN_LEN: usize = 256;
    pub const MIN_CACHE_ENTRIES: usize = 64;
    pub const MAX_CACHE_ENTRIES: usize = 65_536;

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn normalize(&mut self) {
        self.max_token_len = self
            .max_token_len
            .clamp(Self::MIN_TOKEN_LEN, Self::MAX_TOKEN_LEN);
        self.cache_entries = self
            .cache_entries
            .clamp(Self::MIN_CACHE_ENTRIES, Self::MAX_CACHE_ENTRIES);
        self.features.retain(|feature| {
            let trimmed = feature.trim();
            !trimmed.is_empty() && trimmed.len() <= 32
        });
        if self.features.is_empty() {
            self.features = Self::default().features;
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            bold_family: None,
            italic_family: None,
            ligatures: LigatureConfig::default(),
            nerd_fonts: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum PaddingBalance {
    #[default]
    Center,
    Fill,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorConfig {
    pub copy_on_select: bool,
    pub disable_default_keybindings: bool,
    pub hide_mouse_when_typing: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            copy_on_select: false,
            disable_default_keybindings: false,
            hide_mouse_when_typing: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PaneManagerMode {
    #[default]
    Tiling,
    Scrolling,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanesConfig {
    pub mode: PaneManagerMode,
}

impl Default for PanesConfig {
    fn default() -> Self {
        Self {
            mode: PaneManagerMode::Tiling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaddingConfig {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl Default for PaddingConfig {
    fn default() -> Self {
        Self {
            top: 4,
            bottom: 4,
            left: 4,
            right: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub padding: PaddingConfig,
    pub pane_padding: PaddingConfig,
    pub padding_balance: PaddingBalance,
    pub opacity: f32,
    pub title: String,
    pub decorations: bool,
    pub gap: u32,
    pub pane_outline_width: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            padding: PaddingConfig::default(),
            pane_padding: PaddingConfig {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            },
            padding_balance: PaddingBalance::default(),
            opacity: 1.0,
            title: "Forge".to_string(),
            decorations: true,
            gap: 0,
            pane_outline_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BlurMethod {
    #[default]
    Auto,
    Kde,
    External,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlurConfig {
    pub enabled: bool,
    pub method: BlurMethod,
    /// Advisory only. Wayland compositor blur protocols used by Forge do not
    /// expose a portable client-controlled radius.
    pub radius: u32,
}

impl Default for BlurConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: BlurMethod::Auto,
            radius: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Block,
    Underline,
    Beam,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorConfig {
    pub style: CursorStyle,
    pub blink: bool,
    pub blink_rate_ms: u32,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: CursorStyle::Block,
            blink: true,
            blink_rate_ms: 530,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScrollbackConfig {
    pub lines: usize,
    pub smooth_scroll: bool,
    pub scroll_multiplier: f32,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self {
            lines: 10000,
            smooth_scroll: true,
            scroll_multiplier: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub background: Color,
    pub foreground: Color,
    pub cursor_color: Color,
    pub selection_bg: Color,
    pub pane_outline_active: Color,
    pub pane_outline_inactive: Color,
    pub ansi_colors: [Color; 16],
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background: Color {
                r: 26,
                g: 27,
                b: 38,
                a: 255,
            },
            foreground: Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            cursor_color: Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            selection_bg: Color {
                r: 65,
                g: 72,
                b: 104,
                a: 200,
            },
            pane_outline_active: Color {
                r: 137,
                g: 180,
                b: 250,
                a: 255, // #89B4FA
            },
            pane_outline_inactive: Color {
                r: 108,
                g: 112,
                b: 134,
                a: 255, // #6C7086
            },
            ansi_colors: [
                Color {
                    r: 65,
                    g: 72,
                    b: 104,
                    a: 255,
                }, // 0: Black
                Color {
                    r: 247,
                    g: 118,
                    b: 142,
                    a: 255,
                }, // 1: Red
                Color {
                    r: 158,
                    g: 206,
                    b: 106,
                    a: 255,
                }, // 2: Green
                Color {
                    r: 224,
                    g: 175,
                    b: 104,
                    a: 255,
                }, // 3: Yellow
                Color {
                    r: 122,
                    g: 162,
                    b: 247,
                    a: 255,
                }, // 4: Blue
                Color {
                    r: 187,
                    g: 154,
                    b: 247,
                    a: 255,
                }, // 5: Magenta
                Color {
                    r: 125,
                    g: 207,
                    b: 255,
                    a: 255,
                }, // 6: Cyan
                Color {
                    r: 192,
                    g: 202,
                    b: 245,
                    a: 255,
                }, // 7: White
                Color {
                    r: 65,
                    g: 72,
                    b: 104,
                    a: 255,
                }, // 8: Bright Black
                Color {
                    r: 247,
                    g: 118,
                    b: 142,
                    a: 255,
                }, // 9: Bright Red
                Color {
                    r: 158,
                    g: 206,
                    b: 106,
                    a: 255,
                }, // 10: Bright Green
                Color {
                    r: 224,
                    g: 175,
                    b: 104,
                    a: 255,
                }, // 11: Bright Yellow
                Color {
                    r: 122,
                    g: 162,
                    b: 247,
                    a: 255,
                }, // 12: Bright Blue
                Color {
                    r: 187,
                    g: 154,
                    b: 247,
                    a: 255,
                }, // 13: Bright Magenta
                Color {
                    r: 125,
                    g: 207,
                    b: 255,
                    a: 255,
                }, // 14: Bright Cyan
                Color {
                    r: 192,
                    g: 202,
                    b: 245,
                    a: 255,
                }, // 15: Bright White
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum BrailleStyle {
    Solid,
    #[default]
    Dots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneAnimationMode {
    None,
    #[default]
    Expand,
    Fade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderConfig {
    pub braille_style: BrailleStyle,
    #[serde(default)]
    pub context_menu_transparent: bool,
    #[serde(default)]
    pub pane_animation: PaneAnimationMode,
    #[serde(default = "default_pane_animation_duration_ms")]
    pub pane_animation_duration_ms: u32,
}

fn default_pane_animation_duration_ms() -> u32 {
    180
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            braille_style: BrailleStyle::default(),
            context_menu_transparent: false,
            pane_animation: PaneAnimationMode::Expand,
            pane_animation_duration_ms: default_pane_animation_duration_ms(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmCloseBackgroundMode {
    #[default]
    OpaquePanel,
    BlankScreen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmCloseConfig {
    pub background_mode: ConfirmCloseBackgroundMode,
    pub panel_color: Color,
    pub selected_color: Color,
}

impl Default for ConfirmCloseConfig {
    fn default() -> Self {
        Self {
            background_mode: ConfirmCloseBackgroundMode::OpaquePanel,
            panel_color: Color {
                r: 11,
                g: 16,
                b: 32,
                a: 255,
            },
            selected_color: Color {
                r: 250,
                g: 204,
                b: 21,
                a: 255,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StatusbarPosition {
    Top,
    #[default]
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StatusbarPlacement {
    Absolute,
    #[default]
    Inside,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatusbarItem {
    String(String),
    Tabs {
        tabs: TabsConfig,
    },
    Table {
        text: String,
        fg: Option<String>,
        bg: Option<String>,
        action: Option<String>,
        bold: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusbarStyle {
    pub fg: Option<String>,
    pub bg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabsConfig {
    pub format: String,
    pub zoom_indicator: String,
    pub left_edge: String,
    pub right_edge: String,
    pub active: Option<StatusbarStyle>,
    pub inactive: Option<StatusbarStyle>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusbarConfig {
    pub enabled: bool,
    pub position: StatusbarPosition,
    pub placement: StatusbarPlacement,
    pub bg_color: String,
    pub fg_color: String,
    pub left: Vec<StatusbarItem>,
    pub center: Vec<StatusbarItem>,
    pub right: Vec<StatusbarItem>,
}

impl Default for StatusbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: StatusbarPosition::Bottom,
            placement: StatusbarPlacement::Inside,
            bg_color: "transparent".to_string(),
            fg_color: "#CDD6F4".to_string(),
            left: vec![
                StatusbarItem::Tabs {
                    tabs: TabsConfig {
                        format: " {index}{zoom} ".to_string(),
                        zoom_indicator: "()".to_string(),
                        left_edge: String::new(),
                        right_edge: String::new(),
                        active: Some(StatusbarStyle {
                            fg: Some("#1E1E2E".to_string()),
                            bg: Some("#89B4FA".to_string()),
                        }),
                        inactive: Some(StatusbarStyle {
                            fg: Some("#A6ADC8".to_string()),
                            bg: None,
                        }),
                    },
                },
                StatusbarItem::Table {
                    text: " + ".to_string(),
                    fg: None,
                    bg: None,
                    action: Some("NewTab".to_string()),
                    bold: None,
                },
            ],
            center: vec![],
            right: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgeConfig {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub blur: BlurConfig,
    pub cursor: CursorConfig,
    pub scrollback: ScrollbackConfig,
    pub shell: ShellConfig,
    pub theme: ThemeConfig,
    pub behavior: BehaviorConfig,
    pub panes: PanesConfig,
    pub render: RenderConfig,
    pub confirm_close: ConfirmCloseConfig,
    pub statusbar: StatusbarConfig,
    pub keybindings: std::collections::HashMap<crate::bindings::KeyStroke, crate::bindings::Action>,
}

#[allow(clippy::derivable_impls)]
impl Default for ForgeConfig {
    fn default() -> Self {
        let mut default_keybindings = std::collections::HashMap::new();
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+C") {
            default_keybindings.insert(keystroke, crate::bindings::Action::Copy);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+V") {
            default_keybindings.insert(keystroke, crate::bindings::Action::Paste);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("f11") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ToggleFullscreen);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+enter") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ToggleFullscreen);
        }

        // Zoom bindings
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+plus") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomIn);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+=") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomIn);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+shift+=") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomIn);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+shift+plus") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomIn);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+minus") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomOut);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+kp_add") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomIn);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+kp_subtract") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomOut);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+0") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomReset);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+kp_0") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ZoomReset);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("ctrl+shift+backslash") {
            default_keybindings.insert(keystroke, crate::bindings::Action::SplitVertical);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+minus") {
            default_keybindings.insert(keystroke, crate::bindings::Action::SplitHorizontal);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+Z") {
            default_keybindings.insert(keystroke, crate::bindings::Action::TogglePaneZoom);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+B") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ToggleSidebar);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+Q") {
            default_keybindings.insert(keystroke, crate::bindings::Action::ClosePane);
        }
        if let Some(keystroke) = crate::bindings::KeyStroke::parse("Ctrl+Shift+F") {
            default_keybindings.insert(keystroke, crate::bindings::Action::SpawnFloatingPane);
        }

        // Pane navigation
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+Left") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneLeft);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+Right") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneRight);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+Up") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneUp);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+Down") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneDown);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+h") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneLeft);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+j") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneDown);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+k") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneUp);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+l") {
            default_keybindings.insert(ks, crate::bindings::Action::FocusPaneRight);
        }

        // Tab bindings
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+Shift+T") {
            default_keybindings.insert(ks, crate::bindings::Action::NewTab);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+Shift+W") {
            default_keybindings.insert(ks, crate::bindings::Action::CloseTab);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+PageDown") {
            default_keybindings.insert(ks, crate::bindings::Action::NextTab);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+PageUp") {
            default_keybindings.insert(ks, crate::bindings::Action::PreviousTab);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+1") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab1);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+2") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab2);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+3") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab3);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+4") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab4);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+5") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab5);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+6") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab6);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+7") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab7);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+8") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab8);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Alt+9") {
            default_keybindings.insert(ks, crate::bindings::Action::SwitchTab9);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+Shift+PageUp") {
            default_keybindings.insert(ks, crate::bindings::Action::MoveTabLeft);
        }
        if let Some(ks) = crate::bindings::KeyStroke::parse("Ctrl+Shift+PageDown") {
            default_keybindings.insert(ks, crate::bindings::Action::MoveTabRight);
        }

        Self {
            font: FontConfig::default(),
            window: WindowConfig::default(),
            blur: BlurConfig::default(),
            cursor: CursorConfig::default(),
            scrollback: ScrollbackConfig::default(),
            shell: ShellConfig::default(),
            theme: ThemeConfig::default(),
            behavior: BehaviorConfig::default(),
            panes: PanesConfig::default(),
            render: RenderConfig::default(),
            confirm_close: ConfirmCloseConfig::default(),
            statusbar: StatusbarConfig::default(),
            keybindings: default_keybindings,
        }
    }
}

impl ForgeConfig {
    pub fn validate(&mut self) {
        self.font.size = self.font.size.clamp(6.0, 72.0);
        self.window.width = self.window.width.clamp(200, 8000);
        self.window.height = self.window.height.clamp(100, 6000);
        self.window.opacity = self.window.opacity.clamp(0.0, 1.0);
        self.cursor.blink_rate_ms = self.cursor.blink_rate_ms.clamp(100, 2000);
        self.scrollback.lines = self.scrollback.lines.clamp(100, 100000);
        self.scrollback.scroll_multiplier = self.scrollback.scroll_multiplier.clamp(0.5, 10.0);
        self.window.padding.top = self.window.padding.top.clamp(0, 100);
        self.window.padding.bottom = self.window.padding.bottom.clamp(0, 100);
        self.window.padding.left = self.window.padding.left.clamp(0, 100);
        self.window.padding.right = self.window.padding.right.clamp(0, 100);
        self.blur.radius = self.blur.radius.min(250);
        if self.blur.method == BlurMethod::Off {
            self.blur.enabled = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_clamps_values() {
        let mut config = ForgeConfig::default();
        config.font.size = 999.0;
        config.window.opacity = 5.0;
        config.window.width = 1;
        config.blur.radius = 999;
        config.blur.enabled = true;
        config.blur.method = BlurMethod::Off;
        config.validate();
        assert_eq!(config.font.size, 72.0);
        assert_eq!(config.window.opacity, 1.0);
        assert_eq!(config.window.width, 200);
        assert_eq!(config.blur.radius, 250);
        assert!(!config.blur.enabled);
    }

    #[test]
    fn default_keybindings_include_pane_splits() {
        let config = ForgeConfig::default();
        let vertical = crate::bindings::KeyStroke::parse("ctrl+shift+backslash").unwrap();
        let horizontal = crate::bindings::KeyStroke::parse("ctrl+shift+minus").unwrap();
        let zoom = crate::bindings::KeyStroke::parse("ctrl+shift+z").unwrap();
        let sidebar = crate::bindings::KeyStroke::parse("ctrl+shift+b").unwrap();
        let close = crate::bindings::KeyStroke::parse("ctrl+shift+q").unwrap();

        assert_eq!(
            config.keybindings.get(&vertical),
            Some(&crate::bindings::Action::SplitVertical)
        );
        assert_eq!(
            config.keybindings.get(&horizontal),
            Some(&crate::bindings::Action::SplitHorizontal)
        );
        assert_eq!(
            config.keybindings.get(&zoom),
            Some(&crate::bindings::Action::TogglePaneZoom)
        );
        assert_eq!(
            config.keybindings.get(&sidebar),
            Some(&crate::bindings::Action::ToggleSidebar)
        );
        assert_eq!(
            config.keybindings.get(&close),
            Some(&crate::bindings::Action::ClosePane)
        );
    }
}
