// Vertex shader for tooltip background

struct VertexInput {
    @location(0) position: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Uniform for fade animation
struct TooltipUniforms {
    opacity: f32,
}

@group(0) @binding(0)
var<uniform> tooltip: TooltipUniforms;

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

// Fragment shader for tooltip with rounded corners and border
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.5);
    let size = vec2<f32>(1.0, 1.0);
    let corner_radius = 0.12; // Nice visible rounded corners

    // Calculate distance from the edge of the rectangle
    let half_size = size * 0.5;
    let dist = abs(in.uv - center) - half_size + corner_radius;
    let dist_to_edge = length(max(dist, vec2<f32>(0.0, 0.0))) - corner_radius;

    // Sharp edge with minimal anti-aliasing
    let edge_width = 0.005;
    let alpha = 1.0 - clamp(dist_to_edge / edge_width + 0.5, 0.0, 1.0);

    // Border effect - lighter color at the edges
    let border_width = 0.02;
    let is_border = dist_to_edge > -border_width && dist_to_edge < 0.0;

    // Dark background with lighter border
    let bg_color = vec3<f32>(0.1, 0.1, 0.1);
    let border_color = vec3<f32>(0.3, 0.3, 0.3);
    let final_color = select(bg_color, border_color, is_border);

    // Apply fade animation via opacity uniform
    let final_opacity = 0.9 * tooltip.opacity;

    return vec4<f32>(final_color, alpha * final_opacity);
}
