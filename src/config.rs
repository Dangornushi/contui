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

        let model: String = std::env::var("MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());
        let gemini_api_key: String = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
        let max_tokens: Option<u32> = std::env::var("MAX_TOKENS").ok().and_then(|v| v.parse().ok()).or(Some(4096));
        let temperature: Option<f32> = std::env::var("TEMPERATURE").ok().and_then(|v| v.parse().ok()).or(Some(0.5));

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
