use crate::{
    config::AppConfig,
    error::Result,
    models::{AnalysisRequest, AnalysisResponse, LlmConfig, MedicalStudy},
};
use log::info;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Default)]
pub struct SumMRIseApp {
    pub config: AppConfig,
    pub studies: HashMap<Uuid, MedicalStudy>,
    pub llm_config: Option<LlmConfig>,
}

impl SumMRIseApp {
    pub async fn new() -> Result<Self> {
        info!("Initializing SumMRIse application");
        
        let config = AppConfig::load().unwrap_or_else(|_| {
            info!("Using default configuration");
            AppConfig::default()
        });

        Ok(Self {
            config,
            studies: HashMap::new(),
            llm_config: None,
        })
    }

    pub async fn load_study_from_folder(&mut self, folder_path: &str) -> Result<Uuid> {
        info!("Loading study from folder: {}", folder_path);
        
        // Placeholder implementation - will be implemented in the DICOM crate
        let study = MedicalStudy {
            id: Uuid::new_v4(),
            patient_id: Some("Unknown".to_string()),
            study_date: Some("2025-01-01".to_string()),
            study_description: Some("Medical Study".to_string()),
            modality: "CT".to_string(),
            series: vec![],
        };
        let study_id = study.id;
        
        self.studies.insert(study_id, study);
        info!("Loaded study with ID: {}", study_id);
        
        Ok(study_id)
    }

    pub fn get_study(&self, study_id: &Uuid) -> Option<&MedicalStudy> {
        self.studies.get(study_id)
    }

    pub fn set_llm_config(&mut self, config: LlmConfig) {
        self.llm_config = Some(config);
    }

    pub async fn analyze_study(&self, request: AnalysisRequest) -> Result<AnalysisResponse> {
        // Placeholder implementation - will be implemented in the LLM crate
        Ok(AnalysisResponse {
            id: Uuid::new_v4(),
            request_id: request.study_id,
            response: "Analysis not yet implemented".to_string(),
            confidence: None,
            timestamp: chrono::Utc::now(),
        })
    }

    pub fn save_config(&self) -> Result<()> {
        Ok(self.config.save()?)
    }
}
