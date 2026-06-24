use forge_core::cell::{Cell, CellWidth};

pub struct CircularGrid {
    pub cells: Vec<Cell>,
    pub wrapped: Vec<bool>,
    pub cols: usize,
    pub lines: usize,
    pub head: usize, // physical index of logical line 0
}

impl CircularGrid {
    pub fn new(cols: usize, lines: usize, default_cell: Cell) -> Self {
        Self {
            cells: vec![default_cell; cols * lines],
            wrapped: vec![false; lines],
            cols,
            lines,
            head: 0,
        }
    }

    #[inline]
    pub fn physical_line(&self, logical_line: usize) -> usize {
        (self.head + logical_line) % self.lines
    }

    pub fn row(&self, logical_line: usize) -> &[Cell] {
        let p = self.physical_line(logical_line);
        &self.cells[p * self.cols .. (p + 1) * self.cols]
    }

    pub fn row_mut(&mut self, logical_line: usize) -> &mut [Cell] {
        let p = self.physical_line(logical_line);
        let cols = self.cols;
        &mut self.cells[p * cols .. (p + 1) * cols]
    }

    pub fn is_wrapped(&self, logical_line: usize) -> bool {
        self.wrapped[self.physical_line(logical_line)]
    }

    pub fn set_wrapped(&mut self, logical_line: usize, wrapped: bool) {
        let p = self.physical_line(logical_line);
        self.wrapped[p] = wrapped;
    }
}
