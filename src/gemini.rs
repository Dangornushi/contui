use serde::{Deserialize, Serialize};
use anyhow::Result;
use regex::Regex;

use crate::config::LlmConfig;
use crate::file_access::FileAccessManager;
use tokio::sync::mpsc;
use crate::app::{ChatEvent, TodoItem};
use unicode_segmentation::UnicodeSegmentation;

/// コマンド実行結果の構造体
#[derive(Debug)]
pub struct CommandResult {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub exit_code: Option<i32>,
}

// ファイル作成結果の構造体
#[derive(Debug)]
pub struct FileCreationResult {
    pub requested_filename: String,
    pub actual_filename: Option<String>, // 実際に作成されたファイル名（重複回避された場合など）
    pub success: bool,
    pub error_message: Option<String>,
}

// ファイル閲覧結果の構造体
#[derive(Debug)]
pub struct FileReadResult {
    pub requested_filename: String,
    pub content: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
}

// ファイル編集結果の構造体
#[derive(Debug)]
pub struct FileEditResult {
    pub requested_filename: String,
    pub success: bool,
    pub error_message: Option<String>,
}

// ファイル追記結果の構造体
#[derive(Debug)]
pub struct FileAppendResult {
    pub requested_filename: String,
    pub success: bool,
    pub error_message: Option<String>,
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


use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct GeminiClient {
    client: reqwest::Client,
    pub config: LlmConfig,
    file_access: FileAccessManager,
    pub event_sender: mpsc::UnboundedSender<ChatEvent>,
    log_file: Arc<Mutex<Option<File>>>,
}

impl GeminiClient {
    pub fn new(config: LlmConfig, event_sender: mpsc::UnboundedSender<ChatEvent>) -> Self {
        let log_file = match File::create("debug.log") {
            Ok(file) => {
                eprintln!("DEBUG: Log file 'debug.log' created successfully.");
                Some(file)
            },
            Err(e) => {
                eprintln!("ERROR: Failed to create log file 'debug.log': {}", e);
                None
            }
        };

        Self {
            client: reqwest::Client::new(),
            config,
            file_access: FileAccessManager::new(),
            event_sender,
            log_file: Arc::new(Mutex::new(log_file)),
        }
    }

    pub fn add_allowed_directory<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        self.file_access.add_allowed_directory(path)
    }

