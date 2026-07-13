with open('crates/forge-renderer/src/renderer.rs', 'r') as f:
    content = f.read()

content = content.replace('            scroll_event,\n        );', '            scroll_event,\n            scroll_id,\n        );')
content = content.replace('            None,\n            );', '            None,\n            0,\n        );')
content = content.replace('            pane.scroll_event,\n            );', '            pane.scroll_event,\n            pane.scroll_id,\n        );')
content = content.replace('            None,\n                    );', '            None,\n            0,\n        );')

with open('crates/forge-renderer/src/renderer.rs', 'w') as f:
    f.write(content)

