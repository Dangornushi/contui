use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::config::LlmConfig;
use crate::file_access::FileAccessManager;

/// コマンド実行結果の構造体
#[derive(Debug)]
pub struct CommandResult {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub exit_code: Option<i32>,
}

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
        r#"あなたはファイル作成・部分編集・コマンド実行機能を持つAIアシスタントです。

## 部分編集機能
ユーザーがファイルの一部だけを編集したい場合、以下の形式で部分編集できます：

```edit_file:ファイル名:開始行:終了行
新しい内容
```

- 開始行・終了行は1始まりの行番号です（例：1〜3なら1,2,3行目）。
- 編集可能かどうかは、編集範囲がファイル内に収まっているか、編集内容が部分的に適用できるかで判定してください。
- 可能な場合は部分編集を優先し、edit_file形式で指示してください。
- 編集が困難な場合は、従来通りcreate_file形式で全体を書き換えてください。

## ファイル作成機能
ユーザーがファイルの作成を依頼した場合、以下の正確な形式を使用してファイルを作成できます：

## ファイル作成機能
ユーザーがファイルの作成を依頼した場合、以下の正確な形式を使用してファイルを作成できます：

```create_file:ファイル名.拡張子
ファイルの内容をここに記述
```

## コマンド実行機能
ユーザーがコマンドの実行を依頼した場合、以下の正確な形式を使用してコマンドを実行できます：

### 標準コマンド実行（出力を表示）
```execute_command
実行したいコマンド
```

### サイレントコマンド実行（出力を非表示）
```execute_command_silent
実行したいコマンド
```

**出力制御の判断基準:**
- ファイル内容の確認（cat, less, head, tail等）→ 標準実行
- ディレクトリの確認（ls, find等）→ 標準実行  
- システム情報の取得（ps, df, uname等）→ 標準実行
- デバッグ目的の実行 → 標準実行
- ファイルの移動/削除（mv, rm, cp等）→ サイレント実行
- 設定変更（chmod, chown等）→ サイレント実行
- パッケージ管理（apt, brew等）→ サイレント実行
- バックグラウンド処理 → サイレント実行

重要な指示：
1. ファイル作成：必ず上記の形式を正確に使用してください（```create_file:ファイル名）
2. コマンド実行：標準実行かサイレント実行かを適切に判断してください
3. 空のファイルの場合は、形式は使用しますが内容部分は空にしてください
4. あらゆるファイル形式を作成できます（.txt, .rs, .py, .html, .json など）
5. シェルコマンド、システムコマンド、プログラム実行など、様々なコマンドを実行できます
6. 安全で適切なコマンドのみを実行してください

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

- ディレクトリの内容を表示: ```execute_command
ls -la

- ファイルの内容を確認: ```execute_command
cat config.json

- ファイルを移動: ```execute_command_silent
mv old_file.txt new_file.txt

