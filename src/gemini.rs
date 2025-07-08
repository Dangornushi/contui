use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::config::LlmConfig;

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
    #[serde(rename = "tools", skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct Part {
    text: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Debug, Serialize)]
struct Tool {
    #[serde(rename = "googleSearchRetrieval")]
    google_search_retrieval: GoogleSearchRetrieval,
}

#[derive(Debug, Serialize)]
struct GoogleSearchRetrieval {
    #[serde(rename = "dynamicRetrievalConfig")]
    dynamic_retrieval_config: DynamicRetrievalConfig,
}

#[derive(Debug, Serialize)]
struct DynamicRetrievalConfig {
    mode: String,
    #[serde(rename = "dynamicThreshold")]
    dynamic_threshold: f32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: ResponseContent,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    text: String,
}

#[derive(Clone)]
pub struct GeminiClient {
    client: reqwest::Client,
    config: LlmConfig,
}

impl GeminiClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub async fn chat(&self, message: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: message.to_string(),
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: None,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        let response_text = response.text().await?;
        
        // デバッグ用のログ出力
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(part.text.clone());
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    pub async fn chat_with_context(&self, message: &str, context: &[String]) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        // コンテキストを含む会話履歴を構築
        let mut conversation_text = String::new();
        
        if !context.is_empty() {
            conversation_text.push_str("Previous conversation:\n");
            for ctx in context {
                conversation_text.push_str(ctx);
                conversation_text.push('\n');
            }
            conversation_text.push_str("\nCurrent message:\n");
        }
        
        conversation_text.push_str(message);

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: conversation_text,
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: None,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        let response_text = response.text().await?;
        
        // デバッグ用のログ出力
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(part.text.clone());
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    pub async fn chat_with_search(&self, message: &str) -> Result<String> {
        // 検索機能が無効の場合は通常のチャットを使用
        if !self.config.enable_search.unwrap_or(false) {
            return self.chat(message).await;
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: message.to_string(),
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: Some(vec![Tool {
                google_search_retrieval: GoogleSearchRetrieval {
                    dynamic_retrieval_config: DynamicRetrievalConfig {
                        mode: "MODE_DYNAMIC".to_string(),
                        dynamic_threshold: 0.7, // デフォルト閾値
                    },
                },
            }]),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        let response_text = response.text().await?;
        
        // 検索機能がサポートされていない場合のフォールバック
        if response_text.contains("Grounding is not supported") || 
           response_text.contains("grounding") ||
           response_text.contains("search") {
            eprintln!("Search not supported, falling back to regular chat");
            return self.chat(message).await;
        }
        
        // デバッグ用のログ出力
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(part.text.clone());
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    pub async fn chat_with_search_and_context(&self, message: &str, context: &[String]) -> Result<String> {
        // 検索機能が無効の場合は通常のチャットを使用
        if !self.config.enable_search.unwrap_or(false) {
            return self.chat_with_context(message, context).await;
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        // コンテキストを含む会話履歴を構築
        let mut conversation_text = String::new();
        
        if !context.is_empty() {
            conversation_text.push_str("Previous conversation:\n");
            for ctx in context {
                conversation_text.push_str(ctx);
                conversation_text.push('\n');
            }
            conversation_text.push_str("\nCurrent message:\n");
        }
        
        conversation_text.push_str(message);

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: conversation_text,
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: Some(vec![Tool {
                google_search_retrieval: GoogleSearchRetrieval {
                    dynamic_retrieval_config: DynamicRetrievalConfig {
                        mode: "MODE_DYNAMIC".to_string(),
                        dynamic_threshold: 0.7, // デフォルト閾値
                    },
                },
            }]),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        let response_text = response.text().await?;
        
        // 検索機能がサポートされていない場合のフォールバック
        if response_text.contains("Grounding is not supported") || 
           response_text.contains("grounding") ||
           response_text.contains("search") {
            eprintln!("Search not supported, falling back to regular chat with context");
            return self.chat_with_context(message, context).await;
        }
        
        // デバッグ用のログ出力
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(part.text.clone());
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }
}
