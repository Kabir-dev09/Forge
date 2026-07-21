use serde::{Deserialize, Serialize};
use std::hash::Hash;

pub mod modifiers {
    pub const NONE: u8 = 0;
    pub const CTRL: u8 = 1 << 0;
    pub const SHIFT: u8 = 1 << 1;
    pub const ALT: u8 = 1 << 2;
    pub const LOGO: u8 = 1 << 3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_uppercase_ascii_keysyms() {
        assert_eq!(KeyStroke::normalized_keysym(b'C' as u32), b'c' as u32);
        assert_eq!(KeyStroke::normalized_keysym(b'c' as u32), b'c' as u32);
    }

    #[test]
    fn maps_shifted_ascii_punctuation_to_physical_base_key() {
        assert_eq!(
            KeyStroke::unshifted_ascii_keysym(b'|' as u32),
            Some(b'\\' as u32)
        );
        assert_eq!(
            KeyStroke::unshifted_ascii_keysym(b'_' as u32),
            Some(b'-' as u32)
        );
        assert_eq!(
            KeyStroke::unshifted_ascii_keysym(b'+' as u32),
            Some(b'=' as u32)
        );
        assert_eq!(KeyStroke::unshifted_ascii_keysym(b'a' as u32), None);
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct KeyStroke {
    pub modifiers: u8,
    pub keysym: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Copy,
    Paste,
    ToggleFullscreen,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    SplitVertical,
    SplitHorizontal,
    TogglePaneZoom,
    ToggleSidebar,
    ClosePane,
    SpawnFloatingPane,
    TogglePaneFloating,
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPaneDown,
    // Tab actions
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    SwitchTab1,
    SwitchTab2,
    SwitchTab3,
    SwitchTab4,
    SwitchTab5,
    SwitchTab6,
    SwitchTab7,
    SwitchTab8,
    SwitchTab9,
    MoveTabLeft,
    MoveTabRight,
}

impl KeyStroke {
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = modifiers::NONE;

        // Handle the literal "+" key carefully since we split by "+"
        let (modifier_part, key_str) = if s.to_lowercase().ends_with("+plus") {
            (s[..s.len() - 5].trim().to_string(), "plus")
        } else if s.ends_with("++") {
            (s[..s.len() - 2].trim().to_string(), "+")
        } else {
            let mut parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
            if parts.is_empty() {
                return None;
            }
            let k = parts.pop().unwrap();
            let m = parts.join("+");
            (m, k)
        };

        let m_parts: Vec<&str> = modifier_part
            .split('+')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        for part in m_parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => mods |= modifiers::CTRL,
                "shift" => mods |= modifiers::SHIFT,
                "alt" => mods |= modifiers::ALT,
                "super" | "logo" | "cmd" | "win" => mods |= modifiers::LOGO,
                _ => return None, // Unknown modifier
            }
        }

        let keysym = Self::parse_keysym(key_str)?;

        Some(Self {
            modifiers: mods,
            keysym,
        })
    }

    fn parse_keysym(s: &str) -> Option<u32> {
        if s.len() == 1 {
            let c = s.chars().next().unwrap();
            if c.is_ascii() {
                return Some(c.to_ascii_lowercase() as u32);
            }
        }

        // Handle special keys
        match s.to_lowercase().as_str() {
            "return" | "enter" => Some(0xff0d),
            "escape" | "esc" => Some(0xff1b),
            "backspace" | "bs" => Some(0xff08),
            "tab" => Some(0xff09),
            "space" => Some(0x0020),
            "up" => Some(0xff52),
            "down" => Some(0xff54),
            "left" => Some(0xff51),
            "right" => Some(0xff53),
            "delete" | "del" => Some(0xffff),
            "home" => Some(0xff50),
            "end" => Some(0xff57),
            "pageup" | "pgup" => Some(0xff55),
            "pagedown" | "pgdn" => Some(0xff56),
            "insert" | "ins" => Some(0xff63),
            "f11" => Some(0xffc8),
            "plus" => Some(0x002b),
            "minus" => Some(0x002d),
            "backslash" | "\\" => Some(0x005c),
            "equal" => Some(0x003d),
            "kp_add" => Some(0xffab),
            "kp_subtract" => Some(0xffad),
            "kp_0" => Some(0xffb0),
            _ => None,
        }
    }

    pub fn normalized_keysym(keysym: u32) -> u32 {
        if (0x0041..=0x005A).contains(&keysym) {
            return keysym + 0x0020;
        }
        keysym
    }

    pub fn unshifted_ascii_keysym(keysym: u32) -> Option<u32> {
        let unshifted = match keysym {
            0x7e => b'`',
            0x21 => b'1',
            0x40 => b'2',
            0x23 => b'3',
            0x24 => b'4',
            0x25 => b'5',
            0x5e => b'6',
            0x26 => b'7',
            0x2a => b'8',
            0x28 => b'9',
            0x29 => b'0',
            0x5f => b'-',
            0x2b => b'=',
            0x7b => b'[',
            0x7d => b']',
            0x7c => b'\\',
            0x3a => b';',
            0x22 => b'\'',
            0x3c => b',',
            0x3e => b'.',
            0x3f => b'/',
            _ => return None,
        };
        Some(unshifted as u32)
    }
}
