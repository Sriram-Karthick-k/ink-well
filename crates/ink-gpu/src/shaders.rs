//! GLSL sources. The `#version` line and precision qualifiers are prepended at run time, because desktop
//! OpenGL and OpenGL ES want different headers.

/// One stamp is a quad around its center, turned by its rotation. `u_view` is (pixels per canvas unit, origin
/// x, origin y, angle), with the origin measured in pixels from the top left of the target and the paper turned
/// about it by the angle, counterclockwise as seen; canvas y points up.
/// `u_mask_mode` is 0 for an analytic disc, 1 for a texture that is the whole stamp, 2 for a 4 by 4 atlas.
pub const STAMP_VERT: &str = r"
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_stamp;
layout(location = 2) in vec2 a_shape;
uniform vec4 u_view;
uniform vec2 u_size;
uniform int u_mask_mode;
out vec2 v_local;
out vec2 v_uv;
out float v_alpha;
out float v_radius;
vec2 seen(vec2 v) {
    float vc = cos(u_view.w);
    float vs = sin(u_view.w);
    return vec2(v.x * vc + v.y * vs, -v.x * vs + v.y * vc);
}
void main() {
    float radius = max(a_stamp.z * u_view.x, 0.6);
    vec2 center = vec2(u_view.y, u_view.z) + seen(vec2(a_stamp.x * u_view.x, -a_stamp.y * u_view.x));
    float reach = u_mask_mode == 0 ? radius + 1.0 : radius;
    float c = cos(a_shape.x);
    float s = sin(a_shape.x);
    vec2 turned = vec2(a_corner.x * c - a_corner.y * s, a_corner.x * s + a_corner.y * c);
    vec2 p = center + seen(turned * reach);
    v_local = a_corner * reach / radius;
    vec2 offset = a_corner * 0.5 + 0.5;
    if (u_mask_mode == 2) {
        vec2 tile = vec2(mod(a_shape.y, 4.0), floor(a_shape.y / 4.0));
        v_uv = (tile + offset) * 0.25;
    } else {
        v_uv = offset;
    }
    v_alpha = a_stamp.w;
    v_radius = radius;
    gl_Position = vec4(p.x / u_size.x * 2.0 - 1.0, 1.0 - p.y / u_size.y * 2.0, 0.0, 1.0);
}
";

/// A round stamp: one pixel of antialiasing at the rim and a falloff from `u_hardness` out to the rim, or the
/// texture when the pen has one.
pub const STAMP_FRAG: &str = r"
uniform float u_hardness;
uniform int u_mask_mode;
uniform float u_force_alpha;
uniform sampler2D u_stamp_mask;
in vec2 v_local;
in vec2 v_uv;
in float v_alpha;
in float v_radius;
out vec4 o_color;
void main() {
    float alpha = u_force_alpha >= 0.0 ? u_force_alpha : v_alpha;
    if (u_mask_mode != 0) {
        o_color = vec4(alpha * texture(u_stamp_mask, v_uv).r);
        return;
    }
    float d = length(v_local);
    float rim = clamp((1.0 - d) * v_radius + 0.5, 0.0, 1.0);
    float soft = 1.0 - smoothstep(min(u_hardness, 0.999), 1.0, d);
    o_color = vec4(alpha * rim * soft);
}
";

/// A triangle that covers the whole viewport; the scissor box limits the work to what changed.
pub const FULLSCREEN_VERT: &str = r"
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

/// Turns the mask into color. With grain, the mask has to beat a per-pixel threshold taken from the paper
/// texture, so light pressure and soft edges break up into speckles and heavy strokes fill in.
pub const COMPOSITE_FRAG: &str = r"
uniform sampler2D u_mask;
uniform sampler2D u_paper;
uniform sampler2D u_screen;
uniform int u_reveal;
uniform float u_gain;
uniform int u_screen_mode;
uniform float u_screen_scale;
uniform vec2 u_size;
uniform vec2 u_target_origin;
uniform vec4 u_view;
uniform vec4 u_color;
uniform vec2 u_grain;
out vec4 o_color;
void main() {
    vec2 p = gl_FragCoord.xy - u_target_origin;
    vec2 stored = texture(u_mask, p / u_size).rg;
    float m = (u_reveal != 0 ? stored.r * stored.g : stored.r) * u_gain;
    if (u_screen_mode != 0) {
        m *= texture(u_screen, vec2(p.x, u_size.y - p.y) * u_screen_scale).r;
    }
    m = clamp(m, 0.0, 1.0);
    float a = m;
    if (u_grain.x > 0.0) {
        // The pixel as a place on the canvas: the paper's turn undone.
        vec2 w = vec2(p.x - u_view.y, (u_size.y - p.y) - u_view.z);
        float vc = cos(u_view.w);
        float vs = sin(u_view.w);
        vec2 v = vec2(w.x * vc - w.y * vs, w.x * vs + w.y * vc);
        vec2 canvas = vec2(v.x, -v.y) / u_view.x;
        float g = texture(u_paper, canvas / u_grain.y).r;
        float t = g * u_grain.x * 0.8;
        a = smoothstep(t, t + 0.25, m);
    }
    a *= u_color.a;
    o_color = vec4(u_color.rgb * a, a);
}
";

/// Copies a premultiplied texture the size of the target.
pub const BLIT_FRAG: &str = r"
uniform sampler2D u_tex;
uniform vec2 u_size;
uniform vec2 u_target_origin;
out vec4 o_color;
void main() {
    vec2 p = gl_FragCoord.xy - u_target_origin;
    o_color = texture(u_tex, p / u_size);
}
";
