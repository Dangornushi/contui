use ratatui::{
    widgets::{ListState},
};
use tokio::sync::mpsc;
use crate::gemini::GeminiClient;
use crate::history::HistoryManager;
use crate::todo_manager::TodoManager;
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;
use regex::Regex;

// モジュール宣言
pub mod handler;
pub mod ui;
pub mod file_operations;
pub mod session_management;
pub mod cursor_movement;
pub mod todo_management;
pub mod visual_mode;
pub mod terminal_util;

#[derive(Debug)]
pub enum ChatEvent {
    AIResponse(String),
    Error(String),
}

pub struct ChatApp {
    pub input: String,
    pub cursor_position: usize,  // カーソルの位置（グラフィフィー単位）
    pub visual_start: Option<usize>,  // Visual Modeの開始位置
    pub messages: Vec<ChatMessage>,
    pub input_mode: InputMode,
    pub gemini_client: GeminiClient,
    pub event_sender: mpsc::UnboundedSender<ChatEvent>,
    pub event_receiver: mpsc::UnboundedReceiver<ChatEvent>,
    pub is_loading: bool,
    pub list_state: ListState,
    pub scroll_offset: usize,
    pub history_manager: HistoryManager,
    pub session_list_state: ListState,
    pub file_browser_state: ListState,
    pub current_directory: String,
    pub directory_contents: Vec<String>,
    pub selected_files: Vec<String>,
    pub input_line_count: usize,  // 入力フィールドの行数
    pub input_history: Vec<String>,  // プロンプト履歴
    pub history_index: Option<usize>,  // 現在の履歴インデックス
    pub temp_input: String,  // 履歴ナビゲーション中の一時的な入力
    pub show_help: bool,  // ヘルプウィンドウ表示フラグ
    pub notification: Option<String>, // ファイル作成通知など一時的な表示
    pub todo_manager: TodoManager,  // TODOリスト管理
    // pub show_todo: bool,  // TODOリスト表示フラグ（不要なので削除）
}

#[derive(Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
    Visual,
    SessionList,
    FileBrowser,
    // TodoList, // 削除
}

#[derive(Debug)]
pub struct ChatMessage {
    pub content: String,
    pub is_user: bool,
}

impl ChatApp {
    pub fn new(mut gemini_client: GeminiClient, mut history_manager: HistoryManager) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        // アクティブなセッションを確保
        let _session_id = history_manager.ensure_active_session();
        
        // 現在のセッションからメッセージを読み込み
        let mut messages = Vec::new();
        if let Some(session) = history_manager.get_history().get_current_session() {
            for hist_msg in &session.messages {
                messages.push(ChatMessage {
                    content: hist_msg.content.clone(),
                    is_user: hist_msg.is_user,
                });
            }
        }

        // 現在のディレクトリを取得
        let current_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string();

        // ファイルアクセス許可を設定（現在のディレクトリとホームディレクトリ）
        if let Err(_e) = gemini_client.add_allowed_directory(&current_dir) {
            // Directory access permission error - silently continue
        }
        if let Some(home_dir) = dirs::home_dir() {
            if let Err(_e) = gemini_client.add_allowed_directory(&home_dir) {
                // Directory access permission error - silently continue
            }
        }
        
        let todo_manager = TodoManager::new().unwrap_or_else(|_| {
            // TODOマネージャーの初期化に失敗した場合は空のマネージャーを作成
            TodoManager { current_list: None, storage_path: "todo_state.json".to_string() }
        });

        let mut app = Self {
            input: String::new(),
            cursor_position: 0,
            visual_start: None,
            messages,
            input_mode: InputMode::Normal,
            gemini_client,
            event_sender,
            event_receiver,
            is_loading: false,
            list_state: ListState::default(),
            scroll_offset: 0,
            history_manager,
            session_list_state: ListState::default(),
            file_browser_state: ListState::default(),
            current_directory: current_dir,
            directory_contents: Vec::new(),
            selected_files: Vec::new(),
            input_line_count: 1,  // 初期値は1行
            input_history: Vec::new(),  // プロンプト履歴を初期化
            history_index: None,  // 履歴インデックスを初期化
            temp_input: String::new(),  // 一時的な入力を初期化
            show_help: false,  // ヘルプウィンドウは初期状態では非表示
            notification: None, // ← 追加
            todo_manager,
            // show_todo: false, // 削除
        };

