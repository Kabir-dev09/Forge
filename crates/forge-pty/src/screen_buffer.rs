use forge_core::cell::{Cell, CellWidth};
use forge_core::color::Color;
use std::collections::VecDeque;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorPos {
    pub row: usize, // 0-indexed
    pub col: usize, // 0-indexed
}

use forge_core::cell::SelectionRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEvent {
    pub direction: ScrollDirection,
    pub top: usize,
    pub bottom: usize,
    pub lines: usize,
    pub full_viewport: bool,
}

#[derive(Clone)]
pub struct Row {
    pub cells: Box<[Cell]>,
    pub len: usize,
    pub wrapped: bool,
    pub reflowable: bool,
}

#[derive(Clone)]
struct Scrollback {
    rows: Vec<Row>,
    start: usize,
    len: usize,
    max_len: usize,
}

impl Scrollback {
    fn new(max_len: usize) -> Self {
        Self {
            rows: Vec::with_capacity(max_len),
            start: 0,
            len: 0,
            max_len,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.rows.clear();
        self.start = 0;
        self.len = 0;
    }

    fn push(&mut self, row: Row) -> Option<Row> {
        if self.max_len == 0 {
            return Some(row);
        }

        if self.len < self.max_len {
            let index = (self.start + self.len) % self.max_len;
            let rows_len = self.rows.len();
            if index == rows_len {
                self.rows.push(row);
                self.len += 1;
                return None;
            } else {
                let cell_ref = self.rows.get_mut(index)
                    .unwrap_or_else(|| panic!("Scrollback logic out of sync: index {} >= rows.len() {}, max_len {}", index, rows_len, self.max_len));
                let old = std::mem::replace(cell_ref, row);
                self.len += 1;
                return Some(old);
            }
        }

        let rows_len = self.rows.len();
        let cell_ref = self.rows.get_mut(self.start)
            .unwrap_or_else(|| panic!("Scrollback logic out of sync: start {} >= rows.len() {}, max_len {}", self.start, rows_len, self.max_len));
        let old = std::mem::replace(cell_ref, row);
        self.start = (self.start + 1) % self.max_len;
        Some(old)
    }

    fn get(&self, index: usize) -> Option<&Row> {
        if index >= self.len {
            return None;
        }
        let physical = if self.len < self.max_len {
            index
        } else {
            (self.start + index) % self.max_len
        };
        self.rows.get(physical)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut Row> {
        if index >= self.len {
            return None;
        }
        let physical = if self.len < self.max_len {
            index
        } else {
            (self.start + index) % self.max_len
        };
        self.rows.get_mut(physical)
    }

    fn drain_to_vec(&mut self) -> Vec<Row> {
        let mut rows = std::mem::take(&mut self.rows);
        let len = self.len;
        let start = self.start;

        self.rows = Vec::with_capacity(self.max_len);
        self.start = 0;
        self.len = 0;

        if len == 0 {
            return Vec::new();
        }
        if start != 0 {
            rows.rotate_left(start);
        }
        rows.truncate(len);
        rows
    }

    fn replace_from_rows(&mut self, rows: Vec<Row>) {
        self.clear();
        let skip = rows.len().saturating_sub(self.max_len);
        for row in rows.into_iter().skip(skip) {
            self.push(row);
        }
    }
}

impl std::ops::Index<usize> for Scrollback {
    type Output = Row;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("scrollback index out of bounds")
    }
}

#[derive(Clone)]
pub struct ScreenBuffer {
    grid: VecDeque<Row>,
    cols: usize,
    rows: usize,
    pub cursor: CursorPos,
    pub selection: Option<SelectionRange>,
    pub application_cursor_keys: bool,
    pub cursor_visible: bool,
    pub cursor_style_override: Option<forge_core::config_registry::CursorStyle>,
    pub cursor_blink_override: Option<bool>,
    pub dirty_generations: Vec<u64>,
    pub current_title: String,
    pub current_dir: Option<String>,
    pub current_command: Option<String>,
    pub is_command_running: bool,
    pub last_exit_code: Option<i32>,
    scrollback: Scrollback,
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
    pub scroll_id: u64,
    pub snapshot_id: u64,
    pub use_alt_buffer: bool,
    saved_primary_grid: Option<VecDeque<Row>>,
    saved_primary_cursor: Option<CursorPos>,
    saved_primary_attrs: Option<(Color, Color, bool, bool, bool, bool)>,
    pub margin_top: usize,
    pub margin_bottom: usize,
    pub mouse_tracking_enabled: bool,
    pub mouse_sgr_mode: bool,
    pub bracketed_paste: bool,
    pub pending_scroll: Option<ScrollEvent>,
    pub clean_generations: Vec<u64>,
}

impl ScreenBuffer {
    fn current_cell(&self, c: char, width: CellWidth) -> Cell {
        let mut cell = Cell {
            c,
            fg: self.current_fg,
            bg: self.current_bg,
            flags: 0,
        };
        if self.attr_bold {
            cell.set_bold(true);
        }
        if self.attr_italic {
            cell.set_italic(true);
        }
        if self.attr_underline {
            cell.set_underline(true);
        }
        if self.attr_strikethrough {
            cell.set_strikethrough(true);
        }
        cell.set_width(width);
        cell
    }

    fn set_cell_if_changed(&mut self, row: usize, col: usize, cell: Cell) -> bool {
        let slot = &mut self.grid[row].cells[col];
        let mut changed = false;
        if slot.c != cell.c || slot.fg != cell.fg || slot.bg != cell.bg || slot.flags != cell.flags
        {
            *slot = cell;
            changed = true;
        }
        if changed && col + 1 > self.grid[row].len {
            self.grid[row].len = col + 1;
        }
        if changed {
            self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
            self.pending_scroll = None;
        }
        changed
    }

    fn prepare_row_for_write(&mut self, row: usize, col: usize) {
        if col == 0 {
            self.grid[row].wrapped = false;
            self.grid[row].reflowable = false;
        }
    }

    fn fill_cell_range_if_changed(
        &mut self,
        row: usize,
        cols: impl Iterator<Item = usize>,
        cell: Cell,
    ) -> bool {
        let mut changed = false;
        for col in cols {
            changed |= self.set_cell_if_changed(row, col, cell);
        }
        changed
    }

    pub fn default_cell(&self) -> Cell {
        self.current_cell(' ', CellWidth::Narrow)
    }

    pub fn new(
        cols: usize,
        rows: usize,
        max_scrollback: usize,
        default_fg: Color,
        default_bg: Color,
    ) -> Self {
        let default_cell = Cell {
            c: ' ',
            fg: default_fg,
            bg: default_bg,
            flags: 0,
        };
        let grid = VecDeque::from(vec![
            Row {
                cells: vec![default_cell; cols].into_boxed_slice(),
                len: 0,
                wrapped: false,
                reflowable: false,
            };
            rows
        ]);
        let palette = forge_core::color::ANSI_16;
        ScreenBuffer {
            grid,
            cols,
            rows,
            cursor: CursorPos { row: 0, col: 0 },
            selection: None,
            application_cursor_keys: false,
            cursor_visible: true,
            cursor_style_override: None,
            cursor_blink_override: None,
            dirty_generations: vec![1; rows],
            current_title: String::new(),
            current_dir: None,
            current_command: None,
            is_command_running: false,
            last_exit_code: None,
            scrollback: Scrollback::new(max_scrollback),
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
            pending_scroll: None,
            scroll_id: 0,
            snapshot_id: 0,
            use_alt_buffer: false,
            saved_primary_grid: None,
            saved_primary_cursor: None,
            saved_primary_attrs: None,
            margin_top: 0,
            margin_bottom: rows.saturating_sub(1),
            mouse_tracking_enabled: false,
            mouse_sgr_mode: false,
            bracketed_paste: false,
            clean_generations: vec![0; rows],
        }
    }