    // システムプロンプトを作成
    fn get_system_prompt(&self) -> String {
        r#"あなたはファイル作成機能とコマンド実行機能を持つAIアシスタントです。

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

## ファイル閲覧機能
ユーザーがファイルの内容を閲覧したいと依頼した場合、以下の正確な形式を使用してファイルの内容を読み取ることができます：

```read_file:ファイル名
```

## ファイル部分編集機能
ユーザーがファイルの一部を編集したいと依頼した場合、以下の正確な形式を使用してファイルを部分的に編集できます：

```edit_file:ファイル名
---OLD---
古い内容
---NEW---
新しい内容
```

**重要な指示:**
- `---OLD---`と`---NEW---`の間の内容は、変更したい正確なテキスト（空白、インデント、改行を含む）である必要があります。
- `---OLD---`の内容は、ファイル内で一意に識別できる十分なコンテキスト（少なくとも3行の前後）を含む必要があります。

## 変更差分表示機能
ユーザーが変更内容を確認したいと依頼した場合、以下の正確な形式を使用して現在の変更差分を表示できます：

```show_diff
```

## ファイル追記機能
ユーザーがファイルに内容を追記したいと依頼した場合、以下の正確な形式を使用してファイルに内容を追記できます：

```append_file:ファイル名
追記する内容
```

## ファイル一覧表示機能
ユーザーがディレクトリ内のファイル一覧を閲覧したいと依頼した場合、以下の正確な形式を使用してディレクトリの内容を一覧表示できます。**ディレクトリパスは必ず単一行で、末尾に余分な改行や空白を含めないでください。**：

```list_directory:ディレクトリパス```

## タスク遂行の指示
あなたは与えられたタスクを、以下の手順で分割し、順序立てて遂行してください。
**コマンド実行（`execute_command`）のみユーザーの許可を得るようにし、それ以外のツールで実行可能なタスクはユーザーに提案することなく自律的に実行してください。**

1. **理解**: ユーザーの要求と、関連するコードベースのコンテキストを理解します。
   - **自発的な探索**: ユーザーの要求を完全に理解するため、または計画を立てるために追加の情報が必要な場合は、`read_file`や`list_directory`ツールを積極的に使用して関連するファイルやディレクトリの内容を自発的に探索してください。
2. **計画**: ユーザーのタスクを解決するための、一貫性のある具体的な計画を立てます。**まず、タスク完了までのTODOリストをMarkdownの番号付きリストで作成し、それをチャットに出力してください。** その後、**ツール実行が失敗した場合、その原因を自律的に診断し、他のツール（例: `read_file`でログを確認、`execute_command`で診断コマンドを実行など）を使用して問題を解決する計画を立ててください。** ユーザーに助けを求める前に、あらゆる可能な自己修正を試みてください。必要であれば、単体テストの作成やデバッグステートメントの追加など、自己検証のループを計画に含めます。
3. **実装**: 計画に基づいて、利用可能なツール（`create_file`, `execute_command`, `read_file`, `edit_file`, `append_file`, `show_diff`, `list_directory`など）を使用して変更を実装します。**`execute_command`以外のツールはユーザーの確認なしに直接実行されます。**
4. **検証**: 変更を検証するために、プロジェクト固有のビルド、リンティング、型チェックコマンドを実行します。必要であれば、テストを実行します。**検証が失敗した場合、その原因を診断し、修正する計画を立て、再度実装と検証を繰り返してください。**

**TODOリストの出力形式:**
タスクの計画を立てる際には、必ず以下の形式でTODOリストを出力してください。
```todo
- [ ] TODO項目1
- [ ] TODO項目2
- [ ] TODO項目3
```

**TODOリストの進捗報告:**
TODOリストの項目が完了するたびに、以下の形式で完了した項目をマークし、LLMにその進捗を報告してください。
```todo
- [x] 完了したTODO項目
- [ ] 未完了のTODO項目
```

**次のTODO項目への指示:**
TODOリストの進捗を報告した後、LLMに次のTODO項目を実行するように促してください。

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
1. ファイルパス: 全てのファイルパスは、プロジェクトのルートディレクトリからの絶対パスで指定してください。
2. ファイル作成：必ず上記の形式を正確に使用してください（```create_file:ファイル名）
3. コマンド実行：標準実行かサイレント実行かを適切に判断してください
4. ファイル閲覧：必ず上記の形式を正確に使用してください（```read_file:ファイル名）
5. ファイル部分編集：必ず上記の形式を正確に使用してください（```edit_file:ファイル名）
6. 変更差分表示：必ず上記の形式を正確に使用してください（```show_diff）
7. ファイル追記：必ず上記の形式を正確に使用してください（```append_file:ファイル名）
8. 空のファイルの場合は、形式は使用しますが内容部分は空にしてください
9. あらゆるファイル形式を作成できます（.txt, .rs, .py, .html, .json など）
10. シェルコマンド、システムコマンド、プログラム実行など、様々なコマンドを実行できます
11. 安全で適切なコマンドのみを実行してください
12. レスポンスの本文中で、不必要なコロン（:）の使用は避けてください。特に、箇条書きや説明の区切りとしてコロンを使う代わりに、より自然な日本語の句読点や表現を使用してください。
13. 曖昧な指示への対応: ユーザーが「このファイルのこの処理をしている部分をこうゆうふうに置き換えて」のように曖昧な指示をした場合、まず`read_file`で対象ファイルを読み込み、その内容を元に`edit_file`コマンドを生成してください。`old_string`には、変更したい部分の前後3行程度のコンテキストを含めてください。

例：
- 空のテキストファイル: ```create_file:test.txt```

- Rustファイル: ```create_file:main.rs
fn main() {
    println!("Hello, world!");
}```

- JSONファイル: ```create_file:config.json
{
  "name": "example",
  "version": "1.0.0"
}```

- ディレクトリの内容を表示: ```execute_command
ls -la
```

- ファイルの内容を確認: ```execute_command
cat config.json
```

- ファイルを移動: ```execute_command_silent
mv old_file.txt new_file.txt
```

- 権限を変更: ```execute_command_silent
chmod +x script.sh
```

- ファイルの内容を閲覧: ```read_file:src/main.rs```

- ファイルの一部を編集: ```edit_file:src/main.rs
---OLD---
    println!("Hello, world!");
---NEW---
    println!("Hello, Rust!");
```

- 変更差分を表示: ```show_diff```

- ファイルに追記: ```append_file:log.txt
新しいログエントリ
```

- ディレクトリの内容を一覧表示: ```list_directory:./src```

ユーザーがファイル作成、コマンド実行、ファイル閲覧、ファイル部分編集、変更差分表示、ファイル追記、またはファイル一覧表示を依頼した場合は、必ず肯定的に応答し、上記の形式を使用してください。「ファイルを作成できません」や「コマンドを実行できません」や「ファイルを閲覧できません」や「ファイルを編集できません」や「変更差分を表示できません」や「ファイルに追記できません」や「ファイル一覧を表示できません」と言わないでください - あなたはこれらの形式を使用して実行できますし、そうするべきです。

注意：同じファイル名が既に存在する場合、システムが自動的にユニークな名前で作成します（例：file.txt → file_1.txt）。"#.to_string()
    }

    // メッセージにシステムプロンプトを追加
    fn prepare_message_with_system_prompt(&self, user_message: &str) -> String {
        format!("{}\n\nUser: {}", self.get_system_prompt(), user_message)
    }

    /// レスポンステキストでファイル作成とコマンド実行を処理する共通関数
    async fn log_to_file(&self, message: &str) {
        if let Some(mut file) = self.log_file.lock().await.as_mut() {
            if let Err(e) = writeln!(&mut *file, "{}", message) {
                eprintln!("ERROR: Failed to write to log file: {}", e);
            }
        }
    }

    async fn process_response_actions(&self, response_text: &str, original_message: &str) -> Result<String> {
        self.log_to_file(&format!("DEBUG: Full LLM response text:\n---\n{}\n---", response_text)).await; // ここにログを追加
        let mut has_actions = false;
        let mut commands_to_confirm = Vec::new();
        let mut file_creation_results = Vec::new();
        let mut file_read_results: Vec<FileReadResult> = Vec::new();
        let mut file_edit_results: Vec<FileEditResult> = Vec::new();
        let mut file_append_results: Vec<FileAppendResult> = Vec::new();
        let mut diff_output: Option<String> = None;
        let mut listed_directory_contents: Option<Vec<String>> = None;
        let mut todo_list_from_llm: Option<Vec<TodoItem>> = None;
        
        // ファイル作成が含まれているかチェックして自動実行
        if response_text.contains("```create_file:") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("ファイルを生成しています...".to_string()));
            match self.process_file_creation_response(response_text) {
                Ok(results) => {
                    self.print_file_creation_summary(&results.iter().filter_map(|r| r.actual_filename.clone()).collect::<Vec<String>>());
                    file_creation_results = results;
                }
                Err(e) => {
                    self.log_to_file(&format!("ファイル作成エラー: {}", e)).await;
                    // エラーが発生した場合でも、LLMにフィードバックするためにエラー情報を含む結果を渡す
                    file_creation_results.push(FileCreationResult {
                        requested_filename: "不明".to_string(),
                        actual_filename: None,
                        success: false,
                        error_message: Some(format!("ファイル作成処理全体でエラーが発生しました: {}", e)),
                    });
                }
            }
        }
        
