use crate::ui::text_renderer::TextRenderer;

const SECTION_LABEL_HEIGHT: f32 = 20.0;

pub struct SectionLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

impl SectionLabel {
    pub fn new(text: &str, x: f32, y: f32, width: f32) -> Self {
        Self {
            text: text.to_string(),
            x,
            y,
            width,
        }
    }

    pub fn height(&self) -> f32 {
        SECTION_LABEL_HEIGHT
    }

    pub fn render(
        &self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        text_renderer: &mut TextRenderer,
        queue: &wgpu::Queue,
    ) {
        let _ = queue; // Queue not needed for text rendering but kept for API consistency
        text_renderer.render_text(
            view,
            encoder,
            &self.text,
            self.x,
            self.y,
            1.2,
            [1.0, 1.0, 1.0, 0.95],
            self.width as u32,
            SECTION_LABEL_HEIGHT as u32,
        );
    }
}