        // 歓迎メッセージを追加（履歴が空の場合のみ）
        if app.messages.is_empty() {
            app.messages.push(ChatMessage {
                content: "Welcome to ConTUI!".to_string(),
                is_user: false,
            });
        }

        // スクロール状態を初期化
        app.scroll_to_bottom();

        app
    }

    pub fn handle_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::AIResponse(msg) => {
                // ファイル作成要求を処理
                let processed_msg = self.process_file_creation_requests(&msg);
                
                // TODOリストの自動更新を実行
                let _updated_items = self.todo_manager.update_from_ai_response(&processed_msg).unwrap_or_default();

                // 失敗したTODOアイテムがあるかチェックし、再帰的修正フローを実行
                self.check_and_handle_failed_todos(&processed_msg);
                
                // AIレスポンスにTODO情報を追加
                let final_msg = self.append_todo_summary_to_response(processed_msg.clone());
                
                // AIレスポンスをメッセージリストに追加
                self.messages.push(ChatMessage {
                    content: final_msg,
                    is_user: false,
                });
                self.is_loading = false;
                self.scroll_to_bottom();
                
                // 履歴管理にAIレスポンスを追加（処理後のメッセージ）
                if let Err(_) = self.history_manager.get_history_mut().add_message(processed_msg.clone(), false) {
                    // エラーは無視
                }
                
                // 自動保存
                if let Err(_) = self.save_history() {
                    // エラーは無視
                }
            }
            ChatEvent::Error(err) => {
                self.messages.push(ChatMessage {
                    content: format!("Error: {}", err),
                    is_user: false,
                });
                self.is_loading = false;
                self.scroll_to_bottom();
            }
        }
    }

    pub fn send_message(&mut self) {
        self.notification = None;
        let original_message = self.input.clone();

        // /clearlogコマンド判定
        if original_message.trim() == "/clearlog" {
            match self.history_manager.clear_messages() {
                Ok(_) => {
                    self.messages.clear();
                    self.messages.push(ChatMessage {
                        content: "✅ ログを全て削除しました.".to_string(),
                        is_user: false,
                    });
                    self.scroll_to_bottom();
                }
                Err(e) => {
                    self.messages.push(ChatMessage {
                        content: format!("❌ ログ削除に失敗しました: {}", e),
                        is_user: false,
                    });
                    self.scroll_to_bottom();
                }
            }
            self.input.clear();
            self.cursor_position = 0;
            self.input_mode = InputMode::Normal;
            self.input_line_count = 1;
            self.selected_files.clear();
            self.history_index = None;
            self.temp_input.clear();
            return;
        }
        
        // プロンプト履歴に追加（空でない場合）
        if !original_message.trim().is_empty() {
            self.add_to_input_history(original_message.clone());
        }
        
        self.input.clear();
        self.cursor_position = 0;
        self.input_mode = InputMode::Normal;
        self.is_loading = true;
        self.input_line_count = 1;  // 送信後は1行にリセット
        
        // 履歴ナビゲーションをリセット
        self.history_index = None;
        self.temp_input.clear();

        // ファイル参照を解析
        let (clean_message, file_paths) = self.parse_file_references(&original_message);
        let message_to_send = if clean_message.is_empty() && !file_paths.is_empty() {
            "Please analyze these files:".to_string()
        } else {
            clean_message
        };

        // TODOリストが存在しない場合、新しく作成するか確認
        if self.todo_manager.current_list.is_none() && self.todo_manager.should_create_new_list(&message_to_send) {
            if let Err(e) = self.todo_manager.create_new_list(
                format!("タスク: {}", message_to_send.chars().take(30).collect::<String>()),
                message_to_send.clone()
            ) {
                self.show_notification(&format!("Error creating todo list: {}", e));
            }
        }

        // 履歴管理にメッセージを追加
        if let Err(_) = self.history_manager.get_history_mut().add_message(message_to_send.clone(), true) {
            // エラーは無視
        }

        // ユーザーメッセージを表示用に整形
        let display_message = if file_paths.is_empty() {
            message_to_send.clone()
        } else {
            format!("{}\nFiles: {}", message_to_send, file_paths.join(", "))
        };

        // ユーザーメッセージを即座に追加
        self.messages.push(ChatMessage {
            content: display_message,
            is_user: true,
        });
        self.scroll_to_bottom();

        // 会話コンテキストを取得
        let mut context = self.history_manager.get_conversation_context(10);
        
        // TODOリストのコンテキストを追加
        let todo_context = self.todo_manager.get_context_for_llm();
        if !todo_context.is_empty() {
            context.push(format!("\n## Current TODO List Context:\n{}", todo_context));
        }

        // AIレスポンスを非同期で取得
        let sender = self.event_sender.clone();
        let client = self.gemini_client.clone();
        
        tokio::spawn(async move {
            let result = if file_paths.is_empty() {
                // ファイルなしの通常チャット
                if context.is_empty() {
                    client.chat(&message_to_send).await
                } else {
                    client.chat_with_context(&message_to_send, &context).await
                }
            } else {
                // ファイル付きチャット
                client.chat_with_file_context(&message_to_send, &file_paths, &context).await
            };

            match result {
                Ok(response) => {
                    let _ = sender.send(ChatEvent::AIResponse(response));
                }
                Err(e) => {
                    let _ = sender.send(ChatEvent::Error(e.to_string()));
                }
            }
        });

        // 選択されたファイルをクリア
        self.selected_files.clear();
    }

    pub fn create_new_session(&mut self) {
        let _session_id = self.history_manager.get_history_mut().new_session(None);
        self.messages.clear();
        self.messages.push(ChatMessage {
            content: "Started new conversation session.".to_string(),
            is_user: false,
        });
        self.scroll_to_bottom();
        
        if let Err(_) = self.save_history() {
            // エラーは無視
        }
    }

    pub fn save_history(&mut self) -> Result<()> {
        self.history_manager.save()
    }

    pub fn scroll_to_bottom(&mut self) {
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len().saturating_sub(1);
            self.list_state.select(Some(self.scroll_offset));
        }
    }

    // 選択されたメッセージを入力欄に挿入
    pub fn insert_selected_message(&mut self) {
        if let Some(selected_index) = self.list_state.selected() {
            if let Some(message) = self.messages.get(selected_index) {
                // プレフィックス（"You: " または "AI: "）を除去して、メッセージ内容のみを取得
                let content = message.content.clone();
                
                // 入力欄が空でない場合は、スペースまたは改行を追加
                if !self.input.is_empty() {
                    self.input.push('\n');
                }
                
                // メッセージ内容を入力欄に追加
                self.input.push_str(&content);
                
                // カーソル位置を最後に移動
                self.cursor_position = self.input.graphemes(true).count();
                
                // 入力行数を更新
                self.update_input_line_count();
                
                // インサートモードに切り替え
                self.input_mode = InputMode::Insert;
            }
        }
    }

    // ファイルパス解析機能
    pub fn parse_file_references(&self, message: &str) -> (String, Vec<String>) {
        let mut clean_message = message.to_string();
        let mut file_paths = Vec::new();
        
        // @file:path 形式を手動で検索
        let mut remaining = message;
        loop {
            if let Some(start) = remaining.find("@file:") {
                let file_start = start + 6; // "@file:" の長さ
                let after_prefix = &remaining[file_start..];
                
                // ファイルパスの終端を見つける（スペースまたは文字列の終端）
                let end_pos = after_prefix.find(' ').unwrap_or(after_prefix.len());
                let file_path = &after_prefix[..end_pos];
                
                if !file_path.is_empty() {
                    file_paths.push(file_path.to_string());
                }
                
                // ファイル参照を削除
                let full_reference = format!("@file:{}", file_path);
                clean_message = clean_message.replace(&full_reference, "");
                
                // 残りの文字列を更新
                remaining = &remaining[start + 6 + file_path.len()..];
            } else {
                break;
            }
        }
        
        // 選択されたファイルも追加
        let mut all_files = file_paths;
        all_files.extend(self.selected_files.clone());
        
        // 重複を削除
        all_files.sort();
        all_files.dedup();
        
        (clean_message.trim().to_string(), all_files)
    }

    // AIレスポンスからファイル作成要求を解析・実行
    pub fn process_file_creation_requests(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        
        // ```create_file:filename の形式でファイル作成要求を検索
        let create_file_pattern = r"(?s)```create_file:([^\n]+)(?:\r?\n(.*?))?```";
        let re = match Regex::new(create_file_pattern) {
            Ok(regex) => regex,
            Err(_) => {
                return self.manual_parse_file_creation(response);
            }
        };
        
        let mut files_created = Vec::new();
        let matches: Vec<_> = re.captures_iter(response).collect();
        if matches.is_empty() {
            return response.to_string();
        }
        
        for caps in matches.iter() {
            if let Some(filename_match) = caps.get(1) {
                let filename = filename_match.as_str().trim();
                let content = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                match self.gemini_client.create_file_with_unique_name(filename, content) {
                    Ok(actual_filename) => {
                        files_created.push(actual_filename.clone());
                        let success_message = if actual_filename == filename {
                            format!("✅ File \'{}\' created successfully!", filename)
                        } else {
                            format!("✅ File \'{}\' created as \'{}\' (original name was taken)", filename, actual_filename)
                        };
                        processed_response = processed_response.replace(
                            &caps[0],
                            &success_message
                        );
                    }
                    Err(e) => {
                        processed_response = processed_response.replace(
                            &caps[0],
                            &format!("❌ Failed to create file \'{}\' : {}", filename, e)
                        );
                        continue;
                    }
                }
            }
        }
        
        if !files_created.is_empty() {
            self.refresh_directory_contents();
            let summary = format!("📁 ファイル作成: {}", files_created.join(", "));
            self.notification = Some(summary);
        }
        
        processed_response
    }

    // Regexが使えない場合の手動解析
    pub fn manual_parse_file_creation(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        let mut files_created = Vec::new();
        
        // ```create_file: で始まる行を検索
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;
        
        while i < lines.len() {
            if lines[i].starts_with("```create_file:") {
                // ファイル名を抽出
                let filename = lines[i].strip_prefix("```create_file:").unwrap_or("").trim();
                if filename.is_empty() {
                    i += 1;
                    continue;
                }
                
                // コンテンツを収集（次の ``` まで）
                let mut content_lines = Vec::new();
                i += 1;
                
                while i < lines.len() && !lines[i].starts_with("```") {
                    content_lines.push(lines[i]);
                    i += 1;
                }
                
                let content = content_lines.join("\n");
                
                // ファイルを作成（重複チェック付き）
                match self.gemini_client.create_file_with_unique_name(filename, &content) {
                    Ok(actual_filename) => {
                        files_created.push(actual_filename.clone());
                        
                        // 成功メッセージで置換
                        let original_block = format!("```create_file:{}\n{}\n```", filename, content);
                        let success_message = if actual_filename == filename {
                            format!("✅ File \'{}\' created successfully!", filename)
                        } else {
                            format!("✅ File \'{}\' created as \'{}\' (original name was taken)", filename, actual_filename)
                        };
                        processed_response = processed_response.replace(&original_block, &success_message);
                    }
                    Err(e) => {
                        // エラーメッセージで置換
                        let original_block = format!("```create_file:{}\n{}\n```", filename, content);
                        let error_msg = format!("❌ Failed to create file \'{}\' : {}", filename, e);
                        processed_response = processed_response.replace(&original_block, &error_msg);
                    }
                }
            }
            i += 1;
        }
        
        if !files_created.is_empty() {
            self.refresh_directory_contents();
            
            let summary = format!("\n\n📁 Created {} file(s): {}", 
                files_created.len(), 
                files_created.join(", ")
            );
            processed_response.push_str(&summary);
        }
        
        processed_response
    }

    pub fn truncate_string_safe(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            s.to_string()
        } else {
            s.chars().take(max_chars).collect::<String>() + "..."
        }
    }

    // calculate_cursor_position は他の場所でも使う可能性があるのでここに残す
    pub fn calculate_cursor_position(&self) -> (usize, usize) {
        let mut current_line = 0;
        let mut current_column = 0;

        for (i, c) in self.input.graphemes(true).enumerate() {
            if i == self.cursor_position {
                current_column = UnicodeWidthStr::width(self.input.graphemes(true).take(i).collect::<String>().as_str());
                break;
            }
            if c == "\n" {
                current_line += 1;
            }
        }
        
        // カーソル位置が入力の最後にある場合
        if self.cursor_position == self.input.graphemes(true).count() {
            let last_line_start_pos = self.get_line_start_position(current_line);
            current_column = UnicodeWidthStr::width(self.input.graphemes(true).skip(last_line_start_pos).collect::<String>().as_str());
        }

        (current_line, current_column)
    }

    // update_input_line_count は他の場所でも使う可能性があるのでここに残す
    pub fn update_input_line_count(&mut self) {
        self.input_line_count = self.input.lines().count().max(1);
    }

    // add_to_input_history は他の場所でも使う可能性があるのでここに残す
    pub fn add_to_input_history(&mut self, message: String) {
        // 重複する最後の履歴は追加しない
        if self.input_history.last().map_or(true, |last| last != &message) {
            self.input_history.push(message);
        }
        // 履歴の最大数を制限（例: 50件）
        if self.input_history.len() > 50 {
            self.input_history.remove(0);
        }
        self.history_index = None; // 新しい入力があったら履歴ナビゲーションをリセット
    }

    // navigate_history_up は他の場所でも使う可能性があるのでここに残す
    pub fn navigate_history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            Some(idx) => {
                if idx > 0 {
                    idx - 1
                } else {
                    0
                }
            }
            None => {
                // 履歴ナビゲーション開始時、現在の入力を一時保存
                self.temp_input = self.input.clone();
                self.input_history.len() - 1
            }
        };
        self.history_index = Some(new_index);
        self.input = self.input_history[new_index].clone();
        self.cursor_position = self.input.graphemes(true).count();
        self.update_input_line_count();
    }

    // navigate_history_down は他の場所でも使う可能性があるのでここに残す
    pub fn navigate_history_down(&mut self) {
        if self.input_history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            Some(idx) => {
                if idx < self.input_history.len() - 1 {
                    idx + 1
                } else {
                    // 履歴の最後に到達したら一時保存した入力に戻す
                    self.reset_history_navigation();
                    return;
                }
            }
            None => return, // 履歴ナビゲーション中でない場合は何もしない
        };
        self.history_index = Some(new_index);
        self.input = self.input_history[new_index].clone();
        self.cursor_position = self.input.graphemes(true).count();
        self.update_input_line_count();
    }

    // reset_history_navigation は他の場所でも使う可能性があるのでここに残す
    pub fn reset_history_navigation(&mut self) {
        if self.history_index.is_some() {
            self.input = self.temp_input.clone();
            self.temp_input.clear();
            self.history_index = None;
            self.cursor_position = self.input.graphemes(true).count();
            self.update_input_line_count();
        }
    }

    /// LLM自動ループをチャット欄に進行状況を表示しながら実行する
    pub async fn chat_loop_with_progress(
        &mut self,
        initial_message: &str,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        let mut message = initial_message.to_string();
        let mut step = 1;
        let sender = self.event_sender.clone();
        loop {
            let progress_msg = format!("🤖 Step {}: LLMに問い合わせ中...", step);
            self.push_ai_progress_message(progress_msg.clone(), terminal);
            let _ = sender.send(ChatEvent::AIResponse(progress_msg));

            let prompt = format!(
                "{}\n\n---\n次に何をすべきか、追加タスクがあるかを必ず明示してください。\n「完了」「終了」「何もする必要がない」などの場合は、その旨を明確に書いてください。",
                message
            );
            let response = self.gemini_client.chat(&prompt).await?;
            let response_msg = format!("🤖 Step {}: LLM応答\n{}", step, response);
            self.push_ai_progress_message(response_msg.clone(), terminal);
            let _ = sender.send(ChatEvent::AIResponse(response_msg));

            let lower = response.to_lowercase();
            if lower.contains("完了") || lower.contains("終了") || lower.contains("何もする必要がない") || lower.contains("nothing to do") {
                let finish_msg = "✅ LLMが終了を指示したためループを終了します。".to_string();
                self.push_ai_progress_message(finish_msg.clone(), terminal);
                let _ = sender.send(ChatEvent::AIResponse(finish_msg));
                break;
            }
            message = response;
            step += 1;
        }
        Ok(())
    }
    
    
}
