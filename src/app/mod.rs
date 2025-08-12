use crate::gemini::Part; // Add this import
use crate::gemini::ResponsePart; // Add this import
use crate::debug_log;
use ratatui::{
    widgets::ListState,
};
use uuid::Uuid;
use chrono::Utc;
use tokio::sync::mpsc;
use crate::gemini::GeminiClient;
use crate::history::HistoryManager;
// use anyhow::Result; // Unused import
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;
use std::sync::{Arc, Mutex};

// モジュール宣言
pub mod handler;
pub mod ui;
pub mod file_operations;
pub mod session_management;
pub mod cursor_movement;
pub mod visual_mode;
pub mod terminal_util;

pub use crate::app::ui::ChatEvent;

pub use crate::app::ui::UiState;

pub struct ChatApp {
    pub ui: UiState,
    pub messages: Vec<crate::history::ChatMessage>,
    pub gemini_client: GeminiClient,
    pub event_sender: mpsc::UnboundedSender<ChatEvent>,
    pub event_receiver: mpsc::UnboundedReceiver<ChatEvent>,
    pub is_loading: bool,
    pub history_manager: Arc<Mutex<HistoryManager>>,
    //pub todo_manager: TodoManager,
    pub llm_task_handle: Option<tokio::task::JoinHandle<()>>, // LLMリクエスト用タスクハンドル
    pub send_buffer: std::collections::VecDeque<String>, // チャット送信バッファ
    // pub terminal: Option<Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>>,
}

pub use crate::app::ui::InputMode;


impl ChatApp {
    pub fn new(
        mut gemini_client: GeminiClient,
        history_manager: Arc<Mutex<HistoryManager>>,
    ) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        // アクティブなセッションを確保
        let _session_id = (*history_manager.lock().unwrap()).ensure_active_session();
        
