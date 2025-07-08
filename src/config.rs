use serde::Deserialize;
use std::fs;
use anyhow::Result;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub gemini_api_key: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
