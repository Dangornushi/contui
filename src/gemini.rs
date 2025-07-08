use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::config::LlmConfig;
use crate::file_access::FileAccessManager;

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
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
    file_access: FileAccessManager,
}

impl GeminiClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            file_access: FileAccessManager::new(),
        }
    }

    pub fn add_allowed_directory<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        self.file_access.add_allowed_directory(path)
    }

    // システムプロンプトを作成
    fn get_system_prompt(&self) -> String {
        r#"あなたはファイル作成機能を持つAIアシスタントです。ユーザーがファイルの作成を依頼した場合、以下の正確な形式を使用してファイルを作成できます：

```create_file:ファイル名.拡張子
ファイルの内容をここに記述
```

重要な指示：
1. 必ず上記の形式を正確に使用してください（```create_file:ファイル名）
2. 空のファイルの場合は、形式は使用しますが内容部分は空にしてください
3. ユーザーがファイル作成を依頼した場合は、必ずこの形式を使用してください
4. あらゆるファイル形式を作成できます（.txt, .rs, .py, .html, .json など）
5. 空のファイルも作成できます
6. ユーザーが要求するあらゆる内容のファイルを作成できます

例：
- 空のテキストファイル: ```create_file:test.txt

- Rustファイル: ```create_file:main.rs
fn main() {
    println!("Hello, world!");
}

- JSONファイル: ```create_file:config.json
{
  "name": "example",
  "version": "1.0.0"
}

ユーザーがファイル作成を依頼した場合は、必ず肯定的に応答し、上記の形式を使用してください。「ファイルを作成できません」と言わないでください - あなたはこの形式を使用してファイルを作成できますし、そうするべきです。

注意：同じファイル名が既に存在する場合、システムが自動的にユニークな名前で作成します（例：file.txt → file_1.txt）。"#.to_string()
    }

    // メッセージにシステムプロンプトを追加
    fn prepare_message_with_system_prompt(&self, user_message: &str) -> String {
        format!("{}\n\nUser: {}", self.get_system_prompt(), user_message)
    }

    pub async fn chat(&self, message: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        // システムプロンプトを含むメッセージを準備
        let full_message = self.prepare_message_with_system_prompt(message);

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: full_message,
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
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
                let response_text = part.text.clone();
                
                // ファイル作成が含まれているかチェックして自動実行
                if response_text.contains("```create_file:") {
                    match self.process_file_creation_response(&response_text) {
                        Ok(created_files) => {
                            self.print_file_creation_summary(&created_files);
                        }
                        Err(e) => {
                            eprintln!("❌ ファイル作成に失敗: {}", e);
                        }
                    }
                }
                
                return Ok(response_text);
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
        
        // システムプロンプトを最初に追加
        conversation_text.push_str(&self.get_system_prompt());
        conversation_text.push_str("\n\n");
        
        if !context.is_empty() {
            conversation_text.push_str("Previous conversation:\n");
            for ctx in context {
                conversation_text.push_str(ctx);
                conversation_text.push('\n');
            }
            conversation_text.push_str("\nCurrent message:\n");
        }
        
        conversation_text.push_str("User: ");
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
                let response_text = part.text.clone();
                
                // ファイル作成が含まれているかチェックして自動実行
                if response_text.contains("```create_file:") {
                    match self.process_file_creation_response(&response_text) {
                        Ok(created_files) => {
                            self.print_file_creation_summary(&created_files);
                        }
                        Err(e) => {
                            eprintln!("❌ ファイル作成に失敗: {}", e);
                        }
                    }
                }
                
                return Ok(response_text);
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    pub async fn chat_with_file_context(&self, message: &str, file_paths: &[String], context: &[String]) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        // ファイル内容を読み取り
        let mut file_contents = String::new();
        for file_path in file_paths {
            match self.file_access.read_file(file_path) {
                Ok(content) => {
                    file_contents.push_str(&format!("\n--- File: {} ---\n", file_path));
                    file_contents.push_str(&content);
                    file_contents.push_str("\n--- End of file ---\n\n");
                }
                Err(e) => {
                    eprintln!("Failed to read file {}: {}", file_path, e);
                    file_contents.push_str(&format!("\n--- Error reading file: {} ---\n", file_path));
                    file_contents.push_str(&format!("Error: {}\n\n", e));
                }
            }
        }

        // 会話テキストを構築
        let mut conversation_text = String::new();
        
        // システムプロンプトを最初に追加
        conversation_text.push_str(&self.get_system_prompt());
        conversation_text.push_str("\n\n");
        
        if !file_contents.is_empty() {
            conversation_text.push_str("=== FILE CONTENTS ===\n");
            conversation_text.push_str(&file_contents);
            conversation_text.push_str("=== END FILE CONTENTS ===\n\n");
        }

        if !context.is_empty() {
            conversation_text.push_str("Previous conversation:\n");
            for ctx in context {
                conversation_text.push_str(ctx);
                conversation_text.push('\n');
            }
            conversation_text.push_str("\nCurrent message:\n");
        }
        
        conversation_text.push_str("User: ");
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
                let response_text = part.text.clone();
                
                // ファイル作成が含まれているかチェックして自動実行
                if response_text.contains("```create_file:") {
                    match self.process_file_creation_response(&response_text) {
                        Ok(created_files) => {
                            self.print_file_creation_summary(&created_files);
                        }
                        Err(e) => {
                            eprintln!("❌ ファイル作成に失敗: {}", e);
                        }
                    }
                }
                
                return Ok(response_text);
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<String>> {
        self.file_access.list_directory(path)
    }

    /// ファイルを作成（重複チェック付き）- 実際に作成されたファイル名を返す
    pub fn create_file_with_unique_name(&self, path: &str, content: &str) -> Result<String> {
        let created_path = self.file_access.create_file_with_unique_name(path, content)?;
        Ok(created_path.to_string_lossy().to_string())
    }

    /// ファイル作成結果を見やすく表示するヘルパーメソッド
    fn print_file_creation_summary(&self, created_files: &[String]) {
        if created_files.is_empty() {
            return;
        }

        println!("📁 ファイル作成完了 ({} 個)", created_files.len());
        println!("┌─────────────────────────────────────────────────");
        
        for (i, file_path) in created_files.iter().enumerate() {
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);
            
            let dir = std::path::Path::new(file_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            
            if i == created_files.len() - 1 {
                println!("└── ✅ {}", file_name);
                if !dir.is_empty() && dir != "." {
                    println!("    📂 {}", dir);
                }
            } else {
                println!("├── ✅ {}", file_name);
                if !dir.is_empty() && dir != "." {
                    println!("│   📂 {}", dir);
                }
            }
        }
        println!();
    }

    /// LLMのレスポンスから create_file: 形式のブロックを解析してファイルを作成
    pub fn process_file_creation_response(&self, response: &str) -> Result<Vec<String>> {
        let mut created_files = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            
            // create_file: 形式の開始を検出
            if line.starts_with("```create_file:") {
                // ファイル名を抽出
                let filename = line.trim_start_matches("```create_file:").trim();
                if filename.is_empty() {
                    i += 1;
                    continue;
                }

                // ファイルの内容を収集
                let mut content = String::new();
                i += 1; // 次の行に移動

                // ``` で終わるまで、または最後の行まで内容を収集
                while i < lines.len() && !lines[i].starts_with("```") {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(lines[i]);
                    i += 1;
                }

                // ファイルを作成
                match self.create_file_with_unique_name(filename, &content) {
                    Ok(created_path) => {
                        created_files.push(created_path);
                    }
                    Err(e) => {
                        eprintln!("❌ ファイル作成失敗 '{}': {}", filename, e);
                        return Err(anyhow::anyhow!("ファイル作成失敗 '{}': {}", filename, e));
                    }
                }
            }
            i += 1;
        }

        if created_files.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにcreate_fileブロックが見つかりませんでした"));
        }

        Ok(created_files)
    }
}
