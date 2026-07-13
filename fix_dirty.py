import re

with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
    content = f.read()

# Replace dirty_rows initialization
content = content.replace('let dirty_rows = vec![true; rows];', 'let dirty_generations = vec![1; rows];')
content = content.replace('dirty_rows,\n', 'dirty_generations,\n')

# Replace dirty_rows[x] = true/false with dirty_generations[x] += 1
content = re.sub(r'self\.dirty_rows\[(.*?)\] = true;', r'self.dirty_generations[\1] = self.dirty_generations[\1].wrapping_add(1);', content)
content = re.sub(r'self\.dirty_rows\[(.*?)\] = false;', r'', content) # Remove false assignments
content = re.sub(r'self\.dirty_rows\.fill\(true\);', r'for g in &mut self.dirty_generations { *g = g.wrapping_add(1); }', content)
content = re.sub(r'self\.dirty_rows\.resize\((.*?), true\);', r'self.dirty_generations.resize(\1, 1);', content)

# Scroll shifts
content = re.sub(r'self\.dirty_rows\.copy_within\((.*?)\);', r'self.dirty_generations.copy_within(\1);', content)

# generate_snapshot
content = content.replace('let dirty_rows = self.dirty_rows.clone();', 'let dirty_generations = self.dirty_generations.clone();')
content = content.replace('self.dirty_rows.iter_mut().for_each(|d| *d = false);', '') # Do not clear!
content = content.replace('dirty_rows,\n            cursor,', 'dirty_generations,\n            cursor,')

with open('crates/forge-pty/src/screen_buffer.rs', 'w') as f:
    f.write(content)

