#version 450

layout(location = 0) in vec2 position;
layout(location = 1) in vec2 tex_coord;
layout(location = 2) in vec4 fg_color;
layout(location = 3) in vec4 bg_color;

layout(location = 0) out vec2 v_tex_coord;
layout(location = 1) out vec4 v_fg_color;
layout(location = 2) out vec4 v_bg_color;

layout(push_constant) uniform PushConstants {
    vec2 cell_size;
    vec2 translation;
    float draw_opacity;
    uint config_flags;
    uvec2 _pad;
} pc;

void main() {
    gl_Position = vec4(position + pc.translation, 0.0, 1.0);
    v_tex_coord = tex_coord;
    v_fg_color = fg_color;
    v_bg_color = bg_color;
    v_fg_color.a *= pc.draw_opacity;
    v_bg_color.a *= pc.draw_opacity;
}
