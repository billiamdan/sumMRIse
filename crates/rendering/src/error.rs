use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderingError {
    #[error("WGPU error: {0}")]
    WgpuError(#[from] wgpu::Error),
    
    #[error("Image error: {0}")]
    ImageError(#[from] image::ImageError),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Shader compilation error: {0}")]
    ShaderError(String),
    
    #[error("Texture creation error: {0}")]
    TextureError(String),
    
    #[error("Volume data error: {0}")]
    VolumeError(String),
}

pub type Result<T> = std::result::Result<T, RenderingError>;

