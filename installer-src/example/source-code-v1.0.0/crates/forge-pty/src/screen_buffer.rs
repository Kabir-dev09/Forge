use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use forge_core::cell::{Cell, CellWidth};
use forge_core::color::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorPos {
    pub row: usize,    // 0-indexed
    pub col: usize,    // 0-indexed
}

use forge_core::cell::SelectionRange;

#[derive(Clone)]
pub struct Row {
    pub cells: Box<[Cell]>,
    pub wrapped: bool,
}

pub struct ScreenBuffer {
    grid: Vec<Row>,
    cols: usize,
    rows: usize,
    pub cursor: CursorPos,
    pub selection: Option<SelectionRange>,
    pub application_cursor_keys: bool,
    pub cursor_style_override: Option<forge_core::config_registry::CursorStyle>,
    pub cursor_blink_override: Option<bool>,
    pub dirty_rows: Vec<bool>,
    scrollback: Vec<Row>,
    max_scrollback: usize,
    pub scroll_offset: usize,
    pub current_fg: Color,
    pub current_bg: Color,
    pub default_fg: Color,
    pub default_bg: Color,
    pub attr_bold: bool,
    pub attr_italic: bool,
    pub attr_underline: bool,
    pub attr_strikethrough: bool,
    pub palette: [Color; 16],
    pub saved_cursor: Option<CursorPos>,
    pub use_alt_buffer: bool,
    saved_primary_grid: Option<Vec<Row>>,
    saved_primary_cursor: Option<CursorPos>,
    saved_primary_attrs: Option<(Color, Color, bool, bool, bool, bool)>,
    pub margin_top: usize,
    pub margin_bottom: usize,
    pub mouse_tracking_enabled: bool,
    pub mouse_sgr_mode: bool,
    pub bracketed_paste: bool,
}

impl ScreenBuffer {
    

    pub fn default_cell(&self) -> Cell {
        let mut cell = Cell {
            c: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            flags: 0,
        };
        if self.attr_bold { cell.set_bold(true); }
        if self.attr_italic { cell.set_italic(true); }
        if self.attr_underline { cell.set_underline(true); }
        if self.attr_strikethrough { cell.set_strikethrough(true); }
        cell
    }

    pub fn new(cols: usize, rows: usize, max_scrollback: usize, default_fg: Color, default_bg: Color) -> Self {
        let default_cell = Cell {
            c: ' ',
            fg: default_fg,
            bg: default_bg,
            flags: 0,
        };
        let grid = vec![Row { cells: vec![default_cell; cols].into_boxed_slice(), wrapped: false }; rows];
        let dirty_rows = vec![true; rows];
        let palette = forge_core::color::ANSI_16;
        ScreenBuffer {
            grid,
            cols,
            rows,
            cursor: CursorPos { row: 0, col: 0 },
            selection: None,
            application_cursor_keys: false,
            cursor_style_override: None,
            cursor_blink_override: None,
            dirty_rows,
            scrollback: Vec::with_capacity(max_scrollback),
            max_scrollback,
            scroll_offset: 0,
            current_fg: default_fg,
            current_bg: default_bg,
            default_fg,
            default_bg,
            attr_bold: false,
            attr_italic: false,
            attr_underline: false,
            attr_strikethrough: false,
            palette,
            saved_cursor: None,
            use_alt_buffer: false,
            saved_primary_grid: None,
            saved_primary_cursor: None,
            saved_primary_attrs: None,
            margin_top: 0,
            margin_bottom: rows.saturating_sub(1),
            mouse_tracking_enabled: false,
            mouse_sgr_mode: false,
            bracketed_paste: false,
        }
    }

