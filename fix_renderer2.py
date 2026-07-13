with open('crates/forge-renderer/src/renderer.rs', 'r') as f:
    content = f.read()

content = content.replace('vec![true;', 'vec![1;')
content = content.replace('dirty_generations: &\'a [bool]', 'dirty_generations: &\'a [u64]')
content = content.replace('.filter(|&&dirty| dirty).count()', '.filter(|&&dirty| dirty > 0).count()')
content = content.replace('pub dirty_generations: &\'a [bool],', 'pub dirty_generations: &\'a [u64],')

with open('crates/forge-renderer/src/renderer.rs', 'w') as f:
    f.write(content)

