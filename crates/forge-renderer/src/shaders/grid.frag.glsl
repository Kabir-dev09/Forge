#version 450

layout(location = 0) in vec2 v_tex_coord;
layout(location = 1) in vec4 v_fg_color;
layout(location = 2) in vec4 v_bg_color;

layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D glyph_atlas;

layout(push_constant) uniform PushConstants {
    vec2 cell_size;
    uint config_flags;
} pc;

void main() {
    // If tex_coord is negative, this is a procedural quad
    if (v_tex_coord.x < 0.0) {
        float proc_id = floor(v_tex_coord.x);
        vec2 local = vec2(fract(v_tex_coord.x), v_tex_coord.y);
        
        // proc_id == -1.0 : Solid Background
        if (proc_id == -1.0) {
            out_color = v_bg_color;
            return;
        }
        
        if (proc_id <= -100.0 && proc_id > -500.0) {
            int data = int(abs(proc_id) - 100.0);
            int u = data & 3;
            int d = (data >> 2) & 3;
            int l = (data >> 4) & 3;
            int r = (data >> 6) & 3;
            bool rnd = ((data >> 8) & 1) == 1;
            
            float W = pc.cell_size.x;
            float H = pc.cell_size.y;
            
            float px = (local.x - 0.5) * W;
            float py = (local.y - 0.5) * H;
            
            float cell_center_x = gl_FragCoord.x - px;
            float cell_center_y = gl_FragCoord.y - py;
            
            float line_x = floor(cell_center_x + 0.01) + 0.5;
            float line_y = floor(cell_center_y + 0.01) + 0.5;
            
            float spx = gl_FragCoord.x - line_x;
            float spy = gl_FragCoord.y - line_y;
            
            float ext_h = 0.0;
            if (l >= 2 || r >= 2) ext_h = 1.0;
            
            float ext_v = 0.0;
            if (u >= 2 || d >= 2) ext_v = 1.0;
            
            float alpha = 0.0;
            
            if (rnd) {
                float R = min(W, H) * 0.5;
                
                float cx = 0.0;
                float cy = 0.0;
                if (d > 0 && r > 0) { cx = R; cy = R; }
                else if (d > 0 && l > 0) { cx = -R; cy = R; }
                else if (u > 0 && l > 0) { cx = -R; cy = -R; }
                else if (u > 0 && r > 0) { cx = R; cy = -R; }
                
                bool in_arc = false;
                if (cx > 0.0 && cy > 0.0 && spx <= cx && spy <= cy) in_arc = true;
                else if (cx < 0.0 && cy > 0.0 && spx >= cx && spy <= cy) in_arc = true;
                else if (cx < 0.0 && cy < 0.0 && spx >= cx && spy >= cy) in_arc = true;
                else if (cx > 0.0 && cy < 0.0 && spx <= cx && spy >= cy) in_arc = true;
                
                if (in_arc) {
                    float d_px = length(vec2(spx, spy) - vec2(cx, cy)) - R;
                    alpha = max(alpha, clamp(1.0 - abs(d_px), 0.0, 1.0));
                }
                
                if (r > 0 && spx >= cx) alpha = max(alpha, clamp(1.0 - abs(spy), 0.0, 1.0));
                if (l > 0 && spx <= cx) alpha = max(alpha, clamp(1.0 - abs(spy), 0.0, 1.0));
                if (d > 0 && spy >= cy) alpha = max(alpha, clamp(1.0 - abs(spx), 0.0, 1.0));
                if (u > 0 && spy <= cy) alpha = max(alpha, clamp(1.0 - abs(spx), 0.0, 1.0));
            } else {
                // u > 0
                if (u == 1 && spy <= ext_h) alpha = max(alpha, clamp(1.0 - abs(spx), 0.0, 1.0));
                if (u == 2 && spy <= ext_h) alpha = max(alpha, clamp(2.0 - abs(spx), 0.0, 1.0));
                if (u == 3 && spy <= ext_h) alpha = max(alpha, clamp(1.0 - abs(abs(spx) - 1.0), 0.0, 1.0));
                
                // d > 0
                if (d == 1 && spy >= -ext_h) alpha = max(alpha, clamp(1.0 - abs(spx), 0.0, 1.0));
                if (d == 2 && spy >= -ext_h) alpha = max(alpha, clamp(2.0 - abs(spx), 0.0, 1.0));
                if (d == 3 && spy >= -ext_h) alpha = max(alpha, clamp(1.0 - abs(abs(spx) - 1.0), 0.0, 1.0));
                
                // l > 0
                if (l == 1 && spx <= ext_v) alpha = max(alpha, clamp(1.0 - abs(spy), 0.0, 1.0));
                if (l == 2 && spx <= ext_v) alpha = max(alpha, clamp(2.0 - abs(spy), 0.0, 1.0));
                if (l == 3 && spx <= ext_v) alpha = max(alpha, clamp(1.0 - abs(abs(spy) - 1.0), 0.0, 1.0));
                
                // r > 0
                if (r == 1 && spx >= -ext_v) alpha = max(alpha, clamp(1.0 - abs(spy), 0.0, 1.0));
                if (r == 2 && spx >= -ext_v) alpha = max(alpha, clamp(2.0 - abs(spy), 0.0, 1.0));
                if (r == 3 && spx >= -ext_v) alpha = max(alpha, clamp(1.0 - abs(abs(spy) - 1.0), 0.0, 1.0));
            }
            
            if (alpha <= 0.0) discard;
            out_color = vec4(v_fg_color.rgb, alpha * v_fg_color.a);
            return;
        } else if (proc_id == -30.0 || proc_id == -31.0) {
            // DA06m - Magically recover the exact physical pixel dimensions of this quad!
            float w = 1.0 / fwidth(local.x);
            float h = 1.0 / fwidth(local.y);

            float px_x = local.x * w;
            float px_y = local.y * h;

            // Calculate distance to the center line of the Pill shape
            float r = w / 2.0; // Radius is 4.0
            float cy = clamp(px_y, r, h - r);
            float dx = px_x - r;
            float dy = px_y - cy;
            float dist = sqrt(dx*dx + dy*dy);

            // Anti-aliased outer edge (Is the pixel inside the pill?)
            float shape_alpha = smoothstep(r + 0.5, r - 0.5, dist);
            if (shape_alpha <= 0.0) discard;

            if (proc_id == -30.0) {
                // Track: Solid Color #2C2B2F with a 1px Dark Grey border
                vec3 border_color = vec3(0.0055, 0.0055, 0.0066); // #18181A
                vec3 track_color = vec3(0.021, 0.020, 0.024);

                // The border is 1 pixel wide. We mix between border and fill based on distance!
                float fill_alpha = smoothstep(r - 1.0 + 0.5, r - 1.0 - 0.5, dist);
                vec3 final_color = mix(border_color, track_color, fill_alpha);

                float global_alpha = v_fg_color.a;
                out_color = vec4(final_color, shape_alpha * global_alpha);
            } else {
                // Thumb: #464447 Fill with a 1px Dark Grey border
                vec3 border_color = vec3(0.0055, 0.0055, 0.0066); // #18181A
                vec3 fill_color = vec3(0.058, 0.055, 0.060);   // #464447

                // The border is 1 pixel wide. We mix between border and fill based on distance!
                float fill_alpha = smoothstep(r - 1.0 + 0.5, r - 1.0 - 0.5, dist);
                vec3 final_color = mix(border_color, fill_color, fill_alpha);

                float global_alpha = v_fg_color.a;
                out_color = vec4(final_color, shape_alpha * global_alpha);
            }
            return;
        } else if (proc_id <= -500.0 && proc_id > -800.0) {
            int pattern = int(abs(proc_id) - 500.0);
            
            float W = pc.cell_size.x;
            float H = pc.cell_size.y;
            
            // Standard braille layout has 2 columns, 4 rows.
            // Width per cell: W/2. Height per cell: H/4.
            float cell_w = W / 2.0;
            float cell_h = H / 4.0;
            
            float px = local.x * W;
            float py = local.y * H;
            
            int col = int(px / cell_w);
            int row = int(py / cell_h);
            
            // Clamp to prevent out-of-bounds just in case
            col = clamp(col, 0, 1);
            row = clamp(row, 0, 3);
            
            // Dot index:
            // Col 0: row 0=1, row 1=2, row 2=4, row 3=64
            // Col 1: row 0=8, row 1=16, row 2=32, row 3=128
            int dot_val = 0;
            if (col == 0) {
                if (row == 0) dot_val = 1;
                else if (row == 1) dot_val = 2;
                else if (row == 2) dot_val = 4;
                else if (row == 3) dot_val = 64;
            } else {
                if (row == 0) dot_val = 8;
                else if (row == 1) dot_val = 16;
                else if (row == 2) dot_val = 32;
                else if (row == 3) dot_val = 128;
            }
            
            if ((pattern & dot_val) != 0) {
                if (pc.config_flags == 1) { // Solid
                    out_color = vec4(v_fg_color.rgb, v_fg_color.a);
                } else { // Dots (0)
                    float dot_radius = min(cell_w, cell_h) * 0.25; // 50% diameter
                    float cx = (float(col) + 0.5) * cell_w;
                    float cy = (float(row) + 0.5) * cell_h;
                    float d = length(vec2(px - cx, py - cy));
                    float alpha = smoothstep(dot_radius + 0.5, dot_radius - 0.5, d);
                    if (alpha <= 0.0) discard;
                    out_color = vec4(v_fg_color.rgb, alpha * v_fg_color.a);
                }
            } else {
                discard;
            }
            return;
        } else if (proc_id < -1.0) {
            float d = 0.0;
            
            if (proc_id == -2.0) {
                // Triangle Right
                d = local.x - 1.0 + abs(local.y - 0.5) * 2.0;
            } else if (proc_id == -3.0) {
                // Triangle Left
                d = abs(local.y - 0.5) * 2.0 - local.x;
            } else if (proc_id == -4.0) {
                // Curve Right
                float dy = (local.y - 0.5) * 2.0;
                d = sqrt(local.x * local.x + dy * dy) - 1.0;
            } else if (proc_id == -5.0) {
                // Curve Left
                float dx = 1.0 - local.x;
                float dy = (local.y - 0.5) * 2.0;
                d = sqrt(dx * dx + dy * dy) - 1.0;
            } else if (proc_id == -6.0) {
                // Angled Right Lower ()
                d = local.x - local.y;
            } else if (proc_id == -7.0) {
                // Angled Left Lower ()
                d = local.y - local.x;
            } else if (proc_id == -8.0) {
                // Angled Right Upper ()
                d = local.x + local.y - 1.0;
            } else if (proc_id == -9.0) {
                // Angled Left Upper ()
                d = 1.0 - (local.x + local.y);
            }
            
            float pixel_w = 1.0 / pc.cell_size.x;
            float edge = pixel_w * 0.5;
            float alpha = smoothstep(edge, -edge, d);
            
            if (alpha <= 0.0) {
                discard; 
            }
            
            out_color = vec4(v_fg_color.rgb, alpha * v_fg_color.a);
            return;
        }
    }

    // Sample the grayscale font atlas. We use the red channel as the alpha mask.
    float alpha = texture(glyph_atlas, v_tex_coord).r;
    
    // Smart Razor: Calculate perceived luminance of foreground and background
    float fg_lum = dot(v_fg_color.rgb, vec3(0.299, 0.587, 0.114));
    float bg_lum = dot(v_bg_color.rgb, vec3(0.299, 0.587, 0.114));
    
    // Contrast ranges from -1.0 (black on white) to 1.0 (white on black)
    float contrast = fg_lum - bg_lum;
    
    // Instead of pow() which destroys the anti-aliasing tail and causes blur, 
    // we use a parabolic curve that only boosts/thins the mid-tones.
    // Equation: alpha + alpha * (1.0 - alpha) * boost
    // If contrast > 0 (white on black), boost is negative -> thins the bloated text.
    // If contrast < 0 (black on white), boost is positive -> thickens the text.
    float boost = -contrast * 1.2; 
    float corrected_alpha = clamp(alpha + alpha * (1.0 - alpha) * boost, 0.0, 1.0);
    
    // We use Vulkan's pipeline color blending to composite this over the background.
    out_color = vec4(v_fg_color.rgb, v_fg_color.a * corrected_alpha);
}
