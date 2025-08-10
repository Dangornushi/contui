use std::io::Write;
use ratatui::{
    widgets::ListState,
};
use uuid::Uuid;
use chrono::Utc;
use tokio::sync::mpsc;
use crate::gemini::GeminiClient;
use crate::history::HistoryManager;
use crate::todo_manager::TodoManager;
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

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
    pub history_manager: HistoryManager,
    //pub todo_manager: TodoManager,
    pub llm_task_handle: Option<tokio::task::JoinHandle<()>>, // LLMリクエスト用タスクハンドル
    pub send_buffer: std::collections::VecDeque<String>, // チャット送信バッファ
    // pub terminal: Option<Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>>,
}

pub use crate::app::ui::InputMode;


impl ChatApp {
    pub fn new(
        mut gemini_client: GeminiClient,
        mut history_manager: HistoryManager,
    ) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        // アクティブなセッションを確保
        let _session_id = history_manager.ensure_active_session();
        
        // 現在のセッションからメッセージを読み込み
        let mut messages = Vec::new();
        if let Some(session) = history_manager.get_history().get_current_session() {
            for hist_msg in &session.messages {
                messages.push(crate::history::ChatMessage {
                    id: hist_msg.id,
                    content: hist_msg.content.clone(),
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
        
        let todo_manager = TodoManager::new().unwrap_or_else(|_| {
            // TODOマネージャーの初期化に失敗した場合は空のマネージャーを作成
            TodoManager { current_list: None, storage_path: "todo_state.json".to_string() }
        });

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
        if app.messages.is_empty() {
            app.messages.push(crate::history::ChatMessage {
                id: Uuid::new_v4(),
                content: "Welcome to ConTUI!".to_string(),
                is_user: false,
                timestamp: Utc::now(),
            });
        }

        app
    }

    pub fn handle_chat_event(&mut self, event: ChatEvent) {
        use std::io::Write;
        match event {
            ChatEvent::AIResponse(msg) => {
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                    let _ = writeln!(f, "[handle_chat_event] AIResponse: {}", msg);
                }
                // ファイル作成要求を処理
                let processed_msg = self.process_file_creation_requests(&msg);
                
                // TODOリストの自動更新を実行
                //let _updated_items = self.todo_manager.update_from_ai_response(&processed_msg).unwrap_or_default();

                let final_msg = if processed_msg.is_empty() {
                    "AIからの応答がありませんでした。".to_string()
                } else {
                    processed_msg
                };
                
                // AIレスポンスをメッセージリストに追加
                let ai_msg = crate::history::ChatMessage {
                    id: Uuid::new_v4(),
                    content: final_msg.clone(),
                    is_user: false,
                    timestamp: Utc::now(),
                };
                self.messages.push(ai_msg);
                self.is_loading = false;
                
                // スクロール位置の自動調整
                self.auto_scroll_if_at_bottom();
                
                // バッファがあれば自動送信イベントを発火
                if let Some(next) = self.send_buffer.pop_front() {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[handle_chat_event] バッファから自動送信イベント: {}", next);
                    }
                    // ChatEvent::AIResponseでバッファ送信要求を通知
                    let _ = self.event_sender.send(ChatEvent::AIResponse(format!("[BUFFERED_SEND]{}", next)));
                }
                
                // 履歴管理にAIレスポンスを追加（画面表示と同じ内容を保存）
                // 必ず表示中セッションに保存する
                if let Some(session) = self.history_manager.get_history().get_current_session() {
                    let session_id = session.id;
                    let _ = self.history_manager.get_history_mut().switch_session(session_id);
                }
                if let Err(e) = self.history_manager.get_history_mut().add_message(final_msg.clone(), false) {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[handle_chat_event] add_message error: {:?}", e);
                        let _ = writeln!(f, "[handle_chat_event] current_session_id: {:?}", self.history_manager.get_history().current_session_id);
                    }
                }
                
                // AIレスポンス追加直後に履歴保存
                if let Err(e) = self.save_history() {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[handle_chat_event] save_history error: {:?}", e);
                    }
                }
            }
            ChatEvent::Error(err) => {
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                    let _ = writeln!(f, "[handle_chat_event] Error: {}", err);
                }
                self.messages.push(crate::history::ChatMessage {
                    id: Uuid::new_v4(),
                    content: format!("Error: {}", err),
                    is_user: false,
                    timestamp: Utc::now(),
                });
                self.is_loading = false;
                // スクロール位置の調整はUI描画時に行うためここでは何もしない
            }
        }
    }

    pub async fn send_message(&mut self, _terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) {
        use std::io::Write;
        self.ui.notification = None;
        let original_message = self.ui.input.clone();
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
            let _ = writeln!(f, "[send_message] called. input={}", original_message);
        }
        // LLM応答待ち中ならバッファに積むだけ
        if self.is_loading {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[send_message] is_loading=true, bufferに積んだ: {}", original_message);
            }
            self.send_buffer.push_back(original_message.clone());
            return;
        }

        // /clearlogコマンド判定
        if original_message.trim() == "/clearlog" {
            match self.history_manager.clear_messages() {
                Ok(_) => {
                    self.messages.clear();
                    self.messages.push(crate::history::ChatMessage {
                        id: Uuid::new_v4(),
                        content: "✅ ログを全て削除しました.".to_string(),
                        is_user: false,
                        timestamp: Utc::now(),
                    });
                }
                Err(e) => {
                    self.messages.push(crate::history::ChatMessage {
                        id: Uuid::new_v4(),
                        content: format!("❌ ログ削除に失敗しました: {}", e),
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

        // TODOリストが存在しない場合、新しく作成するか確認
        /*
        if self.todo_manager.current_list.is_none() && self.todo_manager.should_create_new_list(&message_to_send) {
            if let Err(e) = self.todo_manager.create_new_list(
                format!("タスク: {}", message_to_send.chars().take(30).collect::<String>()),
                message_to_send.clone()
            ) {
                self.show_notification(&format!("Error creating todo list: {}", e));
            }
        }*/

        // ユーザーメッセージを表示用に整形
        let display_message = if file_paths.is_empty() {
            message_to_send.clone()
        } else {
            format!("{}\nFiles: {}", message_to_send, file_paths.join(", "))
        };

        // ユーザーメッセージを即座に追加（新しいUUIDで）
        let user_msg = crate::history::ChatMessage {
            id: Uuid::new_v4(),
            content: display_message.clone(),
            is_user: true,
            timestamp: Utc::now(),
        };
        self.messages.push(user_msg.clone());

        // 履歴管理にメッセージを追加（表示用と同じ内容）
        if let Err(_) = self.history_manager.get_history_mut().add_message(display_message, true) {
            // エラーは無視
        }
        
        // ユーザーメッセージ送信後に履歴保存
        if let Err(e) = self.save_history() {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[send_message] save_history error: {:?}", e);
            }
        }

        // 会話コンテキストを取得
        let mut context = self.history_manager.get_conversation_context(10);

        use crate::history::ChatMessage;

        // TODOリストのコンテキストを追加
        /* 
        let todo_context = self.todo_manager.get_context_for_llm();
        if !todo_context.is_empty() {
            context.push(ChatMessage {
                id: Uuid::new_v4(),
                content: format!("\n## Current TODO List Context:\n{}", todo_context),
                is_user: true, // TODOリストはユーザーからの情報とみなす
                timestamp: Utc::now(),
            });
        }*/

        // 非同期でLLMに送信
        // 既存のLLMタスクがあればキャンセル
        if let Some(handle) = self.llm_task_handle.take() {
            handle.abort();
        }
        let message = message_to_send.clone();
        let sender = self.event_sender.clone();
        let gemini_client = self.gemini_client.clone();
        let handle = tokio::spawn(async move {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[tokio::spawn] chat_loop_with_progress_static spawn. message={}", message);
            }
            let res = ChatApp::chat_loop_with_progress_static(gemini_client, &message, sender.clone()).await;
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
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut message = initial_message.to_string();
        let mut step = 1;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
            let _ = writeln!(f, "[chat_loop_with_progress_static] start. message={}", message);
        }
        for _ in 0..10 {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[chat_loop_with_progress_static] step={}", step);
            }
            let progress_msg = format!("🤖 Step {}: LLMに問い合わせ中...", step);
            let _ = sender.send(ChatEvent::AIResponse(progress_msg));
            let prompt = format!(
                "{}\n\n---\n次に何をすべきか、追加タスクがあるかを必ず明示してください。\n「完了」「終了」「何もする必要がない」などの場合は、その旨を明確に書いてください。",
                message
            );
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[chat_loop_with_progress_static] prompt={}", prompt);
            }
            let response = match tokio::time::timeout(std::time::Duration::from_secs(30), gemini_client.chat(&prompt, None)).await {
                Ok(r) => r,
                Err(_) => {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[chat_loop_with_progress_static] LLMリクエストがタイムアウトしました");
                    }
                    let error_msg = "❌ LLMリクエストがタイムアウトしました".to_string();
                    let _ = sender.send(ChatEvent::Error(error_msg));
                    return Err(anyhow::anyhow!("LLMリクエストがタイムアウト"));
                }
            };
            match response {
                Ok(response) => {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[chat_loop_with_progress_static] LLM response={}", response);
                    }
                    if response.is_empty() {
                        let error_msg = "❌ LLMからの応答が空です。再試行してください。".to_string();
                        let _ = sender.send(ChatEvent::Error(error_msg));
                        return Err(anyhow::anyhow!("LLM応答が空"));
                    }
                    let response_msg = format!("🤖 Step {}: LLM応答\n{}", step, response);
                    let _ = sender.send(ChatEvent::AIResponse(response_msg));
                    let lower = response.to_lowercase();
                    if gemini_client.extract_is_finished_flag(&lower).unwrap_or(false) {
                        // 最終的なAIレスポンスを送信（履歴保存用）
                        let _ = sender.send(ChatEvent::AIResponse(response.clone()));
                        let finish_msg = "✅ LLMが終了を指示したためループを終了します。".to_string();
                        let _ = sender.send(ChatEvent::AIResponse(finish_msg));
                        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                            let _ = writeln!(f, "[chat_loop_with_progress_static] finish (done)");
                        }
                        return Ok(());
                    }
                    message = response.clone();
                    step += 1;
                }
                Err(e) => {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                        let _ = writeln!(f, "[chat_loop_with_progress_static] LLM error={}", e);
                    }
                    let error_msg = format!("❌ LLMとの通信に失敗しました: {}", e);
                    let _ = sender.send(ChatEvent::Error(error_msg));
                    return Err(e.into());
                }
            };
        }
        // 最後のメッセージを最終レスポンスとして送信
        if !message.is_empty() {
            let _ = sender.send(ChatEvent::AIResponse(message));
        }
        let finish_msg = "⚠️ LLM応答に「完了」等が含まれなかったため自動終了しました。".to_string();
        let _ = sender.send(ChatEvent::AIResponse(finish_msg));
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
            let _ = writeln!(f, "[chat_loop_with_progress_static] finish (timeout)");
        }
        Ok(())
    }

    // ファイル作成関連は file_operations.rs へ移譲

    pub fn save_history(&mut self) -> Result<()> {
        self.history_manager.save()
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
                let content = message.content.clone();
                if !self.ui.input.is_empty() {
                    self.ui.input.push('\n');
                }
                self.ui.input.push_str(&content);
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
                self.update_input_line_count();
                self.ui.input_mode = InputMode::Insert;
            }
        }
    }

    pub fn create_new_session(&mut self) {
        let _session_id = self.history_manager.get_history_mut().new_session(None);
        self.messages.clear();
        self.messages.push(crate::history::ChatMessage {
            id: Uuid::new_v4(),
            content: "Started new conversation session.".to_string(),
            is_user: false,
            timestamp: Utc::now(),
        });
        if let Err(e) = self.save_history() {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("contui_debug.log") {
                let _ = writeln!(f, "[create_new_session] save_history error: {:?}", e);
            }
        }
    }

    pub fn scroll_to_bottom(&mut self, visible_height: usize) {
        if !self.messages.is_empty() {
            let total_lines = self.messages.iter().map(|msg| {
                let prefix = if msg.is_user { "You" } else { "AI" };
                let content = format!("{}: {}", prefix, msg.content);
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
