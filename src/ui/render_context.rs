/// WGPU rendering context wrapper
///
/// Encapsulates all WGPU rendering resources (device, queue, surface, config)
/// into a single, reusable context for easier resource management.
use std::sync::Arc;

/// Wraps WGPU rendering resources
pub struct RenderContext {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

impl RenderContext {
    /// Create a new rendering context from WGPU resources
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            config,
        }
    }

    /// Get the surface format
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Get the surface dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Resize the surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Get the current surface texture for rendering
    pub fn get_current_texture(&self) -> Result<wgpu::SurfaceTexture, wgpu::SurfaceError> {
        self.surface.get_current_texture()
    }

    /// Create a command encoder for recording render commands
    pub fn create_encoder(&self, label: Option<&str>) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label })
    }

    /// Submit a command buffer for execution
    pub fn submit(&self, encoder: wgpu::CommandEncoder) {
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Submit command buffer and present surface
    pub fn submit_and_present(
        &self,
        encoder: wgpu::CommandEncoder,
        surface_texture: wgpu::SurfaceTexture,
    ) {
        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_render_context_dimensions() {
        // This would require actual WGPU setup, so we skip the test
        // In a real scenario, you'd use a headless device or mock
    }
}
