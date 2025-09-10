pub mod error;
pub mod client;
pub mod models;

pub use error::{LlmError, Result};
pub use client::LlmClient;
pub use models::{ChatRequest, ChatResponse, Message, Role};

use summrise_core::models::{AnalysisRequest, AnalysisResponse, LlmConfig};
use uuid::Uuid;

pub async fn analyze_study(
    config: &LlmConfig,
    request: &AnalysisRequest,
) -> Result<AnalysisResponse> {
    let client = LlmClient::new(config)?;
    let response = client.analyze_medical_study(request).await?;
    
    Ok(AnalysisResponse {
        id: Uuid::new_v4(),
        request_id: request.study_id,
        response,
        confidence: None,
        timestamp: chrono::Utc::now(),
    })
}