    pub fn write_grapheme(&mut self, grapheme: &str) {
        let display_width = UnicodeWidthStr::width(grapheme);
        if grapheme.chars().any(|c| c.is_control()) {
            tracing::warn!("Control character passed to write_grapheme: {:?}", grapheme);
            return;
        }
        if display_width == 0 {
            // Combining character
            tracing::trace!("Skipping combining character for now: {:?}", grapheme);
            return;
        }

        // Auto-wrap if next character won't fit
        if self.cursor.col + display_width > self.cols {
            self.grid[self.cursor.row].wrapped = true;
            self.carriage_return();
            self.line_feed();
        }

        let width_type = if display_width >= 2 { CellWidth::Wide } else { CellWidth::Narrow };

        let r = self.cursor.row;
        let c = self.cursor.col;

        let mut cell = Cell {
            c: grapheme.chars().next().unwrap_or(' '),
            fg: self.current_fg,
            bg: self.current_bg,
            flags: 0,
        };
        if self.attr_bold { cell.set_bold(true); }
        if self.attr_italic { cell.set_italic(true); }
        if self.attr_underline { cell.set_underline(true); }
        if self.attr_strikethrough { cell.set_strikethrough(true); }
        cell.set_width(width_type);
        self.grid[r].cells[c] = cell;
        self.dirty_rows[r] = true;

        if display_width >= 2 {
            // Ensure we don't go out of bounds if there's a wide char at the exact edge
            if c + 1 < self.cols {
                self.grid[r].cells[c + 1] = Cell::wide_placeholder();
            }
        }

        self.cursor.col += display_width;
    }

