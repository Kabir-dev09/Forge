with open('crates/forge-renderer/src/grid_tessellator.rs', 'r') as f:
    content = f.read()

content = content.replace('dirty_generations: &[bool]', 'dirty_generations: &[u64]')
content = content.replace('vec![true;', 'vec![1;')
content = content.replace('vec![false;', 'vec![0;')
content = content.replace('vec![true]', 'vec![1]')
content = content.replace('vec![false]', 'vec![0]')
content = content.replace('.filter(|&&dirty| dirty)', '.filter(|&&dirty| dirty > 0)')

with open('crates/forge-renderer/src/grid_tessellator.rs', 'w') as f:
    f.write(content)

with open('crates/forge-main/src/event_loop.rs', 'r') as f:
    content = f.read()

content = content.replace('&snapshot.dirty_rows', '&snapshot.dirty_generations')

with open('crates/forge-main/src/event_loop.rs', 'w') as f:
    f.write(content)

