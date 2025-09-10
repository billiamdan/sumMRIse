use crate::error::{RenderingError, Result};
use summrise_core::models::ImageData;

pub struct VolumeData {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub data: Vec<u16>,
    pub spacing: (f64, f64, f64),
}

impl VolumeData {
    pub fn from_image_data(image_data: &[ImageData]) -> Result<Self> {
        if image_data.is_empty() {
            return Err(RenderingError::VolumeError("No image data provided".to_string()));
        }

        let first_image = &image_data[0];
        let width = first_image.width;
        let height = first_image.height;
        let depth = image_data.len() as u32;

        // Verify all images have the same dimensions
        for image in image_data {
            if image.width != width || image.height != height {
                return Err(RenderingError::VolumeError(
                    "All images must have the same dimensions".to_string()
                ));
            }
        }

        // Combine all pixel data
        let mut data = Vec::new();
        for image in image_data {
            data.extend_from_slice(&image.pixel_data);
        }

        // Calculate spacing
        let spacing = (
            first_image.pixel_spacing.0,
            first_image.pixel_spacing.1,
            first_image.slice_thickness,
        );

        Ok(Self {
            width,
            height,
            depth,
            data,
            spacing,
        })
    }

    pub fn get_voxel(&self, x: u32, y: u32, z: u32) -> Option<u16> {
        if x >= self.width || y >= self.height || z >= self.depth {
            return None;
        }

        let index = (z * self.width * self.height + y * self.width + x) as usize;
        self.data.get(index).copied()
    }

    pub fn get_slice(&self, z: u32) -> Option<&[u16]> {
        if z >= self.depth {
            return None;
        }

        let start = (z * self.width * self.height) as usize;
        let end = start + (self.width * self.height) as usize;
        self.data.get(start..end)
    }

    pub fn get_min_max(&self) -> (u16, u16) {
        let mut min = u16::MAX;
        let mut max = u16::MIN;

        for &value in &self.data {
            min = min.min(value);
            max = max.max(value);
        }

        (min, max)
    }
}

