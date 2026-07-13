import re
with open('crates/forge-main/src/event_loop.rs', 'r') as f:
    content = f.read()

content = content.replace('        startup_start: std::time::Instant::now(),\n    };\n', '        startup_start: std::time::Instant::now(),\n        last_snapshot_ptrs: std::collections::HashMap::new(),\n    };\n')

with open('crates/forge-main/src/event_loop.rs', 'w') as f:
    f.write(content)
