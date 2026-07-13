import re

with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
    content = f.read()

# Fix .fill(true) in screen_buffer.rs
content = re.sub(r'self\.dirty_rows\[(.*?)\].fill\(true\);', r'for g in &mut self.dirty_generations[\1] { *g = g.wrapping_add(1); }', content)
content = content.replace('self.dirty_rows', 'self.dirty_generations')
content = content.replace('has_dirty_generations', 'has_dirty_rows') # restore method name

# Fix has_dirty_rows (dirty_generations is never cleared, so everything is technically "dirty" but we don't care, we just return true always? No, has_dirty_rows should just compare with a tracked variable if needed. For now, just return true so it always generates snapshots if elapsed time > 8ms)
content = content.replace('self.dirty_generations.iter().any(|&d| d)', 'true')

with open('crates/forge-pty/src/screen_buffer.rs', 'w') as f:
    f.write(content)

with open('crates/forge-pty/src/snapshot.rs', 'r') as f:
    content = f.read()
content = content.replace('pub dirty_rows: Vec<bool>,', 'pub dirty_generations: Vec<u64>,')
content = content.replace('dirty_rows: vec![true; rows],', 'dirty_generations: vec![1; rows],')
with open('crates/forge-pty/src/snapshot.rs', 'w') as f:
    f.write(content)

with open('crates/forge-renderer/src/grid_tessellator.rs', 'r') as f:
    content = f.read()

# Replace actual_dirty usage
content = content.replace('actual_dirty: Vec<bool>,', 'last_dirty_generations: Vec<u64>,\n    actual_dirty: Vec<bool>,')
content = content.replace('actual_dirty: Vec::new(),', 'last_dirty_generations: Vec::new(),\n            actual_dirty: Vec::new(),')
content = content.replace('self.actual_dirty.clear();\n        self.actual_dirty.extend_from_slice(dirty_rows);\n        self.actual_dirty.resize(grid.len(), true);', '''
        self.actual_dirty.clear();
        self.actual_dirty.resize(grid.len(), false);
        self.last_dirty_generations.resize(grid.len(), 0);
        for i in 0..grid.len() {
            if i < dirty_generations.len() {
                if self.last_dirty_generations[i] != dirty_generations[i] {
                    self.actual_dirty[i] = true;
                    self.last_dirty_generations[i] = dirty_generations[i];
                }
            }
        }
''')
content = content.replace('dirty_rows', 'dirty_generations')

# Fix apply_scroll_reuse for last_dirty_generations
content = content.replace('self.rows.rotate_left(lines);', 'self.rows.rotate_left(lines);\n                self.last_dirty_generations.rotate_left(lines);')
content = content.replace('self.rows.rotate_right(lines);', 'self.rows.rotate_right(lines);\n                self.last_dirty_generations.rotate_right(lines);')

with open('crates/forge-renderer/src/grid_tessellator.rs', 'w') as f:
    f.write(content)

