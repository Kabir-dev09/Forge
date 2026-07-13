with open('crates/forge-renderer/src/renderer.rs', 'r') as f:
    content = f.read()

content = content.replace('pub dirty_rows: &\'a [bool],', 'pub dirty_rows: &\'a [u64],')
content = content.replace('let all_dirty = vec![true; rows];', 'let all_dirty = vec![1; rows];')
content = content.replace('dirty_rows', 'dirty_generations')

with open('crates/forge-renderer/src/renderer.rs', 'w') as f:
    f.write(content)

