import re

with open("crates/forge-main/src/event_loop.rs", "r") as f:
    lines = f.readlines()

new_lines = []
for i, line in enumerate(lines):
    # Determine if this line needs write or read
    if "app_data.screen_buffer" in line:
        if any(x in line for x in [".dirty_rows", ".cursor", ".scrollback_len", ".selection", ".rows", ".cols", ".visible_row", ".use_alt_buffer", ".mouse_tracking_enabled", ".mouse_sgr_mode", ".bracketed_paste", ".has_dirty_rows", ".cursor_blink_override", ".cursor_style_override", ".get_text_in_range"]):
            if any(x in line for x in ["=", "fill(", ".take()"]):
                line = line.replace("app_data.screen_buffer.", "app_data.screen_buffer.write().unwrap().")
            else:
                line = line.replace("app_data.screen_buffer.", "app_data.screen_buffer.read().unwrap().")
        else:
            line = line.replace("app_data.screen_buffer.", "app_data.screen_buffer.write().unwrap().")
            
        # Exception for render_grid where we pass the reference
        line = line.replace("&app_data.screen_buffer.read().unwrap().dirty_rows", "&app_data.screen_buffer.read().unwrap().dirty_rows.clone()")
        
    new_lines.append(line)

with open("crates/forge-main/src/event_loop.rs", "w") as f:
    f.writelines(new_lines)
