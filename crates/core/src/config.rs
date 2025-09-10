use anyhow::Result;
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub window: WindowConfig,
    pub rendering: RenderingConfig,
    pub llm: LlmConfig,
    pub dicom: DicomConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: i32,
    pub height: i32,
    pub fullscreen: bool,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderingConfig {
    pub vsync: bool,
    pub msaa_samples: u32,
    pub max_texture_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub default_model: String,
    pub max_tokens: u32,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DicomConfig {
    pub auto_load: bool,
    pub cache_size_mb: u64,
    pub supported_formats: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig {
                width: 1200,
                height: 800,
                fullscreen: false,
                theme: "dark".to_string(),
            },
            rendering: RenderingConfig {
                vsync: true,
                msaa_samples: 4,
                max_texture_size: 4096,
            },
            llm: LlmConfig {
                default_model: "gpt-4".to_string(),
                max_tokens: 2000,
                temperature: 0.7,
            },
            dicom: DicomConfig {
                auto_load: true,
                cache_size_mb: 512,
                supported_formats: vec!["dcm".to_string(), "dicom".to_string()],
            },
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_file = config_dir.join("config.toml");

        let builder = Config::builder()
            .add_source(File::from(config_file).required(false))
            .add_source(Environment::with_prefix("SUMMRISE"));

        Ok(builder.build()?.try_deserialize()?)
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        std::fs::create_dir_all(&config_dir)?;
        
        let config_file = config_dir.join("config.toml");
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(config_file, toml)?;
        
        Ok(())
    }

    fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("summrise");
        Ok(dir)
    }
}
