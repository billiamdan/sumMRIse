use crate::error::{DicomError, Result};
use dicom::core::PrimitiveValue;
use dicom::object::InMemDicomObject;
use log::debug;

pub fn extract_pixel_data(obj: &InMemDicomObject) -> Result<Vec<u16>> {
    use dicom::core::Tag;
    
    // Look for pixel data in various tags
    let pixel_data_tags = [
        Tag(0x7FE0, 0x0010), // Pixel Data
        Tag(0x7FE0, 0x0008), // Float Pixel Data
        Tag(0x7FE0, 0x0009), // Double Float Pixel Data
    ];

    for tag in &pixel_data_tags {
        if let Ok(elem) = obj.element(*tag) {
            debug!("Found pixel data in tag {:?}", tag);
            if let dicom::core::DicomValue::Primitive(pv) = elem.value() {
                return extract_pixel_data_from_element(pv);
            }
        }
    }

    Err(DicomError::MissingTag("Pixel Data".to_string()))
}

fn extract_pixel_data_from_element(value: &PrimitiveValue) -> Result<Vec<u16>> {
    match value {
        PrimitiveValue::U8(data) => {
            // Convert u8 to u16
            Ok(data.iter().map(|&x| x as u16).collect())
        }
        PrimitiveValue::U16(data) => {
            Ok(data.to_vec())
        }
        PrimitiveValue::U32(data) => {
            // Convert u32 to u16 (may lose precision)
            Ok(data.iter().map(|&x| x as u16).collect())
        }
        PrimitiveValue::I16(data) => {
            // Convert i16 to u16 (handle negative values)
            Ok(data.iter().map(|&x| x.max(0) as u16).collect())
        }
        PrimitiveValue::I32(data) => {
            // Convert i32 to u16
            Ok(data.iter().map(|&x| x.max(0) as u16).collect())
        }
        _ => Err(DicomError::ImageDecodingError(
            format!("Unsupported pixel data type: {:?}", value)
        )),
    }
}

pub fn apply_window_level(pixel_data: &[u16], window_center: f64, window_width: f64) -> Vec<u8> {
    let window_min = window_center - window_width / 2.0;
    let window_max = window_center + window_width / 2.0;
    
    pixel_data
        .iter()
        .map(|&pixel| {
            let pixel_f = pixel as f64;
            let normalized = if pixel_f < window_min {
                0.0
            } else if pixel_f > window_max {
                1.0
            } else {
                (pixel_f - window_min) / window_width
            };
            (normalized * 255.0).clamp(0.0, 255.0) as u8
        })
        .collect()
}
