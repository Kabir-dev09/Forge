use std::hash::Hash;
use serde::{Serialize, Deserialize};

pub mod modifiers {
    pub const NONE: u8  = 0;
    pub const CTRL: u8  = 1 << 0;
    pub const SHIFT: u8 = 1 << 1;
    pub const ALT: u8   = 1 << 2;
    pub const LOGO: u8  = 1 << 3;
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
}

impl KeyStroke {
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = modifiers::NONE;
        let mut parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
        
        if parts.is_empty() {
            return None;
        }
        
        let key_str = parts.pop().unwrap(); // The last part is the key
        
        for part in parts {
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
            // Fast path for single characters.
            // XKB keysyms for ASCII match the ASCII value.
            let c = s.chars().next().unwrap();
            if c.is_ascii() {
                return Some(c.to_ascii_lowercase() as u32);
            }
        }
        
        // Handle special keys (mapped to standard X11/XKB keysyms)
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
            _ => None,
        }
    }
}
