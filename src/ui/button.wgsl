// Vertex shader output structure to pass texture coordinates to fragment shader
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

// Uniform for rotation of close button and mode toggle
struct RotationUniform {
    rotation: f32,
    mode: f32, // 0.0 for RealTime (R), 1.0 for Manual (M)
}
@group(0) @binding(0) var<uniform> rotation_data: RotationUniform;

// Binding for texture and sampler - used by copy and reset buttons only
// These use the same group/binding positions as the rotation uniform
// but they're used in different pipelines, so it's OK
@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;

// Opacity uniform for single-texture buttons with state-based opacity
struct OpacityUniform {
    opacity: f32,
}
@group(0) @binding(2) var<uniform> opacity_data: OpacityUniform;

// Vertex shader for copy button
@vertex
fn vs_copy(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    // Map from [-1, 1] to [0, 1] for texture coordinates
    out.tex_coords = vec2<f32>(position.x * 0.5 + 0.5, -position.y * 0.5 + 0.5);
    return out;
}

// Vertex shader for reset button
@vertex
fn vs_reset(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    // Map from [-1, 1] to [0, 1] for texture coordinates
    out.tex_coords = vec2<f32>(position.x * 0.5 + 0.5, -position.y * 0.5 + 0.5);
    return out;
}

// Vertex shader for pause button
@vertex
fn vs_pause(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    // Map from [-1, 1] to [0, 1] for texture coordinates
    out.tex_coords = vec2<f32>(position.x * 0.5 + 0.5, -position.y * 0.5 + 0.5);
    return out;
}

// Vertex shader for close button - with rotation support
@vertex
fn vs_close(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    
    // Apply rotation to vertex position
    let angle = rotation_data.rotation;
    let cos_angle = cos(angle);
    let sin_angle = sin(angle);
    
    // Rotate the position around center (0,0)
    let rotated_x = position.x * cos_angle - position.y * sin_angle;
    let rotated_y = position.x * sin_angle + position.y * cos_angle;
    
    out.position = vec4<f32>(rotated_x, rotated_y, 0.0, 1.0);
    
    // For texture coordinates, we need to ensure they're still in [0,1] range
    // Map from [-1, 1] to [0, 1] for texture coordinates
    // We don't rotate texture coordinates to keep X appearance consistent
    out.tex_coords = vec2<f32>(position.x * 0.5 + 0.5, -position.y * 0.5 + 0.5);
    
    return out;
}

// Fragment shader for copy button - uses texture
@fragment
fn fs_copy(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    // Make button semi-transparent (85% opacity)
    color.a *= 0.85;

    return color;
}

// Fragment shader for texture buttons with dynamic opacity (e.g., MagicMode)
@fragment
fn fs_texture_opacity(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    // Apply dynamic opacity from uniform
    color.a *= opacity_data.opacity;

    return color;
}

// Fragment shader for reset button - uses texture
@fragment
fn fs_reset(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    // Make button semi-transparent (85% opacity)
    color.a *= 0.85;

    return color;
}

// Fragment shader for pause button - uses texture
@fragment
fn fs_pause(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    // Make button semi-transparent (85% opacity)
    color.a *= 0.85;

    return color;
}

// Fragment shader for close button - draws an X (NO texture binding needed)
@fragment
fn fs_close(in: VertexOutput) -> @location(0) vec4<f32> {
    // Draw an X for close button
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0); // Start transparent
    
    // Coordinates from 0-1
    let uv = in.tex_coords;
    
    // Define the thickness of the X lines
    let thickness = 0.12;
    
    // Check if we're on either diagonal
    let on_diagonal1 = abs(uv.x - uv.y) < thickness;
    let on_diagonal2 = abs(uv.x - (1.0 - uv.y)) < thickness;
    
    // If we're on either diagonal, color is white
    if (on_diagonal1 || on_diagonal2) {
        // White color for the X at 50% opacity
        color = vec4<f32>(1.0, 1.0, 1.0, 0.5);
    }

    return color;
}

