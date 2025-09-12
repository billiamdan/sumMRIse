use crate::error::{DicomError, Result};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::Path;
use summrise_core::models::{ImageData, Instance, MedicalStudy, Series};
use uuid::Uuid;
use walkdir::WalkDir;
use dicom::object::open_file;

#[derive(Debug, Clone)]
pub struct DicomInvestigation {
    pub study_uid: String,
    pub study_info: StudyInfo,
    pub file_paths: Vec<String>,
    pub directory_path: String,
}

#[derive(Debug, Clone)]
pub struct StudyInfo {
    pub patient_id: Option<String>,
    pub study_date: Option<String>,
    pub study_description: Option<String>,
    pub modality: String,
    pub file_count: usize,
}

pub struct DicomParser;

impl DicomParser {
    pub fn new() -> Self {
        Self
    }

    pub async fn find_dicom_investigations(&self, folder_path: &str) -> Result<Vec<DicomInvestigation>> {
        info!("Searching for DICOM investigations in folder: {}", folder_path);
        
        let mut investigations = Vec::new();
        
        // Group DICOM files by their containing directory
        // Each directory with DICOM files is considered one investigation
        let mut directory_map: HashMap<String, Vec<String>> = HashMap::new();
        
        for entry in WalkDir::new(folder_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if self.is_dicom_file(path) {
                // Skip DICOMDIR files - they're not actual image data
                if path.file_name().unwrap_or_default().to_string_lossy().to_uppercase() == "DICOMDIR" {
                    continue;
                }
                
                // Group by the main DICOM directory
                // Find the "Dicom" directory that contains the actual image files
                let mut current_dir = path.parent().unwrap_or(path);
                let mut dicom_dir = current_dir;
                
                // Walk up the directory tree to find the main DICOM directory
                // Stop when we find a directory that contains "Dicom" and has subdirectories
                while let Some(parent) = current_dir.parent() {
                    let parent_str = parent.to_string_lossy();
                    if parent_str.contains("Dicom") || parent_str.to_lowercase().contains("dicom") {
                        // Check if this directory has subdirectories (like PAT0, STUDY0, etc.)
                        if parent.read_dir().map(|mut dir| dir.next().is_some()).unwrap_or(false) {
                            dicom_dir = parent;
                            break;
                        }
                    }
                    current_dir = parent;
                }
                
                let directory_key = dicom_dir.to_string_lossy().to_string();
                let path_str = path.to_string_lossy().to_string();
                directory_map.entry(directory_key).or_insert_with(Vec::new).push(path_str);
            }
        }
        
        // Create investigation objects for each directory
        for (directory_path, file_paths) in directory_map {
            if let Some(first_file) = file_paths.first() {
                // Get study UID from the first file for identification
                let study_uid = self.get_study_instance_uid(std::path::Path::new(first_file)).await
                    .unwrap_or_else(|_| directory_path.clone());
                
                info!("Processing directory {} with {} files, first file: {}", directory_path, file_paths.len(), first_file);
                // Get basic study info from first file
                match self.get_study_info(first_file).await {
                    Ok(study_info) => {
                        investigations.push(DicomInvestigation {
                            study_uid: study_uid.clone(),
                            study_info,
                            file_paths,
                            directory_path: directory_path.clone(),
                        });
                        info!("Successfully created investigation for directory {}", directory_path);
                    }
                    Err(e) => {
                        warn!("Failed to get study info for {}: {}", first_file, e);
                    }
                }
            }
        }
        
        info!("Found {} DICOM investigations", investigations.len());
        Ok(investigations)
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
                        match self.get_series_instance_uid(&instance) {
                            Ok(series_uid) => {
                                series_map.entry(series_uid).or_insert_with(Vec::new).push(instance);
                            }
                            Err(e) => {
                                warn!("Failed to get series UID for file {:?}: {}", path, e);
                                // Use file path as fallback series identifier
                                let fallback_uid = path.to_string_lossy().to_string();
                                series_map.entry(fallback_uid).or_insert_with(Vec::new).push(instance);
                            }
                        }
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
        
        // Try to get SeriesInstanceUID, fallback to SOPInstanceUID if not found
        if let Some(series_uid) = obj.element_by_name("SeriesInstanceUID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()) {
            return Ok(series_uid);
        }
        
        // Fallback: use SOPInstanceUID as series identifier
        if let Some(sop_uid) = obj.element_by_name("SOPInstanceUID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string()) {
            return Ok(sop_uid);
        }
        
        // Last resort: use file path as identifier
        Ok(instance.file_path.clone())
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

    async fn get_study_instance_uid(&self, path: &Path) -> Result<String> {
        let obj = open_file(path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        obj.element_by_name("StudyInstanceUID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| DicomError::InvalidFile("StudyInstanceUID not found".to_string()))
    }

    async fn get_study_info(&self, file_path: &str) -> Result<StudyInfo> {
        let obj = open_file(file_path)
            .map_err(|e| DicomError::InvalidFile(format!("Failed to parse DICOM file: {}", e)))?;
        
        let patient_id = obj.element_by_name("PatientID")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string());
        
        let study_date = obj.element_by_name("StudyDate")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string());
        
        let study_description = obj.element_by_name("StudyDescription")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string());
        
        let modality = obj.element_by_name("Modality")
            .ok()
            .and_then(|e| e.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        Ok(StudyInfo {
            patient_id,
            study_date,
            study_description,
            modality,
            file_count: 0, // Will be set later
        })
    }

}
