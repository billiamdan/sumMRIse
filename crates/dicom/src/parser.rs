use crate::error::{DicomError, Result};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::Path;
use summrise_core::models::{ImageData, Instance, MedicalStudy, Series};
use uuid::Uuid;
use walkdir::WalkDir;
use dicom::object::open_file;

pub struct DicomParser;

impl DicomParser {
    pub fn new() -> Self {
        Self
    }

    pub async fn load_study_from_folder(&self, folder_path: &str) -> Result<MedicalStudy> {
        info!("Loading DICOM files from folder: {}", folder_path);
        
        let _instances: Vec<Instance> = Vec::new();
        let mut series_map: HashMap<String, Vec<Instance>> = HashMap::new();
        
        // Walk through all files in the folder
        for entry in WalkDir::new(folder_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if self.is_dicom_file(path) {
                match self.parse_dicom_file(path).await {
                    Ok(instance) => {
                        let series_uid = self.get_series_instance_uid(&instance)?;
                        series_map.entry(series_uid).or_insert_with(Vec::new).push(instance);
                    }
                    Err(e) => {
                        warn!("Failed to parse DICOM file {:?}: {}", path, e);
                    }
                }
            }
        }

        if series_map.is_empty() {
            return Err(DicomError::InvalidFile("No valid DICOM files found".to_string()));
        }

        // Convert series map to series vector
        let mut series = Vec::new();
        for (_series_uid, mut instances) in series_map {
            instances.sort_by_key(|i| i.instance_number.unwrap_or(0));
            
            let first_instance = &instances[0];
            let series_description = self.get_series_description(first_instance)?;
            let modality = self.get_modality(first_instance)?;
            
            series.push(Series {
                id: Uuid::new_v4(),
                series_number: self.get_series_number(first_instance)?,
                series_description,
                modality,
                instances,
            });
        }

        // Get study information from first series
        let first_series = &series[0];
        let first_instance = &first_series.instances[0];
        
        let study = MedicalStudy {
            id: Uuid::new_v4(),
            patient_id: self.get_patient_id(first_instance)?,
            study_date: self.get_study_date(first_instance)?,
            study_description: self.get_study_description(first_instance)?,
            modality: first_series.modality.clone(),
            series,
        };

        info!("Successfully loaded study with {} series", study.series.len());
        Ok(study)
    }

    async fn parse_dicom_file(&self, path: &Path) -> Result<Instance> {
        debug!("Parsing DICOM file: {:?}", path);
        
        // Parse the DICOM file
        let obj = open_file(path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;

        // Extract basic DICOM tags
        let sop_instance_uid = obj
            .element_by_name("SOPInstanceUID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .unwrap_or(std::borrow::Cow::Borrowed("Unknown"))
            .to_string();

        let instance_number = obj
            .element_by_name("InstanceNumber")
            .ok()
            .and_then(|e| e.to_int().ok())
            .map(|v: i32| v as u32);

        // Extract image dimensions
        let rows = obj
            .element_by_name("Rows")
            .ok()
            .and_then(|e| e.to_int().ok())
            .unwrap_or(512) as u32;

        let cols = obj
            .element_by_name("Columns")
            .ok()
            .and_then(|e| e.to_int().ok())
            .unwrap_or(512) as u32;

        // Extract pixel spacing
        let pixel_spacing = obj
            .element_by_name("PixelSpacing")
            .ok()
            .and_then(|e| e.to_multi_float32().ok())
            .and_then(|v| {
                if v.len() >= 2 {
                    Some((v[0] as f64, v[1] as f64))
                } else {
                    None
                }
            })
            .unwrap_or((1.0, 1.0));

        // Extract slice thickness
        let slice_thickness = obj
            .element_by_name("SliceThickness")
            .ok()
            .and_then(|e| e.to_float32().ok())
            .unwrap_or(1.0) as f64;

        // Extract window center and width
        let window_center = obj
            .element_by_name("WindowCenter")
            .ok()
            .and_then(|e| e.to_float32().ok())
            .unwrap_or(0.0) as f64;

        let window_width = obj
            .element_by_name("WindowWidth")
            .ok()
            .and_then(|e| e.to_float32().ok())
            .unwrap_or(1000.0) as f64;

        // Try to extract and decode pixel data
        let pixel_data = if let Ok(pixel_element) = obj.element_by_name("PixelData") {
            // Try to decode the actual pixel data
            if let Ok(pixel_bytes) = pixel_element.to_bytes() {
                // For now, create a simple grayscale representation
                // In a full implementation, you would handle different pixel formats
                let mut decoded_pixels = Vec::new();
                let bytes_per_pixel = 2; // Assuming 16-bit grayscale
                
                for i in (0..pixel_bytes.len()).step_by(bytes_per_pixel) {
                    if i + 1 < pixel_bytes.len() {
                        let pixel_value = ((pixel_bytes[i] as u16) << 8) | (pixel_bytes[i + 1] as u16);
                        decoded_pixels.push(pixel_value);
                    }
                }
                
                // Ensure we have the right number of pixels
                if decoded_pixels.len() >= (rows * cols) as usize {
                    decoded_pixels.truncate((rows * cols) as usize);
                } else {
                    // Pad with zeros if we don't have enough data
                    decoded_pixels.resize((rows * cols) as usize, 0);
                }
                
                decoded_pixels
            } else {
                // Fallback to placeholder data
                vec![0u16; (rows * cols) as usize]
            }
        } else {
            // No pixel data found, create placeholder
            vec![0u16; (rows * cols) as usize]
        };

        let image_data = Some(ImageData {
            width: cols,
            height: rows,
            depth: 1,
            pixel_data,
            pixel_spacing,
            slice_thickness,
            window_center,
            window_width,
        });

        Ok(Instance {
            id: Uuid::new_v4(),
            instance_number,
            sop_instance_uid,
            file_path: path.to_string_lossy().to_string(),
            image_data,
        })
    }


    fn is_dicom_file(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension() {
            let ext = extension.to_string_lossy().to_lowercase();
            ext == "dcm" || ext == "dicom" || ext == "ima"
        } else {
            // Check if file starts with DICOM preamble
            if let Ok(mut file) = std::fs::File::open(path) {
                let mut buffer = [0u8; 132];
                if let Ok(_) = std::io::Read::read(&mut file, &mut buffer) {
                    // Check for DICOM magic number "DICM" at offset 128
                    &buffer[128..132] == b"DICM"
                } else {
                    false
                }
            } else {
                false
            }
        }
    }


    fn get_series_instance_uid(&self, instance: &Instance) -> Result<String> {
        // Parse the file to get series UID
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        obj.element_by_name("SeriesInstanceUID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| DicomError::InvalidFile("SeriesInstanceUID not found".to_string()))
    }

    fn get_series_description(&self, instance: &Instance) -> Result<Option<String>> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("SeriesDescription")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()))
    }

    fn get_modality(&self, instance: &Instance) -> Result<String> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("Modality")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string()))
    }

    fn get_series_number(&self, instance: &Instance) -> Result<Option<u32>> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("SeriesNumber")
            .ok()
            .and_then(|e| e.to_int().ok())
            .map(|v: i32| v as u32))
    }

    fn get_patient_id(&self, instance: &Instance) -> Result<Option<String>> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("PatientID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()))
    }

    fn get_study_date(&self, instance: &Instance) -> Result<Option<String>> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("StudyDate")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()))
    }

    fn get_study_description(&self, instance: &Instance) -> Result<Option<String>> {
        let obj = open_file(&instance.file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        Ok(obj.element_by_name("StudyDescription")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()))
    }
}
