use forge_core::cell::Cell;

#[derive(Clone)]
pub struct FlatGrid {
    cells: Vec<Cell>,
    wrapped: Vec<bool>,
    pub cols: usize,
    pub rows: usize,
}

impl FlatGrid {
    pub fn new(cols: usize, rows: usize, default_cell: Cell) -> Self {
        Self {
            cells: vec![default_cell; cols * rows],
            wrapped: vec![false; rows],
            cols,
            rows,
        }
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize, default_cell: Cell) {
        let mut new_cells = vec![default_cell; new_cols * new_rows];
        let mut new_wrapped = vec![false; new_rows];
        
        let min_cols = self.cols.min(new_cols);
        let min_rows = self.rows.min(new_rows);
        
        for (r, wrapped_item) in new_wrapped.iter_mut().enumerate().take(min_rows) {
            let src_start = r * self.cols;
            let dst_start = r * new_cols;
            new_cells[dst_start..dst_start + min_cols]
                .copy_from_slice(&self.cells[src_start..src_start + min_cols]);
            *wrapped_item = self.wrapped[r];
        }
        
        self.cells = new_cells;
        self.wrapped = new_wrapped;
        self.cols = new_cols;
        self.rows = new_rows;
    }

    #[inline(always)]
    pub fn get_cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row * self.cols + col]
    }

    #[inline(always)]
    pub fn get_cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        &mut self.cells[row * self.cols + col]
    }

    #[inline(always)]
    pub fn get_row(&self, row: usize) -> &[Cell] {
        &self.cells[row * self.cols .. (row + 1) * self.cols]
    }

    #[inline(always)]
    pub fn get_row_mut(&mut self, row: usize) -> &mut [Cell] {
        &mut self.cells[row * self.cols .. (row + 1) * self.cols]
    }

    pub fn is_wrapped(&self, row: usize) -> bool {
        self.wrapped[row]
    }

    pub fn set_wrapped(&mut self, row: usize, wrapped: bool) {
        self.wrapped[row] = wrapped;
    }

    pub fn scroll_up_in_region(&mut self, top: usize, bottom: usize, n: usize, default_cell: Cell) {
        if top >= bottom || bottom >= self.rows || n == 0 { return; }
        let count = n.min(bottom - top + 1);
        
        let dst = top * self.cols;
        let src = (top + count) * self.cols;
        let len = (bottom - top + 1 - count) * self.cols;
        
        self.cells.copy_within(src..src + len, dst);
        self.wrapped.copy_within(top + count..=bottom, top);
        
        // Clear bottom
        let clear_start = (bottom + 1 - count) * self.cols;
        let clear_end = (bottom + 1) * self.cols;
        self.cells[clear_start..clear_end].fill(default_cell);
        self.wrapped[bottom + 1 - count..=bottom].fill(false);
    }

    pub fn scroll_down_in_region(&mut self, top: usize, bottom: usize, n: usize, default_cell: Cell) {
        if top >= bottom || bottom >= self.rows || n == 0 { return; }
        let count = n.min(bottom - top + 1);
        
        let dst = (top + count) * self.cols;
        let src = top * self.cols;
        let len = (bottom - top + 1 - count) * self.cols;
        
        self.cells.copy_within(src..src + len, dst);
        self.wrapped.copy_within(top..=bottom - count, top + count);
        
        // Clear top
        let clear_start = top * self.cols;
        let clear_end = (top + count) * self.cols;
        self.cells[clear_start..clear_end].fill(default_cell);
        self.wrapped[top..top + count].fill(false);
    }
}

pub struct FlatScrollback {
    cells: Vec<Cell>,
    wrapped: Vec<bool>,
    cols: usize,
    max_rows: usize,
    head: usize,
    len: usize,
}

impl FlatScrollback {
    pub fn new(cols: usize, max_rows: usize, default_cell: Cell) -> Self {
        Self {
            cells: vec![default_cell; cols * max_rows],
            wrapped: vec![false; max_rows],
            cols,
            max_rows,
            head: 0,
            len: 0,
        }
    }

    pub fn push_row(&mut self, row: &[Cell], wrapped: bool) {
        if self.max_rows == 0 { return; }
        
        let tail = (self.head + self.len) % self.max_rows;
        let dst_start = tail * self.cols;
        
        let cols_to_copy = self.cols.min(row.len());
        self.cells[dst_start..dst_start + cols_to_copy].copy_from_slice(&row[..cols_to_copy]);
        
        self.wrapped[tail] = wrapped;
        
        if self.len < self.max_rows {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % self.max_rows;
        }
    }

    pub fn pop_front(&mut self) -> Option<(Vec<Cell>, bool)> {
        if self.len == 0 { return None; }
        
        let src_start = self.head * self.cols;
        let mut row = vec![Cell::default(); self.cols];
        row.copy_from_slice(&self.cells[src_start..src_start + self.cols]);
        let wrapped = self.wrapped[self.head];
        
        self.head = (self.head + 1) % self.max_rows;
        self.len -= 1;
        
        Some((row, wrapped))
    }

    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }

    pub fn get_row(&self, index: usize) -> &[Cell] {
        let p = (self.head + index) % self.max_rows;
        &self.cells[p * self.cols .. (p + 1) * self.cols]
    }

    pub fn is_wrapped(&self, index: usize) -> bool {
        self.wrapped[(self.head + index) % self.max_rows]
    }
}
