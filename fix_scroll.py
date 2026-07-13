with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
    content = f.read()

content = content.replace('pending_scroll: None,\n            use_alt_buffer: false,', 'pending_scroll: None,\n            scroll_id: 0,\n            use_alt_buffer: false,')

content = content.replace('self.pending_scroll = None;', 'self.pending_scroll = None;\n        self.scroll_id = self.scroll_id.wrapping_add(1);')

content = content.replace('self.pending_scroll = match self.pending_scroll {', 'self.scroll_id = self.scroll_id.wrapping_add(1);\n        self.pending_scroll = match self.pending_scroll {')

content = content.replace('let scroll_event = self.take_pending_scroll();', 'let scroll_event = self.pending_scroll.clone();\n        let scroll_id = self.scroll_id;')

content = content.replace('scroll_event,\n            rows: self.rows,', 'scroll_event,\n            scroll_id,\n            rows: self.rows,')

with open('crates/forge-pty/src/screen_buffer.rs', 'w') as f:
    f.write(content)


with open('crates/forge-pty/src/snapshot.rs', 'r') as f:
    content = f.read()

content = content.replace('pub scroll_event: Option<crate::screen_buffer::ScrollEvent>,', 'pub scroll_event: Option<crate::screen_buffer::ScrollEvent>,\n    pub scroll_id: u64,')
content = content.replace('scroll_event: None,', 'scroll_event: None,\n            scroll_id: 0,')

with open('crates/forge-pty/src/snapshot.rs', 'w') as f:
    f.write(content)


with open('crates/forge-renderer/src/grid_tessellator.rs', 'r') as f:
    content = f.read()

content = content.replace('last_dirty_generations: Vec<u64>,', 'last_dirty_generations: Vec<u64>,\n    last_scroll_id: u64,')
content = content.replace('last_dirty_generations: Vec::new(),', 'last_dirty_generations: Vec::new(),\n            last_scroll_id: 0,')

content = content.replace('scroll_event: Option<ScrollEvent>,', 'scroll_event: Option<ScrollEvent>,\n        scroll_id: u64,')
content = content.replace('    ) -> bool {\n        let Some(event) = scroll_event else {\n            return true;\n        };', '''    ) -> bool {
        let Some(event) = scroll_event else {
            self.last_scroll_id = scroll_id;
            return true;
        };
        if scroll_id == self.last_scroll_id {
            return true;
        }
        self.last_scroll_id = scroll_id;
''')

# Now fix the call sites to tessellate and apply_scroll_reuse
content = content.replace('self.apply_scroll_reuse(scroll_event, grid.len(), cell_h, vp_h, selection)', 'self.apply_scroll_reuse(scroll_event, scroll_id, grid.len(), cell_h, vp_h, selection)')
content = content.replace('context_menu: Option<ContextMenuRenderData<\'_>>,\n        scroll_event: Option<ScrollEvent>,', 'context_menu: Option<ContextMenuRenderData<\'_>>,\n        scroll_event: Option<ScrollEvent>,\n        scroll_id: u64,')

# Fix tests
content = content.replace('None,\n            scroll_event,', 'None,\n            scroll_event,\n            0,')
content = content.replace('None,\n            None,\n        );', 'None,\n            None,\n            0,\n        );')

with open('crates/forge-renderer/src/grid_tessellator.rs', 'w') as f:
    f.write(content)

with open('crates/forge-renderer/src/renderer.rs', 'r') as f:
    content = f.read()

content = content.replace('pub scroll_event: Option<forge_pty::screen_buffer::ScrollEvent>,', 'pub scroll_event: Option<forge_pty::screen_buffer::ScrollEvent>,\n    pub scroll_id: u64,')
content = content.replace('scroll_event: None,', 'scroll_event: None,\n            scroll_id: 0,')

content = content.replace('pane.scroll_event,\n            );', 'pane.scroll_event,\n                pane.scroll_id,\n            );')
content = content.replace('None,\n                );', 'None,\n                    0,\n                );')

with open('crates/forge-renderer/src/renderer.rs', 'w') as f:
    f.write(content)

