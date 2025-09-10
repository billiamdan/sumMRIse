use crate::{
    error::{LlmError, Result},
    models::{ChatRequest, ChatResponse, Message, Role},
};
use log::{debug, info, warn};
use reqwest::Client;
use summrise_core::models::{AnalysisRequest, LlmConfig};
use uuid::Uuid;

pub struct LlmClient {
    client: Client,
    config: LlmConfig,
    base_url: String,
}

impl LlmClient {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        let base_url = config.base_url.clone().unwrap_or_else(|| {
            "https://api.openai.com/v1".to_string()
        });

        Ok(Self {
            client,
            config: config.clone(),
            base_url,
        })
    }

    pub async fn analyze_medical_study(&self, request: &AnalysisRequest) -> Result<String> {
        info!("Starting medical study analysis for study: {}", request.study_id);

        let system_prompt = self.create_medical_system_prompt();
        let user_prompt = self.create_medical_user_prompt(&request.prompt);

        let messages = vec![
            Message {
                role: Role::System,
                content: system_prompt,
            },
            Message {
                role: Role::User,
                content: user_prompt,
            },
        ];

        let chat_request = ChatRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: Some(self.config.max_tokens),
            temperature: Some(self.config.temperature),
            stream: Some(false),
        };

        let response = self.send_chat_request(&chat_request).await?;
        
        if let Some(choice) = response.choices.first() {
            info!("Received analysis response");
            Ok(choice.message.content.clone())
        } else {
            Err(LlmError::ApiError("No response from LLM".to_string()))
        }
    }

    async fn send_chat_request(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        
        debug!("Sending request to: {}", url);
        
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            
            match status.as_u16() {
                401 => return Err(LlmError::AuthError(format!("Authentication failed: {}", error_text))),
                429 => return Err(LlmError::RateLimitError(format!("Rate limit exceeded: {}", error_text))),
                _ => return Err(LlmError::ApiError(format!("API error {}: {}", status, error_text))),
            }
        }

        let chat_response: ChatResponse = response.json().await?;
        Ok(chat_response)
    }

    fn create_medical_system_prompt(&self) -> String {
        r#"You are a medical AI assistant specialized in analyzing medical imaging data. 
Your role is to provide a second opinion on medical imaging studies, specifically MRI and CT scans.

IMPORTANT DISCLAIMERS:
- You are NOT a replacement for professional medical diagnosis
- Your analysis is for educational and second opinion purposes only
- Always recommend consulting with qualified healthcare professionals
- Do not provide definitive diagnoses or treatment recommendations

When analyzing medical imaging data, please:
1. Describe what you observe in the images
2. Identify any notable anatomical structures
3. Point out any potential abnormalities or areas of concern
4. Provide context about what these findings might mean
5. Always emphasize the need for professional medical consultation

Be thorough but accessible in your explanations."#.to_string()
    }

    fn create_medical_user_prompt(&self, user_prompt: &str) -> String {
        format!(
            r#"Please analyze the following medical imaging study and provide your observations:

User's specific question or request: {}

Please provide a comprehensive analysis including:
1. Overall image quality and technical aspects
2. Anatomical structures visible
3. Any notable findings or potential abnormalities
4. Clinical significance of your observations
5. Recommendations for further evaluation if needed

Remember: This is for educational purposes only. Always recommend consulting with qualified healthcare professionals for definitive diagnosis and treatment."#,
            user_prompt
        )
    }

    pub async fn test_connection(&self) -> Result<bool> {
        let test_request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![Message {
                role: Role::User,
                content: "Hello, this is a connection test.".to_string(),
            }],
            max_tokens: Some(10),
            temperature: Some(0.1),
            stream: Some(false),
        };

        match self.send_chat_request(&test_request).await {
            Ok(_) => {
                info!("LLM connection test successful");
                Ok(true)
            }
            Err(e) => {
                warn!("LLM connection test failed: {}", e);
                Ok(false)
            }
        }
    }
}