// Fragment shader for mode toggle button - draws R/M letters based on mode
@fragment
fn fs_mode_toggle(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0); // Start transparent

    // Coordinates from 0-1
    let uv = in.tex_coords;

    // Define the thickness of the letter lines
    let thickness = 0.1;

    // Choose letter based on mode: 0.0 = RealTime (R), 1.0 = Manual (M)
    let is_manual = rotation_data.mode > 0.5;

    if (is_manual) {
        // Draw letter "M" - consists of two vertical lines and two diagonal lines
        let center_x = 0.5;
        let left_x = 0.2;
        let right_x = 0.8;

        // Left vertical line
        let on_left_vertical = abs(uv.x - left_x) < thickness && uv.y > 0.1 && uv.y < 0.9;

        // Right vertical line
        let on_right_vertical = abs(uv.x - right_x) < thickness && uv.y > 0.1 && uv.y < 0.9;

        // Left diagonal (from top-left to center-middle)
        let left_diag_slope = (0.5 - 0.1) / (center_x - left_x);
        let left_diag_y = 0.1 + left_diag_slope * (uv.x - left_x);
        let on_left_diagonal = abs(uv.y - left_diag_y) < thickness && uv.x >= left_x && uv.x <= center_x && uv.y >= 0.1 && uv.y <= 0.5;

        // Right diagonal (from center-middle to top-right)
        let right_diag_slope = (0.1 - 0.5) / (right_x - center_x);
        let right_diag_y = 0.5 + right_diag_slope * (uv.x - center_x);
        let on_right_diagonal = abs(uv.y - right_diag_y) < thickness && uv.x >= center_x && uv.x <= right_x && uv.y >= 0.1 && uv.y <= 0.5;

        // If we're on any part of the M, color is white
        if (on_left_vertical || on_right_vertical || on_left_diagonal || on_right_diagonal) {
            color = vec4<f32>(1.0, 1.0, 1.0, 0.85);
        }
    } else {
        // Draw letter "R" - consists of vertical line, top horizontal, middle horizontal, and diagonal
        let left_x = 0.2;
        let right_x = 0.7;
        let middle_x = 0.45;

        // Left vertical line (full height)
        let on_left_vertical = abs(uv.x - left_x) < thickness && uv.y > 0.1 && uv.y < 0.9;

        // Top horizontal line
        let on_top_horizontal = abs(uv.y - 0.1) < thickness && uv.x >= left_x && uv.x <= right_x;

        // Middle horizontal line (shorter)
        let on_middle_horizontal = abs(uv.y - 0.5) < thickness && uv.x >= left_x && uv.x <= middle_x;

        // Right vertical line (top half only)
        let on_right_vertical = abs(uv.x - right_x) < thickness && uv.y > 0.1 && uv.y < 0.5;

        // Diagonal line from middle to bottom-right
        let diag_slope = (0.9 - 0.5) / (right_x - middle_x);
        let diag_y = 0.5 + diag_slope * (uv.x - middle_x);
        let on_diagonal = abs(uv.y - diag_y) < thickness && uv.x >= middle_x && uv.x <= right_x && uv.y >= 0.5 && uv.y <= 0.9;

        // If we're on any part of the R, color is white
        if (on_left_vertical || on_top_horizontal || on_middle_horizontal || on_right_vertical || on_diagonal) {
            color = vec4<f32>(1.0, 1.0, 1.0, 0.85);
        }
    }

    return color;
}

