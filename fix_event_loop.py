with open('crates/forge-main/src/event_loop.rs', 'r') as f:
    content = f.read()

# Add last_snapshot_ptrs to AppData
content = content.replace('pub startup_start: std::time::Instant,', 'pub startup_start: std::time::Instant,\n    pub last_snapshot_ptrs: std::collections::HashMap<crate::mux::PaneId, usize>,')
content = content.replace('startup_start: std::time::Instant::now(),', 'startup_start: std::time::Instant::now(),\n            last_snapshot_ptrs: std::collections::HashMap::new(),')

# Replace the has_dirty_rows calculation block
old_block = '''        let mut pane_dirty_rows = std::collections::HashMap::new();
        let mut has_dirty_rows = false;
        {
            let active_pane_id = app_data.tab_manager.active_mux().active_pane;
            let sb = app_data.tab_manager.active_mux().panes.get(&active_pane_id).unwrap().snapshot.load();
            let dirty = sb.dirty_generations.clone();
            has_dirty_rows = dirty.iter().any(|&d| d);
            pane_dirty_rows.insert(active_pane_id, dirty);
        }'''
new_block = '''        let mut pane_dirty_rows = std::collections::HashMap::new();
        let mut has_dirty_rows = false;
        {
            let active_pane_id = app_data.tab_manager.active_mux().active_pane;
            let sb_arc = app_data.tab_manager.active_mux().panes.get(&active_pane_id).unwrap().snapshot.load_full();
            let ptr = std::sync::Arc::as_ptr(&sb_arc) as usize;
            if app_data.last_snapshot_ptrs.get(&active_pane_id) != Some(&ptr) {
                has_dirty_rows = true;
                app_data.last_snapshot_ptrs.insert(active_pane_id, ptr);
            }
            pane_dirty_rows.insert(active_pane_id, sb_arc.dirty_generations.clone());
        }'''
content = content.replace(old_block, new_block)

# Fix the fallback vec on line 1235
content = content.replace('vec![true; sb.grid.len()]', 'vec![1; sb.grid.len()]')

with open('crates/forge-main/src/event_loop.rs', 'w') as f:
    f.write(content)

