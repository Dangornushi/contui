use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Config {
    pub llm: LlmConfig,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub gemini_api_key: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        dotenv::dotenv().ok();

        let model = std::env::var("MODEL")?;
        let gemini_api_key = std::env::var("GEMINI_API_KEY")?;
        let max_tokens = std::env::var("MAX_TOKENS").ok().and_then(|v| v.parse().ok());
        let temperature = std::env::var("TEMPERATURE").ok().and_then(|v| v.parse().ok());

        Ok(Config {
            llm: LlmConfig {
                model,
                max_tokens,
                temperature,
                gemini_api_key,
            },
        })
    }
}
