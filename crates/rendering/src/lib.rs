pub mod error;
pub mod renderer;
pub mod shaders;
pub mod texture;
pub mod volume;
pub mod viewport;

pub use error::{RenderingError, Result};
pub use renderer::Renderer;
pub use viewport::Viewport;

use summrise_core::models::{ImageData, ViewportSettings};

pub struct VolumeRenderer {
    renderer: Renderer,
    viewport: Viewport,
}

impl VolumeRenderer {
    pub async fn new(width: u32, height: u32) -> Result<Self> {
        let renderer = Renderer::new(width, height).await?;
        let viewport = Viewport::new();
        
        Ok(Self { renderer, viewport })
    }

    pub fn load_volume_data(&mut self, image_data: &[ImageData]) -> Result<()> {
        self.renderer.load_volume_data(image_data)
    }

    pub fn update_viewport(&mut self, settings: &ViewportSettings) {
        self.viewport.update(settings);
    }

    pub fn render(&mut self) -> Result<()> {
        self.renderer.render(&self.viewport)
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.renderer.resize(width, height)
    }
}