    fn record_scroll_event(
        &mut self,
        direction: ScrollDirection,
        top: usize,
        bottom: usize,
        lines: usize,
    ) {
        if lines == 0 {
            return;
        }

        let event = ScrollEvent {
            direction,
            top,
            bottom,
            lines,
            full_viewport: top == 0 && bottom + 1 == self.rows,
        };

        self.scroll_id = self.scroll_id.wrapping_add(1);
        self.pending_scroll = match self.pending_scroll {
            Some(previous)
                if previous.direction == event.direction
                    && previous.top == event.top
                    && previous.bottom == event.bottom =>
            {
                Some(ScrollEvent {
                    lines: previous.lines.saturating_add(event.lines),
                    ..event
                })
            }
            _ => Some(event),
        };
    }

    fn scroll_reuse_is_safe_before_scroll(
        &self,
        top: usize,
        bottom: usize,
        direction: ScrollDirection,
    ) -> bool {
        let is_clean = !(top..=bottom).any(|r| self.is_dirty(r));
        match &self.pending_scroll {
            None => is_clean,
            Some(prev) => prev.top == top && prev.bottom == bottom && prev.direction == direction,
        }
    }

    pub fn take_pending_scroll(&mut self) -> Option<ScrollEvent> {
        self.pending_scroll.take()
    }

    pub fn write_grapheme(&mut self, grapheme: &str) {
        let display_width = if grapheme.is_ascii() {
            1
        } else {
            UnicodeWidthStr::width(grapheme)
        };
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
            self.grid[self.cursor.row].reflowable = true;
            self.carriage_return();
            self.line_feed();
        }

        let width_type = if display_width >= 2 {
            CellWidth::Wide
        } else {
            CellWidth::Narrow
        };

        let r = self.cursor.row;
        let c = self.cursor.col;

        self.prepare_row_for_write(r, c);
        // TODO(RISK-10): write_grapheme truncates multi-codepoint grapheme clusters
        // The current Cell struct only stores a single `char`. We need to change `Cell`
        // to support a string or a grapheme cluster index to properly store them.
        let cell = self.current_cell(grapheme.chars().next().unwrap_or(' '), width_type);
        self.set_cell_if_changed(r, c, cell);

        if display_width >= 2 {
            // Ensure we don't go out of bounds if there's a wide char at the exact edge
            if c + 1 < self.cols {
                self.set_cell_if_changed(r, c + 1, Cell::wide_placeholder());
            }
        }

        self.cursor.col += display_width;
    }

    pub fn write_str(&mut self, s: &str) {
        for grapheme in s.graphemes(true) {
            self.write_grapheme(grapheme);
        }
    }

    /// Fast ASCII path: writes a run of printable ASCII bytes (0x20-0x7E).
    /// Avoids per-cell Cell struct construction when attributes haven't changed.
    pub fn write_ascii_run(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.iter().all(|&b| (0x20..=0x7e).contains(&b)));

        // Pre-compute the attribute flags once for the whole run.
        let mut attr_flags: u8 = 0;
        if self.attr_bold {
            attr_flags |= Cell::FLAG_BOLD;
        }
        if self.attr_italic {
            attr_flags |= Cell::FLAG_ITALIC;
        }
        if self.attr_underline {
            attr_flags |= Cell::FLAG_UNDERLINE;
        }
        if self.attr_strikethrough {
            attr_flags |= Cell::FLAG_STRIKETHROUGH;
        }
        let cur_fg = self.current_fg;
        let cur_bg = self.current_bg;

