use forge_core::{
    cell::Cell,
    color::Color,
    config_registry::{ConfirmCloseBackgroundMode, ConfirmCloseConfig, ThemeConfig},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmCloseTarget {
    Pane(crate::mux::PaneId),
    Tab(crate::mux::TabId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmSelection {
    Close,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmCloseModal {
    pub target: ConfirmCloseTarget,
    pub target_label: &'static str,
    pub program_name: Option<String>,
    pub selected: ConfirmSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalAction {
    Ignored,
    Redraw,
    Confirm(ConfirmCloseTarget),
    Cancel,
}

impl ConfirmCloseModal {
    pub fn for_pane(pane_id: crate::mux::PaneId, program_name: Option<String>) -> Self {
        Self {
            target: ConfirmCloseTarget::Pane(pane_id),
            target_label: "pane",
            program_name,
            selected: ConfirmSelection::Cancel,
        }
    }

    pub fn for_tab(tab_id: crate::mux::TabId, program_name: Option<String>) -> Self {
        Self {
            target: ConfirmCloseTarget::Tab(tab_id),
            target_label: "tab",
            program_name,
            selected: ConfirmSelection::Cancel,
        }
    }

    pub fn handle_key_bytes(&mut self, bytes: &[u8]) -> ModalAction {
        match bytes {
            b"\r" | b"\n" => match self.selected {
                ConfirmSelection::Close => ModalAction::Confirm(self.target),
                ConfirmSelection::Cancel => ModalAction::Cancel,
            },
            b"\x1b" | b"n" | b"N" | b"q" | b"Q" => ModalAction::Cancel,
            b"y" | b"Y" => ModalAction::Confirm(self.target),
            b"\t" | b"\x1b[C" | b"\x1b[D" => {
                self.toggle_selection();
                ModalAction::Redraw
            }
            _ => ModalAction::Ignored,
        }
    }

    fn toggle_selection(&mut self) {
        self.selected = match self.selected {
            ConfirmSelection::Close => ConfirmSelection::Cancel,
            ConfirmSelection::Cancel => ConfirmSelection::Close,
        };
    }

    pub fn render_base_grid(
        &self,
        max_cols: usize,
        max_rows: usize,
        theme: &ThemeConfig,
        config: &ConfirmCloseConfig,
    ) -> Vec<Vec<Cell>> {
        let cols = max_cols.max(1);
        let rows = max_rows.max(1);
        let palette = ConfirmModalPalette::from_theme(theme, config);
        let mut grid = vec![vec![transparent_cell(palette); cols]; rows];
        let Some(layout) = self.layout(cols, rows) else {
            return grid;
        };

        if config.background_mode == ConfirmCloseBackgroundMode::OpaquePanel {
            for row in grid
                .iter_mut()
                .skip(layout.modal_row)
                .take(layout.modal_rows)
            {
                for cell in row.iter_mut().skip(layout.modal_col).take(layout.width) {
                    cell.bg = palette.panel_bg;
                }
            }
        }

        put_line(
            &mut grid[layout.modal_row],
            layout.modal_col,
            &format!("╭{}╮", "─".repeat(layout.inner)),
            palette.border,
        );
        put_line(
            &mut grid[layout.modal_row + layout.modal_rows - 1],
            layout.modal_col,
            &format!("╰{}╯", "─".repeat(layout.inner)),
            palette.border,
        );
        for row in grid
            .iter_mut()
            .skip(layout.modal_row + 1)
            .take(layout.modal_rows - 2)
        {
            put_char(row, layout.modal_col, '│', palette.border);
            put_char(
                row,
                layout.modal_col + layout.width - 1,
                '│',
                palette.border,
            );
        }

        grid
    }

    pub fn render_text_grid(
        &self,
        max_cols: usize,
        max_rows: usize,
        theme: &ThemeConfig,
        config: &ConfirmCloseConfig,
    ) -> Vec<Vec<Cell>> {
        let cols = max_cols.max(1);
        let rows = max_rows.max(1);
        let palette = ConfirmModalPalette::from_theme(theme, config);
        let mut grid = vec![vec![transparent_cell(palette); cols]; rows];

        let title = self.title();
        let warning = "This will terminate the program.";
        let actions = self.action_line();
        let full_help = "[Enter] Confirm   [Esc] Cancel   [←/→/Tab] Switch";
        let short_help = "Enter Confirm   Esc Cancel   Tab Switch";

        let Some(layout) = self.layout(cols, rows) else {
            self.draw_compact_grid(&mut grid, palette);
            return grid;
        };

        let inner_start = layout.modal_col + 1;
        put_centered_in_range(
            &mut grid[layout.modal_row + 2],
            inner_start,
            layout.inner,
            &title,
            palette.text,
        );
        put_centered_in_range(
            &mut grid[layout.modal_row + 4],
            inner_start,
            layout.inner,
            warning,
            palette.warning,
        );
        put_centered_in_range_with_spans(
            &mut grid[layout.modal_row + 6],
            inner_start,
            layout.inner,
            &actions,
            self.action_spans(&actions, palette),
            palette.text,
        );

        let footer_row = layout.modal_row + layout.modal_rows + 1;
        if footer_row < rows {
            if full_help.chars().count() <= cols {
                put_centered_plain(&mut grid[footer_row], full_help, palette.warning);
            } else if short_help.chars().count() <= cols {
                put_centered_plain(&mut grid[footer_row], short_help, palette.warning);
            }
        }

        grid
    }

    fn layout(&self, cols: usize, rows: usize) -> Option<ModalGeometry> {
        if cols < 24 || rows < 7 {
            return None;
        }

        let title = self.title();
        let warning = "This will terminate the program.";
        let actions = self.action_line();
        let content_width = title
            .chars()
            .count()
            .max(warning.chars().count())
            .max(actions.chars().count())
            .max(40);
        let width = (content_width + 8)
            .max(40)
            .min(cols.saturating_sub(4).max(1));
        if width < 4 {
            return None;
        }

        let modal_rows = 9usize;
        let modal_row = rows.saturating_sub(modal_rows + 2) / 2;
        if modal_row + modal_rows > rows {
            return None;
        }

        Some(ModalGeometry {
            width,
            inner: width.saturating_sub(2),
            modal_col: cols.saturating_sub(width) / 2,
            modal_row,
            modal_rows,
        })
    }

    fn draw_compact_grid(&self, grid: &mut [Vec<Cell>], palette: ConfirmModalPalette) {
        let text = if let Some(program) = &self.program_name {
            format!("Close {} running {}? [y/N]", self.target_label, program)
        } else {
            format!("Close {}? [y/N]", self.target_label)
        };
        let Some(row) = grid.get_mut(grid.len().saturating_sub(1) / 2) else {
            return;
        };
        let visible: String = text.chars().take(row.len()).collect();
        put_centered_plain(row, &visible, palette.text);
    }

    fn title(&self) -> String {
        if let Some(program) = self.program_name.as_deref().filter(|s| !s.is_empty()) {
            format!("Close this {} running {}?", self.target_label, program)
        } else {
            format!("Close this {}?", self.target_label)
        }
    }

    fn action_line(&self) -> String {
        match self.selected {
            ConfirmSelection::Close => "› Close           Cancel".to_string(),
            ConfirmSelection::Cancel => "  Close           › Cancel".to_string(),
        }
    }

    fn action_spans(
        &self,
        line: &str,
        palette: ConfirmModalPalette,
    ) -> Vec<(usize, usize, Color, u8)> {
        let selected = match self.selected {
            ConfirmSelection::Close => "› Close",
            ConfirmSelection::Cancel => "› Cancel",
        };
        let start = line.find(selected).unwrap_or(0);
        vec![(
            start,
            selected.chars().count(),
            palette.accent,
            Cell::FLAG_UNDERLINE,
        )]
    }
}

#[derive(Clone, Copy, Debug)]
struct ModalGeometry {
    width: usize,
    inner: usize,
    modal_col: usize,
    modal_row: usize,
    modal_rows: usize,
}

#[derive(Clone, Copy, Debug)]
struct ConfirmModalPalette {
    panel_bg: Color,
    border: Color,
    text: Color,
    warning: Color,
    accent: Color,
}

impl ConfirmModalPalette {
    fn from_theme(theme: &ThemeConfig, config: &ConfirmCloseConfig) -> Self {
        let background = theme.parsed_background;
        let foreground = with_alpha(theme.parsed_foreground, 255);

        Self {
            panel_bg: with_alpha(config.parsed_panel_color, 255),
            border: with_alpha(
                mix_color(theme.parsed_pane_outline_inactive, foreground, 0.20),
                255,
            ),
            text: brighten_color(foreground, 0.35),
            warning: brighten_color(mix_color(foreground, background, 0.45), 0.20),
            accent: brighten_color(with_alpha(config.parsed_selected_color, 255), 0.15),
        }
    }
}

fn transparent_cell(palette: ConfirmModalPalette) -> Cell {
    Cell {
        c: ' ',
        fg: palette.text,
        bg: Color::TRANSPARENT,
        flags: 0,
    }
}

fn with_alpha(mut color: Color, alpha: u8) -> Color {
    color.a = alpha;
    color
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

fn brighten_color(color: Color, amount: f32) -> Color {
    mix_color(Color::WHITE, color, (1.0 - amount).clamp(0.0, 1.0))
}

fn put_char(row: &mut [Cell], col: usize, c: char, fg: Color) {
    if let Some(cell) = row.get_mut(col) {
        cell.c = c;
        cell.fg = fg;
    }
}

fn put_line(row: &mut [Cell], start: usize, text: &str, fg: Color) {
    for (i, c) in text.chars().enumerate() {
        put_char(row, start + i, c, fg);
    }
}

fn put_centered_plain(row: &mut [Cell], text: &str, fg: Color) {
    let start = row.len().saturating_sub(text.chars().count()) / 2;
    put_line(row, start, text, fg);
}

fn put_centered_in_range(row: &mut [Cell], start_col: usize, width: usize, text: &str, fg: Color) {
    let text_len = text.chars().count();
    let start = start_col + width.saturating_sub(text_len) / 2;
    put_line(row, start, text, fg);
}

fn put_centered_in_range_with_spans(
    row: &mut [Cell],
    start_col: usize,
    width: usize,
    text: &str,
    spans: Vec<(usize, usize, Color, u8)>,
    base_fg: Color,
) {
    let start = start_col + width.saturating_sub(text.chars().count()) / 2;
    put_line(row, start, text, base_fg);
    for (span_start, span_len, fg, flags) in spans {
        for idx in start + span_start..start + span_start + span_len {
            if let Some(cell) = row.get_mut(idx) {
                cell.fg = fg;
                cell.flags |= flags;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_cancels_by_default() {
        let mut modal =
            ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), Some("nvim".into()));
        assert_eq!(modal.handle_key_bytes(b"\r"), ModalAction::Cancel);
    }

    #[test]
    fn tab_then_enter_confirms_close() {
        let mut modal = ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), None);
        assert_eq!(modal.handle_key_bytes(b"\t"), ModalAction::Redraw);
        assert_eq!(
            modal.handle_key_bytes(b"\r"),
            ModalAction::Confirm(ConfirmCloseTarget::Pane(crate::mux::PaneId::new(1)))
        );
    }

    #[test]
    fn footer_is_hidden_when_it_does_not_fit() {
        let modal = ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), Some("yes".into()));
        let grid = modal.render_text_grid(
            32,
            12,
            &ThemeConfig::default(),
            &ConfirmCloseConfig::default(),
        );
        let text: String = grid
            .iter()
            .flat_map(|row| row.iter().map(|cell| cell.c))
            .collect();
        assert!(!text.contains("[←/→/Tab]"));
    }

    #[test]
    fn modal_uses_theme_foreground_for_final_text() {
        let modal = ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), Some("yes".into()));
        let theme = ThemeConfig::default();
        let config = ConfirmCloseConfig::default();
        let palette = ConfirmModalPalette::from_theme(&theme, &config);
        let grid = modal.render_text_grid(80, 24, &theme, &config);
        assert!(grid
            .iter()
            .flat_map(|row| row.iter())
            .any(|cell| cell.c == 'C' && cell.fg == palette.text));
    }

    #[test]
    fn base_layer_keeps_viewport_outside_panel_transparent() {
        let modal = ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), Some("yes".into()));
        let grid = modal.render_base_grid(
            80,
            24,
            &ThemeConfig::default(),
            &ConfirmCloseConfig::default(),
        );
        assert_eq!(grid[0][0].bg, Color::TRANSPARENT);
        assert!(grid
            .iter()
            .flat_map(|row| row.iter())
            .any(|cell| cell.c == '╭'));
    }

    #[test]
    fn default_base_layer_has_opaque_panel_background() {
        let modal = ConfirmCloseModal::for_pane(crate::mux::PaneId::new(1), Some("yes".into()));
        let config = ConfirmCloseConfig::default();
        let grid = modal.render_base_grid(80, 24, &ThemeConfig::default(), &config);
        assert!(grid
            .iter()
            .flat_map(|row| row.iter())
            .any(|cell| cell.bg == config.parsed_panel_color));
    }
}
