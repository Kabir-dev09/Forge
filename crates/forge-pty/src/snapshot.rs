use crate::screen_buffer::ScrollEvent;
use forge_core::cell::{Cell, SelectionRange};
use forge_core::config_registry::CursorStyle;

#[derive(Clone)]
pub struct RenderSnapshot {
    pub grid: Vec<Vec<Cell>>,
    pub dirty_generations: Vec<u64>,
    pub cursor: Option<(usize, usize)>,
    pub cursor_style_override: Option<CursorStyle>,
    pub cursor_blink_override: Option<bool>,
    pub selection: Option<SelectionRange>,
    pub use_alt_buffer: bool,
    pub alt_buffer_generation: u64,
    pub visible_screen_lines: f64,
    pub history_lines: f64,
    pub viewport_offset: f64,
    pub mouse_tracking_enabled: bool,
    pub mouse_sgr_mode: bool,
    pub bracketed_paste: bool,
    pub scroll_event: Option<ScrollEvent>,
    pub scroll_id: u64,
    pub snapshot_id: u64,
    pub current_dir: Option<String>,
    pub current_title: Option<String>,
    pub current_command: Option<String>,
    pub is_command_running: bool,
}

impl RenderSnapshot {
    pub fn empty(cols: usize, rows: usize) -> Self {
        Self {
            grid: vec![
                vec![
                    Cell {
                        c: ' ',
                        fg: forge_core::color::Color::WHITE,
                        bg: forge_core::color::Color::BLACK,
                        flags: 0
                    };
                    cols
                ];
                rows
            ],
            dirty_generations: vec![1; rows],
            cursor: Some((0, 0)),
            cursor_style_override: None,
            cursor_blink_override: None,
            selection: None,
            use_alt_buffer: false,
            alt_buffer_generation: 0,
            visible_screen_lines: rows as f64,
            history_lines: 0.0,
            viewport_offset: 0.0,
            mouse_tracking_enabled: false,
            mouse_sgr_mode: false,
            bracketed_paste: false,
            scroll_event: None,
            scroll_id: 0,
            snapshot_id: 0,
            current_dir: None,
            current_title: None,
            current_command: None,
            is_command_running: false,
        }
    }

    pub fn text_in_range(&self, range: SelectionRange) -> String {
        let (start_row, start_col, end_row, end_col) = if range.start_row < range.end_row
            || (range.start_row == range.end_row && range.start_col <= range.end_col)
        {
            (
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col,
            )
        } else {
            (
                range.end_row,
                range.end_col,
                range.start_row,
                range.start_col,
            )
        };

        let mut result = String::new();

        for row_idx in start_row..=end_row {
            let Some(row) = self.grid.get(row_idx) else {
                break;
            };
            if row.is_empty() {
                if row_idx != end_row {
                    result.push('\n');
                }
                continue;
            }

            let start = if row_idx == start_row { start_col } else { 0 };
            let end = if row_idx == end_row {
                end_col
            } else {
                row.len().saturating_sub(1)
            };

            if start < row.len() {
                let mut line = String::new();
                let end = end.min(row.len().saturating_sub(1));
                for cell in &row[start..=end] {
                    if cell.c != '\0' {
                        line.push(cell.c);
                    }
                }
                result.push_str(line.trim_end());
            }

            if row_idx != end_row {
                result.push('\n');
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|c| Cell {
                c,
                fg: forge_core::color::Color::WHITE,
                bg: forge_core::color::Color::BLACK,
                flags: 0,
            })
            .collect()
    }

    #[test]
    fn text_in_range_extracts_multiline_selection() {
        let mut snapshot = RenderSnapshot::empty(8, 2);
        snapshot.grid = vec![row("abc     "), row("def     ")];

        let text = snapshot.text_in_range(SelectionRange {
            start_row: 0,
            start_col: 1,
            end_row: 1,
            end_col: 1,
        });

        assert_eq!(text, "bc\nde");
    }

    #[test]
    fn text_in_range_ignores_wide_placeholders_and_trims_line_ends() {
        let mut snapshot = RenderSnapshot::empty(8, 1);
        snapshot.grid = vec![vec![
            Cell {
                c: 'a',
                fg: forge_core::color::Color::WHITE,
                bg: forge_core::color::Color::BLACK,
                flags: 0,
            },
            Cell {
                c: '\0',
                fg: forge_core::color::Color::WHITE,
                bg: forge_core::color::Color::BLACK,
                flags: 0,
            },
            Cell {
                c: ' ',
                fg: forge_core::color::Color::WHITE,
                bg: forge_core::color::Color::BLACK,
                flags: 0,
            },
        ]];

        let text = snapshot.text_in_range(SelectionRange {
            start_row: 0,
            start_col: 0,
            end_row: 0,
            end_col: 2,
        });

        assert_eq!(text, "a");
    }
}
