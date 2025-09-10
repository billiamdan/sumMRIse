use thiserror::Error;

#[derive(Error, Debug)]
pub enum SumMRIseError {
    #[error("Configuration error: {0}")]
    ConfigError(#[from] config::ConfigError),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("Generic error: {0}")]
    Generic(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SumMRIseError>;