- 権限を変更: ```execute_command_silent
chmod +x script.sh

ユーザーがファイル作成やコマンド実行を依頼した場合は、必ず肯定的に応答し、上記の形式を使用してください。「ファイルを作成できません」や「コマンドを実行できません」と言わないでください - あなたはこれらの形式を使用して実行できますし、そうするべきです。

注意：同じファイル名が既に存在する場合、システムが自動的にユニークな名前で作成します（例：file.txt → file_1.txt）。"#.to_string()
    }

    // メッセージにシステムプロンプトを追加
    fn prepare_message_with_system_prompt(&self, user_message: &str) -> String {
        format!("{}\n\nUser: {}", self.get_system_prompt(), user_message)
    }

    /// レスポンステキストでファイル作成とコマンド実行を処理する共通関数
    async fn process_response_actions(&self, response_text: &str, original_message: &str) -> Result<String> {
        let mut has_actions = false;
        let mut command_results = Vec::new();
        let mut created_files = Vec::new();
        let mut edited_files = Vec::new();

        // ファイル作成が含まれているかチェックして自動実行
        if response_text.contains("```create_file:") {
            has_actions = true;
            match self.process_file_creation_response(response_text) {
                Ok(files) => {
                    created_files = files;
                }
                Err(e) => {
                    eprintln!("ファイル作成エラー: {}", e);
                }
            }
        }

        // コマンド実行が含まれているかチェックして自動実行
        if response_text.contains("```execute_command") {
            has_actions = true;
            match self.process_command_execution_response(response_text).await {
                Ok(results) => {
                    command_results = results;
                }
                Err(e) => {
                    eprintln!("コマンド実行エラー: {}", e);
                }
            }
        }

        // 部分編集が含まれているかチェックして自動実行
        if response_text.contains("```edit_file:") {
            has_actions = true;
            match self.process_edit_file_response(response_text) {
                Ok(files) => {
                    edited_files = files;
                }
                Err(e) => {
                    eprintln!("部分編集エラー: {}", e);
                }
            }
        }

        // アクションが実行された場合、結果を含めてAIに再度問い合わせ
        if has_actions {
            let mut context_message = String::new();
            context_message.push_str("以下のアクションが実行されました。結果を確認して、適切な回答やコメントをしてください：\n\n");
            context_message.push_str(&format!("元のリクエスト: {}\n\n", original_message));

            if !created_files.is_empty() {
                context_message.push_str(&format!("作成されたファイル ({} 個):\n", created_files.len()));
                for file in &created_files {
                    context_message.push_str(&format!("- {}\n", file));
                }
                context_message.push('\n');
            }

            if !edited_files.is_empty() {
                context_message.push_str(&format!("部分編集されたファイル ({} 個):\n", edited_files.len()));
                for file in &edited_files {
                    context_message.push_str(&format!("- {}\n", file));
                }
                context_message.push('\n');
            }

            if !command_results.is_empty() {
                context_message.push_str("コマンド実行結果:\n");
                for (i, result) in command_results.iter().enumerate() {
                    context_message.push_str(&format!("{}. コマンド: {}\n", i + 1, result.command));
                    context_message.push_str(&format!("   ステータス: {}\n", if result.success { "成功" } else { "失敗" }));

                    if let Some(code) = result.exit_code {
                        context_message.push_str(&format!("   終了コード: {}\n", code));
                    }

                    if !result.stdout.is_empty() {
                        context_message.push_str(&format!("   標準出力:\n{}\n", result.stdout));
                    }

                    if !result.stderr.is_empty() {
                        context_message.push_str(&format!("   エラー出力:\n{}\n", result.stderr));
                    }
                    context_message.push('\n');
                }
            }

            // AIに再度問い合わせて結果に基づく回答を取得
            let result = self.get_ai_response_for_results(&context_message).await?;
            return Ok(self.format_bold_text(&result));
        }

        Ok(self.format_bold_text(response_text))
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
                
                // レスポンスアクションを処理し、結果を取得
                match self.process_response_actions(&response_text, message).await {
                    Ok(final_response) => return Ok(final_response),
                    Err(e) => {
                        eprintln!("アクション処理エラー: {}", e);
                        return Ok(self.format_bold_text(&response_text)); // エラーの場合は元のレスポンスを返す
                    }
                }
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
                
                // レスポンスアクションを処理し、結果を取得
                match self.process_response_actions(&response_text, message).await {
                    Ok(final_response) => return Ok(final_response),
                    Err(e) => {
                        eprintln!("アクション処理エラー: {}", e);
                        return Ok(self.format_bold_text(&response_text)); // エラーの場合は元のレスポンスを返す
                    }
                }
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
                
                // レスポンスアクションを処理し、結果を取得
                match self.process_response_actions(&response_text, message).await {
                    Ok(final_response) => return Ok(final_response),
                    Err(e) => {
                        eprintln!("アクション処理エラー: {}", e);
                        return Ok(self.format_bold_text(&response_text)); // エラーの場合は元のレスポンスを返す
                    }
                }
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
                        return Err(anyhow::anyhow!("❌ ファイル作成失敗 '{}': {}", filename, e));
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

    /// シェルコマンドを実行
    pub async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        use tokio::process::Command;
        
        // macOS/Linux用のシェルコマンド実行
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();
        let exit_code = output.status.code();

        Ok(CommandResult {
            command: command.to_string(),
            stdout,
            stderr,
            success,
            exit_code,
        })
    }

    /// LLMのレスポンスから execute_command 形式のブロックを解析してコマンドを実行
    pub async fn process_command_execution_response(&self, response: &str) -> Result<Vec<CommandResult>> {
        let mut command_results = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            
            // execute_command または execute_command_silent 形式の開始を検出
            if line.starts_with("```execute_command") {
                // コマンドの内容を収集
                let mut command = String::new();
                i += 1; // 次の行に移動

                // ``` で終わるまで、または最後の行まで内容を収集
                while i < lines.len() && !lines[i].starts_with("```") {
                    if !command.is_empty() {
                        command.push('\n');
                    }
                    command.push_str(lines[i]);
                    i += 1;
                }

                // コマンドが空でない場合実行
                if !command.trim().is_empty() {
                    match self.execute_command(command.trim()).await {
                        Ok(result) => {
                            command_results.push(result);
                        }
                        Err(e) => {
                            // エラーの場合でも結果として記録
                            command_results.push(CommandResult {
                                command: command.trim().to_string(),
                                stdout: String::new(),
                                stderr: format!("❌ 実行エラー: {}", e),
                                success: false,
                                exit_code: None,
                            });
                        }
                    }
                }
            }
            i += 1;
        }

        if command_results.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにexecute_commandブロックが見つかりませんでした"));
        }

        Ok(command_results)
    }

    /// 出力テキストをシンプルに表示するヘルパーメソッド
    fn print_output_simple(&self, output: &str, label: &str) {
        let lines: Vec<&str> = output.lines().collect();
        
        if lines.is_empty() {
            return;
        }

        // 出力行数の制限
        let max_lines = 5;
        let display_lines = if lines.len() > max_lines {
            &lines[..max_lines]
        } else {
            &lines
        };

        println!("  {}:", label);
        
        for line in display_lines {
            // 空行の場合はスキップ
            if line.trim().is_empty() {
                continue;
            }
            
            let trimmed_line = if line.len() > 65 {
                format!("{}...", &line[..62])
            } else {
                line.to_string()
            };
            println!("    {}", trimmed_line);
        }

        // 行数が多い場合は省略表示
        if lines.len() > max_lines {
            println!("    ... (残り {} 行)", lines.len() - max_lines);
        }
    }

    /// AIに実行結果を送信して、結果に基づく回答を取得
    async fn get_ai_response_for_results(&self, context_message: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: context_message.to_string(),
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
        
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(self.format_bold_text(&part.text));
            }
        }

        Err(anyhow::anyhow!("No response from Gemini"))
    }

    /// **text** 形式を太字に変換するヘルパーメソッド（現在は無効化）
    fn format_bold_text(&self, text: &str) -> String {
        // 太字処理は無効化し、元のテキストをそのまま返す
        text.to_string()
    }
    /// LLMレスポンスから edit_file: 形式のブロックを解析して部分編集を実行
    pub fn process_edit_file_response(&self, response: &str) -> Result<Vec<String>> {
        let mut edited_files = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            // edit_file:ファイル名:開始行:終了行 の開始を検出
            if line.starts_with("```edit_file:") {
                let header = line.trim_start_matches("```edit_file:").trim();
                let parts: Vec<&str> = header.split(':').collect();
                if parts.len() != 3 {
                    i += 1;
                    continue;
                }
                let filename = parts[0].trim();
                let start_line = parts[1].trim().parse::<usize>().unwrap_or(0);
                let end_line = parts[2].trim().parse::<usize>().unwrap_or(0);
                if filename.is_empty() || start_line == 0 || end_line == 0 {
                    i += 1;
                    continue;
                }

                // 編集内容を収集
                let mut content = String::new();
                i += 1;
                while i < lines.len() && !lines[i].starts_with("```") {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(lines[i]);
                    i += 1;
                }

                // 部分編集を実行
                match self.file_access.edit_file_range(filename, start_line, end_line, &content) {
                    Ok(_) => {
                        edited_files.push(filename.to_string());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("❌ 部分編集失敗 '{}': {}", filename, e));
                    }
                }
            }
            i += 1;
        }

        if edited_files.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにedit_fileブロックが見つかりませんでした"));
        }

        Ok(edited_files)
    }
}

