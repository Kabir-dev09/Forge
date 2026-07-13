use forge_core::{cell::Cell, color::Color, config_registry::ThemeConfig};

pub const SIDEBAR_COLS: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarState {
    pub visible: bool,
    pub generation: u64,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            visible: false,
            generation: 1,
        }
    }
}

impl SidebarState {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn width_cols(&self) -> usize {
        if self.visible {
            SIDEBAR_COLS
        } else {
            0
        }
    }

    pub fn render_grid(&self, rows: usize, theme: &ThemeConfig) -> Vec<Vec<Cell>> {
        let rows = rows.max(1);
        let cols = SIDEBAR_COLS;
        let palette = SidebarPalette::from_theme(theme);
        vec![vec![cell(' ', palette.fg, palette.bg); cols]; rows]
    }
}

#[derive(Clone, Copy)]
struct SidebarPalette {
    bg: Color,
    fg: Color,
}

impl SidebarPalette {
    fn from_theme(theme: &ThemeConfig) -> Self {
        let bg = mix_color(theme.background, Color::BLACK, 0.20);
        Self {
            bg,
            fg: theme.foreground,
        }
    }
}

fn cell(c: char, fg: Color, bg: Color) -> Cell {
    Cell {
        c,
        fg,
        bg,
        flags: 0,
    }
}

fn mix_color(fg: Color, bg: Color, bg_weight: f32) -> Color {
    let bg_weight = bg_weight.clamp(0.0, 1.0);
    let fg_weight = 1.0 - bg_weight;
    Color {
        r: ((fg.r as f32) * fg_weight + (bg.r as f32) * bg_weight).round() as u8,
        g: ((fg.g as f32) * fg_weight + (bg.g as f32) * bg_weight).round() as u8,
        b: ((fg.b as f32) * fg_weight + (bg.b as f32) * bg_weight).round() as u8,
        a: 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_sidebar_reserves_no_columns() {
        let sidebar = SidebarState::default();
        assert_eq!(sidebar.width_cols(), 0);
    }

    #[test]
    fn visible_sidebar_renders_blank_design_surface() {
        let mut sidebar = SidebarState::default();
        sidebar.toggle();
        let grid = sidebar.render_grid(8, &ThemeConfig::default());

        assert_eq!(grid.len(), 8);
        assert_eq!(grid[0].len(), SIDEBAR_COLS);
        assert!(grid.iter().flatten().all(|cell| cell.c == ' '));
    }
}
