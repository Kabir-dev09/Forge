import re

def process_file():
    with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
        content = f.read()

    # Imports and definitions
    content = content.replace('use forge_core::cell::{Cell, CellWidth};', 'use forge_core::cell::{Cell, CellWidth};\nuse crate::grid::{FlatGrid, FlatScrollback};')
    content = re.sub(r'#\[derive\(Clone\)\]\s*pub struct Row \{\s*pub cells: Vec<Cell>,\s*pub wrapped: bool,\s*\}\s*', '', content)

    # Struct fields
    content = content.replace('grid: Vec<Row>,', 'pub grid: FlatGrid,')
    content = content.replace('scrollback: Vec<Row>,', 'pub scrollback: FlatScrollback,')
    content = content.replace('saved_primary_grid: Option<Vec<Row>>,', 'saved_primary_grid: Option<FlatGrid>,')

    # Initialization
    content = re.sub(r'let grid = vec\!\[Row \{ cells: vec\!\[default_cell; cols\], wrapped: false \}; rows\];', 'let grid = FlatGrid::new(cols, rows, default_cell);', content)
    content = content.replace('scrollback: Vec::with_capacity(max_scrollback),', 'scrollback: FlatScrollback::new(cols, max_scrollback, default_cell),')

    # Wrapped flag
    content = re.sub(r'self\.grid\[(.*?)\]\.wrapped = (.*?);', r'self.grid.set_wrapped(\1, \2);', content)
    content = re.sub(r'self\.grid\[(.*?)\]\.wrapped', r'self.grid.is_wrapped(\1)', content)

    # Grid Cell Access (mut)
    content = re.sub(r'self\.grid\[(.*?)\]\.cells\[(.*?)\] = (.*?);', r'*self.grid.get_cell_mut(\1, \2) = \3;', content)
    
    # Grid Cell Access (clone)
    content = re.sub(r'self\.grid\[(.*?)\]\.cells\[(.*?)\]\.clone\(\)', r'*self.grid.get_cell(\1, \2)', content)

    # Grid Cell Access (read)
    content = re.sub(r'self\.grid\[(.*?)\]\.cells\[(.*?)\]', r'(*self.grid.get_cell(\1, \2))', content)
    
    # Grid Row Iter
    content = re.sub(r'self\.grid\[(.*?)\]\.cells\.iter\(\)', r'self.grid.get_row(\1).iter()', content)
    content = re.sub(r'&self\.grid\[(.*?)\]\.cells', r'self.grid.get_row(\1)', content)
    content = re.sub(r'&mut self\.grid\[(.*?)\]\.cells', r'self.grid.get_row_mut(\1)', content)
    content = re.sub(r'let grid_row = &self\.grid\[(.*?)\];', r'let grid_row = self.grid.get_row(\1);', content)
    content = re.sub(r'grid_row\.cells\.len\(\)', r'self.cols', content)
    content = re.sub(r'let cell = &grid_row\.cells\[(.*?)\];', r'let cell = &grid_row[\1];', content)

    # Grid length
    content = content.replace('self.grid.len()', 'self.grid.rows')

    # Scrollback access
    content = re.sub(r'self\.scrollback\[(.*?)\]\.cells\.len\(\)', r'self.cols', content)
    content = re.sub(r'self\.scrollback\[(.*?)\]\.cells\[(.*?)\]', r'(*self.scrollback.get_row(\1)[\2])', content)

    # Scroll up / down
    content = re.sub(r'for _ in 0\.\.n \{\s*let row = self\.grid\.remove\(top\);\s*if top == 0 && bottom == self\.rows - 1 && self\.max_scrollback > 0 && \!self\.use_alt_buffer \{\s*if self\.scrollback\.len\(\) >= self\.max_scrollback \{\s*self\.scrollback\.remove\(0\);\s*\}\s*self\.scrollback\.push\(row\);\s*\}\s*self\.grid\.insert\(bottom, Row \{ cells: vec\!\[self\.default_cell\(\); self\.cols\], wrapped: false \}\);\s*\}', 
r'''if top == 0 && bottom == self.rows - 1 && self.max_scrollback > 0 && !self.use_alt_buffer {
            for i in 0..n {
                let r = top + i;
                if r < self.rows {
                    let wrapped = self.grid.is_wrapped(r);
                    self.scrollback.push_row(self.grid.get_row(r), wrapped);
                }
            }
        }
        self.grid.scroll_up_in_region(top, bottom, n, self.default_cell());''', content)

    content = re.sub(r'for _ in 0\.\.n \{\s*self\.grid\.remove\(bottom\);\s*self\.grid\.insert\(top, Row \{ cells: vec\!\[self\.default_cell\(\); self\.cols\], wrapped: false \}\);\s*\}', r'self.grid.scroll_down_in_region(top, bottom, n, self.default_cell());', content)

    content = re.sub(r'let count = n\.min\(bottom - top \+ 1\);\s*for _ in 0\.\.count \{\s*self\.grid\.remove\(bottom\);\s*self\.grid\.insert\(top, Row \{ cells: vec\!\[self\.default_cell\(\); self\.cols\], wrapped: false \}\);\s*\}', r'self.grid.scroll_down_in_region(top, bottom, n, self.default_cell());', content)

    content = re.sub(r'let count = n\.min\(bottom - top \+ 1\);\s*for _ in 0\.\.count \{\s*self\.grid\.remove\(top\);\s*self\.grid\.insert\(bottom, Row \{ cells: vec\!\[self\.default_cell\(\); self\.cols\], wrapped: false \}\);\s*\}', r'self.grid.scroll_up_in_region(top, bottom, n, self.default_cell());', content)

    # Scrollback push
    content = re.sub(r'self\.scrollback\.push\(self\.grid\[r\]\.clone\(\)\);', r'self.scrollback.push_row(self.grid.get_row(r), self.grid.is_wrapped(r));', content)

    # Tests
    content = re.sub(r'buf\.grid\[(.*?)\]\.cells\[(.*?)\]\.c', r'buf.grid.get_cell(\1, \2).c', content)
    content = re.sub(r'buf\.grid\[(.*?)\]\.cells\[(.*?)\]\.is_empty\(\)', r'buf.grid.get_cell(\1, \2).is_empty()', content)
    content = re.sub(r'buf\.grid\[(.*?)\]\.cells\[(.*?)\]\.width\(\)', r'buf.grid.get_cell(\1, \2).width()', content)
    content = re.sub(r'buf\.scrollback\[(.*?)\]\.cells\[(.*?)\]\.c', r'buf.scrollback.get_row(\1)[\2].c', content)

    # Iteration over grid
    content = content.replace('for row in &mut self.grid {', 'for r in 0..self.rows {\n            let row = self.grid.get_row_mut(r);')
    content = content.replace('for r in &mut self.grid {', 'for row_idx in 0..self.rows {\n            let r = self.grid.get_row_mut(row_idx);')
    
    with open('crates/forge-pty/src/screen_buffer_new.rs', 'w') as f:
        f.write(content)

process_file()
