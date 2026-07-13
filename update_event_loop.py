import re

with open("crates/forge-main/src/event_loop.rs", "r") as f:
    code = f.read()

# 1. Add field to AppData
code = re.sub(
    r"pub original_font_size: f32,\n\}",
    "pub original_font_size: f32,\n    pub last_rendered_revisions: std::collections::HashMap<crate::mux::PaneId, Vec<u64>>,\n}",
    code
)

# 2. Add initialization to AppData
code = re.sub(
    r"original_font_size: config\.font\.size,\n\s*\}",
    "original_font_size: config.font.size,\n            last_rendered_revisions: std::collections::HashMap::new(),\n        }",
    code
)

# 3. Replace has_dirty_rows computation
# Original computation:
#         let (active_has_dirty_rows, use_alt_buffer, scrollback_lines) = {
#             let sb = screen_buffer.load();
#             (sb.dirty_rows.iter().any(|&b| b), sb.use_alt_buffer, sb.history_lines as usize)
#         };
#         let has_dirty_rows = active_has_dirty_rows
#             || app_data
#                 .tab_manager
#                 .active_mux()
#                 .panes
#                 .values()
#                 .any(|pane| pane.snapshot.load().dirty_rows.iter().any(|&b| b));

old_dirty_rows_computation = """        let (active_has_dirty_rows, use_alt_buffer, scrollback_lines) = {
            let sb = screen_buffer.load();
            (sb.dirty_rows.iter().any(|&b| b), sb.use_alt_buffer, sb.history_lines as usize)
        };
        let has_dirty_rows = active_has_dirty_rows
            || app_data
                .tab_manager
                .active_mux()
                .panes
                .values()
                .any(|pane| pane.snapshot.load().dirty_rows.iter().any(|&b| b));"""

new_dirty_rows_computation = """        let mut has_dirty_rows = false;
        let mut pane_dirty_rows = std::collections::HashMap::new();
        
        for pane in app_data.tab_manager.active_mux().panes.values() {
            let snap = pane.snapshot.load();
            let mut dirty = vec![false; snap.grid.len()];
            let last_revs = app_data.last_rendered_revisions.entry(pane.id).or_insert_with(|| vec![0; snap.grid.len()]);
            if last_revs.len() != snap.grid.len() {
                last_revs.resize(snap.grid.len(), 0);
            }
            for r in 0..snap.grid.len() {
                if snap.row_revisions[r] != last_revs[r] {
                    dirty[r] = true;
                    has_dirty_rows = true;
                }
            }
            pane_dirty_rows.insert(pane.id, dirty);
        }

        let (use_alt_buffer, scrollback_lines) = {
            let sb = screen_buffer.load();
            (sb.use_alt_buffer, sb.history_lines as usize)
        };"""

code = code.replace(old_dirty_rows_computation, new_dirty_rows_computation)

# 4. In the single pane rendering block:
#                             let sb = screen_buffer.load();
#                             let dirty_rows = sb.dirty_rows.clone();
# ...
#                                 dirty_rows: &dirty_rows,

code = re.sub(
    r"let sb = screen_buffer\.load\(\);\n\s*let dirty_rows = sb\.dirty_rows\.clone\(\);",
    "let sb = screen_buffer.load();\n                            let dirty_rows = pane_dirty_rows.remove(&active_pane_id).unwrap_or_else(|| vec![true; sb.grid.len()]);",
    code
)

# 5. In the multiple panes rendering block:
#                                     ClonedPane {
#                                         pane_id,
#                                         rect,
#                                         scroll_event: pane_scroll_event,
#                                         grid: sb.grid.clone(),
#                                         dirty_rows: sb.dirty_rows.clone(),
#                                         cursor,
#                                         cursor_style: sb.cursor_style_override.unwrap_or(app_data.config.cursor.style),
#                                         selection: sb.selection,
#                                     }

code = re.sub(
    r"grid: sb\.grid\.clone\(\),\n\s*dirty_rows: sb\.dirty_rows\.clone\(\),",
    "grid: sb.grid.clone(),\n                                        dirty_rows: pane_dirty_rows.remove(&pane_id).unwrap_or_else(|| vec![true; sb.grid.len()]),",
    code
)

# 6. MarkAllClean removal
#                 if !frame_should_mark_clean(needs_recreate) {
# ...
#                     if rendering_multiple_panes {
#                         mark_all_mux_panes_dirty(app_data.tab_manager.active_mux());
#                     } else {
#                         app_data.pane_io.send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllDirty(active_pane_id));
#                     }
#                 } else {
#                     if rendering_multiple_panes {
#                         for pane in app_data.tab_manager.active_mux().panes.values() {
#                             app_data.pane_io.send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllClean(pane.id));
#                         }
#                     } else {
#                         app_data.pane_io.send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllClean(active_pane_id));
#                     }
#                 }

code = re.sub(
    r"if !frame_should_mark_clean\(needs_recreate\) \{[\s\S]*?\} else \{[\s\S]*?\}",
    """if !frame_should_mark_clean(needs_recreate) {
                    if let Some(window) = app_data.wayland_state.window.as_ref() {
                        let _ = renderer.recreate_swapchain(window.size.width, window.size.height);
                    }
                    if rendering_multiple_panes {
                        mark_all_mux_panes_dirty(app_data.tab_manager.active_mux());
                    } else {
                        app_data.pane_io.send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllDirty(active_pane_id));
                    }
                } else {
                    // Update our tracked revisions to match what we just rendered
                    for pane in app_data.tab_manager.active_mux().panes.values() {
                        if let Some(revs) = app_data.last_rendered_revisions.get_mut(&pane.id) {
                            let snap = pane.snapshot.load();
                            if revs.len() == snap.row_revisions.len() {
                                revs.copy_from_slice(&snap.row_revisions);
                            }
                        }
                    }
                }""",
    code
)

with open("crates/forge-main/src/event_loop.rs", "w") as f:
    f.write(code)

