struct WidgetUniforms {
    color: vec4<f32>,
    rect: vec4<f32>,      // x, y, width, height (in pixels)
    corner_radius: f32,
    viewport_width: f32,
    viewport_height: f32,
    _padding: f32,
};

var<push_constant> uniforms: WidgetUniforms;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(position, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let pos = frag_coord.xy;
    let rect_min = uniforms.rect.xy;
    let rect_max = uniforms.rect.xy + uniforms.rect.zw;
    let r = uniforms.corner_radius;

    // Outside rect bounds entirely - discard
    if pos.x < rect_min.x || pos.x > rect_max.x || pos.y < rect_min.y || pos.y > rect_max.y {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Rounded corner SDF
    let center = clamp(pos, rect_min + r, rect_max - r);
    let dist = length(pos - center);
    let alpha = 1.0 - smoothstep(r - 0.5, r + 0.5, dist);

    return vec4<f32>(uniforms.color.rgb, uniforms.color.a * alpha);
}
