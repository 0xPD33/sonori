// Uniform for hover animation
struct HoverUniforms {
    opacity_multiplier: f32,
}

@group(0) @binding(0)
var<uniform> hover: HoverUniforms;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(position, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    // Transcription box opacity animation (darker than spectrogram)
    let base_opacity = 0.4;
    let target_opacity = 0.9;
    let animated_opacity = base_opacity + (target_opacity - base_opacity) * hover.opacity_multiplier;

    return vec4<f32>(0.0, 0.0, 0.0, animated_opacity);
}
