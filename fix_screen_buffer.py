import re

with open("crates/forge-pty/src/screen_buffer.rs", "r") as f:
    code = f.read()

# 1. Add revision to Row
code = re.sub(
    r"pub struct Row \{\n    pub cells: Box<\[Cell\]>,\n    pub wrapped: bool,\n    pub reflowable: bool,\n\}",
    "pub struct Row {\n    pub cells: Box<[Cell]>,\n    pub wrapped: bool,\n    pub reflowable: bool,\n    pub revision: u64,\n}",
    code
)

# 2. Add grid_revision to ScreenBuffer (and remove dirty_rows if needed, actually let's just replace dirty_rows with grid_revision)
code = re.sub(
    r"pub dirty_rows: Vec<bool>,",
    "pub grid_revision: u64,",
    code
)

# 3. ScreenBuffer::new
code = re.sub(
    r"reflowable: false,\n            };\n            rows\n        \]\);\n        let dirty_rows = vec!\[true; rows\];",
    "reflowable: false,\n                revision: 1,\n            };\n            rows\n        ]);",
    code
)
code = re.sub(
    r"dirty_rows,\n            scrollback: Scrollback::new\(max_scrollback\),",
    "grid_revision: 2,\n            scrollback: Scrollback::new(max_scrollback),",
    code
)

# 4. set_cell_if_changed
code = re.sub(
    r"self\.pending_scroll = None;\n        self\.dirty_rows\[row\] = true;",
    "self.pending_scroll = None;\n        self.grid[row].revision = self.grid_revision;",
    code
)

# 5. Row instantiation with revision
code = re.sub(
    r"reflowable: false,\n\s*\}",
    "reflowable: false,\n                        revision: self.grid_revision,\n                    }",
    code
)
code = re.sub(
    r"reflowable: false,\n\s*\}\)",
    "reflowable: false,\n                        revision: self.grid_revision,\n                    })",
    code
)
code = re.sub(
    r"reflowable: line\.reflow_on_resize,\n\s*\}\)",
    "reflowable: line.reflow_on_resize,\n                    revision: self.grid_revision,\n                })",
    code
)
code = re.sub(
    r"reflowable: line\.reflow_on_resize,\n\s*\};",
    "reflowable: line.reflow_on_resize,\n                    revision: self.grid_revision,\n                };",
    code
)

# 6. mark_all_dirty
code = re.sub(
    r"pub fn mark_all_dirty\(\&mut self\) \{\n        if self\.dirty_rows\.len\(\) != self\.rows \{\n            self\.dirty_rows\.resize\(self\.rows, true\);\n        \}\n        self\.dirty_rows\.fill\(true\);\n    \}",
    "pub fn mark_all_dirty(&mut self) {\n        self.grid_revision += 1;\n        for r in 0..self.grid.len() {\n            self.grid[r].revision = self.grid_revision;\n        }\n    }",
    code
)
# 7. mark_cursor_viewport_row_dirty
code = re.sub(
    r"if row < self\.dirty_rows\.len\(\) \{\n            self\.dirty_rows\[row\] = true;\n        \}",
    "if row < self.grid.len() {\n            self.grid[row].revision = self.grid_revision;\n        }",
    code
)

# 8. mark_selection_rows_dirty
code = re.sub(
    r"if row < self\.dirty_rows\.len\(\) \{\n                    self\.dirty_rows\[row\] = true;\n                \}",
    "if row < self.grid.len() {\n                    self.grid[row].revision = self.grid_revision;\n                }",
    code
)

# 9. remove self.dirty_rows[r] assignments in scroll functions
code = re.sub(r"self\.dirty_rows\[r\] = self\.dirty_rows\[next_r\];", "", code)
code = re.sub(r"self\.dirty_rows\[r\] = self\.dirty_rows\[prev_r\];", "", code)
code = re.sub(r"self\.dirty_rows\[r\] = self\.dirty_rows\[r \+ n\];", "", code)
code = re.sub(r"self\.dirty_rows\[r\] = self\.dirty_rows\[r - n\];", "", code)
code = re.sub(r"self\.dirty_rows\[r\] = true;", "self.grid[r].revision = self.grid_revision;", code)

# 10. remove dirty_rows[self.cursor.row] assignments
code = re.sub(r"self\.dirty_rows\[self\.cursor\.row\] = true;", "self.grid[self.cursor.row].revision = self.grid_revision;", code)

# 11. mark_row_clean, mark_all_clean, has_dirty_rows
code = re.sub(
    r"pub fn mark_row_clean\(\&mut self, row: usize\) \{\n        if row < self\.rows \{\n            self\.dirty_rows\[row\] = false;\n        \}\n    \}",
    "pub fn mark_row_clean(&mut self, _row: usize) {}",
    code
)
code = re.sub(
    r"pub fn mark_all_clean\(\&mut self\) \{\n        self\.dirty_rows\.iter_mut\(\)\.for_each\(\|d\| \*d = false\);\n    \}",
    "pub fn mark_all_clean(&mut self) {}",
    code
)
code = re.sub(
    r"pub fn has_dirty_rows\(\&self\) -> bool \{\n        self\.dirty_rows\.iter\(\)\.any\(\|\&d\| d\)\n    \}",
    "pub fn has_dirty_rows(&self) -> bool {\n        true\n    }",
    code
)

# 12. generate_snapshot
code = re.sub(
    r"pub fn generate_snapshot\(\&mut self\) -> crate::snapshot::RenderSnapshot \{\n        let cursor_row_in_viewport = self\.cursor\.row as isize \+ self\.scroll_offset as isize;",
    "pub fn generate_snapshot(&mut self) -> crate::snapshot::RenderSnapshot {\n        self.grid_revision += 1;\n        let cursor_row_in_viewport = self.cursor.row as isize + self.scroll_offset as isize;",
    code
)
code = re.sub(
    r"let scroll_event = self\.take_pending_scroll\(\);\n        let snapshot = crate::snapshot::RenderSnapshot \{\n            grid: cloned_grid,\n            dirty_rows: self\.dirty_rows\.clone\(\),",
    "let scroll_event = self.take_pending_scroll();\n        let row_revisions = (0..self.rows()).map(|i| self.grid[i].revision).collect();\n        let snapshot = crate::snapshot::RenderSnapshot {\n            grid: cloned_grid,\n            row_revisions,",
    code
)

# 13. scroll_reuse_is_safe_before_scroll
code = re.sub(
    r"self\.pending_scroll\.is_some\(\) \|\| !self\.dirty_rows\[top\.\.=bottom\]\.iter\(\)\.any\(\|dirty\| \*dirty\)",
    "self.pending_scroll.is_some()",
    code
)

# Write back
with open("crates/forge-pty/src/screen_buffer.rs", "w") as f:
    f.write(code)