        for &byte in bytes {
            // Handle line wrap before writing.
            if self.cursor.col >= self.cols {
                self.grid[self.cursor.row].wrapped = true;
                self.grid[self.cursor.row].reflowable = true;
                self.cursor.col = 0;
                self.line_feed();
            }

            let row = self.cursor.row;
            let col = self.cursor.col;

            // Only clear reflowable/wrapped flag at column 0 (prepare_row_for_write).
            if col == 0 {
                self.grid[row].wrapped = false;
                self.grid[row].reflowable = false;
            }

            // Write directly: skip Cell construction + equality check when cell is unchanged.
            let slot = &mut self.grid[row].cells[col];
            let new_c = byte as char;
            if slot.c != new_c || slot.fg != cur_fg || slot.bg != cur_bg || slot.flags != attr_flags
            {
                *slot = Cell {
                    c: new_c,
                    fg: cur_fg,
                    bg: cur_bg,
                    flags: attr_flags,
                };
                self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
            }
            if col + 1 > self.grid[row].len {
                self.grid[row].len = col + 1;
                self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
            }

            self.cursor.col += 1;
        }
    }

    #[inline(always)]
    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    pub fn reverse_index(&mut self) {
        if self.cursor.row == self.margin_top {
            self.scroll_down_in_region(1);
        } else {
            self.move_cursor_relative(-1, 0);
        }
    }

    #[inline]
    pub fn line_feed(&mut self) {
        if self.cursor.row == self.margin_bottom {
            self.scroll_up_in_region(1);
        } else if self.cursor.row + 1 < self.rows {
            self.cursor.row += 1;
        }
    }

    /// Process `n` consecutive line feeds as efficiently as possible.
    /// This is the critical hot path for `cat large_file` throughput.
    /// Batches all scrolling into a single `scroll_up_in_region(n)` call
    /// instead of n individual calls, dramatically reducing overhead.
    #[inline]
    pub fn line_feeds_n(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        if n == 1 {
            return self.line_feed();
        }

        let rows_below = self.margin_bottom - self.cursor.row;
        if rows_below >= n {
            // All n line feeds fit by moving cursor down — no scrolling needed.
            self.cursor.row += n;
        } else {
            // Move cursor to bottom, then scroll the remainder in one shot.
            let scroll_count = n - rows_below;
            self.cursor.row = self.margin_bottom;
            self.scroll_up_in_region(scroll_count);
        }
    }
    pub fn scroll_up_in_region(&mut self, n: usize) {
        let top = self.margin_top;
        let bottom = self.margin_bottom;
        if top >= bottom || bottom >= self.rows {
            return;
        }
        let region_height = bottom - top + 1;
        // Cap n to region_height: scrolling more than the viewport discards all content anyway.
        let n = n.min(region_height);

        let can_reuse_scroll =
            self.scroll_reuse_is_safe_before_scroll(top, bottom, ScrollDirection::Up);

        if top == 0 && bottom == self.rows - 1 {
            // Full-viewport scroll: the common case during `cat large_file`.
            // Pop rows from the front, push to scrollback, push fresh rows to back.
            let default = self.default_cell();
            for _ in 0..n {
                if let Some(row) = self.grid.pop_front() {
                    let recycled_row = if self.max_scrollback > 0 && !self.use_alt_buffer {
                        self.scrollback.push(row)
                    } else {
                        Some(row)
                    };

                    if let Some(mut r) = recycled_row {
                        // Clear cells properly to respect BCE (Back Color Erase).
                        // If the background color hasn't changed, we only need to clear the written text.
                        let old_len = r.len.min(self.cols);
                        if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                            r.cells.fill(default);
                        } else {
                            r.cells[..old_len].fill(default);
                        }
                        r.len = 0;
                        r.wrapped = false;
                        r.reflowable = false;
                        self.grid.push_back(r);
                    } else {
                        self.grid.push_back(Row {
                            cells: vec![default; self.cols].into_boxed_slice(),
                            len: 0,
                            wrapped: false,
                            reflowable: false,
                        });
                    }
                } else {
                    self.grid.push_back(Row {
                        cells: vec![default; self.cols].into_boxed_slice(),
                        len: 0,
                        wrapped: false,
                        reflowable: false,
                    });
                }
            }
        } else {
            // Partial-region scroll (less common, e.g. scrolling regions).
            let default = self.default_cell();
            self.grid.make_contiguous();
            let slice = self.grid.as_mut_slices().0;
            slice[top..=bottom].rotate_left(n);

            for r in &mut slice[bottom + 1 - n..=bottom] {
                let old_len = r.len.min(self.cols);
                if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                    r.cells.fill(default);
                } else {
                    r.cells[..old_len].fill(default);
                }
                r.len = 0;
                r.wrapped = false;
                r.reflowable = false;
            }
        }

        if can_reuse_scroll {
            self.record_scroll_event(ScrollDirection::Up, top, bottom, n);
            // Shift dirty rows to match the scrolled grid.
            if n < region_height {
                self.dirty_generations.copy_within(top + n..=bottom, top);
                for g in &mut self.dirty_generations[bottom - n + 1..=bottom] {
                    *g = g.wrapping_add(1);
                }
            } else {
                for g in &mut self.dirty_generations[top..=bottom] {
                    *g = g.wrapping_add(1);
                }
            }
        } else {
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            // Renderer won't scroll, so the entire region must be redrawn
            for g in &mut self.dirty_generations[top..=bottom] {
                *g = g.wrapping_add(1);
            }
        }
    }

    pub fn scroll_down_in_region(&mut self, n: usize) {
        let _span = tracing::trace_span!(
            "screen_buffer.scroll_down_in_region",
            rows = self.rows,
            cols = self.cols,
            n = n
        )
        .entered();
        let top = self.margin_top;
        let bottom = self.margin_bottom;
        if top >= bottom || bottom >= self.rows {
            return;
        }
        let can_reuse_scroll =
            self.scroll_reuse_is_safe_before_scroll(top, bottom, ScrollDirection::Down);

        if top == 0 && bottom == self.rows - 1 {
            for _ in 0..n {
                if let Some(mut r) = self.grid.pop_back() {
                    let default = self.default_cell();
                    let old_len = r.len.min(self.cols);
                    if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                        r.cells.fill(default);
                    } else {
                        r.cells[..old_len].fill(default);
                    }
                    r.len = 0;
                    r.wrapped = false;
                    r.reflowable = false;
                    self.grid.push_front(r);
                } else {
                    self.grid.push_front(Row {
                        cells: vec![self.default_cell(); self.cols].into_boxed_slice(),
                        len: 0,
                        wrapped: false,
                        reflowable: false,
                    });
                }
            }
        } else {
            let default = self.default_cell();
            self.grid.make_contiguous();
            let slice = self.grid.as_mut_slices().0;
            slice[top..=bottom].rotate_right(n);

            for r in &mut slice[top..top + n] {
                let old_len = r.len.min(self.cols);
                if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                    r.cells.fill(default);
                } else {
                    r.cells[..old_len].fill(default);
                }
                r.len = 0;
                r.wrapped = false;
                r.reflowable = false;
            }
        }
        if can_reuse_scroll {
            self.record_scroll_event(ScrollDirection::Down, top, bottom, n);
            // Shift dirty rows to match the scrolled grid.
            let region_height = bottom - top + 1;
            if n < region_height {
                self.dirty_generations
                    .copy_within(top..=bottom - n, top + n);
                for g in &mut self.dirty_generations[top..top + n] {
                    *g = g.wrapping_add(1);
                }
            } else {
                for g in &mut self.dirty_generations[top..=bottom] {
                    *g = g.wrapping_add(1);
                }
            }
        } else {
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            // Renderer won't scroll, so the entire region must be redrawn
            for g in &mut self.dirty_generations[top..=bottom] {
                *g = g.wrapping_add(1);
            }
        }
    }

    pub fn insert_lines(&mut self, n: usize) {
        let top = self.cursor.row.max(self.margin_top);
        let bottom = self.margin_bottom;
        if top > bottom || bottom >= self.rows {
            return;
        }
        self.pending_scroll = None;
        self.scroll_id = self.scroll_id.wrapping_add(1);
        let count = n.min(bottom - top + 1);

        let default = self.default_cell();
        self.grid.make_contiguous();
        let slice = self.grid.as_mut_slices().0;
        slice[top..=bottom].rotate_right(count);

        for r in &mut slice[top..top + count] {
            let old_len = r.len.min(self.cols);
            if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                r.cells.fill(default);
            } else {
                r.cells[..old_len].fill(default);
            }
            r.len = 0;
            r.wrapped = false;
            r.reflowable = false;
        }

        for r in top..=bottom {
            self.dirty_generations[r] = self.dirty_generations[r].wrapping_add(1);
        }
    }

    pub fn delete_lines(&mut self, n: usize) {
        let top = self.cursor.row.max(self.margin_top);
        let bottom = self.margin_bottom;
        if top > bottom || bottom >= self.rows {
            return;
        }
        self.pending_scroll = None;
        self.scroll_id = self.scroll_id.wrapping_add(1);
        let count = n.min(bottom - top + 1);

        let default = self.default_cell();
        self.grid.make_contiguous();
        let slice = self.grid.as_mut_slices().0;
        slice[top..=bottom].rotate_left(count);

        for r in &mut slice[bottom + 1 - count..=bottom] {
            let old_len = r.len.min(self.cols);
            if old_len < self.cols && r.cells[self.cols.saturating_sub(1)] != default {
                r.cells.fill(default);
            } else {
                r.cells[..old_len].fill(default);
            }
            r.len = 0;
            r.wrapped = false;
            r.reflowable = false;
        }

        for r in top..=bottom {
            self.dirty_generations[r] = self.dirty_generations[r].wrapping_add(1);
        }
    }

    pub fn move_cursor_to(&mut self, row: usize, col: usize) {
        self.dirty_generations[self.cursor.row] =
            self.dirty_generations[self.cursor.row].wrapping_add(1);
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.dirty_generations[self.cursor.row] =
            self.dirty_generations[self.cursor.row].wrapping_add(1);
    }

    pub fn move_cursor_relative(&mut self, dr: i32, dc: i32) {
        self.dirty_generations[self.cursor.row] =
            self.dirty_generations[self.cursor.row].wrapping_add(1);
        let new_row =
            (self.cursor.row as i32 + dr).clamp(0, self.rows.saturating_sub(1) as i32) as usize;
        let new_col =
            (self.cursor.col as i32 + dc).clamp(0, self.cols.saturating_sub(1) as i32) as usize;
        self.cursor = CursorPos {
            row: new_row,
            col: new_col,
        };
        self.dirty_generations[self.cursor.row] =
            self.dirty_generations[self.cursor.row].wrapping_add(1);
    }

    pub fn insert_chars(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return;
        }

        let shift = n.min(self.cols - col);
        let count = self.cols - col - shift;

        let old_cells = self.grid[row].cells.clone();
        self.prepare_row_for_write(row, col);

        // Shift existing characters to the right
        for c in (0..count).rev() {
            self.grid[row].cells[col + shift + c] = self.grid[row].cells[col + c];
        }

        // Fill the new gap with default cells
        for c in col..col + shift {
            self.grid[row].cells[c] = self.default_cell();
        }

        if self.grid[row].cells != old_cells {
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
        }
    }

    pub fn delete_chars(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return;
        }

        let shift = n.min(self.cols - col);
        let count = self.cols - col - shift;

        let old_cells = self.grid[row].cells.clone();
        self.prepare_row_for_write(row, col);

        // Shift existing characters to the left
        for c in 0..count {
            self.grid[row].cells[col + c] = self.grid[row].cells[col + shift + c];
        }

        // Fill the end with default cells
        for c in (self.cols - shift)..self.cols {
            self.grid[row].cells[c] = self.default_cell();
        }

        if self.grid[row].cells != old_cells {
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
        }
    }

    pub fn erase_chars(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return;
        }

        let count = n.min(self.cols - col);
        let default_cell = self.default_cell();
        self.fill_cell_range_if_changed(row, col..col + count, default_cell);
    }

    pub fn erase_to_end_of_line(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col == 0 {
            self.grid[row].wrapped = false;
        }
        let default_cell = self.default_cell();
        self.fill_cell_range_if_changed(row, col..self.cols, default_cell);
    }

    pub fn erase_to_start_of_line(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let default_cell = self.default_cell();
        self.fill_cell_range_if_changed(
            row,
            0..=col.min(self.cols.saturating_sub(1)),
            default_cell,
        );
    }

    pub fn erase_to_start_of_screen(&mut self) {
        let r = self.cursor.row;
        let default_cell = self.default_cell();
        for row in 0..r {
            self.grid[row].wrapped = false;
            self.fill_cell_range_if_changed(row, 0..self.cols, default_cell);
        }
        self.erase_to_start_of_line();
    }

    pub fn erase_line(&mut self) {
        let row = self.cursor.row;
        self.grid[row].wrapped = false;
        let default_cell = self.default_cell();
        self.fill_cell_range_if_changed(row, 0..self.cols, default_cell);
    }

    pub fn erase_to_end_of_screen(&mut self) {
        self.erase_to_end_of_line();
        let start = self.cursor.row + 1;
        for r in start..self.rows {
            self.grid[r].wrapped = false;
            let default_cell = self.default_cell();
            self.fill_cell_range_if_changed(r, 0..self.cols, default_cell);
        }
    }

    pub fn erase_screen(&mut self) {
        let _span = tracing::trace_span!(
            "screen_buffer.erase_screen",
            rows = self.rows,
            cols = self.cols,
            scrollback_len = self.scrollback.len()
        )
        .entered();
        // Push non-empty lines to scrollback
        let mut last_content_row = 0;
        for r in (0..self.rows).rev() {
            let len = self.grid[r].len;
            if self.grid[r].cells[0..len].iter().any(|c| !c.is_empty()) {
                last_content_row = r;
                break;
            }
        }

        if self.max_scrollback > 0 && !self.use_alt_buffer {
            for r in 0..=last_content_row {
                self.scrollback.push(self.grid[r].clone());
            }
        }

        let default_cell = self.default_cell();
        for r in 0..self.rows {
            self.grid[r].wrapped = false;
            self.fill_cell_range_if_changed(r, 0..self.cols, default_cell);
        }
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.scroll_offset = 0;
        self.mark_all_dirty();
    }

    pub fn resize_reflow(&mut self, new_cols: usize, new_rows: usize) {
        // TODO(PERF-03): resize_reflow materializes entire scrollback into memory.
        // It should ideally reflow incrementally or retain the ring buffer structure
        // during reflow to avoid allocating a massive Vec.
        let _span = tracing::trace_span!(
            "screen_buffer.resize_reflow",
            old_cols = self.cols,
            old_rows = self.rows,
            new_cols = new_cols,
            new_rows = new_rows,
            scrollback_len = self.scrollback.len()
        )
        .entered();
        if new_cols == 0 || new_rows == 0 || (self.cols == new_cols && self.rows == new_rows) {
            return;
        }

        self.pending_scroll = None;
        self.scroll_id = self.scroll_id.wrapping_add(1);
        self.selection = None;

        let absolute_cursor_row = self.scrollback.len() + self.cursor.row;
        let cursor_col = self.cursor.col;

        struct LogicalLine {
            cells: Vec<Cell>,
            cursor_offset: Option<usize>,
            reflow_on_resize: bool,
        }

        let mut logical_lines = Vec::new();
        let mut current_line = LogicalLine {
            cells: Vec::new(),
            cursor_offset: None,
            reflow_on_resize: false,
        };

        let mut all_rows = self.scrollback.drain_to_vec();
        let mut old_grid = std::mem::take(&mut self.grid);
        all_rows.extend(old_grid.drain(..));

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
                current_line.cursor_offset =
                    Some(current_line.cells.len() + cursor_col.min(keep_len));
            }

            current_line.cells.extend_from_slice(&row.cells[..keep_len]);

            if row.wrapped || row.reflowable {
                current_line.reflow_on_resize = true;
            }

            if !row.wrapped {
                logical_lines.push(current_line);
                current_line = LogicalLine {
                    cells: Vec::new(),
                    cursor_offset: None,
                    reflow_on_resize: false,
                };
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
                    new_cursor = CursorPos {
                        row: reflowed_rows.len(),
                        col: c_off,
                    };
                }
                reflowed_rows.push(Row {
                    cells: vec![self.default_cell(); new_cols].into_boxed_slice(),
                    len: 0,
                    wrapped: false,
                    reflowable: line.reflow_on_resize,
                });
                continue;
            }

            if !line.reflow_on_resize {
                // Non-reflowable line (e.g. nushell table, program-drawn TUI output).
                // We must NOT discard cells that lie beyond new_cols — they would be
                // unrecoverable when the window grows back. Instead we keep the full
                // cell slice in `row.cells` even when it is wider than new_cols.
                // Rendering clips at the viewport boundary automatically (visible_row
                // returns &row.cells which the tessellator reads up to `cols` cells from).
                // On a subsequent grow the wider row is re-sliced to the larger new_cols.
                let visible_len = cells.len().min(new_cols);
                let full_len = cells.len().max(new_cols);
                let mut new_cells = vec![self.default_cell(); full_len];
                new_cells[..cells.len()].clone_from_slice(&cells);
                let new_len = new_cells
                    .iter()
                    .rposition(|c| !c.is_empty())
                    .map_or(0, |i| i + 1);
                let new_row = Row {
                    cells: new_cells.into_boxed_slice(),
                    len: new_len,
                    wrapped: false,
                    reflowable: false,
                };

                if let Some(c_off) = line.cursor_offset {
                    new_cursor = CursorPos {
                        row: reflowed_rows.len(),
                        col: c_off.min(visible_len),
                    };
                }

                reflowed_rows.push(new_row);
                continue;
            }

            while i < cells.len() {
                let chunk_len = (cells.len() - i).min(new_cols);
                let mut new_row = Row {
                    cells: vec![self.default_cell(); new_cols].into_boxed_slice(),
                    len: 0,
                    wrapped: i + chunk_len < cells.len(),
                    reflowable: line.reflow_on_resize,
                };
                new_row.cells[..chunk_len].clone_from_slice(&cells[i..i + chunk_len]);
                new_row.len = new_row
                    .cells
                    .iter()
                    .rposition(|c| !c.is_empty())
                    .map_or(0, |idx| idx + 1);

                if let Some(c_off) = line.cursor_offset {
                    if (c_off >= i && c_off < i + new_cols)
                        || (c_off == i + new_cols && i + new_cols >= cells.len())
                    {
                        new_cursor = CursorPos {
                            row: reflowed_rows.len(),
                            col: c_off - i,
                        };
                    }
                }

                reflowed_rows.push(new_row);
                i += chunk_len;
            }
        }

        while reflowed_rows.len() > new_cursor.row + 1 {
            if reflowed_rows
                .last()
                .unwrap()
                .cells
                .iter()
                .all(|c| c.is_empty())
            {
                reflowed_rows.pop();
            } else {
                break;
            }
        }

        let total_rows = reflowed_rows.len();
        let grid_start = total_rows.saturating_sub(new_rows);

        let mut new_grid_rows = Vec::new();
        new_grid_rows.extend_from_slice(&reflowed_rows[grid_start..total_rows]);
        while new_grid_rows.len() < new_rows {
            new_grid_rows.push(Row {
                cells: vec![self.default_cell(); new_cols].into_boxed_slice(),
                len: 0,
                wrapped: false,
                reflowable: false,
            });
        }

        let scrollback_start = grid_start.saturating_sub(self.max_scrollback);
        let new_scrollback = reflowed_rows[scrollback_start..grid_start].to_vec();

        self.grid = VecDeque::from(new_grid_rows);
        self.scrollback.replace_from_rows(new_scrollback);
        self.cols = new_cols;
        self.rows = new_rows;

        if new_cursor.row >= grid_start {
            self.cursor.row = new_cursor.row - grid_start;
        } else {
            self.cursor.row = 0;
        }
        self.cursor.col = new_cursor.col.min(new_cols.saturating_sub(1));
        self.dirty_generations.resize(new_rows, 1);
        self.clean_generations.resize(new_rows, 0);
        for g in &mut self.dirty_generations {
            *g = g.wrapping_add(1);
        }
        self.scroll_offset = 0;
        self.margin_top = 0;
        self.margin_bottom = new_rows.saturating_sub(1);
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn mark_all_dirty(&mut self) {
        self.pending_scroll = None;
        self.scroll_id = self.scroll_id.wrapping_add(1);
        for g in &mut self.dirty_generations {
            *g = g.wrapping_add(1);
        }
    }

    pub fn mark_cursor_viewport_row_dirty(&mut self) {
        let row = self.cursor.row + self.scroll_offset;
        if row < self.dirty_generations.len() {
            self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
        }
    }

    pub fn set_cursor_style_override(
        &mut self,
        style: forge_core::config_registry::CursorStyle,
        blink: Option<bool>,
    ) {
        let changed =
            self.cursor_style_override != Some(style) || self.cursor_blink_override != blink;
        self.cursor_style_override = Some(style);
        self.cursor_blink_override = blink;
        if changed {
            self.mark_cursor_viewport_row_dirty();
        }
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        if self.cursor_visible != visible {
            self.cursor_visible = visible;
            self.mark_cursor_viewport_row_dirty();
        }
    }

    pub fn clear_cursor_style_override(&mut self) {
        if self.cursor_style_override.is_some() || self.cursor_blink_override.is_some() {
            self.cursor_style_override = None;
            self.cursor_blink_override = None;
            self.mark_cursor_viewport_row_dirty();
        }
    }

    pub fn mark_selection_rows_dirty(&mut self, selection: Option<SelectionRange>) {
        if let Some(selection) = selection {
            let start = selection.start_row.min(selection.end_row);
            let end = selection.start_row.max(selection.end_row);
            for row in start..=end {
                if row < self.dirty_generations.len() {
                    self.dirty_generations[row] = self.dirty_generations[row].wrapping_add(1);
                }
            }
        }
    }

    pub fn set_selection(&mut self, selection: Option<SelectionRange>) {
        let previous = self.selection;
        if previous == selection {
            return;
        }

        self.mark_selection_rows_dirty(previous);
        self.mark_selection_rows_dirty(selection);
        self.selection = selection;
    }

    pub fn clear_selection(&mut self) {
        self.set_selection(None);
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

    /// Sets the viewport scroll offset directly.
    pub fn view_scroll_to_offset(&mut self, offset: usize) {
        let max_offset = self.scrollback.len();
        let new_offset = offset.min(max_offset);
        if self.scroll_offset != new_offset {
            self.scroll_offset = new_offset;
            self.mark_all_dirty();
        }
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Retrieves a visible row based on the current scroll offset.
    /// `index` is 0-indexed from the top of the viewport.
    pub fn visible_row(&self, index: usize) -> &[Cell] {
        let cells = if self.scroll_offset == 0 {
            &self.grid[index].cells
        } else {
            let scrollback_lines = self.scrollback.len();
            if index < self.scroll_offset {
                let sb_idx = scrollback_lines - self.scroll_offset + index;
                &self.scrollback[sb_idx].cells
            } else {
                let grid_idx = index - self.scroll_offset;
                &self.grid[grid_idx].cells
            }
        };

        &cells[..cells.len().min(self.cols)]
    }

    pub fn visible_row_len(&self, index: usize) -> usize {
        let len = if self.scroll_offset == 0 {
            self.grid[index].len
        } else {
            let scrollback_lines = self.scrollback.len();
            if index < self.scroll_offset {
                let sb_idx = scrollback_lines - self.scroll_offset + index;
                self.scrollback[sb_idx].len
            } else {
                let grid_idx = index - self.scroll_offset;
                self.grid[grid_idx].len
            }
        };

        len.min(self.cols)
    }

    pub fn enable_alt_buffer(&mut self) {
        if !self.use_alt_buffer {
            self.use_alt_buffer = true;
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            // Save primary grid and attributes
            self.saved_primary_grid = Some(self.grid.clone());
            self.saved_primary_cursor = Some(self.cursor);
            self.saved_primary_attrs = Some((
                self.current_fg,
                self.current_bg,
                self.attr_bold,
                self.attr_italic,
                self.attr_underline,
                self.attr_strikethrough,
            ));

            // Clear current grid (which becomes alt grid)
            let default_cell = self.default_cell();
            for r in &mut self.grid {
                for c in &mut r.cells {
                    *c = default_cell;
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
            self.pending_scroll = None;
            self.scroll_id = self.scroll_id.wrapping_add(1);
            if let Some(mut grid) = self.saved_primary_grid.take() {
                grid.resize(
                    self.rows,
                    Row {
                        cells: vec![self.default_cell(); self.cols].into_boxed_slice(),
                        len: 0,
                        wrapped: false,
                        reflowable: false,
                    },
                );
                for row in &mut grid {
                    let mut vec = std::mem::replace(&mut row.cells, Box::new([])).into_vec();
                    vec.resize(self.cols, self.default_cell());
                    row.cells = vec.into_boxed_slice();
                    row.len = row.len.min(self.cols);
                }
                self.grid = grid;
            }
            if let Some(cursor) = self.saved_primary_cursor.take() {
                self.cursor.row = cursor.row.min(self.rows.saturating_sub(1));
                self.cursor.col = cursor.col.min(self.cols.saturating_sub(1));
            }
            if let Some((fg, bg, bold, italic, underline, strike)) = self.saved_primary_attrs.take()
            {
                self.current_fg = fg;
                self.current_bg = bg;
                self.attr_bold = bold;
                self.attr_italic = italic;
                self.attr_underline = underline;
                self.attr_strikethrough = strike;
            }
            self.clear_cursor_style_override();
            self.scroll_offset = 0;
            self.mark_all_dirty();
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn mark_row_clean(&mut self, row: usize) {
        if row < self.rows {
            self.clean_generations[row] = self.dirty_generations[row];
        }
    }

    pub fn mark_all_clean(&mut self) {
        self.clean_generations.copy_from_slice(&self.dirty_generations);
    }

    pub fn is_dirty(&self, row: usize) -> bool {
        self.dirty_generations[row] > self.clean_generations.get(row).copied().unwrap_or(0)
    }

    pub fn has_dirty_rows(&self) -> bool {
        self.dirty_generations.iter().zip(&self.clean_generations).any(|(d, c)| d > c)
    }

    pub fn get_text_in_range(&self, range: SelectionRange) -> String {
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

        for row in start_row..=end_row {
            if row >= self.grid.len() {
                break;
            }
            let grid_row = &self.grid[row];

            let start = if row == start_row { start_col } else { 0 };
            let end = if row == end_row {
                end_col
            } else {
                grid_row.cells.len().saturating_sub(1)
            };

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

    // TODO(PERF-02): update_theme iterates all cells including scrollback.
    // This is O(scrollback_len * cols) which is too slow. We should instead
    // store theme colors abstractly and resolve them at render time.
    pub fn update_theme(&mut self, new_fg: Color, new_bg: Color, palette: [Color; 16]) {
        let old_fg = self.default_fg;
        let old_bg = self.default_bg;

        self.default_fg = new_fg;
        self.default_bg = new_bg;
        self.palette = palette;

        if self.current_fg == old_fg {
            self.current_fg = new_fg;
        }
        if self.current_bg == old_bg {
            self.current_bg = new_bg;
        }

        // Update primary grid
        for row in &mut self.grid {
            for cell in &mut row.cells {
                if cell.fg == old_fg {
                    cell.fg = new_fg;
                }
                if cell.bg == old_bg {
                    cell.bg = new_bg;
                }
            }
        }

        // Update alt grid if saved
        if let Some(saved_grid) = &mut self.saved_primary_grid {
            for row in saved_grid {
                for cell in &mut row.cells {
                    if cell.fg == old_fg {
                        cell.fg = new_fg;
                    }
                    if cell.bg == old_bg {
                        cell.bg = new_bg;
                    }
                }
            }
        }

        // Update scrollback
        for row_idx in 0..self.scrollback.len() {
            if let Some(row) = self.scrollback.get_mut(row_idx) {
                for cell in &mut row.cells {
                    if cell.fg == old_fg {
                        cell.fg = new_fg;
                    }
                    if cell.bg == old_bg {
                        cell.bg = new_bg;
                    }
                }
            }
        }

        if let Some((fg, bg, _, _, _, _)) = &mut self.saved_primary_attrs {
            if *fg == old_fg {
                *fg = new_fg;
            }
            if *bg == old_bg {
                *bg = new_bg;
            }
        }

        self.mark_all_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_buffer(cols: usize, rows: usize) -> ScreenBuffer {
        ScreenBuffer::new(
            cols,
            rows,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        )
    }

    #[test]
    fn test_write_ascii() {
        let mut buf = ScreenBuffer::new(
            10,
            10,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.write_str("Hello");
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 5);
        assert_eq!(buf.grid[0].cells[0].c, 'H');
        assert_eq!(buf.grid[0].cells[4].c, 'o');
    }

    #[test]
    fn test_write_ascii_run_wraps_and_preserves_attrs() {
        let mut buf = ScreenBuffer::new(
            5,
            3,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.attr_bold = true;
        buf.write_ascii_run(b"abcdef");

        assert_eq!(buf.cursor.row, 1);
        assert_eq!(buf.cursor.col, 1);
        assert_eq!(buf.visible_row(0)[0].c, 'a');
        assert_eq!(buf.visible_row(0)[4].c, 'e');
        assert_eq!(buf.visible_row(1)[0].c, 'f');
        assert!(buf.visible_row(0)[0].is_bold());
        assert!(buf.is_dirty(0));
        assert!(buf.is_dirty(1));
    }

    #[test]
    fn repeated_identical_ascii_write_does_not_dirty_row() {
        let mut buf = test_buffer(10, 3);

        buf.write_ascii_run(b"abc");
        buf.mark_all_clean();
        buf.cursor = CursorPos { row: 0, col: 0 };
        buf.write_ascii_run(b"abc");

        assert!(!buf.is_dirty(0));
    }

    #[test]
    fn repeated_ascii_write_with_changed_style_dirties_row() {
        let mut buf = test_buffer(10, 3);

        buf.write_ascii_run(b"abc");
        buf.mark_all_clean();
        buf.cursor = CursorPos { row: 0, col: 0 };
        buf.attr_bold = true;
        buf.write_ascii_run(b"abc");

        assert!(buf.is_dirty(0));
        assert!(buf.visible_row(0)[0].is_bold());
    }

    #[test]
    fn repeated_identical_wide_write_does_not_dirty_row() {
        let mut buf = test_buffer(10, 3);

        buf.write_grapheme("中");
        buf.mark_all_clean();
        buf.cursor = CursorPos { row: 0, col: 0 };
        buf.write_grapheme("中");

        assert!(!buf.is_dirty(0));
    }

    #[test]
    fn erase_blank_cells_does_not_dirty_row() {
        let mut buf = test_buffer(10, 3);

        buf.mark_all_clean();
        buf.erase_line();
        assert!(!buf.is_dirty(0));

        buf.write_ascii_run(b"abc");
        buf.mark_all_clean();
        buf.cursor = CursorPos { row: 0, col: 0 };
        buf.erase_to_end_of_line();
        assert!(buf.is_dirty(0));
    }

    #[test]
    fn insert_delete_chars_on_blank_row_do_not_dirty_row() {
        let mut buf = test_buffer(10, 3);

        buf.mark_all_clean();
        buf.insert_chars(2);
        assert!(!buf.is_dirty(0));

        buf.delete_chars(2);
        assert!(!buf.is_dirty(0));
    }

    #[test]
    fn test_write_wide_char() {
        let mut buf = ScreenBuffer::new(
            10,
            10,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.write_str("中");
        assert_eq!(buf.cursor.col, 2);
        assert_eq!(buf.grid[0].cells[0].width(), CellWidth::Wide);
        assert_eq!(buf.grid[0].cells[1].c, '\0');
    }

    #[test]
    fn test_line_feed_and_scroll() {
        let mut buf = ScreenBuffer::new(
            10,
            2,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
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
    fn full_viewport_scroll_records_coalesced_scroll_event() {
        let mut buf = test_buffer(10, 3);
        buf.mark_all_clean();

        buf.scroll_up_in_region(1);
        buf.scroll_up_in_region(2);

        assert_eq!(
            buf.take_pending_scroll(),
            Some(ScrollEvent {
                direction: ScrollDirection::Up,
                top: 0,
                bottom: 2,
                lines: 3,
                full_viewport: true,
            })
        );
        assert_eq!(buf.take_pending_scroll(), None);
    }

    #[test]
    fn partial_region_scroll_records_region_bounds() {
        let mut buf = test_buffer(10, 5);
        buf.margin_top = 1;
        buf.margin_bottom = 3;
        buf.mark_all_clean();

        buf.scroll_down_in_region(1);

        assert_eq!(
            buf.take_pending_scroll(),
            Some(ScrollEvent {
                direction: ScrollDirection::Down,
                top: 1,
                bottom: 3,
                lines: 1,
                full_viewport: false,
            })
        );
    }

    #[test]
    fn resize_clears_pending_scroll_event() {
        let mut buf = test_buffer(10, 3);

        buf.scroll_up_in_region(1);
        buf.resize_reflow(12, 4);

        assert_eq!(buf.take_pending_scroll(), None);
    }

    #[test]
    fn write_after_scroll_clears_pending_scroll_event() {
        let mut buf = test_buffer(10, 3);
        buf.mark_all_clean();

        buf.scroll_up_in_region(1);
        assert!(buf.take_pending_scroll().is_some());

        buf.scroll_up_in_region(1);
        buf.write_str("prompt");

        assert_eq!(buf.take_pending_scroll(), None);
    }

    #[test]
    fn scroll_after_dirty_write_does_not_record_reusable_scroll_event() {
        let mut buf = test_buffer(10, 3);
        buf.mark_all_clean();

        buf.write_str("table row");
        buf.move_cursor_to(2, 0);
        buf.scroll_up_in_region(1);

        assert_eq!(buf.take_pending_scroll(), None);
        assert!((0..buf.rows()).all(|r| buf.is_dirty(r)));
    }

    #[test]
    fn coalesced_scrolls_still_record_when_no_other_dirty_rows_exist() {
        let mut buf = test_buffer(10, 3);
        buf.mark_all_clean();

        buf.scroll_up_in_region(1);
        buf.scroll_up_in_region(1);

        assert_eq!(
            buf.take_pending_scroll(),
            Some(ScrollEvent {
                direction: ScrollDirection::Up,
                top: 0,
                bottom: 2,
                lines: 2,
                full_viewport: true,
            })
        );
    }

    #[test]
    fn erase_after_scroll_clears_pending_scroll_event() {
        let mut buf = test_buffer(10, 3);

        buf.move_cursor_to(0, 0);
        buf.write_str("row0");
        buf.move_cursor_to(1, 0);
        buf.write_str("row1");
        buf.move_cursor_to(2, 0);
        buf.write_str("row2");
        buf.mark_all_clean();
        buf.scroll_up_in_region(1);
        assert!(buf.take_pending_scroll().is_some());

        buf.move_cursor_to(0, 0);
        buf.scroll_up_in_region(1);
        buf.erase_screen();

        assert_eq!(buf.take_pending_scroll(), None);
    }

    #[test]
    fn insert_and_delete_after_scroll_clear_pending_scroll_event() {
        let mut buf = test_buffer(10, 3);

        buf.move_cursor_to(0, 0);
        buf.write_str("row0");
        buf.move_cursor_to(1, 0);
        buf.write_str("abcdef");
        buf.move_cursor_to(2, 0);
        buf.write_str("row2");
        buf.scroll_up_in_region(1);
        buf.move_cursor_to(0, 0);
        buf.insert_chars(1);

        assert_eq!(buf.take_pending_scroll(), None);

        buf.scroll_up_in_region(1);
        buf.move_cursor_to(0, 0);
        buf.delete_chars(1);

        assert_eq!(buf.take_pending_scroll(), None);
    }

    #[test]
    fn test_erase() {
        let mut buf = ScreenBuffer::new(
            10,
            10,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.write_str("12345");
        buf.cursor.col = 2;
        buf.erase_to_end_of_line();
        assert_eq!(buf.grid[0].cells[1].c, '2');
        assert!(buf.grid[0].cells[2].is_empty());
        assert!(buf.grid[0].cells[3].is_empty());
    }

    #[test]
    fn test_resize() {
        let mut buf = ScreenBuffer::new(
            5,
            5,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.write_str("12345");
        buf.resize_reflow(10, 10);
        assert_eq!(buf.cols, 10);
        assert_eq!(buf.rows, 10);
        assert_eq!(buf.grid[0].cells[0].c, '1');
        assert!(buf.grid[0].cells[6].is_empty());
    }

    #[test]
    fn test_resize_shrink_grow() {
        let mut buf = ScreenBuffer::new(
            80,
            24,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.move_cursor_to(23, 79);
        buf.write_str("X");

        // Shrink
        buf.resize_reflow(40, 12);
        assert_eq!(buf.cols, 40);
        assert_eq!(buf.rows, 12);
        assert_eq!(buf.cursor.row, 11); // Clamped
        assert_eq!(buf.cursor.col, 39); // Clamped

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
        let mut buf = ScreenBuffer::new(
            10,
            5,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        ); // max 5 scrollback
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

    #[test]
    fn test_scrollback_viewport_uses_logical_order_after_wrap() {
        let mut buf = ScreenBuffer::new(
            10,
            2,
            3,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        for i in 0..5 {
            buf.write_str(&format!("Line {}", i));
            buf.carriage_return();
            buf.line_feed();
        }

        assert_eq!(buf.scrollback.len(), 3);
        assert_eq!(buf.scrollback[0].cells[5].c, '1');
        assert_eq!(buf.scrollback[2].cells[5].c, '3');

        buf.view_scroll_up(2);
        assert_eq!(buf.visible_row(0)[5].c, '2');
        assert_eq!(buf.visible_row(1)[5].c, '3');

        buf.view_scroll_down(1);
        assert_eq!(buf.visible_row(0)[5].c, '3');
        assert_eq!(buf.visible_row(1)[5].c, '4');
    }

    #[test]
    fn test_clear_scrollback_resets_viewport() {
        let mut buf = ScreenBuffer::new(
            10,
            2,
            3,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        for i in 0..4 {
            buf.write_str(&format!("Line {}", i));
            buf.carriage_return();
            buf.line_feed();
        }
        buf.view_scroll_up(2);

        buf.clear_scrollback();

        assert_eq!(buf.scrollback_len(), 0);
        assert_eq!(buf.scroll_offset, 0);
        assert!((0..buf.rows()).all(|r| buf.is_dirty(r)));
    }

    #[test]
    fn test_alt_buffer_does_not_append_scrollback() {
        let mut buf = ScreenBuffer::new(
            10,
            2,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        for i in 0..3 {
            buf.write_str(&format!("Line {}", i));
            buf.carriage_return();
            buf.line_feed();
        }
        let primary_scrollback_len = buf.scrollback_len();

        buf.enable_alt_buffer();
        for i in 0..5 {
            buf.write_str(&format!("Alt {}", i));
            buf.carriage_return();
            buf.line_feed();
        }
        assert_eq!(buf.scrollback_len(), primary_scrollback_len);

        buf.disable_alt_buffer();
        assert_eq!(buf.scrollback_len(), primary_scrollback_len);
        assert_eq!(buf.visible_row(0)[5].c, '2');
    }

    #[test]
    fn test_partial_scroll_region_preserves_outer_rows() {
        let mut buf = ScreenBuffer::new(
            6,
            4,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        for row in 0..4 {
            buf.move_cursor_to(row, 0);
            buf.write_str(&format!("Row{}", row));
        }

        buf.margin_top = 1;
        buf.margin_bottom = 2;
        buf.scroll_up_in_region(1);

        assert_eq!(buf.grid[0].cells[3].c, '0');
        assert_eq!(buf.grid[1].cells[3].c, '2');
        assert!(buf.grid[2].cells[0].is_empty());
        assert_eq!(buf.grid[3].cells[3].c, '3');
        assert_eq!(buf.scrollback_len(), 0);

        buf.scroll_down_in_region(1);
        assert_eq!(buf.grid[0].cells[3].c, '0');
        assert!(buf.grid[1].cells[0].is_empty());
        assert_eq!(buf.grid[2].cells[3].c, '2');
        assert_eq!(buf.grid[3].cells[3].c, '3');
    }

    #[test]
    fn test_selection_dirty_rows_are_limited_to_changed_ranges() {
        let mut buf = ScreenBuffer::new(
            10,
            5,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.mark_all_clean();

        buf.set_selection(Some(SelectionRange {
            start_row: 1,
            start_col: 0,
            end_row: 2,
            end_col: 3,
        }));
        assert_eq!(
            (0..5).map(|r| buf.is_dirty(r)).collect::<Vec<_>>(),
            vec![false, true, true, false, false]
        );

        buf.mark_all_clean();
        buf.set_selection(Some(SelectionRange {
            start_row: 2,
            start_col: 0,
            end_row: 3,
            end_col: 3,
        }));
        assert_eq!(
            (0..5).map(|r| buf.is_dirty(r)).collect::<Vec<_>>(),
            vec![false, true, true, true, false]
        );

        buf.mark_all_clean();
        buf.clear_selection();
        assert_eq!(
            (0..5).map(|r| buf.is_dirty(r)).collect::<Vec<_>>(),
            vec![false, false, true, true, false]
        );
    }

    #[test]
    fn test_cursor_viewport_dirty_row_respects_scroll_offset() {
        let mut buf = ScreenBuffer::new(
            10,
            5,
            5,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        );
        buf.mark_all_clean();
        buf.cursor.row = 1;
        buf.scroll_offset = 2;

        buf.mark_cursor_viewport_row_dirty();

        assert_eq!(
            (0..5).map(|r| buf.is_dirty(r)).collect::<Vec<_>>(),
            vec![false, false, false, true, false]
        );
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
        assert_eq!(buf.cursor.row, 3);
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

    #[test]
    fn writing_from_column_zero_clears_stale_wrap_flag() {
        let mut buf = ScreenBuffer::new(5, 4, 100, Color::WHITE, Color::BLACK);

        buf.write_str("abcdef");
        assert!(buf.grid[0].wrapped);

        buf.move_cursor_to(0, 0);
        buf.write_str("z");

        assert!(!buf.grid[0].wrapped);
    }

    #[test]
    fn resize_does_not_join_rows_after_wrapped_row_is_rewritten() {
        let mut buf = ScreenBuffer::new(5, 4, 100, Color::WHITE, Color::BLACK);

        buf.write_str("abcdef");
        assert!(buf.grid[0].wrapped);

        buf.move_cursor_to(0, 0);
        buf.erase_line();
        buf.write_str("  1");
        buf.move_cursor_to(1, 0);
        buf.erase_line();
        buf.write_str("  2");

        buf.resize_reflow(10, 4);

        let first: String = buf.visible_row(0).iter().map(|cell| cell.c).collect();
        let second: String = buf.visible_row(1).iter().map(|cell| cell.c).collect();
        assert_eq!(first, "  1       ");
        assert_eq!(second, "  2       ");
    }

    #[test]
    fn resize_preserves_non_reflowable_overflow_in_wider_row() {
        // When shrinking, content beyond the new width must NOT be discarded.
        // It is preserved by keeping the row's cell buffer wider than new_cols.
        // On grow-back, resize_reflow finds the full content and restores it.
        let mut buf = ScreenBuffer::new(12, 4, 100, Color::WHITE, Color::BLACK);

        buf.write_str("name | size");
        buf.move_cursor_to(1, 0);
        buf.write_str("file | 36kB");

        buf.resize_reflow(8, 4);

        // Viewport rows stay at 4 (no extra rows created).
        // Row 0 shows the first 8 visible chars; row 1 shows "file | 3...".
        assert_eq!(buf.visible_row(0).len(), 8);
        assert_eq!(buf.visible_row(1).len(), 8);
        assert_eq!(buf.visible_row_len(0), 8);
        assert_eq!(buf.visible_row_len(1), 8);
        let row0: String = buf
            .visible_row(0)
            .iter()
            .take(8)
            .map(|cell| cell.c)
            .collect();
        let row1: String = buf
            .visible_row(1)
            .iter()
            .take(8)
            .map(|cell| cell.c)
            .collect();
        assert_eq!(row0, "name | s");
        assert_eq!(row1, "file | 3");

        // The backing cells are wider than new_cols, preserving the overflow.
        assert!(buf.grid[0].cells.len() >= 11, "overflow should be stored");
        let full0: String = buf.grid[0].cells.iter().take(11).map(|c| c.c).collect();
        let full1: String = buf.grid[1].cells.iter().take(11).map(|c| c.c).collect();
        assert_eq!(full0, "name | size");
        assert_eq!(full1, "file | 36kB");

        // Grow back — the full content is restored in the viewport.
        buf.resize_reflow(12, 4);
        let restored0: String = buf.visible_row(0).iter().take(11).map(|c| c.c).collect();
        let restored1: String = buf.visible_row(1).iter().take(11).map(|c| c.c).collect();
        assert_eq!(restored0, "name | size");
        assert_eq!(restored1, "file | 36kB");
    }

    #[test]
    fn resize_still_reflows_soft_wrapped_rows() {
        let mut buf = ScreenBuffer::new(5, 4, 100, Color::WHITE, Color::BLACK);

        buf.write_str("abcdefgh");
        assert!(buf.grid[0].wrapped);

        buf.resize_reflow(8, 4);

        let first: String = buf.visible_row(0).iter().map(|c| c.c).collect();
        assert_eq!(first, "abcdefgh");
    }
}

impl ScreenBuffer {
    pub fn generate_snapshot(&mut self) -> crate::snapshot::RenderSnapshot {
        let cursor_row_in_viewport = self.cursor.row as isize + self.scroll_offset as isize;
        let cursor = if self.cursor_visible && cursor_row_in_viewport < self.rows() as isize {
            Some((self.cursor.col, cursor_row_in_viewport as usize))
        } else {
            None
        };

        // Clone the visible grid. For the common case of large `cat` output where scroll_offset==0,
        // this directly slices VecDeque rows — no scrollback lookup needed.
        // We clone the dirty_rows vec alongside, then immediately clear dirty flags in the live buffer
        // so the VTE thread can start tracking new changes right away.
        let rows = self.rows();
        let cloned_grid = (0..rows)
            .map(|i| {
                let visible = self.visible_row(i);
                let clipped_len = visible.len().min(self.cols);
                visible[..clipped_len].to_vec()
            })
            .collect();
        let dirty_generations = self.dirty_generations.clone();

        // Clear dirty flags immediately so the PTY thread starts accumulating fresh dirty state
        // for the next snapshot, without waiting for the renderer to ack.
        self.mark_all_clean();

        let scroll_event = self.pending_scroll.clone();
        let scroll_id = self.scroll_id;
        self.snapshot_id = self.snapshot_id.wrapping_add(1);
        let current_snapshot_id = self.snapshot_id;

        crate::snapshot::RenderSnapshot {
            grid: cloned_grid,
            dirty_generations,
            cursor,
            cursor_style_override: self.cursor_style_override,
            cursor_blink_override: self.cursor_blink_override,
            selection: self.selection,
            use_alt_buffer: self.use_alt_buffer,
            visible_screen_lines: rows.max(1) as f64,
            history_lines: self.scrollback_len() as f64,
            viewport_offset: self.scroll_offset as f64,
            mouse_tracking_enabled: self.mouse_tracking_enabled,
            mouse_sgr_mode: self.mouse_sgr_mode,
            bracketed_paste: self.bracketed_paste,
            scroll_event,
            scroll_id,
            snapshot_id: current_snapshot_id,
            current_dir: self.current_dir.clone(),
            current_title: Some(self.current_title.clone()),
            current_command: self.current_command.clone(),
            is_command_running: self.is_command_running,
        }
    }
}
