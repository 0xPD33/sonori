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
    
    return color;
}

// Fragment shader for reset button - uses texture
@fragment
fn fs_reset(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    
    return color;
}

// Fragment shader for pause button - uses texture
@fragment
fn fs_pause(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    
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
        // Pure white color for the X
        color = vec4<f32>(1.0, 1.0, 1.0, 0.9);
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
            color = vec4<f32>(1.0, 1.0, 1.0, 0.9);
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
            color = vec4<f32>(1.0, 1.0, 1.0, 0.9);
        }
    }
    
    return color;
}