        // コマンド実行が含まれているかチェックして自動実行
        if response_text.contains("```execute_command") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("コマンドを解析しています...".to_string()));
            match self.process_command_execution_response(response_text).await {
                Ok(commands) => {
                    commands_to_confirm = commands;
                }
                Err(e) => {
                    self.log_to_file(&format!("コマンド解析エラー: {}", e)).await;
                    // エラーが発生した場合でも、LLMにフィードバックするためにエラー情報を含む結果を渡す
                    // ここでは、エラーメッセージを直接LLMに送る
                    let _ = self.get_ai_response_for_results(&format!("コマンド解析エラー: {}", e)).await?;
                    return Ok(self.format_bold_text(response_text)); // 元のレスポンスを返す
                }
            }
        }

        // ファイル閲覧が含まれているかチェックして自動実行
        if response_text.contains("```read_file:") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("ファイルを閲覧しています...".to_string()));
            match self.process_file_read_response(response_text) {
                Ok(results) => {
                    file_read_results = results;
                }
                Err(e) => {
                    self.log_to_file(&format!("ファイル閲覧エラー: {}", e)).await;
                    // エラーが発生した場合でも、LLMにフィードバックするためにエラー情報を含む結果を渡す
                    file_read_results.push(FileReadResult {
                        requested_filename: "不明".to_string(),
                        content: None,
                        success: false,
                        error_message: Some(format!("ファイル閲覧処理全体でエラーが発生しました: {}", e)),
                    });
                }
            }
        }

        // ファイル編集が含まれているかチェックして自動実行
        if response_text.contains("```edit_file:") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("ファイルを編集しています...".to_string()));
            match self.process_file_edit_response(response_text) {
                Ok(results) => {
                    file_edit_results = results;
                }
                Err(e) => {
                    self.log_to_file(&format!("ファイル編集エラー: {}", e)).await;
                    file_edit_results.push(FileEditResult {
                        requested_filename: "不明".to_string(),
                        success: false,
                        error_message: Some(format!("ファイル編集処理全体でエラーが発生しました: {}", e)),
                    });
                }
            }
        }

        // ファイル追記が含まれているかチェックして自動実行
        if response_text.contains("```append_file:") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("ファイルに追記しています...".to_string()));
            match self.process_file_append_response(response_text) {
                Ok(results) => {
                    file_append_results = results;
                }
                Err(e) => {
                    self.log_to_file(&format!("ファイル追記エラー: {}", e)).await;
                    file_append_results.push(FileAppendResult {
                        requested_filename: "不明".to_string(),
                        success: false,
                        error_message: Some(format!("ファイル追記処理全体でエラーが発生しました: {}", e)),
                    });
                }
            }
        }

        // 変更差分表示が含まれているかチェックして自動実行
        if response_text.contains("```show_diff") {
            has_actions = true;
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("変更差分を生成しています...".to_string()));
            match self.process_show_diff_response(response_text).await {
                Ok(output) => {
                    diff_output = output;
                }
                Err(e) => {
                    eprintln!("変更差分表示エラー: {}", e);
                    diff_output = Some(format!("変更差分表示失敗: {}", e));
                }
            }
        }

        // ディレクトリ一覧表示が含まれているかチェックして自動実行
        if response_text.contains("```list_directory:") {
            has_actions = true;
            self.log_to_file(&format!("DEBUG: Detected list_directory block in response.")).await;
            self.log_to_file(&format!("DEBUG: Response text (partial):\n{}", response_text.graphemes(true).take(500).collect::<String>())).await; // 最初の500文字をログ
            let _ = self.event_sender.send(ChatEvent::ToolCallStatus("ディレクトリの内容を一覧表示しています...".to_string()));
            match self.process_list_directory_response(response_text).await {
                Ok(contents) => {
                    listed_directory_contents = Some(contents);
                }
                Err(e) => {
                    self.log_to_file(&format!("ディレクトリ一覧表示エラー: {}", e)).await;
                    listed_directory_contents = Some(vec![format!("ディレクトリ一覧表示失敗: {}", e)]);
                }
            }
        }
        
        // TODOリストが含まれているかチェックして解析
        if response_text.contains("```todo") {
            self.log_to_file("DEBUG: Detected todo block in response.").await;
            match self.parse_todo_list_from_response(response_text) {
                Ok(todo_items) => {
                    todo_list_from_llm = Some(todo_items);
                }
                Err(e) => {
                    self.log_to_file(&format!("TODOリスト解析エラー: {}", e)).await;
                }
            }
        }
        if has_actions {
            let mut context_message = String::new();
            context_message.push_str("以下のアクションが実行されました。結果を確認して、適切な回答やコメントをしてください：\n\n");
            context_message.push_str(&format!("元のリクエスト: {}\n\n", original_message));
            
            if !file_creation_results.is_empty() {
                context_message.push_str(&format!("ファイル作成結果 ({} 個):\n", file_creation_results.len()));
                for result in &file_creation_results {
                    context_message.push_str(&format!("- リクエストファイル名: {}\n", result.requested_filename));
                    context_message.push_str(&format!("  ステータス: {}\n", if result.success { "成功" } else { "失敗" }));
                    if let Some(actual_name) = &result.actual_filename {
                        context_message.push_str(&format!("  実際に作成されたファイル名: {}\n", actual_name));
                    }
                    if let Some(err_msg) = &result.error_message {
                        context_message.push_str(&format!("  エラー: {}\n", err_msg));
                    }
                    context_message.push('\n');
                }
                // ファイルブラウザのコンテンツを更新するイベントを送信
                let _ = self.event_sender.send(ChatEvent::RefreshDirectory);
            }
            
            if !commands_to_confirm.is_empty() {
                // コマンド確認を要求するイベントを送信
                let _ = self.event_sender.send(ChatEvent::ToolCallStatus("コマンド実行の確認を求めています...".to_string()));
                for command in commands_to_confirm {
                    let _ = self.event_sender.send(ChatEvent::RequestCommandConfirmation(command));
                }
                // ここで処理を中断し、ユーザーの確認を待つ
                return Ok(self.format_bold_text("コマンド実行の確認を待っています..."));
            }

            if !file_read_results.is_empty() {
                context_message.push_str(&format!("ファイル閲覧結果 ({} 個):\n", file_read_results.len()));
                for result in &file_read_results {
                    context_message.push_str(&format!("- リクエストファイル名: {}\n", result.requested_filename));
                    context_message.push_str(&format!("  ステータス: {}\n", if result.success { "成功" } else { "失敗" }));
                    if let Some(content) = &result.content {
                        context_message.push_str(&format!("  内容:\n```\n{}\n```\n", content));
                    }
                    if let Some(err_msg) = &result.error_message {
                        context_message.push_str(&format!("  エラー: {}\n", err_msg));
                    }
                    context_message.push('\n');
                }
            }

            if !file_edit_results.is_empty() {
                context_message.push_str(&format!("ファイル編集結果 ({} 個):\n", file_edit_results.len()));
                for result in &file_edit_results {
                    context_message.push_str(&format!("- リクエストファイル名: {}\n", result.requested_filename));
                    context_message.push_str(&format!("  ステータス: {}\n", if result.success { "成功" } else { "失敗" }));
                    if let Some(err_msg) = &result.error_message {
                        context_message.push_str(&format!("  エラー: {}\n", err_msg));
                    }
                    context_message.push('\n');
                }
            }

            if !file_append_results.is_empty() {
                context_message.push_str(&format!("ファイル追記結果 ({} 個):\n", file_append_results.len()));
                for result in &file_append_results {
                    context_message.push_str(&format!("- リクエストファイル名: {}\n", result.requested_filename));
                    context_message.push_str(&format!("  ステータス: {}\n", if result.success { "成功" } else { "失敗" }));
                    if let Some(err_msg) = &result.error_message {
                        context_message.push_str(&format!("  エラー: {}\n", err_msg));
                    }
                    context_message.push('\n');
                }
            }

            if let Some(diff) = diff_output {
                context_message.push_str("変更差分:\n```diff\n");
                context_message.push_str(&diff);
                context_message.push_str("\n```\n\n");
            }

            if let Some(contents) = listed_directory_contents {
                context_message.push_str(&format!("ディレクトリの内容:\n```\n{}\n```\n\n", contents.join("\n")));
            }
            
            if let Some(todo_items) = todo_list_from_llm {
                let _ = self.event_sender.send(ChatEvent::TodoListUpdated(todo_items));
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

        self.execute_gemini_api_call(&url, &request, message).await
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

        self.execute_gemini_api_call(&url, &request, message).await
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
                Err(e) => {                    self.log_to_file(&format!("Failed to read file {}: {}", file_path, e)).await;                    file_contents.push_str(&format!("\n--- Error reading file: {} ---\n", file_path));                    file_contents.push_str(&format!("Error: {}\n\n", e));                }
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

        self.execute_gemini_api_call(&url, &request, message).await
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
    pub fn process_file_creation_response(&self, response: &str) -> Result<Vec<FileCreationResult>> {
        let mut results = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            
            if line.starts_with("```create_file:") {
                let filename = line.trim_start_matches("```create_file:").trim();
                if filename.is_empty() {
                    results.push(FileCreationResult {
                        requested_filename: "".to_string(),
                        actual_filename: None,
                        success: false,
                        error_message: Some("ファイル名が指定されていません。".to_string()),
                    });
                    i += 1;
                    continue;
                }

                let mut content = String::new();
                i += 1;

                while i < lines.len() && !lines[i].starts_with("```") {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(lines[i]);
                    i += 1;
                }

                match self.create_file_with_unique_name(filename, &content) {
                    Ok(created_path) => {
                        results.push(FileCreationResult {
                            requested_filename: filename.to_string(),
                            actual_filename: Some(created_path),
                            success: true,
                            error_message: None,
                        });
                    }
                    Err(e) => {
                        results.push(FileCreationResult {
                            requested_filename: filename.to_string(),
                            actual_filename: None,
                            success: false,
                            error_message: Some(format!("ファイル作成失敗: {}", e)),
                        });
                    }
                }
            }
            i += 1;
        }

        if results.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにcreate_fileブロックが見つかりませんでした"));
        }

        Ok(results)
    }

    /// LLMのレスポンスから edit_file: 形式のブロックを解析してファイルを編集
    pub fn process_file_edit_response(&self, response: &str) -> Result<Vec<FileEditResult>> {
        let mut results = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            
            if line.starts_with("```edit_file:") {
                let filename = line.trim_start_matches("```edit_file:").trim();
                if filename.is_empty() {
                    results.push(FileEditResult {
                        requested_filename: "".to_string(),
                        success: false,
                        error_message: Some("ファイル名が指定されていません。".to_string()),
                    });
                    i += 1;
                    continue;
                }

                i += 1;
                let mut old_content_lines = Vec::new();
                let mut new_content_lines = Vec::new();
                let mut in_old = false;
                let mut in_new = false;

                while i < lines.len() && !lines[i].starts_with("```") {
                    let current_line = lines[i];
                    if current_line.trim() == "---OLD---" {
                        in_old = true;
                        in_new = false;
                    } else if current_line.trim() == "---NEW---" {
                        in_old = false;
                        in_new = true;
                    } else if in_old {
                        old_content_lines.push(current_line);
                    } else if in_new {
                        new_content_lines.push(current_line);
                    }
                    i += 1;
                }

                let old_string = old_content_lines.join("\n");
                let new_string = new_content_lines.join("\n");

                match self.file_access.replace_content(filename, &old_string, &new_string) {
                    Ok(_) => {
                        results.push(FileEditResult {
                            requested_filename: filename.to_string(),
                            success: true,
                            error_message: None,
                        });
                    }
                    Err(e) => {
                        results.push(FileEditResult {
                            requested_filename: filename.to_string(),
                            success: false,
                            error_message: Some(format!("ファイル編集失敗: {}", e)),
                        });
                    }
                }
            }
            i += 1;
        }

        if results.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにedit_fileブロックが見つかりませんでした"));
        }

        Ok(results)
    }

    /// LLMのレスポンスから show_diff 形式のブロックを解析してgit diffを実行
    pub async fn process_show_diff_response(&self, response: &str) -> Result<Option<String>> {
        if response.contains("```show_diff") {
            match self.file_access.get_git_diff() {
                Ok(diff_output) => {
                    return Ok(Some(diff_output));
                }
                Err(e) => {
                    self.log_to_file(&format!("git diff 実行エラー: {}", e)).await;
                    return Ok(Some(format!("git diff 実行失敗: {}", e)));
                }
            }
        }
        Ok(None)
    }

    /// LLMのレスポンスから append_file: 形式のブロックを解析してファイルに追記
    pub fn process_file_append_response(&self, response: &str) -> Result<Vec<FileAppendResult>> {
        let mut results = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            
            if line.starts_with("```append_file:") {
                let filename = line.trim_start_matches("```append_file:").trim();
                if filename.is_empty() {
                    results.push(FileAppendResult {
                        requested_filename: "".to_string(),
                        success: false,
                        error_message: Some("ファイル名が指定されていません。".to_string()),
                    });
                    i += 1;
                    continue;
                }

                let mut content = String::new();
                i += 1;

                while i < lines.len() && !lines[i].starts_with("```") {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(lines[i]);
                    i += 1;
                }

                match self.file_access.append_to_file(filename, &content) {
                    Ok(_) => {
                        results.push(FileAppendResult {
                            requested_filename: filename.to_string(),
                            success: true,
                            error_message: None,
                        });
                    }
                    Err(e) => {
                        results.push(FileAppendResult {
                            requested_filename: filename.to_string(),
                            success: false,
                            error_message: Some(format!("ファイル追記失敗: {}", e)),
                        });
                    }
                }
            }
            i += 1;
        }

        if results.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにappend_fileブロックが見つかりませんでした"));
        }

        Ok(results)
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
    pub async fn process_command_execution_response(&self, response: &str) -> Result<Vec<String>> {
        let mut commands_to_execute = Vec::new();
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

                // コマンドが空でない場合、リストに追加
                if !command.trim().is_empty() {
                    commands_to_execute.push(command.trim().to_string());
                }
            }
            i += 1;
        }

        if commands_to_execute.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにexecute_commandブロックが見つかりませんでした"));
        }

        Ok(commands_to_execute)
    }

    /// LLMのレスポンスから read_file: 形式のブロックを解析してファイルの内容を読み取る
    pub fn process_file_read_response(&self, response: &str) -> Result<Vec<FileReadResult>> {
        let mut results = Vec::new();
        let re = Regex::new(r"```read_file:(.*?)```").unwrap(); // 大文字・小文字を区別しない

        for caps in re.captures_iter(response) {
            let mut filename = caps.get(1).map_or("", |m| m.as_str()).trim().to_string();
            if filename.ends_with('\n') {
                filename.pop(); // 末尾の改行コードを削除
            }
            
            if filename.is_empty() {
                results.push(FileReadResult {
                    requested_filename: "".to_string(),
                    content: None,
                    success: false,
                    error_message: Some("ファイル名が指定されていません。".to_string()),
                });
                continue;
            }

            // ファイルの内容を読み取り
            match self.file_access.read_file(&filename) {
                Ok(content) => {
                    results.push(FileReadResult {
                        requested_filename: filename.to_string(),
                        content: Some(content),
                        success: true,
                        error_message: None,
                    });
                }
                Err(e) => {
                    let error_message = if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                        match io_err.kind() {
                            std::io::ErrorKind::NotFound => {
                                format!("ファイルが見つかりませんでした: {}", filename)
                            },
                            std::io::ErrorKind::PermissionDenied => {
                                format!("ファイルへのアクセスが拒否されました: {}", filename)
                            },
                            _ => {
                                format!("ファイル閲覧失敗: {}", e)
                            }
                        }
                    } else {
                        format!("ファイル閲覧失敗: {}", e)
                    };
                    results.push(FileReadResult {
                        requested_filename: filename.to_string(),
                        content: None,
                        success: false,
                        error_message: Some(error_message),
                    });
                }
            }
        }

        if results.is_empty() {
            return Err(anyhow::anyhow!("レスポンスにread_fileブロックが見つかりませんでした"));
        }

        Ok(results)
    }

    /// LLMのレスポンスから list_directory: 形式のブロックを解析してディレクトリの内容を一覧表示
    pub async fn process_list_directory_response(&self, response: &str) -> Result<Vec<String>> {
        let mut listed_contents = Vec::new();
                let re = Regex::new(r"```list_directory:(.*?)```").unwrap(); // 大文字・小文字を区別しない

        for caps in re.captures_iter(response) {
            let mut path = caps.get(1).map_or("", |m| m.as_str()).trim().to_string();
            if path.ends_with('\n') {
                path.pop(); // 末尾の改行コードを削除
            }
            self.log_to_file(&format!("DEBUG: Extracted path (trailing newline removed if present): '{}'", path)).await;

            if path.is_empty() {
                self.log_to_file("DEBUG: Directory path is empty after regex extraction.").await;
                return Err(anyhow::anyhow!("ディレクトリパスが指定されていません。"));
            }

            match self.list_directory(&path) {
                Ok(contents) => {
                    self.log_to_file("DEBUG: Successfully listed directory contents.").await;
                    listed_contents.extend(contents);
                }
                Err(e) => {
                    let error_message = if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                        match io_err.kind() {
                            std::io::ErrorKind::NotFound => {
                                format!("ディレクトリが見つかりませんでした: {}", path)
                            },
                            std::io::ErrorKind::PermissionDenied => {
                                format!("ディレクトリへのアクセスが拒否されました: {}", path)
                            },
                            _ => {
                                format!("ディレクトリ一覧表示失敗: {}", e)
                            }
                        }
                    } else {
                        format!("ディレクトリ一覧表示失敗: {}", e)
                    };
                    self.log_to_file(&format!("DEBUG: Failed to list directory: {}", error_message)).await;
                    return Err(anyhow::anyhow!(error_message));
                }
            }
        }

        if listed_contents.is_empty() {
            self.log_to_file("DEBUG: No listed_directory contents found, returning error.").await;
            return Err(anyhow::anyhow!("レスポンスにlist_directoryブロックが見つかりませんでした"));
        }

        self.log_to_file("DEBUG: Successfully processed list_directory response.").await;
        Ok(listed_contents)
    }

    /// AIに実行結果を送信して、結果に基づく回答を取得
    async fn get_ai_response_for_results(&self, context_message: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.gemini_api_key
        );

        // 現在のTODOリストの状態を取得し、コンテキストメッセージに追加
        let mut full_context_message = context_message.to_string();

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: full_context_message,
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
            self.log_to_file(&format!("Gemini API Error: {}", response_text)).await;
            let error_message = if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&response_text) {
                if let Some(error_msg) = error_json["error"]["message"].as_str() {
                    format!("Gemini API Error: {}", error_msg)
                } else {
                    self.log_to_file(&format!("Gemini API Unknown Error: Failed to parse error response or message not found. Raw response: {}", response_text)).await;
                    "Gemini API Unknown Error: Message not found in response.".to_string()
                }
            } else {
                self.log_to_file(&format!("Gemini API Unknown Error: Response was not valid JSON. Raw response: {}", response_text)).await;
                "Gemini API Unknown Error: Response was not valid JSON.".to_string()
            };
            return Err(anyhow::anyhow!(error_message));
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

    fn truncate_output(&self, s: &str, max_len: usize) -> String {
        if s.len() > max_len {
            format!("{}... (truncated)", &s[..max_len])
        } else {
            s.to_string()
        }
    }

    async fn execute_gemini_api_call(&self, url: &str, request: &GeminiRequest, original_message: &str) -> Result<String> {
        let mut attempts = 0;
        let max_attempts = 3; // 最大再試行回数

        loop {
            attempts += 1;
            let response = self
                .client
                .post(url)
                .json(request)
                .send()
                .await?;

            let status = response.status();

            if status.is_success() {
                let response_text = response.text().await?;
                // ... (既存の成功時の処理) ...
                match self.process_response_actions(&response_text, original_message).await {
                    Ok(final_response) => return Ok(final_response),
                    Err(e) => {
                        self.log_to_file(&format!("アクション処理エラー: {}", e)).await;
                        return Ok(self.format_bold_text(&response_text)); // エラーの場合は元のレスポンスを返す
                    }
                }
            } else {
                let response_text = response.text().await?;
                
                // 429エラーの場合の再試行ロジック
                if status.as_u16() == 429 && attempts < max_attempts {
                    self.log_to_file(&format!("Rate limit exceeded (429). Retrying... (Attempt {}/{})", attempts, max_attempts)).await;
                    tokio::time::sleep(std::time::Duration::from_secs(1 * attempts)).await; // 指数バックオフ
                    continue; // 再試行
                }

                // その他のエラーの場合
                let error_message = if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&response_text) {
                    if let Some(error_msg) = error_json["error"]["message"].as_str() {
                        format!("Gemini API Error: {}", error_msg)
                    } else {
                        self.log_to_file(&format!("Gemini API Unknown Error: Failed to parse error response or message not found. Raw response: {}", response_text)).await;
                        "Gemini API Unknown Error: Message not found in response.".to_string()
                    }
                } else {
                    self.log_to_file(&format!("Gemini API Unknown Error: Response was not valid JSON. Raw response: {}", response_text)).await;
                    "Gemini API Unknown Error: Response was not valid JSON.".to_string()
                };
                return Err(anyhow::anyhow!(error_message));
            }
        }
    }
}

impl GeminiClient {
    // ... 既存のコード ...

    /// LLMのレスポンスからTODOリストを解析
    fn parse_todo_list_from_response(&self, response: &str) -> Result<Vec<TodoItem>> {
        let mut todo_items = Vec::new();
        let re = Regex::new(r"```todo\n([\s\S]*?)\n```[\s\S]*").unwrap();

        if let Some(caps) = re.captures(response) {
            if let Some(todo_block) = caps.get(1) {
                for line in todo_block.as_str().lines() {
                    if line.trim().starts_with("- [ ] ") {
                        todo_items.push(TodoItem {
                            description: line.trim_start_matches("- [ ] ").to_string(),
                            completed: false,
                        });
                    } else if line.trim().starts_with("- [x] ") {
                        todo_items.push(TodoItem {
                            description: line.trim_start_matches("- [x] ").to_string(),
                            completed: true,
                        });
                    }
                }
            }
        }

        if todo_items.is_empty() {
            Err(anyhow::anyhow!("レスポンスにTODOブロックが見つかりませんでした、またはTODO項目がありません"))
        } else {
            Ok(todo_items)
        }
    }
}
