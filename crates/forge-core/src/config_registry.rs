use crate::color::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::bindings::{Action, KeyStroke};
use std::fmt;

#[derive(Debug, Clone)]
pub enum ConfigError {
    InvalidColor { path: String, value: String },
    InvalidKeybinding { key: String, action: String, reason: String },
    InvalidEnvVar { value: String },
    OutOfRange { path: String, value: String, expected: String },
    InvalidValue { path: String, value: String, reason: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidColor { path, value } => write!(f, "Invalid color at {}: {}", path, value),
            Self::InvalidKeybinding { key, action, reason } => write!(f, "Invalid keybinding {}={} ({})", key, action, reason),
            Self::InvalidEnvVar { value } => write!(f, "Invalid environment variable format (expected KEY=VALUE): {}", value),
            Self::OutOfRange { path, value, expected } => write!(f, "{} must be {}, but received {}", path, expected, value),
            Self::InvalidValue { path, value, reason } => write!(f, "Invalid value for {}: {} ({})", path, value, reason),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EarlyWindowConfig {
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub decorations: bool,
    pub center_on_launch: bool,
}

impl Default for EarlyWindowConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            opacity: 1.0,
            decorations: true,
            center_on_launch: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EarlyThemeConfig {
    pub background: String,
}

impl Default for EarlyThemeConfig {
    fn default() -> Self {
        Self {
            background: "#1a1b26".to_string(), // match default theme background
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EarlyStartupConfig {
    pub window: EarlyWindowConfig,
    pub theme: EarlyThemeConfig,
}

impl EarlyStartupConfig {
    pub fn load(path: &std::path::Path) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(config) = toml::from_str::<Self>(&content) {
                return config;
            }
        }
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub bold_family: Option<String>,
    pub italic_family: Option<String>,
    pub bold_italic_family: Option<String>,
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
#[serde(default)]
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
            family: "builtin".to_string(),
            size: 14.0,
            bold_family: None,
            italic_family: None,
            bold_italic_family: None,
            ligatures: LigatureConfig::default(),
            nerd_fonts: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
pub struct PanesConfig {
    pub mode: PaneManagerMode,
    pub scroll_animation_duration_ms: u64,
}

impl Default for PanesConfig {
    fn default() -> Self {
        Self {
            mode: PaneManagerMode::Tiling,
            scroll_animation_duration_ms: 120,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub center_on_launch: bool,
    pub padding: PaddingConfig,
    pub pane_padding: PaddingConfig,
    pub center_grid: bool, // replacing padding_balance
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
            center_on_launch: false,
            padding: PaddingConfig::default(),
            pane_padding: PaddingConfig {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            },
            center_grid: true,
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
#[serde(default)]
pub struct BlurConfig {
    pub enabled: bool,
    pub method: BlurMethod,
}

impl Default for BlurConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: BlurMethod::Auto,
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
#[serde(default)]
pub struct CursorConfig {
    pub style: CursorStyle,
    pub blink: bool,
    pub blink_rate_ms: u32,
    pub trail: CursorTrailConfig,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: CursorStyle::Block,
            blink: true,
            blink_rate_ms: 530,
            trail: CursorTrailConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorTrailConfig {
    pub enabled: bool,
    #[serde(alias = "experimental_segmented")]
    pub segmented: bool,
    pub fast_decay_ms: u32,
    pub slow_decay_ms: u32,
    pub minimum_distance_x: u32,
    pub minimum_distance_y: u32,
    pub trigger_delay_ms: u32,
    pub color: String,
    #[serde(skip)]
    pub parsed_color: Option<Color>,
}

impl Default for CursorTrailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            segmented: false,
            fast_decay_ms: 100,
            slow_decay_ms: 400,
            minimum_distance_x: 2,
            minimum_distance_y: 2,
            trigger_delay_ms: 50,
            color: "cursor".to_string(),
            parsed_color: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrollbackConfig {
    pub lines: Option<usize>, // None implies infinite or max
    pub smooth_scroll: bool,
    pub scroll_multiplier: f32,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self {
            lines: Some(10000),
            smooth_scroll: true,
            scroll_multiplier: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub integration_enabled: bool,
    #[serde(skip)]
    pub parsed_env: Vec<(String, String)>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
            integration_enabled: true,
            parsed_env: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnsiColorsMap {
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

impl Default for AnsiColorsMap {
    fn default() -> Self {
        Self {
            black: "#414868".to_string(),
            red: "#f7768e".to_string(),
            green: "#9ece6a".to_string(),
            yellow: "#e0af68".to_string(),
            blue: "#7aa2f7".to_string(),
            magenta: "#bb9af7".to_string(),
            cyan: "#7dcfff".to_string(),
            white: "#c0caf5".to_string(),
            bright_black: "#414868".to_string(),
            bright_red: "#f7768e".to_string(),
            bright_green: "#9ece6a".to_string(),
            bright_yellow: "#e0af68".to_string(),
            bright_blue: "#7aa2f7".to_string(),
            bright_magenta: "#bb9af7".to_string(),
            bright_cyan: "#7dcfff".to_string(),
            bright_white: "#c0caf5".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub background: String,
    pub foreground: String,
    pub popup_background: String,
    pub cursor_color: String,
    pub selection_bg: String,
    pub pane_outline_active: String,
    pub pane_outline_inactive: String,
    pub ansi: AnsiColorsMap,
    /// Compact palette form accepted by existing Forge TOML configurations.
    /// When present, entries map directly to ANSI palette slots 0 through 15.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_colors: Option<Box<[String; 16]>>,

    #[serde(skip)]
    pub parsed_background: Color,
    #[serde(skip)]
    pub parsed_foreground: Color,
    #[serde(skip)]
    pub parsed_popup_background: Color,
    #[serde(skip)]
    pub parsed_cursor_color: Color,
    #[serde(skip)]
    pub parsed_selection_bg: Color,
    #[serde(skip)]
    pub parsed_pane_outline_active: Color,
    #[serde(skip)]
    pub parsed_pane_outline_inactive: Color,
    #[serde(skip)]
    pub parsed_ansi_colors: [Color; 16],
}

impl Default for ThemeConfig {
    fn default() -> Self {
        let mut t = Self {
            background: "#1a1b26".to_string(),
            foreground: "#c0caf5".to_string(),
            popup_background: "#1e1e2e".to_string(),
            cursor_color: "#c0caf5".to_string(),
            selection_bg: "#414868c8".to_string(), // approx 200 alpha
            pane_outline_active: "#89b4fa".to_string(),
            pane_outline_inactive: "#6c7086".to_string(),
            ansi: AnsiColorsMap::default(),
            ansi_colors: None,
            parsed_background: Color::TRANSPARENT,
            parsed_foreground: Color::TRANSPARENT,
            parsed_popup_background: Color::TRANSPARENT,
            parsed_cursor_color: Color::TRANSPARENT,
            parsed_selection_bg: Color::TRANSPARENT,
            parsed_pane_outline_active: Color::TRANSPARENT,
            parsed_pane_outline_inactive: Color::TRANSPARENT,
            parsed_ansi_colors: [Color::TRANSPARENT; 16],
        };
        // Just rely on validate() or init_defaults() for actual parsed colors.
        // For default, we just populate them directly.
        t.parsed_background = parse_hex_color(&t.background).unwrap();
        t.parsed_foreground = parse_hex_color(&t.foreground).unwrap();
        t.parsed_popup_background = parse_hex_color(&t.popup_background).unwrap();
        t.parsed_cursor_color = parse_hex_color(&t.cursor_color).unwrap();
        t.parsed_selection_bg = parse_hex_color(&t.selection_bg).unwrap();
        t.parsed_pane_outline_active = parse_hex_color(&t.pane_outline_active).unwrap();
        t.parsed_pane_outline_inactive = parse_hex_color(&t.pane_outline_inactive).unwrap();
        t.parsed_ansi_colors = [
            parse_hex_color(&t.ansi.black).unwrap(),
            parse_hex_color(&t.ansi.red).unwrap(),
            parse_hex_color(&t.ansi.green).unwrap(),
            parse_hex_color(&t.ansi.yellow).unwrap(),
            parse_hex_color(&t.ansi.blue).unwrap(),
            parse_hex_color(&t.ansi.magenta).unwrap(),
            parse_hex_color(&t.ansi.cyan).unwrap(),
            parse_hex_color(&t.ansi.white).unwrap(),
            parse_hex_color(&t.ansi.bright_black).unwrap(),
            parse_hex_color(&t.ansi.bright_red).unwrap(),
            parse_hex_color(&t.ansi.bright_green).unwrap(),
            parse_hex_color(&t.ansi.bright_yellow).unwrap(),
            parse_hex_color(&t.ansi.bright_blue).unwrap(),
            parse_hex_color(&t.ansi.bright_magenta).unwrap(),
            parse_hex_color(&t.ansi.bright_cyan).unwrap(),
            parse_hex_color(&t.ansi.bright_white).unwrap(),
        ];
        t
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
#[serde(default)]
pub struct RenderConfig {
    pub braille_style: BrailleStyle,
    pub context_menu_transparent: bool,
    pub pane_animation: PaneAnimationMode,
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
#[serde(default)]
pub struct ConfirmCloseConfig {
    pub background_mode: ConfirmCloseBackgroundMode,
    pub panel_color: String,
    pub selected_color: String,

    #[serde(skip)]
    pub parsed_panel_color: Color,
    #[serde(skip)]
    pub parsed_selected_color: Color,
}

impl Default for ConfirmCloseConfig {
    fn default() -> Self {
        let mut t = Self {
            background_mode: ConfirmCloseBackgroundMode::OpaquePanel,
            panel_color: "#0b1020".to_string(),
            selected_color: "#facc15".to_string(),
            parsed_panel_color: Color::TRANSPARENT,
            parsed_selected_color: Color::TRANSPARENT,
        };
        t.parsed_panel_color = parse_hex_color(&t.panel_color).unwrap();
        t.parsed_selected_color = parse_hex_color(&t.selected_color).unwrap();
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandCompletionIndicatorMode {
    #[default]
    Enabled,
    Disabled,
    DisabledOnZoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandCompletionIndicatorDismissal {
    #[default]
    Timeout,
    OnInteraction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandCompletionIndicatorConfig {
    pub mode: CommandCompletionIndicatorMode,
    pub minimum_duration_ms: u64,
    pub dismissal: CommandCompletionIndicatorDismissal,
    pub display_duration_ms: u64,
    pub transparent: bool,
}

impl Default for CommandCompletionIndicatorConfig {
    fn default() -> Self {
        Self {
            mode: CommandCompletionIndicatorMode::Enabled,
            minimum_duration_ms: 10_000,
            dismissal: CommandCompletionIndicatorDismissal::Timeout,
            display_duration_ms: 3_000,
            transparent: false,
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusbarItem {
    Text {
        text: String,
        fg: Option<String>,
        bg: Option<String>,
        action: Option<String>,
        bold: Option<bool>,
    },
    Tabs {
        #[serde(default = "default_tabs_format")]
        format: String,
        #[serde(default = "default_tabs_zoom")]
        zoom_indicator: String,
        #[serde(default)]
        left_edge: String,
        #[serde(default)]
        right_edge: String,
        #[serde(default = "default_tabs_separator")]
        separator: String,
        active: Box<Option<StatusbarStyle>>,
        inactive: Box<Option<StatusbarStyle>>,
    },
}

fn default_tabs_separator() -> String { String::new() }
fn default_tabs_format() -> String { " {index}{zoom} ".to_string() }
fn default_tabs_zoom() -> String { "()".to_string() }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusbarStyle {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub format: Option<String>,
    pub zoom_indicator: Option<String>,
    pub left_edge: Option<String>,
    pub right_edge: Option<String>,
    pub separator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
                    format: default_tabs_format(),
                    zoom_indicator: default_tabs_zoom(),
                    left_edge: String::new(),
                    right_edge: String::new(),
                    separator: default_tabs_separator(),
                    active: Box::new(Some(StatusbarStyle {
                        fg: Some("#1E1E2E".to_string()),
                        bg: Some("#89B4FA".to_string()),
                        ..Default::default()
                    })),
                    inactive: Box::new(Some(StatusbarStyle {
                        fg: Some("#A6ADC8".to_string()),
                        bg: None,
                        ..Default::default()
                    })),
                },
                StatusbarItem::Text {
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
#[serde(default)]
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
    pub command_completion_indicator: CommandCompletionIndicatorConfig,
    pub statusbar: StatusbarConfig,

    #[serde(rename = "keybinds", alias = "keybindings")]
    pub raw_keybindings: HashMap<String, String>,

    #[serde(skip)]
    pub keybindings: HashMap<KeyStroke, Action>,
}

#[allow(clippy::derivable_impls)]
impl Default for ForgeConfig {
    fn default() -> Self {
        let mut config = Self {
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
            command_completion_indicator: CommandCompletionIndicatorConfig::default(),
            statusbar: StatusbarConfig::default(),
            raw_keybindings: HashMap::new(),
            keybindings: HashMap::new(),
        };

        install_default_keybindings(&mut config.keybindings);

        config
    }
}

pub fn parse_hex_color(hex: &str) -> Option<Color> {
    if hex.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 || hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = if hex.len() == 8 {
            u8::from_str_radix(&hex[6..8], 16).ok()?
        } else {
            255
        };
        Some(Color { r, g, b, a })
    } else {
        None
    }
}

pub fn parse_action(value: &str) -> Option<Action> {
    match value.to_ascii_lowercase().replace('-', "_").as_str() {
        "copy" => Some(Action::Copy),
        "paste" => Some(Action::Paste),
        "toggle_fullscreen" | "togglefullscreen" => Some(Action::ToggleFullscreen),
        "zoom_in" | "zoomin" => Some(Action::ZoomIn),
        "zoom_out" | "zoomout" => Some(Action::ZoomOut),
        "zoom_reset" | "zoomreset" => Some(Action::ZoomReset),
        "split_vertical" | "splitvertical" => Some(Action::SplitVertical),
        "split_horizontal" | "splithorizontal" => Some(Action::SplitHorizontal),
        "toggle_pane_zoom" | "togglepanezoom" => Some(Action::TogglePaneZoom),
        "toggle_sidebar" | "togglesidebar" => Some(Action::ToggleSidebar),
        "close_pane" | "closepane" => Some(Action::ClosePane),
        "spawn_floating_pane" | "spawnfloatingpane" => Some(Action::SpawnFloatingPane),
        "toggle_pane_floating" | "togglepanefloating" => Some(Action::TogglePaneFloating),
        "focus_pane_left" | "focuspaneleft" => Some(Action::FocusPaneLeft),
        "focus_pane_right" | "focuspaneright" => Some(Action::FocusPaneRight),
        "focus_pane_up" | "focuspaneup" => Some(Action::FocusPaneUp),
        "focus_pane_down" | "focuspanedown" => Some(Action::FocusPaneDown),
        "move_pane_left" | "movepaneleft" => Some(Action::MovePaneLeft),
        "move_pane_right" | "movepaneright" => Some(Action::MovePaneRight),
        "move_pane_up" | "movepaneup" => Some(Action::MovePaneUp),
        "move_pane_down" | "movepanedown" => Some(Action::MovePaneDown),
        "move_pane_to_tab_1" | "movepanetotab1" => Some(Action::MovePaneToTab1),
        "move_pane_to_tab_2" | "movepanetotab2" => Some(Action::MovePaneToTab2),
        "move_pane_to_tab_3" | "movepanetotab3" => Some(Action::MovePaneToTab3),
        "move_pane_to_tab_4" | "movepanetotab4" => Some(Action::MovePaneToTab4),
        "move_pane_to_tab_5" | "movepanetotab5" => Some(Action::MovePaneToTab5),
        "move_pane_to_tab_6" | "movepanetotab6" => Some(Action::MovePaneToTab6),
        "move_pane_to_tab_7" | "movepanetotab7" => Some(Action::MovePaneToTab7),
        "move_pane_to_tab_8" | "movepanetotab8" => Some(Action::MovePaneToTab8),
        "move_pane_to_tab_9" | "movepanetotab9" => Some(Action::MovePaneToTab9),
        "new_tab" | "newtab" => Some(Action::NewTab),
        "close_tab" | "closetab" => Some(Action::CloseTab),
        "next_tab" | "nexttab" => Some(Action::NextTab),
        "previous_tab" | "previoustab" => Some(Action::PreviousTab),
        "switch_tab_1" | "switchtab1" => Some(Action::SwitchTab1),
        "switch_tab_2" | "switchtab2" => Some(Action::SwitchTab2),
        "switch_tab_3" | "switchtab3" => Some(Action::SwitchTab3),
        "switch_tab_4" | "switchtab4" => Some(Action::SwitchTab4),
        "switch_tab_5" | "switchtab5" => Some(Action::SwitchTab5),
        "switch_tab_6" | "switchtab6" => Some(Action::SwitchTab6),
        "switch_tab_7" | "switchtab7" => Some(Action::SwitchTab7),
        "switch_tab_8" | "switchtab8" => Some(Action::SwitchTab8),
        "switch_tab_9" | "switchtab9" => Some(Action::SwitchTab9),
        "move_tab_left" | "movetableft" => Some(Action::MoveTabLeft),
        "move_tab_right" | "movetabright" => Some(Action::MoveTabRight),
        _ => None,
    }
}

fn install_default_keybindings(keybindings: &mut HashMap<KeyStroke, Action>) {
    const DEFAULTS: &[(&str, &str)] = &[
        ("Ctrl+Shift+C", "Copy"),
        ("Ctrl+Shift+V", "Paste"),
        ("f11", "ToggleFullscreen"),
        ("ctrl+enter", "ToggleFullscreen"),
        ("ctrl+plus", "ZoomIn"),
        ("ctrl+=", "ZoomIn"),
        ("ctrl+shift+=", "ZoomIn"),
        ("ctrl+shift+plus", "ZoomIn"),
        ("ctrl+minus", "ZoomOut"),
        ("ctrl+kp_add", "ZoomIn"),
        ("ctrl+kp_subtract", "ZoomOut"),
        ("ctrl+0", "ZoomReset"),
        ("ctrl+kp_0", "ZoomReset"),
        ("ctrl+shift+backslash", "SplitVertical"),
        ("Ctrl+Shift+minus", "SplitHorizontal"),
        ("Ctrl+Shift+Z", "TogglePaneZoom"),
        ("Ctrl+Shift+B", "ToggleSidebar"),
        ("Ctrl+Shift+Q", "ClosePane"),
        ("Ctrl+Shift+F", "SpawnFloatingPane"),
        ("Ctrl+Shift+D", "TogglePaneFloating"),
        ("Alt+Left", "FocusPaneLeft"),
        ("Alt+Right", "FocusPaneRight"),
        ("Alt+Up", "FocusPaneUp"),
        ("Alt+Down", "FocusPaneDown"),
        ("Alt+h", "FocusPaneLeft"),
        ("Alt+j", "FocusPaneDown"),
        ("Alt+k", "FocusPaneUp"),
        ("Alt+l", "FocusPaneRight"),
        ("Ctrl+Shift+Left", "MovePaneLeft"),
        ("Ctrl+Shift+Right", "MovePaneRight"),
        ("Ctrl+Shift+Up", "MovePaneUp"),
        ("Ctrl+Shift+Down", "MovePaneDown"),
        ("Ctrl+Shift+1", "MovePaneToTab1"),
        ("Ctrl+Shift+2", "MovePaneToTab2"),
        ("Ctrl+Shift+3", "MovePaneToTab3"),
        ("Ctrl+Shift+4", "MovePaneToTab4"),
        ("Ctrl+Shift+5", "MovePaneToTab5"),
        ("Ctrl+Shift+6", "MovePaneToTab6"),
        ("Ctrl+Shift+7", "MovePaneToTab7"),
        ("Ctrl+Shift+8", "MovePaneToTab8"),
        ("Ctrl+Shift+9", "MovePaneToTab9"),
        ("Ctrl+Shift+T", "NewTab"),
        ("Ctrl+Shift+W", "CloseTab"),
        ("Ctrl+PageDown", "NextTab"),
        ("Ctrl+PageUp", "PreviousTab"),
        ("Alt+1", "SwitchTab1"),
        ("Alt+2", "SwitchTab2"),
        ("Alt+3", "SwitchTab3"),
        ("Alt+4", "SwitchTab4"),
        ("Alt+5", "SwitchTab5"),
        ("Alt+6", "SwitchTab6"),
        ("Alt+7", "SwitchTab7"),
        ("Alt+8", "SwitchTab8"),
        ("Alt+9", "SwitchTab9"),
        ("Ctrl+Shift+PageUp", "MoveTabLeft"),
        ("Ctrl+Shift+PageDown", "MoveTabRight"),
    ];

    for &(key, action) in DEFAULTS {
        if let (Some(keystroke), Some(action)) = (KeyStroke::parse(key), parse_action(action)) {
            keybindings.insert(keystroke, action);
        }
    }
}

impl ForgeConfig {
    pub fn validate(&mut self) -> Vec<ConfigError> {
        let mut errors = Vec::new();

        self.font.size = self.font.size.clamp(6.0, 72.0);
        self.font.ligatures.normalize();
        self.window.width = self.window.width.clamp(200, 8000);
        self.window.height = self.window.height.clamp(100, 6000);
        self.window.opacity = self.window.opacity.clamp(0.0, 1.0);
        self.cursor.blink_rate_ms = self.cursor.blink_rate_ms.clamp(100, 2000);
        self.cursor.trail.fast_decay_ms = self.cursor.trail.fast_decay_ms.clamp(1, 2_000);
        self.cursor.trail.slow_decay_ms = self
            .cursor
            .trail
            .slow_decay_ms
            .clamp(self.cursor.trail.fast_decay_ms, 4_000);
        self.cursor.trail.minimum_distance_x = self.cursor.trail.minimum_distance_x.min(1_000);
        self.cursor.trail.minimum_distance_y = self.cursor.trail.minimum_distance_y.min(1_000);
        self.cursor.trail.trigger_delay_ms = self.cursor.trail.trigger_delay_ms.min(1_000);
        if let Some(lines) = self.scrollback.lines {
            self.scrollback.lines = Some(lines.max(100)); // allow unbounded if None, but min 100
        }
        self.scrollback.scroll_multiplier = self.scrollback.scroll_multiplier.clamp(0.5, 10.0);
        self.command_completion_indicator.display_duration_ms = self
            .command_completion_indicator
            .display_duration_ms
            .clamp(250, 60_000);
        self.window.padding.top = self.window.padding.top.clamp(0, 100);
        self.window.padding.bottom = self.window.padding.bottom.clamp(0, 100);
        self.window.padding.left = self.window.padding.left.clamp(0, 100);
        self.window.padding.right = self.window.padding.right.clamp(0, 100);
        if self.blur.method == BlurMethod::Off {
            self.blur.enabled = false;
        }

        // Validate Colors
        let mut parse_c = |hex: &str, path: &str, fallback: Color| -> Color {
            if let Some(c) = parse_hex_color(hex) {
                c
            } else {
                errors.push(ConfigError::InvalidColor { path: path.to_string(), value: hex.to_string() });
                fallback
            }
        };

        let default_theme = ThemeConfig::default();
        self.theme.parsed_background = parse_c(&self.theme.background, "theme.background", default_theme.parsed_background);
        self.theme.parsed_foreground = parse_c(&self.theme.foreground, "theme.foreground", default_theme.parsed_foreground);
        self.theme.parsed_popup_background = parse_c(&self.theme.popup_background, "theme.popup_background", default_theme.parsed_popup_background);
        self.theme.parsed_cursor_color = parse_c(&self.theme.cursor_color, "theme.cursor_color", default_theme.parsed_cursor_color);
        self.theme.parsed_selection_bg = parse_c(&self.theme.selection_bg, "theme.selection_bg", default_theme.parsed_selection_bg);
        self.theme.parsed_pane_outline_active = parse_c(&self.theme.pane_outline_active, "theme.pane_outline_active", default_theme.parsed_pane_outline_active);
        self.theme.parsed_pane_outline_inactive = parse_c(&self.theme.pane_outline_inactive, "theme.pane_outline_inactive", default_theme.parsed_pane_outline_inactive);

        self.cursor.trail.parsed_color = if self.cursor.trail.color.eq_ignore_ascii_case("cursor") {
            None
        } else {
            Some(parse_c(
                &self.cursor.trail.color,
                "cursor.trail.color",
                default_theme.parsed_cursor_color,
            ))
        };

        if let Some(ansi_colors) = self.theme.ansi_colors.take() {
            self.theme.parsed_ansi_colors = std::array::from_fn(|index| {
                parse_c(
                    &ansi_colors[index],
                    &format!("theme.ansi_colors[{index}]"),
                    default_theme.parsed_ansi_colors[index],
                )
            });
        } else {
            self.theme.parsed_ansi_colors = [
                parse_c(&self.theme.ansi.black, "theme.ansi.black", default_theme.parsed_ansi_colors[0]),
                parse_c(&self.theme.ansi.red, "theme.ansi.red", default_theme.parsed_ansi_colors[1]),
                parse_c(&self.theme.ansi.green, "theme.ansi.green", default_theme.parsed_ansi_colors[2]),
                parse_c(&self.theme.ansi.yellow, "theme.ansi.yellow", default_theme.parsed_ansi_colors[3]),
                parse_c(&self.theme.ansi.blue, "theme.ansi.blue", default_theme.parsed_ansi_colors[4]),
                parse_c(&self.theme.ansi.magenta, "theme.ansi.magenta", default_theme.parsed_ansi_colors[5]),
                parse_c(&self.theme.ansi.cyan, "theme.ansi.cyan", default_theme.parsed_ansi_colors[6]),
                parse_c(&self.theme.ansi.white, "theme.ansi.white", default_theme.parsed_ansi_colors[7]),
                parse_c(&self.theme.ansi.bright_black, "theme.ansi.bright_black", default_theme.parsed_ansi_colors[8]),
                parse_c(&self.theme.ansi.bright_red, "theme.ansi.bright_red", default_theme.parsed_ansi_colors[9]),
                parse_c(&self.theme.ansi.bright_green, "theme.ansi.bright_green", default_theme.parsed_ansi_colors[10]),
                parse_c(&self.theme.ansi.bright_yellow, "theme.ansi.bright_yellow", default_theme.parsed_ansi_colors[11]),
                parse_c(&self.theme.ansi.bright_blue, "theme.ansi.bright_blue", default_theme.parsed_ansi_colors[12]),
                parse_c(&self.theme.ansi.bright_magenta, "theme.ansi.bright_magenta", default_theme.parsed_ansi_colors[13]),
                parse_c(&self.theme.ansi.bright_cyan, "theme.ansi.bright_cyan", default_theme.parsed_ansi_colors[14]),
                parse_c(&self.theme.ansi.bright_white, "theme.ansi.bright_white", default_theme.parsed_ansi_colors[15]),
            ];
        }

        let default_confirm = ConfirmCloseConfig::default();
        self.confirm_close.parsed_panel_color = parse_c(&self.confirm_close.panel_color, "confirm_close.panel_color", default_confirm.parsed_panel_color);
        self.confirm_close.parsed_selected_color = parse_c(&self.confirm_close.selected_color, "confirm_close.selected_color", default_confirm.parsed_selected_color);

        // Validate Env
        for env_str in &self.shell.env {
            if let Some((k, v)) = env_str.split_once('=') {
                self.shell.parsed_env.push((k.to_string(), v.to_string()));
            } else {
                errors.push(ConfigError::InvalidEnvVar { value: env_str.clone() });
            }
        }

        // Validate Keybindings
        self.keybindings.clear();
        if !self.behavior.disable_default_keybindings {
            install_default_keybindings(&mut self.keybindings);
        }

        for (action_name, key) in &self.raw_keybindings {
            let ks = KeyStroke::parse(key);
            let act = parse_action(action_name);

            match (ks, act) {
                (Some(k), Some(a)) => {
                    self.keybindings.retain(|_, existing| existing != &a);
                    self.keybindings.insert(k, a);
                }
                (None, _) => {
                    errors.push(ConfigError::InvalidKeybinding { key: key.clone(), action: action_name.clone(), reason: "Invalid key format".to_string() });
                }
                (_, None) => {
                    errors.push(ConfigError::InvalidKeybinding { key: key.clone(), action: action_name.clone(), reason: "Unknown action".to_string() });
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::modifiers;

    #[test]
    fn shell_integration_defaults_enabled_and_parses_boolean() {
        assert!(ForgeConfig::default().shell.integration_enabled);

        let omitted: ForgeConfig = toml::from_str("[shell]\n").unwrap();
        assert!(omitted.shell.integration_enabled);

        let config: ForgeConfig =
            toml::from_str("[shell]\nintegration_enabled = false\n").unwrap();
        assert!(!config.shell.integration_enabled);
    }

    #[test]
    fn center_on_launch_is_disabled_by_default_and_available_to_early_config() {
        assert!(!ForgeConfig::default().window.center_on_launch);

        let early: EarlyStartupConfig =
            toml::from_str("[window]\ncenter_on_launch = true\n").unwrap();
        assert!(early.window.center_on_launch);
    }

    #[test]
    fn default_ctrl_shift_d_docks_floating_pane() {
        let config = ForgeConfig::default();
        let key = KeyStroke {
            modifiers: modifiers::CTRL | modifiers::SHIFT,
            keysym: KeyStroke::normalized_keysym('D' as u32),
        };

        assert_eq!(
            config.keybindings.get(&key),
            Some(&Action::TogglePaneFloating)
        );
    }

    #[test]
    fn default_ctrl_shift_q_remains_close_pane() {
        let config = ForgeConfig::default();
        let key = KeyStroke {
            modifiers: modifiers::CTRL | modifiers::SHIFT,
            keysym: KeyStroke::normalized_keysym('Q' as u32),
        };

        assert_eq!(config.keybindings.get(&key), Some(&Action::ClosePane));
    }

    #[test]
    fn default_ctrl_shift_bindings_move_scrolling_panes() {
        let config = ForgeConfig::default();
        for (key, action) in [
            ("ctrl+shift+left", Action::MovePaneLeft),
            ("ctrl+shift+right", Action::MovePaneRight),
            ("ctrl+shift+up", Action::MovePaneUp),
            ("ctrl+shift+down", Action::MovePaneDown),
            ("ctrl+shift+1", Action::MovePaneToTab1),
            ("ctrl+shift+2", Action::MovePaneToTab2),
            ("ctrl+shift+3", Action::MovePaneToTab3),
            ("ctrl+shift+4", Action::MovePaneToTab4),
            ("ctrl+shift+5", Action::MovePaneToTab5),
            ("ctrl+shift+6", Action::MovePaneToTab6),
            ("ctrl+shift+7", Action::MovePaneToTab7),
            ("ctrl+shift+8", Action::MovePaneToTab8),
            ("ctrl+shift+9", Action::MovePaneToTab9),
        ] {
            assert_eq!(
                config.keybindings.get(&KeyStroke::parse(key).unwrap()),
                Some(&action)
            );
        }
        assert!(!config
            .keybindings
            .contains_key(&KeyStroke::parse("alt+shift+left").unwrap()));
    }

    #[test]
    fn toml_keybinds_action_to_key_syntax_compiles_copy_binding() {
        let mut config: ForgeConfig = toml::from_str(
            r#"
            [keybinds]
            copy = "ctrl+shift+c"
            "#,
        )
        .expect("keybind config should deserialize");

        assert!(config.validate().is_empty());
        let copy = KeyStroke::parse("ctrl+shift+c").unwrap();
        assert_eq!(config.keybindings.get(&copy), Some(&Action::Copy));
    }

    #[test]
    fn keybindings_alias_and_custom_binding_replace_default_action_key() {
        let mut config: ForgeConfig = toml::from_str(
            r#"
            [keybindings]
            copy = "alt+c"
            "#,
        )
        .expect("legacy keybindings section should deserialize");

        assert!(config.validate().is_empty());
        let custom = KeyStroke::parse("alt+c").unwrap();
        let default = KeyStroke::parse("ctrl+shift+c").unwrap();
        assert_eq!(config.keybindings.get(&custom), Some(&Action::Copy));
        assert_eq!(config.keybindings.get(&default), None);
    }

    #[test]
    fn validation_rebuilds_defaults_after_toml_deserialization() {
        let mut config: ForgeConfig = toml::from_str("").expect("empty config should deserialize");

        assert!(config.validate().is_empty());
        let copy = KeyStroke::parse("ctrl+shift+c").unwrap();
        let paste = KeyStroke::parse("ctrl+shift+v").unwrap();
        assert_eq!(config.keybindings.get(&copy), Some(&Action::Copy));
        assert_eq!(config.keybindings.get(&paste), Some(&Action::Paste));
    }

    #[test]
    fn command_completion_indicator_defaults_to_enabled_ten_seconds() {
        let config = ForgeConfig::default();

        assert_eq!(
            config.command_completion_indicator.mode,
            CommandCompletionIndicatorMode::Enabled
        );
        assert_eq!(
            config.command_completion_indicator.minimum_duration_ms,
            10_000
        );
        assert_eq!(
            config.command_completion_indicator.dismissal,
            CommandCompletionIndicatorDismissal::Timeout
        );
        assert_eq!(config.command_completion_indicator.display_duration_ms, 3_000);
        assert!(!config.command_completion_indicator.transparent);
        assert_eq!(config.theme.popup_background, "#1e1e2e");
        assert_eq!(
            config.theme.parsed_popup_background,
            Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            }
        );
    }

    #[test]
    fn command_completion_indicator_toml_parses_disabled_on_zoom() {
        let mut config: ForgeConfig = toml::from_str(
            r##"
            [command_completion_indicator]
            mode = "disabled_on_zoom"
            minimum_duration_ms = 2500
            dismissal = "on_interaction"
            display_duration_ms = 3500
            transparent = true

            [theme]
            popup_background = "#1e1e2e80"
            "##,
        )
        .expect("config should parse");
        let errors = config.validate();

        assert!(errors.is_empty());
        assert_eq!(
            config.command_completion_indicator.mode,
            CommandCompletionIndicatorMode::DisabledOnZoom
        );
        assert_eq!(config.command_completion_indicator.minimum_duration_ms, 2500);
        assert_eq!(
            config.command_completion_indicator.dismissal,
            CommandCompletionIndicatorDismissal::OnInteraction
        );
        assert_eq!(config.command_completion_indicator.display_duration_ms, 3500);
        assert!(config.command_completion_indicator.transparent);
        assert_eq!(
            config.theme.parsed_popup_background,
            Color {
                r: 30,
                g: 30,
                b: 46,
                a: 128,
            }
        );
    }

    #[test]
    fn popup_background_rejects_invalid_color() {
        let mut invalid: ForgeConfig = toml::from_str(
            r#"
            [theme]
            popup_background = "not-a-color"
            "#,
        )
        .expect("config should parse");
        let errors = invalid.validate();
        assert!(errors.iter().any(|error| matches!(
            error,
            ConfigError::InvalidColor { path, .. }
                if path == "theme.popup_background"
        )));
        assert_eq!(
            invalid.theme.parsed_popup_background,
            ThemeConfig::default().parsed_popup_background
        );
    }

    #[test]
    fn cursor_trail_defaults_disabled_with_balanced_decay() {
        let config = ForgeConfig::default();

        assert!(!config.cursor.trail.enabled);
        assert!(!config.cursor.trail.segmented);
        assert_eq!(config.cursor.trail.fast_decay_ms, 100);
        assert_eq!(config.cursor.trail.slow_decay_ms, 400);
        assert_eq!(config.cursor.trail.minimum_distance_x, 2);
        assert_eq!(config.cursor.trail.minimum_distance_y, 2);
        assert_eq!(config.cursor.trail.trigger_delay_ms, 50);
        assert_eq!(config.cursor.trail.color, "cursor");
        assert_eq!(config.cursor.trail.parsed_color, None);
    }

    #[test]
    fn cursor_trail_toml_parses_and_normalizes_once() {
        let mut config: ForgeConfig = toml::from_str(
            r##"
            [cursor.trail]
            enabled = true
            segmented = true
            fast_decay_ms = 250
            slow_decay_ms = 100
            minimum_distance_x = 3
            minimum_distance_y = 5
            trigger_delay_ms = 75
            color = "#ff8040c0"
            "##,
        )
        .expect("cursor trail config should parse");

        assert!(config.validate().is_empty());
        assert!(config.cursor.trail.enabled);
        assert!(config.cursor.trail.segmented);
        assert_eq!(config.cursor.trail.fast_decay_ms, 250);
        assert_eq!(config.cursor.trail.slow_decay_ms, 250);
        assert_eq!(config.cursor.trail.minimum_distance_x, 3);
        assert_eq!(config.cursor.trail.minimum_distance_y, 5);
        assert_eq!(config.cursor.trail.trigger_delay_ms, 75);
        assert_eq!(
            config.cursor.trail.parsed_color,
            Some(Color {
                r: 255,
                g: 128,
                b: 64,
                a: 192,
            })
        );
    }

    #[test]
    fn cursor_trail_accepts_legacy_experimental_segmented_name() {
        let config: ForgeConfig = toml::from_str(
            r#"
            [cursor.trail]
            experimental_segmented = true
            "#,
        )
        .expect("legacy segmented trail option should remain compatible");

        assert!(config.cursor.trail.segmented);
    }

    #[test]
    fn cursor_trail_rejects_invalid_custom_color() {
        let mut config: ForgeConfig = toml::from_str(
            r#"
            [cursor.trail]
            enabled = true
            color = "not-a-color"
            "#,
        )
        .expect("cursor trail config should parse structurally");

        let errors = config.validate();
        assert!(errors.iter().any(|error| matches!(
            error,
            ConfigError::InvalidColor { path, .. } if path == "cursor.trail.color"
        )));
    }
}
