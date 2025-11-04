struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) frag_position: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.frag_position = position;
    return output;
}

@fragment
fn fs_main(@location(0) frag_position: vec2<f32>) -> @location(0) vec4<f32> {
    // Calculate distance from center (0, 0) in normalized coordinates
    let distance = length(frag_position);

    // Create a circle with smooth edges
    let radius = 0.8;
    let edge_softness = 0.1;
    let alpha = 1.0 - smoothstep(radius - edge_softness, radius, distance);

    return vec4<f32>(1.0, 1.0, 1.0, alpha);
}