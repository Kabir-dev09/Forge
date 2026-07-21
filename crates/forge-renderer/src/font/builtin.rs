//! Fonts shipped inside the executable. These reads are compile-time includes;
//! the runtime never depends on an `assets` directory.

pub const FAMILY_NAME: &str = "JetBrainsMono Nerd Font";
pub const DEFAULT_SIZE: f32 = 14.0;

pub fn regular() -> &'static [u8] {
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf")
}

pub fn bold() -> &'static [u8] {
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Bold.ttf")
}

pub fn italic() -> &'static [u8] {
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Italic.ttf")
}

pub fn bold_italic() -> &'static [u8] {
    include_bytes!("../../../../assets/fonts/JetBrainsMono-BoldItalic.ttf")
}

pub fn default_atlas() -> &'static [u8] {
    include_bytes!("../../../../assets/fonts/JetBrainsMono-14.fga")
}