        // 現在のセッションからメッセージを読み込み
        let mut messages = Vec::new();
                if let Some(session) = (*history_manager.lock().unwrap()).get_history().get_current_session() {
            for hist_msg in &session.messages {
                messages.push(crate::history::ChatMessage {
                    id: hist_msg.id,
                    parts: hist_msg.parts.clone(), // Changed from content
                    is_user: hist_msg.is_user,
                    timestamp: hist_msg.timestamp,
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
        let mut app = Self {
            ui: UiState {
                input: String::new(),
                cursor_position: 0,
                visual_start: None,
                input_mode: InputMode::Normal,
                list_state: ListState::default(),
                scroll_offset: 0,
                session_list_state: ListState::default(),
                file_browser_state: ListState::default(),
                current_directory: current_dir,
                directory_contents: Vec::new(),
                selected_files: Vec::new(),
                input_line_count: 1,
                input_history: Vec::new(),
                history_index: None,
                temp_input: String::new(),
                show_help: false,
                notification: None,
            },
            messages,
            gemini_client,
            event_sender,
            event_receiver,
            is_loading: false,
            history_manager,
            llm_task_handle: None,
            send_buffer: std::collections::VecDeque::new(),
        };

        // 歓迎メッセージを追加（履歴が空の場合のみ）
        // 歓迎メッセージを追加（履歴が空の場合のみ）
        if app.messages.is_empty() {
            app.messages.push(crate::history::ChatMessage {
                id: Uuid::new_v4(),
                parts: vec![crate::gemini::Part::Text { text: "Welcome to ConTUI!".to_string() }], // Changed from content
                is_user: false,
                timestamp: Utc::now(),
            });
        }

        app
    }

    pub fn handle_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::AIResponse(response_part) => { // Changed msg to response_part and type
                let response_text = match &response_part {
                    ResponsePart::Text { text } => text.clone(),
                    ResponsePart::FunctionCall { function_call } => {
                        format!("FunctionCall: {}", serde_json::to_string_pretty(&function_call).unwrap_or_default())
                    },
                    ResponsePart::FunctionResponse { function_response } => {
                        format!("FunctionResponse: {}", serde_json::to_string_pretty(&function_response).unwrap_or_default())
                    },
                };
                debug_log!("[handle_chat_event] AIResponse: {}", response_text);

                // ファイル作成要求を処理 (This part needs to be re-evaluated if it's still needed)
                // For now, let's assume process_file_creation_requests expects a String
                let processed_msg = self.process_file_creation_requests(&response_text);
                
                let final_msg_content = if processed_msg.is_empty() {
                    "AIからの応答がありませんでした。".to_string()
                } else {
                    processed_msg
                };
                
                // AIレスポンスをメッセージリストに追加
                let ai_msg = crate::history::ChatMessage {
                    id: Uuid::new_v4(),
                    parts: vec![crate::gemini::Part::Text { text: final_msg_content.clone() }], // Changed content to parts
                    is_user: false,
                    timestamp: Utc::now(),
                };
                self.messages.push(ai_msg);
                debug_log!("[handle_chat_event] メッセージ追加: {}", final_msg_content);
                self.is_loading = false;
                
                // スクロール位置の自動調整
                self.auto_scroll_if_at_bottom();
                
                // バッファがあれば自動送信イベントを発火
                if let Some(next) = self.send_buffer.pop_front() {
                    debug_log!("[handle_chat_event] バッファから自動送信イベント: {}", next);
                    // ChatEvent::AIResponseでバッファ送信要求を通知
                    let _ = self.event_sender.send(ChatEvent::AIResponse(ResponsePart::Text { text: format!("[BUFFERED_SEND]{}", next) }));
                }
                
                // 履歴管理にAIレスポンスを追加（画面表示と同じ内容を保存）
                // 必ず表示中セッションに保存する
                {
                    let mut history_guard = self.history_manager.lock().unwrap();
                    if let Some(session) = history_guard.get_history().get_current_session() {
                        let session_id = session.id;
                        let _ = history_guard.get_history_mut().switch_session(session_id);
                    }
                    // Add the original ResponsePart to history
                    let parts_to_add_to_history = match response_part {
                        ResponsePart::Text { text } => vec![Part::Text { text }],
                        ResponsePart::FunctionCall { function_call } => vec![Part::Text { text: serde_json::to_string(&function_call).unwrap_or_default() }],
                        ResponsePart::FunctionResponse { function_response } => vec![Part::Text { text: serde_json::to_string(&function_response).unwrap_or_default() }],
                    };
                    let _ = (*history_guard).get_history_mut().add_message(parts_to_add_to_history, false);
                    debug_log!("[handle_chat_event] current_session_id: {:?}", history_guard.get_history().current_session_id);
                }

                // AIレスポンス追加直後に履歴保存
                if let Err(e) = (*self.history_manager.lock().unwrap()).save() {
                    debug_log!("[handle_chat_event] save_history error: {:?}", e);
                }
            }
            ChatEvent::Error(msg) => {
                debug_log!("[handle_chat_event] Error: {}", msg);
                self.ui.notification = Some(msg);
                self.is_loading = false;
            }
        }
    }

    pub async fn send_message(&mut self, _terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) {
        self.ui.notification = None;
        let original_message = self.ui.input.clone();
        debug_log!("[send_message] called. input={}", original_message);
        // LLM応答待ち中ならバッファに積むだけ
        if self.is_loading {
            debug_log!("[send_message] is_loading=true, bufferに積んだ: {}", original_message);
            self.send_buffer.push_back(original_message.clone());
            return;
        }

        // /clearlogコマンド判定
        if original_message.trim() == "/clearlog" {
            match (*self.history_manager.lock().unwrap()).clear_messages() {
                Ok(_) => {
                    self.messages.clear();
                    self.messages.push(crate::history::ChatMessage {
                        id: Uuid::new_v4(),
                        parts: vec![Part::Text { text: "✅ ログを全て削除しました.".to_string() }], // Changed content to parts
                        is_user: false,
                        timestamp: Utc::now(),
                    });
                }
                Err(e) => {
                    self.messages.push(crate::history::ChatMessage {
                        id: Uuid::new_v4(),
                        parts: vec![Part::Text { text: format!("❌ ログ削除に失敗しました: {}", e) }], // Changed content to parts
                        is_user: false,
                        timestamp: Utc::now(),
                    });
                }
            }
            self.ui.input.clear();
            self.ui.cursor_position = 0;
            self.ui.input_mode = InputMode::Normal;
            self.ui.input_line_count = 1;
            self.ui.selected_files.clear();
            self.ui.history_index = None;
            self.ui.temp_input.clear();
            return;
        }

        // プロンプト履歴に追加（空でない場合）
        if !original_message.trim().is_empty() {
            self.add_to_input_history(original_message.clone());
        }

        self.ui.input.clear();
        self.ui.cursor_position = 0;
        self.ui.input_mode = InputMode::Normal;
        self.is_loading = true;
        self.ui.input_line_count = 1;  // 送信後は1行にリセット

        // 履歴ナビゲーションをリセット
        self.ui.history_index = None;
        self.ui.temp_input.clear();

        // ファイル参照を解析
        let (clean_message, file_paths) = self.parse_file_references(&original_message);
        let message_to_send = if clean_message.is_empty() && !file_paths.is_empty() {
            "Please analyze these files:".to_string()
        } else {
            clean_message
        };


        // ユーザーメッセージを表示用に整形
        let display_message = if file_paths.is_empty() {
            message_to_send.clone()
        } else {
            format!("{}\nFiles: {}", message_to_send, file_paths.join(", "))
        };

        // ユーザーメッセージを即座に追加（新しいUUIDで）
        let user_msg = crate::history::ChatMessage {
            id: Uuid::new_v4(),
            parts: vec![Part::Text { text: display_message.clone() }], // Changed content to parts
            is_user: true,
            timestamp: Utc::now(),
        };
        self.messages.push(user_msg.clone());
        debug_log!("[send_message] メッセージ追加: {}", display_message); // Log the display_message

        // 履歴管理にメッセージを追加（表示用と同じ内容）
        if let Err(_) = (*self.history_manager.lock().unwrap()).get_history_mut().add_message(vec![Part::Text { text: display_message.clone() }], true) { // Pass Vec<Part>
            // エラーは無視
        }
        
        // ユーザーメッセージ送信後に履歴保存
        if let Err(e) = (*self.history_manager.lock().unwrap()).save() {
            debug_log!("[send_message] save_history error: {:?}", e);
        }

        // 非同期でLLMに送信
        // 既存のLLMタスクがあればキャンセル
        if let Some(handle) = self.llm_task_handle.take() {
            handle.abort();
        }
        let message = message_to_send.clone();
        let sender = self.event_sender.clone();
        let gemini_client = self.gemini_client.clone();
        let history_manager_clone = self.history_manager.clone();
        let handle = tokio::spawn(async move {
            debug_log!("[tokio::spawn] chat_loop_with_progress_static spawn. message={}", message);
            let res = ChatApp::chat_loop_with_progress_static(gemini_client, &message, sender.clone(), history_manager_clone).await;
            if let Err(_e) = res {
                // 通常のエラーは既に送信済み
            }
        });
        self.llm_task_handle = Some(handle);

        // 選択されたファイルをクリア
        self.ui.selected_files.clear();
        self.is_loading = false;
    }

    /// LLMリクエストをspawn用にstatic化したバージョン
    pub async fn chat_loop_with_progress_static(
        gemini_client: crate::gemini::GeminiClient,
        initial_message: &str,
        sender: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
        history_manager: Arc<Mutex<HistoryManager>>, // Added this
    ) -> anyhow::Result<()> {
        let mut message = initial_message.to_string();
        let mut step = 1;
        debug_log!("[chat_loop_with_progress_static] start. message={}", message);
        for _ in 0..10 {
            debug_log!("[chat_loop_with_progress_static] step={}", step);
            let progress_msg = format!("🤖 Step {}: LLMに問い合わせ中...", step);
            let _ = sender.send(ChatEvent::AIResponse(ResponsePart::Text { text: progress_msg }));
            let prompt = format!(
                "{}

---
次に何をすべきか、追加タスクがあるかを必ず明示してください。
「完了」「終了」「何もする必要がない」などの場合は、その旨を明確に書いてください。",
                message
            );
            debug_log!("[chat_loop_with_progress_static] prompt={}", prompt);

            // Get conversation context from history_manager
            let conversation_context = (*history_manager.lock().unwrap()).get_conversation_context(10); // Use history_manager
            let response_part = match tokio::time::timeout(std::time::Duration::from_secs(30), gemini_client.chat(&prompt, Some(&conversation_context))).await {
                Ok(r) => r,
                Err(_) => {
                    debug_log!("[chat_loop_with_progress_static] LLMリクエストがタイムアウトしました");
                    let error_msg = "❌ LLMリクエストがタイムアウトしました".to_string();
                    let _ = sender.send(ChatEvent::Error(error_msg));
                    return Err(anyhow::anyhow!("LLMリクエストがタイムアウト"));
                }
            };
            match response_part {
                Ok(response_part_content) => {
                    let response_text = match &response_part_content {
                        ResponsePart::Text { text } => text.clone(),
                        ResponsePart::FunctionCall { function_call } => {
                            format!("FunctionCall: {}", serde_json::to_string_pretty(&function_call).unwrap_or_default())
                        },
                        ResponsePart::FunctionResponse { function_response } => {
                            format!("FunctionResponse: {}", serde_json::to_string_pretty(&function_response).unwrap_or_default())
                        },
                    };
                    debug_log!("[chat_loop_with_progress_static] LLM response={}", response_text);
                    if response_text.is_empty() {
                        let error_msg = "❌ LLMからの応答が空です。再試行してください。".to_string();
                        let _ = sender.send(ChatEvent::Error(error_msg));
                        return Err(anyhow::anyhow!("LLM応答が空"));
                    }
                    let _response_msg = format!("🤖 Step {}: LLM応答\n{}", step, response_text);
                    let _ = sender.send(ChatEvent::AIResponse(response_part_content.clone())); // Send ResponsePart

                    // Add AI's response to history
                    let mut history_guard = history_manager.lock().unwrap();
                    let parts_to_add_to_history = match response_part_content.clone() {
                        ResponsePart::Text { text } => vec![Part::Text { text }],
                        ResponsePart::FunctionCall { function_call } => vec![Part::Text { text: serde_json::to_string(&function_call).unwrap_or_default() }],
                        ResponsePart::FunctionResponse { function_response } => vec![Part::Text { text: serde_json::to_string(&function_response).unwrap_or_default() }],
                    };
                    (*history_guard).get_history_mut().add_message(parts_to_add_to_history, false)?;

                    let lower = response_text.to_lowercase();
                    if gemini_client.extract_is_finished_flag(&lower).unwrap_or(false) {
                        // 最終的なAIレスポンスを送信（履歴保存用）
                        let _ = sender.send(ChatEvent::AIResponse(response_part_content.clone())); // Send ResponsePart
                        let finish_msg = "✅ LLMが終了を指示したためループを終了します。".to_string();
                        let _ = sender.send(ChatEvent::AIResponse(ResponsePart::Text { text: finish_msg })); // Send as Text
                        debug_log!("[chat_loop_with_progress_static] finish (done)");
                        return Ok(())
                    }
                    message = response_text; // Use extracted text for next prompt
                    step += 1;
                }
                Err(e) => {
                    debug_log!("[chat_loop_with_progress_static] LLM error={}", e);
                    let error_msg = format!("❌ LLMとの通信に失敗しました: {}", e);
                    let _ = sender.send(ChatEvent::Error(error_msg));
                    return Err(e.into());
                }
            };
        }
        // 最後のメッセージを最終レスポンスとして送信
        if !message.is_empty() {
            let _ = sender.send(ChatEvent::AIResponse(ResponsePart::Text { text: message }));
        }
        let finish_msg = "⚠️ LLM応答に「完了」等が含まれなかったため自動終了しました。".to_string();
        let _ = sender.send(ChatEvent::AIResponse(ResponsePart::Text { text: finish_msg }));
        debug_log!("[chat_loop_with_progress_static] finish (timeout)");
        Ok(())
    }


    

    pub fn add_to_input_history(&mut self, message: String) {
        if self.ui.input_history.last().map_or(true, |last| last != &message) {
            self.ui.input_history.push(message);
        }
        if self.ui.input_history.len() > 50 {
            self.ui.input_history.remove(0);
        }
        self.ui.history_index = None;
    }

    pub fn parse_file_references(&self, message: &str) -> (String, Vec<String>) {
        let mut clean_message = message.to_string();
        let mut file_paths = Vec::new();
        let mut remaining = message;
        loop {
            if let Some(start) = remaining.find("@file:") {
                let file_start = start + 6;
                let after_prefix = &remaining[file_start..];
                let end_pos = after_prefix.find(' ').unwrap_or(after_prefix.len());
                let file_path = &after_prefix[..end_pos];
                if !file_path.is_empty() {
                    file_paths.push(file_path.to_string());
                }
                let full_reference = format!("@file:{}", file_path);
                clean_message = clean_message.replace(&full_reference, "");
                remaining = &remaining[start + 6 + file_path.len()..];
            } else {
                break;
            }
        }
        let mut all_files = file_paths;
        all_files.extend(self.ui.selected_files.clone());
        all_files.sort();
        all_files.dedup();
        (clean_message.trim().to_string(), all_files)
    }

    pub fn calculate_cursor_position(&self) -> (usize, usize) {
        let mut current_line = 0;
        let mut current_column = 0;
        for (i, c) in self.ui.input.graphemes(true).enumerate() {
            if i == self.ui.cursor_position {
                current_column = UnicodeWidthStr::width(self.ui.input.graphemes(true).take(i).collect::<String>().as_str());
                break;
            }
            if c == "\n" {
                current_line += 1;
            }
        }
        if self.ui.cursor_position == self.ui.input.graphemes(true).count() {
            let last_line_start_pos = self.get_line_start_position(current_line);
            current_column = UnicodeWidthStr::width(self.ui.input.graphemes(true).skip(last_line_start_pos).collect::<String>().as_str());
        }
        (current_line, current_column)
    }

    pub fn update_input_line_count(&mut self) {
        self.ui.input_line_count = self.ui.input.lines().count().max(1);
    }

    pub fn navigate_history_up(&mut self) {
        if self.ui.input_history.is_empty() {
            return;
        }
        let new_index = match self.ui.history_index {
            Some(idx) => {
                if idx > 0 {
                    idx - 1
                } else {
                    0
                }
            }
            None => {
                self.ui.temp_input = self.ui.input.clone();
                self.ui.input_history.len() - 1
            }
        };
        self.ui.history_index = Some(new_index);
        self.ui.input = self.ui.input_history[new_index].clone();
        self.ui.cursor_position = self.ui.input.graphemes(true).count();
        self.update_input_line_count();
    }

    pub fn navigate_history_down(&mut self) {
        if self.ui.input_history.is_empty() {
            return;
        }
        let new_index = match self.ui.history_index {
            Some(idx) => {
                if idx < self.ui.input_history.len() - 1 {
                    idx + 1
                } else {
                    self.reset_history_navigation();
                    return;
                }
            }
            None => return,
        };
        self.ui.history_index = Some(new_index);
        self.ui.input = self.ui.input_history[new_index].clone();
        self.ui.cursor_position = self.ui.input.graphemes(true).count();
        self.update_input_line_count();
    }

    pub fn reset_history_navigation(&mut self) {
        if self.ui.history_index.is_some() {
            self.ui.input = self.ui.temp_input.clone();
            self.ui.temp_input.clear();
            self.ui.history_index = None;
            self.ui.cursor_position = self.ui.input.graphemes(true).count();
            self.update_input_line_count();
        }
    }

    pub fn insert_selected_message(&mut self) {
        if let Some(selected_index) = self.ui.list_state.selected() {
            if let Some(message) = self.messages.get(selected_index) {
                let mut content_to_insert = String::new();
                for part in &message.parts {
                    if let crate::gemini::Part::Text { text } = part {
                        content_to_insert.push_str(text);
                    }
                }
                if !self.ui.input.is_empty() {
                    self.ui.input.push('\n');
                }
                self.ui.input.push_str(&content_to_insert);
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
                self.update_input_line_count();
                self.ui.input_mode = InputMode::Insert;
            }
        }
    }

    pub fn create_new_session(&mut self) {
        let _session_id = (*self.history_manager.lock().unwrap()).get_history_mut().new_session(None);
        self.messages.clear();
        self.messages.push(crate::history::ChatMessage {
            id: Uuid::new_v4(),
            parts: vec![Part::Text { text: "Started new conversation session.".to_string() }], // Changed content to parts
            is_user: false,
            timestamp: Utc::now(),
        });
        if let Err(e) = (*self.history_manager.lock().unwrap()).save() {
            debug_log!("[create_new_session] save_history error: {:?}", e);
        }
    }

    pub fn scroll_to_bottom(&mut self, visible_height: usize) {
        if !self.messages.is_empty() {
            let total_lines = self.messages.iter().map(|msg| {
                let prefix = if msg.is_user { "You" } else { "AI" };
                let mut msg_content_text = String::new();
                for part in &msg.parts {
                    if let crate::gemini::Part::Text { text } = part {
                        msg_content_text.push_str(text);
                    }
                }
                let content = format!("{}: {}", prefix, msg_content_text);
                crate::markdown::wrap_text(&content, 72).lines().count()
            }).sum::<usize>();
            self.ui.scroll_offset = total_lines.saturating_sub(visible_height);
            self.ui.list_state.select(Some(self.ui.scroll_offset));
        }
    }

    pub fn truncate_string_safe(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            s.to_string()
        } else {
            s.chars().take(max_chars).collect::<String>() + "..."
        }
    }

    fn get_line_start_position(&self, line: usize) -> usize {
        let mut current_line = 0;
        for (i, c) in self.ui.input.graphemes(true).enumerate() {
            if current_line == line {
                return i;
            }
            if c == "\n" {
                current_line += 1;
            }
        }
        0
    }
}
