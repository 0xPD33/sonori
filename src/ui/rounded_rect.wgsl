// Vertex shader for a rounded rectangle

struct VertexInput {
    @location(0) position: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Uniform for hover animation
struct HoverUniforms {
    opacity_multiplier: f32,
}

@group(0) @binding(0)
var<uniform> hover: HoverUniforms;

@vertex
fn vs_main(
    vertex: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(vertex.position, 0.0, 1.0);
    // Convert from clip space [-1,1] to UV space [0,1]
    out.uv = vertex.position * 0.5 + 0.5;
    return out;
}

// Fragment shader for a rounded rectangle with drop shadow and hover animation
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.5);
    let size = vec2<f32>(1.0, 1.0);
    let corner_radius = 0.16;

    // Shadow parameters
    let shadow_offset = vec2<f32>(0.004, -0.004); // 2px offset (right, down in screen space)
    let shadow_blur = 0.015; // Blur radius for soft shadow

    // Calculate shadow
    let shadow_center = center + shadow_offset;
    let half_size = size * 0.5;
    let shadow_dist = abs(in.uv - shadow_center) - half_size + corner_radius;
    let shadow_dist_to_edge = length(max(shadow_dist, vec2<f32>(0.0, 0.0))) - corner_radius;

    // Soft shadow using smooth falloff
    let shadow_alpha = 1.0 - clamp((shadow_dist_to_edge + shadow_blur) / shadow_blur, 0.0, 1.0);
    let shadow_color = vec4<f32>(0.0, 0.0, 0.0, shadow_alpha * 0.25 * hover.opacity_multiplier);

    // Calculate main rectangle
    let dist = abs(in.uv - center) - half_size + corner_radius;
    let dist_to_edge = length(max(dist, vec2<f32>(0.0, 0.0))) - corner_radius;

    // Sharp edges for main rectangle
    let edge_width = 0.005;
    let main_alpha = 1.0 - clamp(dist_to_edge / edge_width + 0.5, 0.0, 1.0);

    // Base opacity is 0.25, on hover it goes to 0.6 (always lighter than transcription box)
    let base_opacity = 0.25;
    let target_opacity = 0.6;
    let animated_opacity = base_opacity + (target_opacity - base_opacity) * hover.opacity_multiplier;

    let main_color = vec4<f32>(0.0, 0.0, 0.0, main_alpha * animated_opacity);

    // Blend shadow and main rectangle: shadow behind, main on top
    let result = mix(shadow_color, main_color, main_alpha);

    return result;
} 