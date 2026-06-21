#version 450

layout(location = 0) in vec2 v_tex_coord;
layout(location = 1) in vec4 v_fg_color;
layout(location = 2) in vec4 v_bg_color;

layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D glyph_atlas;

layout(push_constant) uniform PushConstants {
    vec2 cell_size;
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
        
        if (proc_id <= -100.0) {
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
            
            float pixel_w = fwidth(v_tex_coord.x);
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
