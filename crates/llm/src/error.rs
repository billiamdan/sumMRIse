use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("HTTP request error: {0}")]
    RequestError(#[from] reqwest::Error),
    
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("API error: {0}")]
    ApiError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Rate limit exceeded: {0}")]
    RateLimitError(String),
    
    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;