    pub fn write_str(&mut self, s: &str) {
        for grapheme in s.graphemes(true) {
            self.write_grapheme(grapheme);
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    pub fn line_feed(&mut self) {
        if self.cursor.row == self.margin_bottom {
            self.scroll_up_in_region(1);
        } else if self.cursor.row + 1 < self.rows {
            self.cursor.row += 1;
        }
    }

    pub fn scroll_up_in_region(&mut self, n: usize) {
        let top = self.margin_top;
        let bottom = self.margin_bottom;
        if top >= bottom || bottom >= self.rows { return; }
        
        for _ in 0..n {
            let row = self.grid.remove(top);
            if top == 0 && bottom == self.rows - 1 && self.max_scrollback > 0 && !self.use_alt_buffer {
                if self.scrollback.len() >= self.max_scrollback {
                    self.scrollback.remove(0);
                }
                self.scrollback.push(row);
            }
            self.grid.insert(bottom, Row { cells: vec![self.default_cell(); self.cols].into_boxed_slice(), wrapped: false });
        }
        for r in top..=bottom {
            self.dirty_rows[r] = true;
        }
    }

    pub fn scroll_down_in_region(&mut self, n: usize) {
        let top = self.margin_top;
        let bottom = self.margin_bottom;
        if top >= bottom || bottom >= self.rows { return; }

        for _ in 0..n {
            self.grid.remove(bottom);
            self.grid.insert(top, Row { cells: vec![self.default_cell(); self.cols].into_boxed_slice(), wrapped: false });
        }
        for r in top..=bottom {
            self.dirty_rows[r] = true;
        }
    }

    pub fn insert_lines(&mut self, n: usize) {
        let top = self.cursor.row.max(self.margin_top);
        let bottom = self.margin_bottom;
        if top > bottom || bottom >= self.rows { return; }
        let count = n.min(bottom - top + 1);
        for _ in 0..count {
            self.grid.remove(bottom);
            self.grid.insert(top, Row { cells: vec![self.default_cell(); self.cols].into_boxed_slice(), wrapped: false });
        }
        for r in top..=bottom {
            self.dirty_rows[r] = true;
        }
    }

    pub fn delete_lines(&mut self, n: usize) {
        let top = self.cursor.row.max(self.margin_top);
        let bottom = self.margin_bottom;
        if top > bottom || bottom >= self.rows { return; }
        let count = n.min(bottom - top + 1);
        for _ in 0..count {
            self.grid.remove(top);
            self.grid.insert(bottom, Row { cells: vec![self.default_cell(); self.cols].into_boxed_slice(), wrapped: false });
        }
        for r in top..=bottom {
            self.dirty_rows[r] = true;
        }
    }

    pub fn move_cursor_to(&mut self, row: usize, col: usize) {
        self.dirty_rows[self.cursor.row] = true;
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.dirty_rows[self.cursor.row] = true;
    }

    pub fn move_cursor_relative(&mut self, dr: i32, dc: i32) {
        self.dirty_rows[self.cursor.row] = true;
        let new_row = (self.cursor.row as i32 + dr).clamp(0, self.rows.saturating_sub(1) as i32) as usize;
        let new_col = (self.cursor.col as i32 + dc).clamp(0, self.cols.saturating_sub(1) as i32) as usize;
        self.cursor = CursorPos { row: new_row, col: new_col };
        self.dirty_rows[self.cursor.row] = true;
    }

    pub fn insert_chars(&mut self, n: usize) {
        if n == 0 { return; }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols { return; }

        let shift = n.min(self.cols - col);
        let count = self.cols - col - shift;

        // Shift existing characters to the right
        for c in (0..count).rev() {
            self.grid[row].cells[col + shift + c] = self.grid[row].cells[col + c].clone();
        }

        // Fill the new gap with default cells
        for c in col..col + shift {
            self.grid[row].cells[c] = self.default_cell();
        }

        self.dirty_rows[row] = true;
    }

    pub fn delete_chars(&mut self, n: usize) {
        if n == 0 { return; }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols { return; }

        let shift = n.min(self.cols - col);
        let count = self.cols - col - shift;

        // Shift existing characters to the left
        for c in 0..count {
            self.grid[row].cells[col + c] = self.grid[row].cells[col + shift + c].clone();
        }

        // Fill the end with default cells
        for c in (self.cols - shift)..self.cols {
            self.grid[row].cells[c] = self.default_cell();
        }

        self.dirty_rows[row] = true;
    }

    pub fn erase_chars(&mut self, n: usize) {
        if n == 0 { return; }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols { return; }

        let count = n.min(self.cols - col);
        for c in 0..count {
            self.grid[row].cells[col + c] = self.default_cell();
        }
        self.dirty_rows[row] = true;
    }

    pub fn erase_to_end_of_line(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        for c in col..self.cols {
            self.grid[row].cells[c] = self.default_cell();
        }
        self.dirty_rows[row] = true;
    }

    pub fn erase_to_start_of_line(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        for c in 0..=col.min(self.cols.saturating_sub(1)) {
            self.grid[row].cells[c] = self.default_cell();
        }
        self.dirty_rows[row] = true;
    }

    pub fn erase_line(&mut self) {
        let row = self.cursor.row;
        for c in 0..self.cols {
            self.grid[row].cells[c] = self.default_cell();
        }
        self.dirty_rows[row] = true;
    }

    pub fn erase_to_end_of_screen(&mut self) {
        self.erase_to_end_of_line();
        let start = self.cursor.row + 1;
        for r in start..self.rows {
            for c in 0..self.cols {
                self.grid[r].cells[c] = self.default_cell();
            }
            self.dirty_rows[r] = true;
        }
    }

    pub fn erase_screen(&mut self) {
        // Push non-empty lines to scrollback
        let mut last_content_row = 0;
        for r in (0..self.rows).rev() {
            if self.grid[r].cells.iter().any(|c| !c.is_empty()) {
                last_content_row = r;
                break;
            }
        }
        
        if self.max_scrollback > 0 && !self.use_alt_buffer {
            for r in 0..=last_content_row {
                if self.scrollback.len() >= self.max_scrollback {
                    self.scrollback.remove(0);
                }
                self.scrollback.push(self.grid[r].clone());
            }
        }

        for r in 0..self.rows {
            for c in 0..self.cols {
                self.grid[r].cells[c] = self.default_cell();
            }
            self.dirty_rows[r] = true;
        }
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.scroll_offset = 0;
        self.mark_all_dirty();
    }

    pub fn resize_reflow(&mut self, new_cols: usize, new_rows: usize) {
        if new_cols == 0 || new_rows == 0 || (self.cols == new_cols && self.rows == new_rows) {
            return;
        }

        self.selection = None;

        let absolute_cursor_row = self.scrollback.len() + self.cursor.row;
        let cursor_col = self.cursor.col;

        struct LogicalLine {
            cells: Vec<Cell>,
            cursor_offset: Option<usize>,
        }

        let mut logical_lines = Vec::new();
        let mut current_line = LogicalLine { cells: Vec::new(), cursor_offset: None };

        let mut all_rows = std::mem::take(&mut self.scrollback);
        let mut old_grid = std::mem::take(&mut self.grid);
        all_rows.append(&mut old_grid);

        for (r_idx, row) in all_rows.into_iter().enumerate() {
            let mut keep_len = row.cells.len();
            while keep_len > 0 && row.cells[keep_len - 1].is_empty() {
                if r_idx == absolute_cursor_row && keep_len > cursor_col {
                    break;
                }
                keep_len -= 1;
            }
            if r_idx == absolute_cursor_row && keep_len <= cursor_col {
                keep_len = cursor_col + 1;
            }
            keep_len = keep_len.min(row.cells.len());

            if r_idx == absolute_cursor_row {
                current_line.cursor_offset = Some(current_line.cells.len() + cursor_col.min(keep_len));
            }

            current_line.cells.extend_from_slice(&row.cells[..keep_len]);

            if !row.wrapped {
                logical_lines.push(current_line);
                current_line = LogicalLine { cells: Vec::new(), cursor_offset: None };
            }
        }
        if !current_line.cells.is_empty() || current_line.cursor_offset.is_some() {
            logical_lines.push(current_line);
        }

        let mut reflowed_rows = Vec::new();
        let mut new_cursor = CursorPos { row: 0, col: 0 };

        for line in logical_lines {
            let cells = line.cells;
            let mut i = 0;
            if cells.is_empty() {
                if let Some(c_off) = line.cursor_offset {
                    new_cursor = CursorPos { row: reflowed_rows.len(), col: c_off };
                }
                reflowed_rows.push(Row { cells: vec![self.default_cell(); new_cols].into_boxed_slice(), wrapped: false });
                continue;
            }

            while i < cells.len() {
                let chunk_len = (cells.len() - i).min(new_cols);
                let mut new_row = Row {
                    cells: vec![self.default_cell(); new_cols].into_boxed_slice(),
                    wrapped: i + chunk_len < cells.len(),
                };
                new_row.cells[..chunk_len].clone_from_slice(&cells[i..i + chunk_len]);

                if let Some(c_off) = line.cursor_offset {
                    if (c_off >= i && c_off < i + new_cols) || (c_off == i + new_cols && i + new_cols >= cells.len()) {
                        new_cursor = CursorPos { row: reflowed_rows.len(), col: c_off - i };
                    }
                }

                reflowed_rows.push(new_row);
                i += chunk_len;
            }
        }

        let total_rows = reflowed_rows.len();
        let grid_start = total_rows.saturating_sub(new_rows);
        
        let mut new_grid_rows = Vec::new();
        new_grid_rows.extend_from_slice(&reflowed_rows[grid_start..total_rows]);
        while new_grid_rows.len() < new_rows {
            new_grid_rows.push(Row { cells: vec![self.default_cell(); new_cols].into_boxed_slice(), wrapped: false });
        }

        let mut new_scrollback = Vec::new();
        let scrollback_start = grid_start.saturating_sub(self.max_scrollback);
        new_scrollback.extend_from_slice(&reflowed_rows[scrollback_start..grid_start]);

        self.grid = new_grid_rows;
        self.scrollback = new_scrollback;
        self.cols = new_cols;
        self.rows = new_rows;

        if new_cursor.row >= grid_start {
            self.cursor.row = new_cursor.row - grid_start;
        } else {
            self.cursor.row = 0;
        }
        self.cursor.col = new_cursor.col.min(new_cols);
        self.dirty_rows.resize(new_rows, true);
        self.dirty_rows.fill(true);
        self.scroll_offset = 0;
        self.margin_top = 0;
        self.margin_bottom = new_rows.saturating_sub(1);
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty_rows.fill(true);
    }

    /// Scrolls the viewport up by `lines` (viewing older history).
    pub fn view_scroll_up(&mut self, lines: usize) {
        let max_offset = self.scrollback.len();
        self.scroll_offset = (self.scroll_offset + lines).min(max_offset);
        self.mark_all_dirty();
    }

    /// Scrolls the viewport down by `lines` (viewing newer history).
    pub fn view_scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        self.mark_all_dirty();
    }

    /// Resets the viewport to the bottom (live output).
    pub fn view_scroll_to_bottom(&mut self) {
        if self.scroll_offset != 0 {
            self.scroll_offset = 0;
            self.mark_all_dirty();
        }
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Retrieves a visible row based on the current scroll offset.
    /// `index` is 0-indexed from the top of the viewport.
    pub fn visible_row(&self, index: usize) -> &[Cell] {
        if self.scroll_offset == 0 {
            &self.grid[index].cells
        } else {
            let scrollback_lines = self.scrollback.len();
            if index < self.scroll_offset {
                // It's in the scrollback buffer
                let sb_idx = scrollback_lines - self.scroll_offset + index;
                &self.scrollback[sb_idx].cells
            } else {
                // It's in the live grid
                let grid_idx = index - self.scroll_offset;
                &self.grid[grid_idx].cells
            }
        }
    }

    pub fn enable_alt_buffer(&mut self) {
        if !self.use_alt_buffer {
            self.use_alt_buffer = true;
            // Save primary grid and attributes
            self.saved_primary_grid = Some(self.grid.clone());
            self.saved_primary_cursor = Some(self.cursor);
            self.saved_primary_attrs = Some((
                self.current_fg, self.current_bg, self.attr_bold, 
                self.attr_italic, self.attr_underline, self.attr_strikethrough
            ));
            
            // Clear current grid (which becomes alt grid)
            let default_cell = self.default_cell();
            for r in &mut self.grid {
                for c in &mut r.cells {
                    *c = default_cell.clone();
                }
            }
            self.cursor = CursorPos { row: 0, col: 0 };
            self.scroll_offset = 0;
            self.mark_all_dirty();
        }
    }

    pub fn disable_alt_buffer(&mut self) {
        if self.use_alt_buffer {
            self.use_alt_buffer = false;
            if let Some(mut grid) = self.saved_primary_grid.take() {
                grid.resize(self.rows, Row { cells: vec![self.default_cell(); self.cols].into_boxed_slice(), wrapped: false });
                for row in &mut grid {
                    let mut vec = std::mem::replace(&mut row.cells, Box::new([])).into_vec();
                    vec.resize(self.cols, self.default_cell());
                    row.cells = vec.into_boxed_slice();
                }
                self.grid = grid;
            }
            if let Some(cursor) = self.saved_primary_cursor.take() {
                self.cursor.row = cursor.row.min(self.rows.saturating_sub(1));
                self.cursor.col = cursor.col.min(self.cols.saturating_sub(1));
            }
            if let Some((fg, bg, bold, italic, underline, strike)) = self.saved_primary_attrs.take() {
                self.current_fg = fg;
                self.current_bg = bg;
                self.attr_bold = bold;
                self.attr_italic = italic;
                self.attr_underline = underline;
                self.attr_strikethrough = strike;
            }
            self.scroll_offset = 0;
            self.mark_all_dirty();
        }
    }

    pub fn cols(&self) -> usize { self.cols }

    pub fn mark_row_clean(&mut self, row: usize) {
        if row < self.rows {
            self.dirty_rows[row] = false;
        }
    }

    pub fn mark_all_clean(&mut self) {
        self.dirty_rows.iter_mut().for_each(|d| *d = false);
    }

    pub fn has_dirty_rows(&self) -> bool {
        self.dirty_rows.iter().any(|&d| d)
    }

    pub fn get_text_in_range(&self, range: SelectionRange) -> String {
        let (start_row, start_col, end_row, end_col) = if range.start_row < range.end_row || (range.start_row == range.end_row && range.start_col <= range.end_col) {
            (range.start_row, range.start_col, range.end_row, range.end_col)
        } else {
            (range.end_row, range.end_col, range.start_row, range.start_col)
        };

        let mut result = String::new();

        for row in start_row..=end_row {
            if row >= self.grid.len() { break; }
            let grid_row = &self.grid[row];
            
            let start = if row == start_row { start_col } else { 0 };
            let end = if row == end_row { end_col } else { grid_row.cells.len().saturating_sub(1) };
            
            let mut line = String::new();
            for col in start..=end {
                if col < grid_row.cells.len() {
                    let cell = &grid_row.cells[col];
                    if cell.c != '\0' {
                        line.push(cell.c);
                    }
                }
            }

            // Strip trailing empty cells
            let trimmed = line.trim_end().to_string();
            result.push_str(&trimmed);

            if row != end_row {
                result.push('\n');
            }
        }

        result
    }

    pub fn update_theme(&mut self, new_fg: Color, new_bg: Color, palette: [Color; 16]) {
        let old_fg = self.default_fg;
        let old_bg = self.default_bg;
        
        self.default_fg = new_fg;
        self.default_bg = new_bg;
        self.palette = palette;
        
        if self.current_fg == old_fg { self.current_fg = new_fg; }
        if self.current_bg == old_bg { self.current_bg = new_bg; }
        
        // Update primary grid
        for row in &mut self.grid {
            for cell in &mut row.cells {
                if cell.fg == old_fg { cell.fg = new_fg; }
                if cell.bg == old_bg { cell.bg = new_bg; }
            }
        }
        
        // Update alt grid if saved
        if let Some(saved_grid) = &mut self.saved_primary_grid {
            for row in saved_grid {
                for cell in &mut row.cells {
                    if cell.fg == old_fg { cell.fg = new_fg; }
                    if cell.bg == old_bg { cell.bg = new_bg; }
                }
            }
        }
        
        // Update scrollback
        for row in &mut self.scrollback {
            for cell in &mut row.cells {
                if cell.fg == old_fg { cell.fg = new_fg; }
                if cell.bg == old_bg { cell.bg = new_bg; }
            }
        }
        
        if let Some((fg, bg, _, _, _, _)) = &mut self.saved_primary_attrs {
            if *fg == old_fg { *fg = new_fg; }
            if *bg == old_bg { *bg = new_bg; }
        }
        
        self.mark_all_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_ascii() {
        let mut buf = ScreenBuffer::new(10, 10, 100, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.write_str("Hello");
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 5);
        assert_eq!(buf.grid[0].cells[0].c, 'H');
        assert_eq!(buf.grid[0].cells[4].c, 'o');
    }

    #[test]
    fn test_write_wide_char() {
        let mut buf = ScreenBuffer::new(10, 10, 100, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.write_str("中");
        assert_eq!(buf.cursor.col, 2);
        assert_eq!(buf.grid[0].cells[0].width(), CellWidth::Wide);
        assert_eq!(buf.grid[0].cells[1].c, '\0');
    }

    #[test]
    fn test_line_feed_and_scroll() {
        let mut buf = ScreenBuffer::new(10, 2, 5, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.write_str("Line 1");
        buf.carriage_return();
        buf.line_feed();
        buf.write_str("Line 2");
        buf.carriage_return();
        buf.line_feed();
        buf.write_str("Line 3");
        
        assert_eq!(buf.scrollback.len(), 1);
        assert_eq!(buf.scrollback[0].cells[0].c, 'L');
        assert_eq!(buf.scrollback[0].cells[5].c, '1');
        
        assert_eq!(buf.grid[0].cells[5].c, '2');
        assert_eq!(buf.grid[1].cells[5].c, '3');
    }

    #[test]
    fn test_erase() {
        let mut buf = ScreenBuffer::new(10, 10, 100, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.write_str("12345");
        buf.cursor.col = 2;
        buf.erase_to_end_of_line();
        assert_eq!(buf.grid[0].cells[1].c, '2');
        assert!(buf.grid[0].cells[2].is_empty());
        assert!(buf.grid[0].cells[3].is_empty());
    }

    #[test]
    fn test_resize() {
        let mut buf = ScreenBuffer::new(5, 5, 100, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.write_str("12345");
        buf.resize_reflow(10, 10);
        assert_eq!(buf.cols, 10);
        assert_eq!(buf.rows, 10);
        assert_eq!(buf.grid[0].cells[0].c, '1');
        assert!(buf.grid[0].cells[6].is_empty());
    }

    #[test]
    fn test_resize_shrink_grow() {
        let mut buf = ScreenBuffer::new(80, 24, 100, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 });
        buf.move_cursor_to(23, 79);
        buf.write_str("X");
        
        // Shrink
        buf.resize_reflow(40, 12);
        assert_eq!(buf.cols, 40);
        assert_eq!(buf.rows, 12);
        assert_eq!(buf.cursor.row, 11); // Clamped
        assert_eq!(buf.cursor.col, 40); // Clamped
        
        // Grow back
        buf.resize_reflow(80, 24);
        assert_eq!(buf.cols, 80);
        assert_eq!(buf.rows, 24);
        // Ensure no panics on writing to new bounds
        buf.move_cursor_to(23, 79);
        buf.write_str("Y");
    }

    #[test]
    fn test_scrollback_overflow() {
        let mut buf = ScreenBuffer::new(10, 5, 5, forge_core::color::Color { r: 192, g: 202, b: 245, a: 255 }, forge_core::color::Color { r: 30, g: 30, b: 46, a: 255 }); // max 5 scrollback
        for i in 0..100 {
            buf.write_str(&format!("Line {}", i));
            buf.carriage_return();
            buf.line_feed();
        }
        assert_eq!(buf.scrollback.len(), 5);
        // The last lines pushed out should be 90 to 94 (since 95-99 are visible)
        assert_eq!(buf.scrollback[4].cells[5].c, '9');
        assert_eq!(buf.scrollback[4].cells[6].c, '5');
    }
}

#[cfg(test)]
mod reflow_tests {
    use super::*;
    use forge_core::color::Color;

    #[test]
    fn test_cursor_linear_index_preserved() {
        let mut buf = ScreenBuffer::new(10, 5, 100, Color::WHITE, Color::BLACK);
        // Write exactly 15 characters. This wraps exactly once, leaving cursor at (1, 5).
        buf.write_str("1234567890ABCDE");
        assert_eq!(buf.cursor.row, 1);
        assert_eq!(buf.cursor.col, 5);

        // Reflow to width 20. The 15 characters fit on one line.
        // Cursor should end up at (0, 15).
        buf.resize_reflow(20, 5);
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 15);

        // Reflow to width 5.
        // The 15 characters take 3 lines (rows 0, 1, 2).
        // The 4 empty rows from before become rows 3, 4, 5, 6.
        // Total rows = 7. Grid start = 2.
        // Cursor is on absolute row 2, which is grid row 0.
        buf.resize_reflow(5, 5);
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 0);
    }

    #[test]
    fn test_insert_chars() {
        let mut buf = ScreenBuffer::new(10, 5, 100, Color::WHITE, Color::BLACK);
        buf.write_str("12345");
        buf.move_cursor_to(0, 2);
        buf.insert_chars(2);
        let row = buf.visible_row(0);
        let chars: String = row.iter().map(|c| c.c).collect();
        assert_eq!(chars, "12  345   ");
    }

    #[test]
    fn test_delete_chars() {
        let mut buf = ScreenBuffer::new(10, 5, 100, Color::WHITE, Color::BLACK);
        buf.write_str("1234567");
        buf.move_cursor_to(0, 2);
        buf.delete_chars(3);
        let row = buf.visible_row(0);
        let chars: String = row.iter().map(|c| c.c).collect();
        assert_eq!(chars, "1267      ");
    }
}
