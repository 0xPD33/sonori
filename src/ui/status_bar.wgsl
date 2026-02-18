struct StatusUniforms {
    error_tint: f32,
    download_progress: f32, // -1.0 = no download, 0.0-1.0 = progress
    loading_phase: f32,     // -1.0 = not loading, >= 0.0 = animated sweep phase
    _pad: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> status: StatusUniforms;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = position * 0.5 + 0.5; // map -1..1 to 0..1
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base_color = vec3<f32>(0.0, 0.0, 0.0);
    let error_color = vec3<f32>(0.3, 0.0, 0.0);
    var color = mix(base_color, error_color, status.error_tint);
    var alpha = 0.35;

    // Draw download progress bar at bottom 20% of the status bar
    if status.download_progress >= 0.0 {
        let bar_height_frac = 0.2;
        if in.uv.y < bar_height_frac && in.uv.x <= status.download_progress {
            color = vec3<f32>(0.0, 0.6, 0.2);
            alpha = 0.8;
        }
    }

    // Indeterminate loading sweep animation at bottom
    if status.loading_phase >= 0.0 {
        let bar_height_frac = 0.15;
        if in.uv.y < bar_height_frac {
            let sweep_pos = fract(status.loading_phase);
            let sweep_width = 0.25;
            let dist = abs(in.uv.x - sweep_pos);
            let glow = smoothstep(sweep_width, 0.0, dist);
            color = mix(color, vec3<f32>(0.2, 0.55, 1.0), glow);
            alpha = mix(alpha, 0.8, glow);
        }
    }

    // Top border line (~1-2px for 18px bar)
    if in.uv.y > 0.92 {
        color = vec3<f32>(0.15, 0.15, 0.18);
        alpha = 0.6;
    }

    return vec4<f32>(color, alpha);
}
