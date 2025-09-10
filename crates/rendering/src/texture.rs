use crate::error::{RenderingError, Result};
use summrise_core::models::ImageData;

pub struct Texture3D {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

impl Texture3D {
    pub fn from_image_data(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image_data: &[ImageData],
    ) -> Result<Self> {
        if image_data.is_empty() {
            return Err(RenderingError::TextureError("No image data provided".to_string()));
        }

        let first_image = &image_data[0];
        let width = first_image.width;
        let height = first_image.height;
        let depth = image_data.len() as u32;

        // Create 3D texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Volume Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: depth,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Create texture view
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Volume Texture View"),
            format: Some(wgpu::TextureFormat::R16Uint),
            dimension: Some(wgpu::TextureViewDimension::D3),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        });

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Volume Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Upload texture data
        let mut texture_data = Vec::new();
        for image in image_data {
            texture_data.extend_from_slice(&image.pixel_data);
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texture_data),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 2), // 2 bytes per u16
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: depth,
            },
        );

        Ok(Self {
            texture,
            view,
            sampler,
            width,
            height,
            depth,
        })
    }
}
