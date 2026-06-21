use crate::color::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub bold_family: Option<String>,
    pub italic_family: Option<String>,
    pub ligatures: bool,
    pub nerd_fonts: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            bold_family: None,
            italic_family: None,
            ligatures: true,
            nerd_fonts: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PaddingBalance {
    Center,
    Fill,
}

impl Default for PaddingBalance {
    fn default() -> Self {
        PaddingBalance::Center
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub padding_balance: PaddingBalance,
    pub opacity: f32,
    pub title: String,
    pub decorations: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            padding: PaddingConfig::default(),
            padding_balance: PaddingBalance::default(),
            opacity: 1.0,
            title: "Forge".to_string(),
            decorations: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    pub ansi_colors: [Color; 16],
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background: Color { r: 26, g: 27, b: 38, a: 255 },
            foreground: Color { r: 192, g: 202, b: 245, a: 255 },
            cursor_color: Color { r: 192, g: 202, b: 245, a: 255 },
            selection_bg: Color { r: 65, g: 72, b: 104, a: 200 },
            ansi_colors: [
                Color { r: 65, g: 72, b: 104, a: 255 },   // 0: Black
                Color { r: 247, g: 118, b: 142, a: 255 }, // 1: Red
                Color { r: 158, g: 206, b: 106, a: 255 }, // 2: Green
                Color { r: 224, g: 175, b: 104, a: 255 }, // 3: Yellow
                Color { r: 122, g: 162, b: 247, a: 255 }, // 4: Blue
                Color { r: 187, g: 154, b: 247, a: 255 }, // 5: Magenta
                Color { r: 125, g: 207, b: 255, a: 255 }, // 6: Cyan
                Color { r: 192, g: 202, b: 245, a: 255 }, // 7: White
                Color { r: 65, g: 72, b: 104, a: 255 },   // 8: Bright Black
                Color { r: 247, g: 118, b: 142, a: 255 }, // 9: Bright Red
                Color { r: 158, g: 206, b: 106, a: 255 }, // 10: Bright Green
                Color { r: 224, g: 175, b: 104, a: 255 }, // 11: Bright Yellow
                Color { r: 122, g: 162, b: 247, a: 255 }, // 12: Bright Blue
                Color { r: 187, g: 154, b: 247, a: 255 }, // 13: Bright Magenta
                Color { r: 125, g: 207, b: 255, a: 255 }, // 14: Bright Cyan
                Color { r: 192, g: 202, b: 245, a: 255 }, // 15: Bright White
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgeConfig {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub cursor: CursorConfig,
    pub scrollback: ScrollbackConfig,
    pub shell: ShellConfig,
    pub theme: ThemeConfig,
    pub behavior: BehaviorConfig,
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

        Self {
            font: FontConfig::default(),
            window: WindowConfig::default(),
            cursor: CursorConfig::default(),
            scrollback: ScrollbackConfig::default(),
            shell: ShellConfig::default(),
            theme: ThemeConfig::default(),
            behavior: BehaviorConfig::default(),
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
        config.validate();
        assert_eq!(config.font.size, 72.0);
        assert_eq!(config.window.opacity, 1.0);
        assert_eq!(config.window.width, 200);
    }
}
