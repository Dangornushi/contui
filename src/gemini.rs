use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::config::LlmConfig;
use crate::file_access::FileAccessManager;
use crate::history::ChatMessage;
use std::io::Write;
use crate::debug_log;
use crate::history::HistoryManager;
use std::sync::{Arc, Mutex};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
}

#[derive(Debug, Serialize)]
struct Content {
    role: String,
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

// Tool definitions for function calling
#[derive(Debug, Serialize)]
struct Tool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
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
#[serde(untagged)]
enum ResponsePart {
    Text { text: String },
    FunctionCall { #[serde(rename = "functionCall")] function_call: FunctionCall },
}

#[derive(Debug, Deserialize)]
struct FunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Clone)]
pub struct GeminiClient {
    client: reqwest::Client,
    config: LlmConfig,
    file_access: FileAccessManager,
    history_manager: Arc<Mutex<HistoryManager>>, // Change type
}

impl GeminiClient {
    pub fn new(config: LlmConfig, history_manager: Arc<Mutex<HistoryManager>>) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            file_access: FileAccessManager::new(),
            history_manager,
        }
    }
        

    /// Google APIリクエスト共通化＋429時3秒リトライ
    async fn send_google_request_with_retry(
        &self,
        url: &str,
        request: &GeminiRequest,
    ) -> Result<String> {
        use tokio::time::{sleep, Duration};
        loop {
            // デバッグ: POST送信直前 (contui_debug.log)
            debug_log!("[send_google_request_with_retry] POST to: {}\n", url);
            debug_log!("[send_google_request_with_retry] Request Body: {} \n", serde_json::to_string_pretty(request).unwrap_or_else(|_| "Failed to serialize request".to_string()));
            // LLMリクエストJSONをcontui_llm_request.logに出力
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("contui_llm_request.log") {
                let _ = writeln!(file, "{}\n", serde_json::to_string_pretty(request).unwrap_or_else(|_| "Failed to serialize request".to_string()));
            }
            let resp = self.client
                .post(url)
                .json(request)
                .send()
                .await;
            // デバッグ: POST送信直後 (contui_debug.log)
            debug_log!("[send_google_request_with_retry] POST result: {:?}\n", resp.as_ref().map(|r| r.status()));
            match resp {
                Ok(response) => {
                    if response.status().as_u16() == 429 {
                        // 429: 3秒待ってリトライ
                        debug_log!("[send_google_request_with_retry] 429 received, sleeping 3s\n");
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                    // デバッグ: response.text().await直前
                    debug_log!("[send_google_request_with_retry] about to await response.text()\n");
                    let text = response.text().await?;
                    // デバッグ: response.text().await直後
                    debug_log!("[send_google_request_with_retry] response.text() done\n");
                    return Ok(text);
                }
                Err(e) => {
                    // 通信エラー時も3秒待ってリトライ
                    debug_log!("[send_google_request_with_retry] Err (retrying): {}\n", e);
                    sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
        }
    }

    pub fn add_allowed_directory<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        self.file_access.add_allowed_directory(path)
    }

    /// Function declarations for Gemini Function Calling
    fn get_function_declarations(&self) -> Vec<Tool> {
        vec![
            Tool {
                function_declarations: vec![
                    FunctionDeclaration {
                        name: "create_file".to_string(),
                        description: "ファイルを作成します".to_string(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "filename": {
                                    "type": "string",
                                    "description": "作成するファイル名"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "ファイルの内容"
                                }
                            },
                            "required": ["filename", "content"]
                        }),
                    },
                    FunctionDeclaration {
                        name: "edit_file".to_string(),
                        description: "ファイルの一部を編集します".to_string(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "filename": {
                                    "type": "string",
                                    "description": "編集するファイル名"
                                },
                                "start_line": {
                                    "type": "integer",
                                    "description": "編集開始行（1始まり）"
                                },
                                "end_line": {
                                    "type": "integer",
                                    "description": "編集終了行（1始まり）"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "新しい内容"
                                }
                            },
                            "required": ["filename", "start_line", "end_line", "content"]
                        }),
                    },
                    FunctionDeclaration {
                        name: "execute_command".to_string(),
                        description: "シェルコマンドを実行します".to_string(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "command": {
                                    "type": "string",
                                    "description": "実行するコマンド"
                                },
                                "silent": {
                                    "type": "boolean",
                                    "description": "サイレント実行かどうか（デフォルト: false）"
                                }
                            },
                            "required": ["command"]
                        }),
                    },
                ],
            }
        ]
    }

    /// Handle function call from Gemini API
    async fn handle_function_call(&self, function_call: &FunctionCall) -> Result<String> {
        debug_log!("[handle_function_call] Function: {}, Args: {:?}", function_call.name, function_call.args);
        
        match function_call.name.as_str() {
            "create_file" => {
                let filename = function_call.args["filename"].as_str()
                    .ok_or(anyhow::anyhow!("filename parameter is required"))?;
                let content = function_call.args["content"].as_str()
                    .ok_or(anyhow::anyhow!("content parameter is required"))?;
                
                match self.create_file_with_unique_name(filename, content) {
                    Ok(created_path) => Ok(format!("✅ ファイルを作成しました: {}", created_path)),
                    Err(e) => Ok(format!("❌ ファイル作成に失敗しました: {}", e)),
                }
            },
            "edit_file" => {
                let filename = function_call.args["filename"].as_str()
                    .ok_or(anyhow::anyhow!("filename parameter is required"))?;
                let start_line = function_call.args["start_line"].as_u64()
                    .ok_or(anyhow::anyhow!("start_line parameter is required"))? as usize;
                let end_line = function_call.args["end_line"].as_u64()
                    .ok_or(anyhow::anyhow!("end_line parameter is required"))? as usize;
                let content = function_call.args["content"].as_str()
                    .ok_or(anyhow::anyhow!("content parameter is required"))?;
                
                match self.file_access.edit_file_range(filename, start_line, end_line, content) {
                    Ok(_) => Ok(format!("✅ ファイルを編集しました: {}", filename)),
                    Err(e) => Ok(format!("❌ ファイル編集に失敗しました: {}", e)),
                }
            },
            "execute_command" => {
                let command = function_call.args["command"].as_str()
                    .ok_or(anyhow::anyhow!("command parameter is required"))?;
                let _silent = function_call.args["silent"].as_bool().unwrap_or(false);
                
                match self.execute_command(command).await {
                    Ok(result) => {
                        if result.success {
                            Ok(format!("✅ コマンド実行成功: {}\n出力: {}", command, result.stdout))
                        } else {
                            Ok(format!("❌ コマンド実行失敗: {}\nエラー: {}", command, result.stderr))
                        }
                    },
                    Err(e) => Ok(format!("❌ コマンド実行エラー: {}", e)),
                }
            },
            _ => Ok(format!("❌ 未知の関数呼び出し: {}", function_call.name)),
        }
    }

    // システムプロンプトを作成
    fn get_system_prompt(&self) -> String {
        r###"あなたはファイル作成・部分編集・コマンド実行機能を持つAIアシスタントです。

ユーザーのリクエストに応じて、以下の機能を提供できます：

1. **ファイル作成**: 新しいファイルを作成
2. **ファイル編集**: 既存ファイルの部分編集
3. **コマンド実行**: シェルコマンドの実行

これらの機能は、Function Calling機能を通じて実行されます。必要に応じて適切な関数を呼び出してください。

---
【重要】全ての返答の末尾に, タスクが終了したかを示すフラグである is_finished: true または is_finished: false を必ず明示してください（JSON形式または "is_finished: true" のような形式でOK）。
また、is_finished:falseの際は、作業を完了させるため適切な関数を呼び出すこと。適切な関数が存在しない、また異常終了しているなどの場合はtrueを返すこと。
"###.to_string()
    }

    /// レスポンステキストでファイル作成とコマンド実行を処理する共通関数
    /// Function Calling移行により、疑似ツール処理は無効化
    fn process_response_actions_sync(
        &self,
        _response_text: &str,
        _original_message: &str,
    ) -> (bool, String) {
        // Function Calling移行により、この処理は不要
        (false, String::new())
    }

    async fn _send_request_and_parse_response(
        &self,
        request: GeminiRequest,
    ) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );
        let response_text = self
            .send_google_request_with_retry(&url, &request)
            .await?;
        // デバッグ: レスポンス内容をファイルに追記 (contui_debug.log)
        debug_log!("[_send_request_and_parse_response] response_text:\n{}\n", response_text);
        // LLMレスポンスJSONをcontui_llm_response.logに出力
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("contui_llm_response.log") {
            let _ = writeln!(file, "{}", response_text);
        }
        if response_text.contains("error") {
            eprintln!("Gemini API Error: {}", response_text);
            return Err(anyhow::anyhow!("Gemini API Error: {}", response_text));
        }
        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)?;
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                match part {
                    ResponsePart::Text { text } => return Ok(text.clone()),
                    ResponsePart::FunctionCall { function_call } => {
                        // Function callの処理
                        return self.handle_function_call(function_call).await;
                    }
                }
            }
        }
        Err(anyhow::anyhow!("No response from Gemini"))
    }

    async fn send_and_process_response(
        &self,
        request: GeminiRequest,
        _process_actions: bool, // process_actions は現在使用されていないため、_ を付けて警告を抑制
    ) -> Result<String> {
        let response_text = self._send_request_and_parse_response(request).await?;
        // process_actions は現在使用されていないため、常に bold text を返す
        Ok(self.format_bold_text(&response_text))
    }

    // chatとchat_with_file_contextの共通処理をまとめたヘルパー関数
    async fn send_chat_request_and_process_response(
        &self,
        contents: Vec<Content>,
        original_message: &str,
    ) -> Result<String> {
        let request = GeminiRequest {
            contents,
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: Some(self.get_function_declarations()),
        };

        let response_text = self._send_request_and_parse_response(request).await?;

        // Function Calling移行により、疑似ツール処理は無効化
        let (_has_actions, _context_message) =
            self.process_response_actions_sync(&response_text, original_message);
        
        // Function Callingで処理されるため、直接レスポンスを返す
        return Ok(self.format_bold_text(&response_text));
    }

    pub async fn chat(&self, message: &str, context: Option<&[ChatMessage]>) -> Result<String> {
        debug_log!("[chat] called with message: {}\n", message);

        let mut contents: Vec<Content> = Vec::new();
        contents.push(Content {
            role: "user".to_string(),
            parts: vec![Part {
                text: self.get_system_prompt(),
            }],
        });

        if let Some(ctxs) = context {
            for msg in ctxs {
                contents.push(Content {
                    role: if msg.is_user { "user".to_string() } else { "model".to_string() },
                    parts: vec![Part {
                        text: msg.content.clone(),
                    }],
                });
            }
        }

        contents.push(Content {
            role: "user".to_string(),
            parts: vec![Part {
                text: message.to_string(),
            }],
        });

        self.send_chat_request_and_process_response(contents, message).await
    }

    pub async fn chat_with_file_context(&self, message: &str, file_paths: &[String], context: Option<&[ChatMessage]>) -> Result<String> {
        let mut file_contents_text = String::new();
        for file_path in file_paths {
            match self.file_access.read_file(file_path) {
                Ok(content) => {
                    file_contents_text.push_str(&format!("\n--- File: {} ---\n", file_path));
                    file_contents_text.push_str(&content);
                    file_contents_text.push_str("\n--- End of file ---\n\n");
                }
                Err(e) => {
                    eprintln!("Failed to read file {}: {}", file_path, e);
                    file_contents_text.push_str(&format!("\n--- Error reading file: {} ---\n", file_path));
                    file_contents_text.push_str(&format!("Error: {}\n\n", e));
                }
            }
        }

        let mut contents: Vec<Content> = Vec::new();
        contents.push(Content {
            role: "user".to_string(),
            parts: vec![Part {
                text: self.get_system_prompt(),
            }],
        });

        if !file_contents_text.is_empty() {
            contents.push(Content {
                role: "user".to_string(),
                parts: vec![Part {
                    text: format!("=== FILE CONTENTS ===\n{}\n=== END FILE CONTENTS ===\n\n", file_contents_text),
                }],
            });
        }

        if let Some(ctxs) = context {
            for msg in ctxs {
                contents.push(Content {
                    role: if msg.is_user { "user".to_string() } else { "model".to_string() },
                    parts: vec![Part {
                        text: msg.content.clone(),
                    }],
                });
            }
        }

        contents.push(Content {
            role: "user".to_string(),
            parts: vec![Part {
                text: message.to_string(),
            }],
        });

        self.send_chat_request_and_process_response(contents, message).await
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<String>> {
        self.file_access.list_directory(path)
    }

    /// ファイルを作成（重複チェック付き）- 実際に作成されたファイル名を返す
    pub fn create_file_with_unique_name(&self, path: &str, content: &str) -> Result<String> {
        let created_path = self.file_access.create_file_with_unique_name(path, content)?;
        Ok(created_path.to_string_lossy().to_string())
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
                        content.push_str("\n");
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
                        return Err(anyhow::anyhow!("❌ ファイル作成失敗 {}: {}", filename, e));
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
                        command.push_str("\n");
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

    /// AIに実行結果を送信して、結果に基づく回答を取得
    async fn get_ai_response_for_results(&self, context_message: &str) -> Result<String> {
        let request = GeminiRequest {
            contents: vec![Content {
                role: "user".to_string(),
                parts: vec![Part {
                    text: context_message.to_string(),
                }],
            }],
            generation_config: GenerationConfig {
                temperature: self.config.temperature.unwrap_or(0.7),
                max_output_tokens: self.config.max_tokens.unwrap_or(1000),
            },
            tools: None, // 結果応答時はツールを使用しない
        };
        self.send_and_process_response(request, false).await
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
                let parts: Vec<&str> = header.split(":").collect();
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
                        content.push_str("\n");
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
                        return Err(anyhow::anyhow!("❌ 部分編集失敗 {}: {}", filename, e));
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
        let mut message: String = initial_message.to_string();
        let mut step = 1;
        loop {
            // 毎回「次に何をすべきか」「追加タスクがあるか」を問うプロンプトを付与
            let prompt = format!(
                "{}\n\n---\n次に何をすべきか、追加タスクがあるかを必ず明示してください。\nis_finished: true/false のJSONフラグを必ず返してください。",
                message
            );
            println!("========== LLM Step {} ==========", step);
            let conversation_context = self.history_manager.lock().unwrap().get_conversation_context(10); // Get last 10 messages
            let response = self.chat(&prompt, Some(&conversation_context)).await?;
            println!("LLM Response:\n{}\n", response);

            // is_finishedフラグで終了判定
            if self.extract_is_finished_flag(&response) == Some(true) {
                println!("LLMがis_finished: trueを返したためループを終了します。");
                break;
            }

            // 次の入力としてLLMの返答をそのまま使う
            message = response;
            step += 1;
        }
        Ok(())
    }

    /// レスポンステキストから is_finished: true/false を抽出
    pub fn extract_is_finished_flag(&self, text: &str) -> Option<bool> {
        // 例: {"is_finished": true} または is_finished: true
        let re = regex::Regex::new(r#""?is_finished"?\s*:\s*(true|false)"#).ok()?;
        if let Some(caps) = re.captures(text) {
            match &caps[1] {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            }
        } else {
            None
        }
    }
}