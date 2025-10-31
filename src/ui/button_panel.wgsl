// Vertex shader for button panel with animation support

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

// Fragment shader for button panel with fade animation
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.5);
    let size = vec2<f32>(1.0, 1.0);
    let corner_radius = 0.16; // Matches main container

    // Calculate distance from the edge of the rectangle
    let half_size = size * 0.5;
    let dist = abs(in.uv - center) - half_size + corner_radius;
    let dist_to_edge = length(max(dist, vec2<f32>(0.0, 0.0))) - corner_radius;

    // Use a very sharp edge with minimal anti-aliasing
    let edge_width = 0.005;
    let alpha = 1.0 - clamp(dist_to_edge / edge_width + 0.5, 0.0, 1.0);

    // Semi-transparent gray background with hover animation
    // Base opacity is 0.15, fades in to full 0.15 * opacity_multiplier on hover
    let base_opacity = 0.15;
    let animated_opacity = base_opacity * hover.opacity_multiplier;

    return vec4<f32>(0.2, 0.2, 0.2, alpha * animated_opacity);
}
