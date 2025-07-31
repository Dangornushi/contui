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
    pub input_history: Vec<String>,  // プロンプト履歴
    pub history_index: Option<usize>,  // 現在の履歴インデックス
    pub temp_input: String,  // 履歴ナビゲーション中の一時的な入力
    pub show_help: bool,  // ヘルプウィンドウ表示フラグ
    pub notification: Option<String>, // ファイル作成通知など一時的な表示
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
            input_history: Vec::new(),  // プロンプト履歴を初期化
            history_index: None,  // 履歴インデックスを初期化
            temp_input: String::new(),  // 一時的な入力を初期化
            show_help: false,  // ヘルプウィンドウは初期状態では非表示
            notification: None, // ← 追加
        };

        // 歓迎メッセージを追加（履歴が空の場合のみ）
        if app.messages.is_empty() {
            app.messages.push(ChatMessage {
                content: "Welcome to ConTUI! Press 'i' to start typing, 'q' to quit, 'n' for new session.\n\n📁 File operations:\n- Use @file:path to reference files\n- Ask me to create files (e.g., \"Create an empty file called test.txt\")\n- Press 'f' to browse files\n\n� Command execution:\n- Ask me to run commands (e.g., \"List files in current directory\")\n- I can execute shell commands for you\n\n�💡 Try asking:\n- \"Create an empty text file called example.txt\"\n- \"List files in current directory\"\n- \"Show git status\"".to_string(),
                is_user: false,
            });
        }

        // スクロール状態を初期化
        app.scroll_to_bottom();

        app
    }

    pub fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        self.notification = None;
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
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.show_help = !self.show_help;
            return Ok(false);
        }
        
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
                } else if self.input.lines().count() > 1 {
                    self.move_cursor_down();
                } else {
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.input.trim().is_empty() {
                    self.scroll_messages_up();
                } else if self.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_up();
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
                } else {
                    // 入力が空の場合、選択されたメッセージを入力欄に挿入
                    self.insert_selected_message();
                }
            }
            
            // ファイルブラウザ
            KeyCode::Char('f') => {
                self.input_mode = InputMode::FileBrowser;
                self.refresh_directory_contents();
                self.file_browser_state.select(Some(0));
            }
            
            // 選択されたメッセージを入力欄に挿入
            KeyCode::Char('y') => {
                self.insert_selected_message();
            }
            
            _ => {}
        }
        Ok(false)
    }

    fn handle_insert_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.show_help = !self.show_help;
            return Ok(false);
        }
        
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
                // 履歴ナビゲーションをリセット
                self.reset_history_navigation();
                self.insert_char(c);
            }
            KeyCode::Backspace => {
                // 履歴ナビゲーションをリセット
                self.reset_history_navigation();
                self.delete_char_before_cursor();
            }
            KeyCode::Delete => {
                // 履歴ナビゲーションをリセット
                self.reset_history_navigation();
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
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_up();
                }
            }
            KeyCode::Down => {
                if self.input.lines().count() > 1 {
                    self.move_cursor_down();
                } else {
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_down();
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_visual_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.show_help = !self.show_help;
            return Ok(false);
        }
        
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
                let processed_msg = self.process_file_creation_requests(&msg);
                
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

    fn send_message(&mut self) {
        self.notification = None;
        let original_message = self.input.clone();

        // /clearlogコマンド判定
        if original_message.trim() == "/clearlog" {
            match self.history_manager.clear_messages() {
                Ok(_) => {
                    self.messages.clear();
                    self.messages.push(ChatMessage {
                        content: "✅ ログを全て削除しました。".to_string(),
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

    // 選択されたメッセージを入力欄に挿入
    fn insert_selected_message(&mut self) {
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
        let create_file_pattern = r"(?s)```create_file:([^\n\r]+)(?:\r?\n(.*?))?```";
        let re = match regex::Regex::new(create_file_pattern) {
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
            self.refresh_directory_contents();
            let summary = format!("📁 ファイル作成: {}", files_created.join(", "));
            self.notification = Some(summary);
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

    pub fn render(&mut self, f: &mut Frame) {
        if self.input_mode == InputMode::SessionList {
            self.render_session_list(f);
        } else if self.input_mode == InputMode::FileBrowser {
            self.render_file_browser(f);
        } else {
            let input_height = (self.input_line_count + 2).clamp(3, 10) as u16;
            let notification_height = if self.notification.is_some() { 2 } else { 0 };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(notification_height),
                    Constraint::Length(input_height),
                ])
                .split(f.area());

            self.render_messages(f, chunks[0]);
            if let Some(ref note) = self.notification {
                self.render_notification(f, chunks[1], note);
            }
            self.render_input(f, chunks[2]);
            if self.show_help {
                self.render_floating_help(f);
            }
        }
    }

    fn render_messages(&mut self, f: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .messages
            .iter()
            .enumerate()
            .map(|(_i, msg)| {
                let style = if msg.is_user {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Blue)
                };
                
                let prefix = if msg.is_user { "You" } else { "AI" };
                let content = format!("{}: {}", prefix, msg.content);
                
                // 幅から境界線とパディングを差し引いて計算（より保守的に）
                let max_width = if area.width > 8 { 
                    area.width as usize - 8 
                } else { 
                    1 
                };
                
                // wrap_text関数を使用してテキストを改行
                let wrapped_content = wrap_text(&content, max_width);
                
                ListItem::new(Text::from(wrapped_content)).style(style)
            })
            .collect();

        let messages_list = List::new(messages)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Chat History")
                    .border_type(BorderType::Rounded),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        f.render_stateful_widget(messages_list, area, &mut self.list_state);

        // スクロール位置を適切に調整
        if !self.messages.is_empty() {
            // 最下部にスクロールしていた場合、新しいメッセージが追加されても最下部に留まる
            if self.scroll_offset >= self.messages.len().saturating_sub(1) {
                self.scroll_offset = self.messages.len().saturating_sub(1);
            }
            
            // 現在のスクロール位置でlist_stateを更新
            self.list_state.select(Some(self.scroll_offset));
        }

        if self.is_loading {
            let loading_area = Rect {
                x: area.x + 2,
                y: area.y + area.height - 2,
                width: area.width - 4,
                height: 1,
            };
            
            let loading_text = Paragraph::new("🤖 AI is thinking...")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC));
            
            f.render_widget(loading_text, loading_area);
        }
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let input_style = match self.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Insert => Style::default().fg(Color::Yellow),
            InputMode::Visual => Style::default().fg(Color::Magenta),
            InputMode::SessionList => Style::default().fg(Color::Cyan),
            InputMode::FileBrowser => Style::default().fg(Color::Cyan),
        };

        let title = match self.input_mode {
            InputMode::Normal => "Input (Press 'i' to insert, 'v' for visual, 'q' to quit)",
            InputMode::Insert => "Insert Mode (Shift+Enter: new line, Enter: send, Esc: normal mode)",
            InputMode::Visual => "Visual Mode (Select text, press 'd' to delete, 'y' to yank, Esc to exit)",
            InputMode::SessionList => "Session List (Press Enter to select, 'd' to delete, 'n' for new)",
            InputMode::FileBrowser => "File Browser (Press Enter to open, 'd' to delete, 'n' for new)",
        };

        let input = Paragraph::new(self.input.as_str())
            .style(input_style)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_type(BorderType::Rounded),
            );

        f.render_widget(input, area);

        // カーソル位置を計算（複数行対応）
        let (cursor_line, cursor_column) = self.calculate_cursor_position();
        let cursor_pos_x = area.x + cursor_column as u16 + 1;
        let cursor_pos_y = area.y + cursor_line as u16 + 1;

        match self.input_mode {
            InputMode::Insert => {
                // Insertモードでは棒線カーソル（デフォルト）
                f.set_cursor_position((cursor_pos_x, cursor_pos_y));
            }
            InputMode::Normal => {
                // Normalモードでは四角いカーソル（文字をハイライト）
                f.set_cursor_position((cursor_pos_x, cursor_pos_y));
                
                // 現在のカーソル位置の文字をハイライト表示
                let graphemes: Vec<&str> = self.input.graphemes(true).collect();
                if self.cursor_position < graphemes.len() {
                    let char_at_cursor = graphemes[self.cursor_position];
                    let highlight_area = Rect {
                        x: cursor_pos_x,
                        y: cursor_pos_y,
                        width: UnicodeWidthStr::width(char_at_cursor).max(1) as u16,
                        height: 1,
                    };
                    let highlight_text = Paragraph::new(char_at_cursor)
                        .style(Style::default().bg(Color::White).fg(Color::Black));
                    f.render_widget(highlight_text, highlight_area);
                } else if self.input.is_empty() {
                    // 空の場合は空白をハイライト
                    let highlight_area = Rect {
                        x: cursor_pos_x,
                        y: cursor_pos_y,
                        width: 1,
                        height: 1,
                    };
                    let highlight_text = Paragraph::new(" ")
                        .style(Style::default().bg(Color::White).fg(Color::Black));
                    f.render_widget(highlight_text, highlight_area);
                }
            }
            InputMode::Visual => {
                // Visual Modeでは選択範囲をハイライト
                f.set_cursor_position((cursor_pos_x, cursor_pos_y));
                
                if let Some((start_pos, end_pos)) = self.get_visual_selection_range() {
                    let graphemes: Vec<&str> = self.input.graphemes(true).collect();
                    let mut x_offset = 0;
                    
                    for (i, grapheme) in graphemes.iter().enumerate() {
                        let char_width = UnicodeWidthStr::width(*grapheme).max(1);
                        
                        if i >= start_pos && i < end_pos {
                            // 選択範囲内の文字は明るい背景色でハイライト
                            let highlight_area = Rect {
                                x: area.x + x_offset as u16 + 1,
                                y: cursor_pos_y,
                                width: char_width as u16,
                                height: 1,
                            };
                            let highlight_text = Paragraph::new(*grapheme)
                                .style(Style::default().bg(Color::LightBlue).fg(Color::Black));
                            f.render_widget(highlight_text, highlight_area);
                        }
                        
                        x_offset += char_width;
                    }
                    
                    // 選択範囲が空の場合でも視覚的フィードバックを提供
                    if start_pos == end_pos {
                        let highlight_area = Rect {
                            x: cursor_pos_x,
                            y: cursor_pos_y,
                            width: 1,
                            height: 1,
                        };
                        let highlight_text = Paragraph::new(" ")
                            .style(Style::default().bg(Color::LightBlue).fg(Color::Black));
                        f.render_widget(highlight_text, highlight_area);
                    }
                }
            }
            InputMode::SessionList => {
                // セッション一覧モードではカーソル非表示
            }
            InputMode::FileBrowser => {
                // ファイルブラウザモードではカーソル非表示
            }
        }
    }

    fn render_floating_help(&self, f: &mut Frame) {
        // 画面中央にフローティングウィンドウを配置
        let area = f.area();
        let popup_width = 80.min(area.width - 4);
        let popup_height = 20.min(area.height - 4);
        
        let popup_area = Rect {
            x: (area.width - popup_width) / 2,
            y: (area.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        // 背景を完全にクリアするために空白文字で埋める
        let clear_lines = vec![" ".repeat(popup_width as usize - 2); popup_height as usize - 2];
        let clear_text = clear_lines.join("\n");
        
        f.render_widget(
            Paragraph::new(clear_text)
                .style(Style::default().bg(Color::Black))
                .block(
                    Block::default()
                        .style(Style::default().bg(Color::Black))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::Cyan)),
                ),
            popup_area,
        );

        let help_text = match self.input_mode {
            InputMode::Normal => vec![
                "=== Normal Mode ===",
                "",
                "Movement:",
                "  h/j/k/l or ←/↓/↑/→  - Move cursor",
                "  0                   - Move to beginning of line",
                "  $                   - Move to end of line",
                "",
                "Editing:",
                "  i                   - Insert mode",
                "  a                   - Append (insert after cursor)",
                "  A                   - Append at end of line",
                "  I                   - Insert at beginning of line",
                "  o                   - Open new line below",
                "  O                   - Open new line above",
                "  v                   - Visual mode",
                "",
                "Actions:",
                "  Enter               - Send message",
                "  y                   - Yank (copy) current message",
                "",
                "Session:",
                "  n                   - New session",
                "  s                   - Save history",
                "  S                   - Session list",
                "  f                   - File browser",
                "  q                   - Quit",
                "",
                "Help:",
                "  Ctrl+H              - Toggle this help window",
            ],
            InputMode::Insert => vec![
                "=== Insert Mode ===",
                "",
                "Text Input:",
                "  Type normally to enter text",
                "  Shift+Enter         - New line (multi-line input)",
                "  Enter               - Send message",
                "  Esc                 - Return to Normal mode",
                "",
                "File References:",
                "  @file:path          - Reference a file in your message",
                "  Example: @file:./config.json",
                "",
                "AI Features:",
                "  Ask AI to create files:",
                "    'Create a file called test.txt with hello world'",
                "  Ask AI to run commands:",
                "    'List files in current directory'",
                "    'Show git status'",
                "",
                "History:",
                "  ↑/↓                 - Navigate input history",
                "",
                "Help:",
                "  Ctrl+H              - Toggle this help window",
            ],
            InputMode::Visual => vec![
                "=== Visual Mode ===",
                "",
                "Selection:",
                "  h/j/k/l or ←/↓/↑/→  - Extend selection",
                "  w                   - Move forward by word",
                "  b                   - Move backward by word",
                "",
                "Actions:",
                "  d                   - Delete selected text",
                "  y                   - Yank (copy) selected text",
                "",
                "Exit:",
                "  v                   - Exit Visual mode",
                "  Esc                 - Exit Visual mode",
                "",
                "Help:",
                "  Ctrl+H              - Toggle this help window",
            ],
            InputMode::SessionList => vec![
                "=== Session List ===",
                "",
                "Navigation:",
                "  j/k or ↓/↑          - Navigate sessions",
                "",
                "Actions:",
                "  Enter               - Select session",
                "  d                   - Delete session",
                "  n                   - Create new session",
                "",
                "Exit:",
                "  q or Esc            - Return to chat",
                "",
                "Help:",
                "  Ctrl+H              - Toggle this help window",
            ],
            InputMode::FileBrowser => vec![
                "=== File Browser ===",
                "",
                "Navigation:",
                "  j/k or ↓/↑          - Navigate files",
                "  u                   - Go to parent directory",
                "  r                   - Refresh directory",
                "",
                "Actions:",
                "  Enter               - Add file path to input",
                "  Space               - Toggle file selection",
                "  i                   - Edit selected file",
                "",
                "Exit:",
                "  q                   - Return to chat",
                "",
                "Help:",
                "  Ctrl+H              - Toggle this help window",
            ],
        };

        // ヘルプテキストを上から重ねてレンダリング
        let content = Text::from(help_text.join("\n"));
        let help_paragraph = Paragraph::new(content)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help (Press Ctrl+H to close) ")
                    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(help_paragraph, popup_area);
    }

    fn render_session_list(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(f.area());

        // セッション一覧を表示
        let sessions = self.history_manager.get_history().get_session_list();
        let session_items: Vec<ListItem> = sessions
            .iter()
            .map(|session| {
                let message_count = session.messages.len();
                let last_message = session.messages.last()
                    .map(|msg| {
                        let preview = Self::truncate_string_safe(&msg.content, 47);
                        format!(" - {}", preview)
                    })
                    .unwrap_or_else(|| " - No messages".to_string());
                
                let title = format!("{} ({} messages){}", 
                    session.title, 
                    message_count, 
                    last_message
                );
                
                ListItem::new(title)
            })
            .collect();

        let session_list = List::new(session_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Chat Sessions")
                    .border_type(BorderType::Rounded)
            )
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .highlight_symbol(">> ");

        f.render_stateful_widget(session_list, chunks[0], &mut self.session_list_state);

        // ヘルプテキストを表示
        let help = Paragraph::new("Use j/k to navigate, Enter to select, d to delete, n for new session, q/Esc to go back")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Help")
                    .border_type(BorderType::Rounded)
            )
            .style(Style::default().fg(Color::Gray));

        f.render_widget(help, chunks[1]);
    }

    fn render_file_browser(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(3),
                Constraint::Length(4),
            ])
            .split(f.area());

        // タイトル
        let title = Paragraph::new(format!("File Browser: {}", self.current_directory))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(title, chunks[0]);

        // ディレクトリコンテンツ
        let items: Vec<ListItem> = self.directory_contents
            .iter()
            .enumerate()
            .map(|(_i, item)| {
                let style = if item.ends_with('/') {
                    Style::default().fg(Color::Blue)
                } else {
                    let mut path = std::path::PathBuf::from(&self.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if self.selected_files.contains(&file_path) || 
                       self.input.contains(&format!("@file:{}", file_path)) {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                };
                
                let prefix = if item.ends_with('/') { "📁" } else { "📄" };
                ListItem::new(format!("{} {}", prefix, item)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Files and Directories")
                    .border_type(BorderType::Rounded),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("➤ ");

        f.render_stateful_widget(list, chunks[1], &mut self.file_browser_state);

        // 現在の入力フィールドを表示
        let input_text = if self.input.is_empty() {
            "Type your message here... (Use @file:path to reference files)".to_string()
        } else {
            self.input.clone()
        };

        let input_paragraph = Paragraph::new(input_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Message Input")
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::White));
        f.render_widget(input_paragraph, chunks[2]);

        // ヘルプ
        let help_text = "↑/↓: Navigate | Enter: Add to input | Space: Toggle | u: Parent | r: Refresh | q: Back";
        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Help")
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::Gray));
        f.render_widget(help, chunks[3]);
    }

    fn truncate_string_safe(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            s.to_string()
        } else {
            s.chars().take(max_chars).collect::<String>() + "..."
        }
    }

    // Visual Modeで使用するヘルパーメソッド
    fn move_to_next_word(&mut self) {
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let mut pos = self.cursor_position;
        
        // 現在の位置が空白でない場合、空白まで移動
        while pos < graphemes.len() && !graphemes[pos].chars().all(char::is_whitespace) {
            pos += 1;
        }
        
        // 空白をスキップ
        while pos < graphemes.len() && graphemes[pos].chars().all(char::is_whitespace) {
            pos += 1;
        }
        
        self.cursor_position = pos.min(graphemes.len());
    }
    
    fn move_to_prev_word(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let mut pos = self.cursor_position - 1;
        
        // 空白をスキップ
        while pos > 0 && graphemes[pos].chars().all(char::is_whitespace) {
            pos -= 1;
        }
        
        // 単語の先頭まで移動
        while pos > 0 && !graphemes[pos - 1].chars().all(char::is_whitespace) {
            pos -= 1;
        }
        
        self.cursor_position = pos;
    }
    
    fn delete_visual_selection(&mut self) {
        if let Some(start) = self.visual_start {
            let (start_pos, end_pos) = if start <= self.cursor_position {
                (start, self.cursor_position + 1)
            } else {
                (self.cursor_position, start + 1)
            };
            
            let graphemes: Vec<&str> = self.input.graphemes(true).collect();
            let mut new_input = String::new();
            
            for (i, grapheme) in graphemes.iter().enumerate() {
                if i < start_pos || i >= end_pos {
                    new_input.push_str(grapheme);
                }
            }
            
            self.input = new_input;
            self.cursor_position = start_pos.min(self.input.graphemes(true).count());
        }
    }
    
    fn get_visual_selection_range(&self) -> Option<(usize, usize)> {
        if let Some(start) = self.visual_start {
            let (start_pos, end_pos) = if start <= self.cursor_position {
                (start, self.cursor_position + 1)
            } else {
                (self.cursor_position, start + 1)
            };
            Some((start_pos, end_pos))
        } else {
            None
        }
    }

    // 入力フィールドの行数を更新
    fn update_input_line_count(&mut self) {
        self.input_line_count = if self.input.is_empty() {
            1
        } else {
            self.input.lines().count().max(1)
        };
    }

    // 複数行のカーソル位置を計算 (行, 列) を返す
    fn calculate_cursor_position(&self) -> (usize, usize) {
        if self.input.is_empty() {
            return (0, 0);
        }

        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let mut line = 0;
        let mut column = 0;
        
        for (i, grapheme) in graphemes.iter().enumerate() {
            if i >= self.cursor_position {
                break;
            }
            
            if *grapheme == "\n" {
                line += 1;
                column = 0;
            } else {
                column += UnicodeWidthStr::width(*grapheme);
            }
        }
        
        (line, column)
    }

    // プロンプト履歴に追加
    fn add_to_input_history(&mut self, input: String) {
        // 空の入力や同じ内容の連続は追加しない
        if input.trim().is_empty() || self.input_history.last() == Some(&input) {
            return;
        }
        
        self.input_history.push(input);
        
        // 履歴のサイズ制限（最大100個）
        if self.input_history.len() > 100 {
            self.input_history.remove(0);
        }
    }

    // 履歴を前に戻る（上矢印）
    fn navigate_history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // 初回の履歴ナビゲーション：現在の入力を保存し、最新の履歴を表示
                self.temp_input = self.input.clone();
                self.history_index = Some(self.input_history.len() - 1);
                self.input = self.input_history[self.input_history.len() - 1].clone();
            }
            Some(index) => {
                // さらに古い履歴に移動
                if index > 0 {
                    self.history_index = Some(index - 1);
                    self.input = self.input_history[index - 1].clone();
                }
            }
        }
        
        // カーソルを末尾に移動
        self.cursor_position = self.input.graphemes(true).count();
        self.update_input_line_count();
    }

    // 履歴を後に進む（下矢印）
    fn navigate_history_down(&mut self) {
        if let Some(index) = self.history_index {
            if index < self.input_history.len() - 1 {
                // より新しい履歴に移動
                self.history_index = Some(index + 1);
                self.input = self.input_history[index + 1].clone();
            } else {
                // 最新の履歴まで来たので、元の入力に戻る
                self.history_index = None;
                self.input = self.temp_input.clone();
                self.temp_input.clear();
            }
            
            // カーソルを末尾に移動
            self.cursor_position = self.input.graphemes(true).count();
            self.update_input_line_count();
        }
    }

    // 履歴ナビゲーションをリセット
    fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.temp_input.clear();
    }
}

impl ChatApp {
    fn render_notification(&self, f: &mut Frame, area: Rect, note: &str) {
        let notification = Paragraph::new(note)
            .style(Style::default().fg(Color::Yellow).bg(Color::Black).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Notification")
                    .border_type(BorderType::Rounded),
            );
        f.render_widget(notification, area);
    }
}
