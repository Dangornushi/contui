use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Text},
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Paragraph,
    },
    Frame,
};
use tokio::sync::mpsc;
use crate::gemini::GeminiClient;
use crate::history::HistoryManager;
use crate::markdown::wrap_text;
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;
use device_query::{DeviceQuery, DeviceState, Keycode};

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
    pub device_state: DeviceState,  // リアルタイムキー状態監視
}

#[derive(Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
    Visual,
    SessionList,
    FileBrowser,
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
            device_state: DeviceState::new(),  // リアルタイムキー状態監視を初期化
        };

        // 歓迎メッセージを追加（履歴が空の場合のみ）
        if app.messages.is_empty() {
            app.messages.push(ChatMessage {
                content: "Welcome to ConTUI! Press 'i' to start typing, 'q' to quit, 'n' for new session.\n\n📁 File operations:\n- Use @file:path to reference files\n- Ask me to create files (e.g., \"Create an empty file called test.txt\")\n- Press 'f' to browse files\n\n⚡ Command execution:\n- Ask me to run commands (e.g., \"Run 'ls -la' to list files\")\n- I can execute safe shell commands for you\n\n💡 Try asking: \"Create an empty text file called example.txt\" or \"Run 'ls -la' command\"".to_string(),
                is_user: false,
            });
        }

        // スクロール状態を初期化
        app.scroll_to_bottom();

        app
    }

    pub fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        if key_event.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_mode_key(key_event),
            InputMode::Insert => self.handle_insert_mode_key(key_event),
            InputMode::Visual => self.handle_visual_mode_key(key_event),
            InputMode::SessionList => self.handle_session_list_key(key_event),
            InputMode::FileBrowser => self.handle_file_browser_key(key_event),
        }
    }

    fn handle_normal_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            // 終了
            KeyCode::Char('q') => {
                return Ok(true);
            }
            
            // セッション一覧
            KeyCode::Char('S') => {
                self.input_mode = InputMode::SessionList;
                self.session_list_state.select(Some(0));
            }
            
            // 新しいセッション
            KeyCode::Char('n') => {
                self.create_new_session();
            }
            
            // 履歴を保存
            KeyCode::Char('s') => {
                if let Err(e) = self.save_history() {
                    self.messages.push(ChatMessage {
                        content: format!("Error saving history: {}", e),
                        is_user: false,
                    });
                } else {
                    self.messages.push(ChatMessage {
                        content: "History saved successfully!".to_string(),
                        is_user: false,
                    });
                }
            }
            
            // インサートモード
            KeyCode::Char('i') => {
                self.input_mode = InputMode::Insert;
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::Insert;
                self.move_cursor_right();
            }
            KeyCode::Char('A') => {
                self.input_mode = InputMode::Insert;
                self.cursor_position = self.input.graphemes(true).count();
            }
            KeyCode::Char('I') => {
                self.input_mode = InputMode::Insert;
                self.cursor_position = 0;
            }
            KeyCode::Char('o') => {
                self.input_mode = InputMode::Insert;
                self.input.push('\n');
                self.cursor_position = self.input.graphemes(true).count();
            }
            KeyCode::Char('O') => {
                self.input_mode = InputMode::Insert;
                self.input.insert(0, '\n');
                self.cursor_position = 0;
            }
            
            // カーソル移動
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_cursor_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_cursor_right();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.input.trim().is_empty() {
                    self.scroll_messages_down();
                } else {
                    self.move_cursor_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.input.trim().is_empty() {
                    self.scroll_messages_up();
                } else {
                    self.move_cursor_up();
                }
            }
            KeyCode::Char('0') => {
                self.cursor_position = 0;
            }
            KeyCode::Char('$') => {
                self.cursor_position = self.input.graphemes(true).count();
            }
            
            // Visual Mode
            KeyCode::Char('v') => {
                self.input_mode = InputMode::Visual;
                self.visual_start = Some(self.cursor_position);
            }
            
            // 削除
            KeyCode::Char('x') => {
                self.delete_char_at_cursor();
            }
            KeyCode::Char('X') => {
                self.move_cursor_left();
                self.delete_char_at_cursor();
            }
            KeyCode::Char('d') => {
                // TODO: dd for delete line
                self.input.clear();
                self.cursor_position = 0;
                self.input_line_count = 1;
            }
            
            // 送信
            KeyCode::Enter => {
                if !self.input.trim().is_empty() {
                    self.send_message();
                }
            }
            
            // ファイルブラウザ
            KeyCode::Char('f') => {
                self.input_mode = InputMode::FileBrowser;
                self.refresh_directory_contents();
                self.file_browser_state.select(Some(0));
            }
            
            _ => {}
        }
        Ok(false)
    }

    fn handle_insert_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Enter => {
                // device_queryを使ってリアルタイムでShiftキーの状態を確認
                let keys = self.device_state.get_keys();
                let shift_pressed = keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
                
                // CRITICAL: device_queryでShiftが検出された場合は絶対に送信しない
                if shift_pressed {
                    self.insert_char('\n');
                    self.update_input_line_count();
                    return Ok(false);
                }
                
                // クロスターム側でもShiftをチェック（二重保護）
                if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_char('\n');
                    self.update_input_line_count();
                    return Ok(false);
                }
                
                // 修飾子が完全に空で、Shiftが押されていない場合のみ送信処理
                if key_event.modifiers.is_empty() && !shift_pressed {
                    if !self.input.trim().is_empty() {
                        self.send_message();
                    } else {
                        // 空の入力の場合は何もしない（改行もしない）
                    }
                } else {
                    // 任意の修飾子がある場合は何もしない
                }
            }
            KeyCode::Char(c) => {
                self.insert_char(c);
            }
            KeyCode::Backspace => {
                self.delete_char_before_cursor();
            }
            KeyCode::Delete => {
                self.delete_char_at_cursor();
            }
            KeyCode::Left => {
                self.move_cursor_left();
            }
            KeyCode::Right => {
                self.move_cursor_right();
            }
            KeyCode::Up => {
                if self.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    self.scroll_messages_up();
                }
            }
            KeyCode::Down => {
                if self.input.lines().count() > 1 {
                    self.move_cursor_down();
                } else {
                    self.scroll_messages_down();
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_visual_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.visual_start = None;
            }
            KeyCode::Char('v') => {
                // Visual Modeを終了してNormalモードに戻る
                self.input_mode = InputMode::Normal;
                self.visual_start = None;
            }
            
            // カーソル移動（選択範囲を拡張）
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_cursor_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_cursor_right();
            }
            KeyCode::Char('0') => {
                self.cursor_position = 0;
            }
            KeyCode::Char('$') => {
                self.cursor_position = self.input.graphemes(true).count();
            }
            KeyCode::Char('w') => {
                // 次の単語の先頭へ
                self.move_to_next_word();
            }
            KeyCode::Char('b') => {
                // 前の単語の先頭へ
                self.move_to_prev_word();
            }
            
            // 削除（選択範囲を削除）
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.delete_visual_selection();
                self.input_mode = InputMode::Normal;
                self.visual_start = None;
            }
            
            // ヤンク（選択範囲をコピー）
            KeyCode::Char('y') => {
                // 今回は実装を簡略化してクリップボードに保存しない
                self.input_mode = InputMode::Normal;
                self.visual_start = None;
            }
            
            // 上下移動（複数行の場合は行移動、そうでなければメッセージスクロール）
            KeyCode::Char('j') | KeyCode::Down => {
                if self.input.lines().count() > 1 {
                    self.move_cursor_down();
                } else {
                    self.scroll_messages_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    self.scroll_messages_up();
                }
            }
            
            _ => {}
        }
        Ok(false)
    }

    fn handle_session_list_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('q') => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.session_list_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.session_list_next();
            }
            KeyCode::Enter => {
                self.switch_to_selected_session();
            }
            KeyCode::Char('d') => {
                self.delete_selected_session();
            }
            KeyCode::Char('n') => {
                self.input_mode = InputMode::Normal;
                self.create_new_session();
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_file_browser_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.file_browser_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.file_browser_next();
            }
            KeyCode::Enter => {
                self.open_selected_file();
            }
            KeyCode::Char(' ') => {
                self.toggle_file_selection();
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                self.delete_selected_file();
            }
            KeyCode::Char('r') => {
                self.refresh_directory_contents();
            }
            KeyCode::Char('u') => {
                self.go_to_parent_directory();
            }
            KeyCode::Char('i') => {
                // 入力モードに切り替え
                self.input_mode = InputMode::Insert;
            }
            _ => {}
        }
        Ok(false)
    }

    fn session_list_next(&mut self) {
        let sessions = self.history_manager.get_history().get_session_list();
        if !sessions.is_empty() {
            let i = match self.session_list_state.selected() {
                Some(i) => (i + 1) % sessions.len(),
                None => 0,
            };
            self.session_list_state.select(Some(i));
        }
    }

    fn session_list_previous(&mut self) {
        let sessions = self.history_manager.get_history().get_session_list();
        if !sessions.is_empty() {
            let i = match self.session_list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        sessions.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.session_list_state.select(Some(i));
        }
    }

    fn switch_to_selected_session(&mut self) {
        if let Some(i) = self.session_list_state.selected() {
            let sessions = self.history_manager.get_history().get_session_list();
            if let Some(session) = sessions.get(i) {
                let session_id = session.id;
                if let Err(_) = self.history_manager.get_history_mut().switch_session(session_id) {
                    // エラーは無視
                    return;
                }
                
                // メッセージを再読み込み
                self.messages.clear();
                if let Some(session) = self.history_manager.get_history().get_current_session() {
                    for hist_msg in &session.messages {
                        self.messages.push(ChatMessage {
                            content: hist_msg.content.clone(),
                            is_user: hist_msg.is_user,
                        });
                    }
                }
                
                self.input_mode = InputMode::Normal;
                self.scroll_to_bottom();
            }
        }
    }

    fn delete_selected_session(&mut self) {
        if let Some(i) = self.session_list_state.selected() {
            let sessions = self.history_manager.get_history().get_session_list();
            if let Some(session) = sessions.get(i) {
                let session_id = session.id;
                
                // セッションを削除
                if let Err(_) = self.history_manager.get_history_mut().delete_session(session_id) {
                    // エラーは無視
                    return;
                }
                
                // 現在のセッションが削除された場合、新しいセッションを作成
                if self.history_manager.get_history().current_session_id.is_none() {
                    self.create_new_session();
                } else {
                    // メッセージを再読み込み
                    self.messages.clear();
                    if let Some(session) = self.history_manager.get_history().get_current_session() {
                        for hist_msg in &session.messages {
                            self.messages.push(ChatMessage {
                                content: hist_msg.content.clone(),
                                is_user: hist_msg.is_user,
                            });
                        }
                    }
                }
                
                self.scroll_to_bottom();
                
                // 選択位置を調整
                let remaining_sessions = self.history_manager.get_history().get_session_list();
                if remaining_sessions.is_empty() {
                    self.session_list_state.select(None);
                } else {
                    let new_index = if i >= remaining_sessions.len() {
                        remaining_sessions.len() - 1
                    } else {
                        i
                    };
                    self.session_list_state.select(Some(new_index));
                }
            }
        }
    }

    // カーソル移動のヘルパー関数
    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        let grapheme_count = self.input.graphemes(true).count();
        if self.cursor_position < grapheme_count {
            self.cursor_position += 1;
        }
    }

    // 上方向への移動
    fn move_cursor_up(&mut self) {
        let lines: Vec<&str> = self.input.lines().collect();
        if lines.len() <= 1 {
            return;
        }
        
        let (current_line, current_column) = self.calculate_cursor_position();
        if current_line > 0 {
            let target_line = current_line - 1;
            let line_start_pos = self.get_line_start_position(target_line);
            let line_length = lines[target_line].graphemes(true).count();
            let new_column = current_column.min(line_length);
            self.cursor_position = line_start_pos + new_column;
        }
    }

    // 下方向への移動
    fn move_cursor_down(&mut self) {
        let lines: Vec<&str> = self.input.lines().collect();
        if lines.len() <= 1 {
            return;
        }
        
        let (current_line, current_column) = self.calculate_cursor_position();
        if current_line < lines.len() - 1 {
            let target_line = current_line + 1;
            let line_start_pos = self.get_line_start_position(target_line);
            let line_length = lines[target_line].graphemes(true).count();
            let new_column = current_column.min(line_length);
            self.cursor_position = line_start_pos + new_column;
        }
    }

    // 指定した行の開始位置を取得
    fn get_line_start_position(&self, line_index: usize) -> usize {
        let lines: Vec<&str> = self.input.lines().collect();
        let mut position = 0;
        
        for (i, line) in lines.iter().enumerate() {
            if i == line_index {
                break;
            }
            position += line.graphemes(true).count() + 1; // +1 for newline character
        }
        
        position
    }

    // 文字入力のヘルパー関数
    fn insert_char(&mut self, c: char) {
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let mut new_input = String::new();
        
        for (i, grapheme) in graphemes.iter().enumerate() {
            if i == self.cursor_position {
                new_input.push(c);
            }
            new_input.push_str(grapheme);
        }
        
        if self.cursor_position >= graphemes.len() {
            new_input.push(c);
        }
        
        self.input = new_input;
        self.cursor_position += 1;
        self.update_input_line_count();
    }

    // 文字削除のヘルパー関数
    fn delete_char_at_cursor(&mut self) {
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        if self.cursor_position < graphemes.len() {
            let mut new_input = String::new();
            for (i, grapheme) in graphemes.iter().enumerate() {
                if i != self.cursor_position {
                    new_input.push_str(grapheme);
                }
            }
            self.input = new_input;
            self.update_input_line_count();
        }
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.delete_char_at_cursor();
        }
    }

    // スクロール関数
    fn scroll_messages_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            // list_stateも更新して表示を同期
            self.update_list_state_from_scroll();
        }
    }

    fn scroll_messages_down(&mut self) {
        if !self.messages.is_empty() && self.scroll_offset < self.messages.len() - 1 {
            self.scroll_offset += 1;
            // list_stateも更新して表示を同期
            self.update_list_state_from_scroll();
        }
    }

    // scroll_offsetからlist_stateを更新
    fn update_list_state_from_scroll(&mut self) {
        if !self.messages.is_empty() {
            self.list_state.select(Some(self.scroll_offset));
        }
    }

    pub fn handle_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::AIResponse(msg) => {
                // ファイル作成要求を処理
                let mut processed_msg = self.process_file_creation_requests(&msg);
                
                // コマンド実行要求を処理
                processed_msg = self.process_command_execution_requests(&processed_msg);
                
                // 履歴管理にAIレスポンスを追加（処理後のメッセージ）
                if let Err(_) = self.history_manager.get_history_mut().add_message(processed_msg.clone(), false) {
                    // エラーは無視
                }
                
                self.messages.push(ChatMessage {
                    content: processed_msg,
                    is_user: false,
                });
                self.is_loading = false;
                self.scroll_to_bottom();
                
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

    fn send_message(&mut self) {
        let original_message = self.input.clone();
        self.input.clear();
        self.cursor_position = 0;
        self.input_mode = InputMode::Normal;
        self.is_loading = true;
        self.input_line_count = 1;  // 送信後は1行にリセット

        // ファイル参照を解析
        let (clean_message, file_paths) = self.parse_file_references(&original_message);
        let message_to_send = if clean_message.is_empty() && !file_paths.is_empty() {
            "Please analyze these files:".to_string()
        } else {
            clean_message
        };

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
        let context = self.history_manager.get_conversation_context(10);

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

    fn create_new_session(&mut self) {
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

    fn save_history(&mut self) -> Result<()> {
        self.history_manager.save()
    }

    fn scroll_to_bottom(&mut self) {
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
            self.list_state.select(Some(self.scroll_offset));
        }
    }

    // ファイルブラウザ関連のメソッド
    fn refresh_directory_contents(&mut self) {
        match self.gemini_client.list_directory(&self.current_directory) {
            Ok(contents) => {
                self.directory_contents = contents;
            }
            Err(_) => {
                // エラーは無視
                self.directory_contents.clear();
            }
        }
    }

    fn file_browser_previous(&mut self) {
        let selected = self.file_browser_state.selected().unwrap_or(0);
        if selected > 0 {
            self.file_browser_state.select(Some(selected - 1));
        }
    }

    fn file_browser_next(&mut self) {
        let selected = self.file_browser_state.selected().unwrap_or(0);
        if selected < self.directory_contents.len().saturating_sub(1) {
            self.file_browser_state.select(Some(selected + 1));
        }
    }

    fn open_selected_file(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if item.ends_with('/') {
                    // ディレクトリに移動
                    let mut path = std::path::PathBuf::from(&self.current_directory);
                    path.push(item.trim_end_matches('/'));
                    self.current_directory = path.to_string_lossy().to_string();
                    self.refresh_directory_contents();
                    self.file_browser_state.select(Some(0));
                } else {
                    // ファイルを入力フィールドに追加
                    let mut path = std::path::PathBuf::from(&self.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    // 入力フィールドにファイル参照を追加
                    if !self.input.is_empty() {
                        self.input.push(' ');
                    }
                    self.input.push_str(&format!("@file:{}", file_path));
                    self.cursor_position = self.input.graphemes(true).count();
                    
                    // ファイルブラウザを閉じて入力モードに切り替え
                    self.input_mode = InputMode::Insert;
                }
            }
        }
    }

    fn toggle_file_selection(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = std::path::PathBuf::from(&self.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if let Some(pos) = self.selected_files.iter().position(|x| x == &file_path) {
                        // 選択を解除して入力フィールドからも削除
                        self.selected_files.remove(pos);
                        let file_ref = format!("@file:{}", file_path);
                        self.input = self.input.replace(&file_ref, "").trim().to_string();
                        self.cursor_position = self.input.graphemes(true).count();
                    } else {
                        // 選択に追加して入力フィールドにも追加
                        self.selected_files.push(file_path.clone());
                        if !self.input.is_empty() {
                            self.input.push(' ');
                        }
                        self.input.push_str(&format!("@file:{}", file_path));
                        self.cursor_position = self.input.graphemes(true).count();
                    }
                }
            }
        }
    }

    fn delete_selected_file(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = std::path::PathBuf::from(&self.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if let Some(pos) = self.selected_files.iter().position(|x| x == &file_path) {
                        self.selected_files.remove(pos);
                    }
                }
            }
        }
    }

    fn go_to_parent_directory(&mut self) {
        let path = std::path::PathBuf::from(&self.current_directory);
        if let Some(parent) = path.parent() {
            self.current_directory = parent.to_string_lossy().to_string();
            self.refresh_directory_contents();
            self.file_browser_state.select(Some(0));
        }
    }

    // ファイルパス解析機能
    fn parse_file_references(&self, message: &str) -> (String, Vec<String>) {
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
    fn process_file_creation_requests(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        
        // ```create_file:filename の形式でファイル作成要求を検索
        // より柔軟な正規表現：複数行にわたる内容とsフラグを使用
        let create_file_pattern = r"(?s)```create_file:([^\n\r]+)(?:\r?\n(.*?))?```";
        
        // Regexを使えない場合は手動で解析
        let re = match regex::Regex::new(create_file_pattern) {
            Ok(regex) => regex,
            Err(_) => {
                return self.manual_parse_file_creation(response);
            }
        };
        
        let mut files_created = Vec::new();
        
        let matches: Vec<_> = re.captures_iter(response).collect();
        
        // マッチが空の場合は、ファイル作成要求がないということなので、そのまま元のレスポンスを返す
        if matches.is_empty() {
            return response.to_string();
        }
        
        // マッチした全てのファイル作成要求を処理
        for caps in matches.iter() {
            if let Some(filename_match) = caps.get(1) {
                let filename = filename_match.as_str().trim();
                let content = caps.get(2).map(|m| m.as_str()).unwrap_or(""); // 内容がない場合は空文字列
                
                // 重複チェック付きでファイル作成
                match self.gemini_client.create_file_with_unique_name(filename, content) {
                    Ok(actual_filename) => {
                        files_created.push(actual_filename.clone());
                        
                        // 元のファイル作成コードブロックを成功メッセージに置換
                        let success_message = if actual_filename == filename {
                            format!("✅ File '{}' created successfully!", filename)
                        } else {
                            format!("✅ File '{}' created as '{}' (original name was taken)", filename, actual_filename)
                        };
                        
                        processed_response = processed_response.replace(
                            &caps[0],
                            &success_message
                        );
                    }
                    Err(e) => {
                        processed_response = processed_response.replace(
                            &caps[0],
                            &format!("❌ Failed to create file '{}': {}", filename, e)
                        );
                        continue;
                    }
                }
            }
        }
        
        if !files_created.is_empty() {
            // ファイルブラウザのコンテンツを更新
            self.refresh_directory_contents();
            
            // 作成されたファイルのサマリーを追加
            let summary = format!("\n\n📁 Created {} file(s): {}", 
                files_created.len(), 
                files_created.join(", ")
            );
            processed_response.push_str(&summary);
        }
        
        processed_response
    }

    // Regexが使えない場合の手動解析
    fn manual_parse_file_creation(&mut self, response: &str) -> String {
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
                            format!("✅ File '{}' created successfully!", filename)
                        } else {
                            format!("✅ File '{}' created as '{}' (original name was taken)", filename, actual_filename)
                        };
                        processed_response = processed_response.replace(&original_block, &success_message);
                    }
                    Err(e) => {
                        // エラーメッセージで置換
                        let original_block = format!("```create_file:{}\n{}\n```", filename, content);
                        let error_msg = format!("❌ Failed to create file '{}': {}", filename, e);
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

    // AIレスポンスからコマンド実行要求を解析・実行
    fn process_command_execution_requests(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        
        // ```run_command: の形式でコマンド実行要求を検索
        let command_pattern = r"(?s)```run_command:([^\n\r]+)(?:\r?\n(.*?))?```";
        
        // Regexを使えない場合は手動で解析
        let re = match regex::Regex::new(command_pattern) {
            Ok(regex) => regex,
            Err(_) => {
                return self.manual_parse_command_execution(response);
            }
        };
        
        let mut commands_executed = Vec::new();
        
        let matches: Vec<_> = re.captures_iter(response).collect();
        
        // マッチが空の場合は、コマンド実行要求がないということなので、そのまま元のレスポンスを返す
        if matches.is_empty() {
            return response.to_string();
        }
        
        // マッチした全てのコマンド実行要求を処理
        for caps in matches.iter() {
            if let Some(command_match) = caps.get(1) {
                let command = command_match.as_str().trim();
                
                // コマンドを実行
                match self.execute_command_safe(command) {
                    Ok(output) => {
                        commands_executed.push(command.to_string());
                        
                        // 元のコマンド実行コードブロックを実行結果に置換
                        let success_message = format!(
                            "✅ Command executed: `{}`\n\n**Output:**\n```\n{}\n```", 
                            command, 
                            output.trim()
                        );
                        
                        processed_response = processed_response.replace(
                            &caps[0],
                            &success_message
                        );
                    }
                    Err(e) => {
                        processed_response = processed_response.replace(
                            &caps[0],
                            &format!("❌ Failed to execute command '{}': {}", command, e)
                        );
                        continue;
                    }
                }
            }
        }
        
        if !commands_executed.is_empty() {
            // 実行されたコマンドのサマリーを追加
            let summary = format!("\n\n⚡ Executed {} command(s): {}", 
                commands_executed.len(), 
                commands_executed.join(", ")
            );
            processed_response.push_str(&summary);
        }
        
        processed_response
    }

    // Regexが使えない場合の手動解析（コマンド実行版）
    fn manual_parse_command_execution(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        let mut commands_executed = Vec::new();
        
        // ```run_command: で始まる行を検索
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;
        
        while i < lines.len() {
            if lines[i].starts_with("```run_command:") {
                // コマンドを抽出
                let command = lines[i].strip_prefix("```run_command:").unwrap_or("").trim();
                if command.is_empty() {
                    i += 1;
                    continue;
                }
                
                // 終了ブロックを探す
                i += 1;
                while i < lines.len() && !lines[i].starts_with("```") {
                    i += 1;
                }
                
                // コマンドを実行
                match self.execute_command_safe(command) {
                    Ok(output) => {
                        commands_executed.push(command.to_string());
                        
                        // 成功メッセージで置換
                        let original_block = format!("```run_command:{}\n```", command);
                        let success_message = format!(
                            "✅ Command executed: `{}`\n\n**Output:**\n```\n{}\n```", 
                            command, 
                            output.trim()
                        );
                        processed_response = processed_response.replace(&original_block, &success_message);
                    }
                    Err(e) => {
                        // エラーメッセージで置換
                        let original_block = format!("```run_command:{}\n```", command);
                        let error_msg = format!("❌ Failed to execute command '{}': {}", command, e);
                        processed_response = processed_response.replace(&original_block, &error_msg);
                    }
                }
            }
            i += 1;
        }
        
        if !commands_executed.is_empty() {
            let summary = format!("\n\n⚡ Executed {} command(s): {}", 
                commands_executed.len(), 
                commands_executed.join(", ")
            );
            processed_response.push_str(&summary);
        }
        
        processed_response
    }

    // 安全にコマンドを実行するメソッド
    fn execute_command_safe(&self, command: &str) -> Result<String> {
        use std::process::Command;
        use std::time::Duration;
        
        // 危険なコマンドをブロック
        let dangerous_commands = [
            "rm", "rmdir", "del", "format", "fdisk", "mkfs", 
            "dd", "shutdown", "reboot", "halt", "init",
            "sudo rm", "sudo rmdir", "sudo dd", "sudo shutdown",
            "chmod 000", "chown root", "passwd", "su", "sudo su"
        ];
        
        let command_lower = command.to_lowercase();
        for dangerous in &dangerous_commands {
            if command_lower.contains(dangerous) {
                return Err(anyhow::anyhow!("Dangerous command blocked for safety: {}", command));
            }
        }
        
        // macOSでのシェル実行
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.current_directory)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        if !output.status.success() {
            return Err(anyhow::anyhow!("Command failed with exit code {}: {}", 
                output.status.code().unwrap_or(-1), 
                stderr
            ));
        }
        
        // 出力が長すぎる場合は短縮
        let combined_output = if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("{}\n--- stderr ---\n{}", stdout, stderr)
        };
        
        if combined_output.len() > 2000 {
            Ok(format!("{}...\n\n[Output truncated - {} characters total]", 
                &combined_output[..2000], 
                combined_output.len()
            ))
        } else {
            Ok(combined_output)
        }
    }
}
