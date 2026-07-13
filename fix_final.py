with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
    content = f.read()

content = content.replace('            scroll_offset: 0,\n            pending_scroll: None,\n            scroll_id: 0,\n            use_alt_buffer: false,\n            saved_primary_grid: None,\n            saved_primary_cursor: None,\n            saved_primary_attrs: None,\n            margin_top: 0,\n            margin_bottom: rows - 1,\n            mouse_tracking_enabled: false,\n            mouse_sgr_mode: false,\n            bracketed_paste: false,\n            scroll_offset: 0,\n            pending_scroll: None,', '            scroll_offset: 0,\n            pending_scroll: None,\n            scroll_id: 0,\n            use_alt_buffer: false,\n            saved_primary_grid: None,\n            saved_primary_cursor: None,\n            saved_primary_attrs: None,\n            margin_top: 0,\n            margin_bottom: rows - 1,\n            mouse_tracking_enabled: false,\n            mouse_sgr_mode: false,\n            bracketed_paste: false,')

with open('crates/forge-pty/src/screen_buffer.rs', 'w') as f:
    f.write(content)

with open('crates/forge-renderer/src/renderer.rs', 'r') as f:
    content = f.read()

content = content.replace('pub viewport_offset: f64,\n    pub scroll_event: Option<forge_pty::screen_buffer::ScrollEvent>,\n    pub scroll_id: u64,', 'pub selection_bg: [f32; 4],\n    pub viewport_offset: f64,\n    pub scroll_event: Option<super::grid_tessellator::ScrollEvent>,\n    pub scroll_id: u64,')

with open('crates/forge-renderer/src/renderer.rs', 'w') as f:
    f.write(content)

