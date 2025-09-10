use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicalStudy {
    pub id: Uuid,
    pub patient_id: Option<String>,
    pub study_date: Option<String>,
    pub study_description: Option<String>,
    pub modality: String,
    pub series: Vec<Series>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: Uuid,
    pub series_number: Option<u32>,
    pub series_description: Option<String>,
    pub modality: String,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: Uuid,
    pub instance_number: Option<u32>,
    pub sop_instance_uid: String,
    pub file_path: String,
    pub image_data: Option<ImageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub pixel_data: Vec<u16>,
    pub pixel_spacing: (f64, f64),
    pub slice_thickness: f64,
    pub window_center: f64,
    pub window_width: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportSettings {
    pub window_center: f64,
    pub window_width: f64,
    pub zoom: f64,
    pub pan: (f64, f64),
    pub rotation: (f64, f64, f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub study_id: Uuid,
    pub prompt: String,
    pub include_images: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResponse {
    pub id: Uuid,
    pub request_id: Uuid,
    pub response: String,
    pub confidence: Option<f64>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