// Fragment shader for settings button - draws a gear icon
@fragment
fn fs_settings(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    let uv = in.tex_coords;
    let center = vec2<f32>(0.5, 0.5);
    let offset = uv - center;
    let dist = length(offset);
    let angle = atan2(offset.y, offset.x);

    // Ring (outer radius 0.33, inner radius 0.18)
    let on_ring = dist > 0.18 && dist < 0.33;

    // Center hole
    let inner_circle = dist < 0.11;

    // 6 teeth around the ring
    let tooth_count = 6.0;
    let tooth_angle = fract(angle / (2.0 * 3.14159265) * tooth_count);
    let on_tooth = dist > 0.28 && dist < 0.43 && tooth_angle > 0.2 && tooth_angle < 0.55;

    if ((on_ring || on_tooth) && !inner_circle) {
        color = vec4<f32>(1.0, 1.0, 1.0, 0.85);
    }
    return color;
}

// Fragment shader for magic mode button - draws a magic wand
// mode: 0.0 = off (wand only, dim), 1.0 = on (wand with sparkles, bright gold)
@fragment
fn fs_magic_mode(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0); // Start transparent

    let uv = in.tex_coords;
    let is_on = rotation_data.mode > 0.5;

    // Wand parameters - diagonal, tip pointing upper-right
    let wand_start = vec2<f32>(0.15, 0.85);   // Handle end (bottom-left)
    let wand_end = vec2<f32>(0.75, 0.2);      // Tip (upper-right)
    let wand_dir = normalize(wand_end - wand_start);
    let wand_length = length(wand_end - wand_start);

    // Project point onto wand line
    let to_point = uv - wand_start;
    let proj_length = dot(to_point, wand_dir);
    let proj_point = wand_start + wand_dir * clamp(proj_length, 0.0, wand_length);
    let dist_to_wand = length(uv - proj_point);

    // Wand thickness varies - thicker at handle, thinner at tip
    let t = clamp(proj_length / wand_length, 0.0, 1.0);
    let wand_thickness = mix(0.09, 0.035, t);  // Handle to tip

    let on_wand = dist_to_wand < wand_thickness && proj_length >= 0.0 && proj_length <= wand_length;

    // Small star at the tip of the wand
    let tip_star_center = wand_end + wand_dir * 0.02;
    let tip_offset = uv - tip_star_center;
    let tip_dist = length(tip_offset);
    let tip_angle = atan2(tip_offset.y, tip_offset.x);
    let tip_star_radius = mix(0.02, 0.055, abs(cos(tip_angle * 4.0)));
    let on_tip_star = tip_dist < tip_star_radius;

    // Sparkles - only visible when on
    var on_sparkle = false;
    if (is_on) {
        // Sparkle 1 - above and right of tip
        let sparkle1_center = vec2<f32>(0.88, 0.12);
        let sparkle1_offset = uv - sparkle1_center;
        let sparkle1_dist = length(sparkle1_offset);
        let sparkle1_angle = atan2(sparkle1_offset.y, sparkle1_offset.x);
        let sparkle1_radius = mix(0.01, 0.045, abs(cos(sparkle1_angle * 4.0)));
        let on_sparkle1 = sparkle1_dist < sparkle1_radius;

        // Sparkle 2 - left of tip
        let sparkle2_center = vec2<f32>(0.58, 0.08);
        let sparkle2_offset = uv - sparkle2_center;
        let sparkle2_dist = length(sparkle2_offset);
        let sparkle2_angle = atan2(sparkle2_offset.y, sparkle2_offset.x);
        let sparkle2_radius = mix(0.01, 0.035, abs(cos(sparkle2_angle * 4.0)));
        let on_sparkle2 = sparkle2_dist < sparkle2_radius;

        // Sparkle 3 - small dot right of tip
        let on_sparkle3 = length(uv - vec2<f32>(0.9, 0.28)) < 0.022;

        on_sparkle = on_sparkle1 || on_sparkle2 || on_sparkle3;
    }

    if (on_wand || on_tip_star || on_sparkle) {
        if (is_on) {
            // Bright gold when enabled
            color = vec4<f32>(1.0, 0.85, 0.3, 0.95);
        } else {
            // Dim white when disabled
            color = vec4<f32>(1.0, 1.0, 1.0, 0.5);
        }
    }

    return color;
}