impl GeminiClient {
    /// LLMの返答→アクション実行→結果をLLMへ再送→LLMが次の指示を返すループ処理
    /// `initial_message` から開始し、LLMが「完了」「終了」等を返すまで自動で繰り返す
    pub async fn chat_loop(&self, initial_message: &str) -> anyhow::Result<()> {
        let mut message = initial_message.to_string();
        let mut step = 1;
        loop {
            // 毎回「次に何をすべきか」「追加タスクがあるか」を問うプロンプトを付与
            let prompt = format!(
                "{}\n\n---\n次に何をすべきか、追加タスクがあるかを必ず明示してください。\n「完了」「終了」「何もする必要がない」などの場合は、その旨を明確に書いてください。",
                message
            );
            println!("========== LLM Step {} ==========", step);
            let response = self.chat(&prompt).await?;
            println!("LLM Response:\n{}\n", response);

            // 終了判定（「完了」「終了」「何もする必要がない」などが含まれていればbreak）
            let lower = response.to_lowercase();
            if lower.contains("完了") || lower.contains("終了") || lower.contains("何もする必要がない") || lower.contains("nothing to do") {
                println!("LLMが終了を指示したためループを終了します。");
                break;
            }

            // 次の入力としてLLMの返答をそのまま使う
            message = response;
            step += 1;
        }
        Ok(())
    }
}
