use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Paragraph,
    },
    Frame,
};
use tokio::sync::mpsc;
use crate::gemini::GeminiClient;
use crate::history::HistoryManager;
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug)]
pub enum ChatEvent {
    UserMessage(String),
    AIResponse(String),
    Error(String),
}

pub struct ChatApp {
    pub input: String,
    pub cursor_position: usize,  // カーソルの位置（グラフィーム単位）
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
}

#[derive(Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
    Visual,
    SessionList,
}

#[derive(Debug)]
pub struct ChatMessage {
    pub content: String,
    pub is_user: bool,
}

impl ChatApp {
    pub fn new(gemini_client: GeminiClient, mut history_manager: HistoryManager) -> Self {
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
        
        let mut app = Self {
            input: String::new(),
            cursor_position: 0,
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
        };

        // 歓迎メッセージを追加（履歴が空の場合のみ）
        if app.messages.is_empty() {
            app.messages.push(ChatMessage {
                content: "Welcome to ConTUI! Press 'i' to start typing, 'q' to quit, 'n' for new session.".to_string(),
                is_user: false,
            });
        }

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
        }
    }

    fn handle_normal_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            // 終了
            KeyCode::Char('q') => return Ok(true),
            
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
                self.scroll_messages_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_messages_up();
            }
            KeyCode::Char('0') => {
                self.cursor_position = 0;
            }
            KeyCode::Char('$') => {
                self.cursor_position = self.input.graphemes(true).count();
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
            }
            
            // 送信
            KeyCode::Enter => {
                if !self.input.trim().is_empty() {
                    self.send_message();
                }
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
                if !self.input.trim().is_empty() {
                    self.send_message();
                } else {
                    self.insert_char('\n');
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
                self.scroll_messages_up();
            }
            KeyCode::Down => {
                self.scroll_messages_down();
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_visual_mode_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
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
                if let Err(e) = self.history_manager.get_history_mut().switch_session(session_id) {
                    eprintln!("Error switching session: {}", e);
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
                if let Err(e) = self.history_manager.get_history_mut().delete_session(session_id) {
                    eprintln!("Error deleting session: {}", e);
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
        }
    }

    fn scroll_messages_down(&mut self) {
        if self.scroll_offset < self.messages.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    pub fn handle_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::UserMessage(msg) => {
                self.messages.push(ChatMessage {
                    content: msg,
                    is_user: true,
                });
                self.scroll_to_bottom();
            }
            ChatEvent::AIResponse(msg) => {
                // 履歴管理にAIレスポンスを追加
                if let Err(e) = self.history_manager.get_history_mut().add_message(msg.clone(), false) {
                    eprintln!("Error adding AI response to history: {}", e);
                }
                
                self.messages.push(ChatMessage {
                    content: msg,
                    is_user: false,
                });
                self.is_loading = false;
                self.scroll_to_bottom();
                
                // 自動保存
                if let Err(e) = self.save_history() {
                    eprintln!("Error auto-saving history: {}", e);
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
        let message = self.input.clone();
        self.input.clear();
        self.cursor_position = 0;
        self.input_mode = InputMode::Normal;
        self.is_loading = true;

        // 履歴管理にメッセージを追加
        if let Err(e) = self.history_manager.get_history_mut().add_message(message.clone(), true) {
            eprintln!("Error adding message to history: {}", e);
        }

        // ユーザーメッセージを即座に追加
        self.messages.push(ChatMessage {
            content: message.clone(),
            is_user: true,
        });
        self.scroll_to_bottom();

        // 会話コンテキストを取得
        let context = self.history_manager.get_conversation_context(10);

        // AIレスポンスを非同期で取得
        let sender = self.event_sender.clone();
        let client = self.gemini_client.clone();
        
        tokio::spawn(async move {
            let result = if context.is_empty() {
                client.chat_with_search(&message).await
            } else {
                client.chat_with_search_and_context(&message, &context).await
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
    }

    fn create_new_session(&mut self) {
        let _session_id = self.history_manager.get_history_mut().new_session(None);
        self.messages.clear();
        self.messages.push(ChatMessage {
            content: "Started new conversation session.".to_string(),
            is_user: false,
        });
        self.scroll_to_bottom();
        
        if let Err(e) = self.save_history() {
            eprintln!("Error saving history: {}", e);
        }
    }

    fn save_history(&mut self) -> Result<()> {
        self.history_manager.save()
    }

    fn scroll_to_bottom(&mut self) {
        if !self.messages.is_empty() {
            self.list_state.select(Some(self.messages.len() - 1));
        }
    }

    pub fn render(&mut self, f: &mut Frame) {
        if self.input_mode == InputMode::SessionList {
            self.render_session_list(f);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ])
                .split(f.area());

            self.render_messages(f, chunks[0]);
            self.render_input(f, chunks[1]);
            self.render_help(f, chunks[2]);
        }
    }

    fn render_messages(&mut self, f: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .messages
            .iter()
            .map(|msg| {
                let style = if msg.is_user {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Blue)
                };
                
                let prefix = if msg.is_user { "You" } else { "AI" };
                let content = format!("{}: {}", prefix, msg.content);
                
                // 長いメッセージを複数行に分割
                let wrapped_content = if Self::calculate_display_width(&content) > (area.width as usize - 6) {
                    let mut lines = Vec::new();
                    let mut current_line = String::new();
                    let words: Vec<&str> = content.split_whitespace().collect();
                    
                    for word in words {
                        let word_width = Self::calculate_display_width(word);
                        let current_width = Self::calculate_display_width(&current_line);
                        
                        if current_width + word_width + 1 > (area.width as usize - 6) {
                            if !current_line.is_empty() {
                                lines.push(current_line.clone());
                                current_line = word.to_string();
                            } else {
                                current_line = word.to_string();
                            }
                        } else {
                            if !current_line.is_empty() {
                                current_line.push(' ');
                            }
                            current_line.push_str(word);
                        }
                    }
                    if !current_line.is_empty() {
                        lines.push(current_line);
                    }
                    lines.join("\n")
                } else {
                    content
                };
                
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
        };

        let title = match self.input_mode {
            InputMode::Normal => "Input (Press 'i' to insert, 'q' to quit)",
            InputMode::Insert => "Insert Mode (Press Esc to normal mode)",
            InputMode::Visual => "Visual Mode (Press Esc to normal mode)",
            InputMode::SessionList => "Session List (Press Enter to select, 'd' to delete, 'n' for new)",
        };

        let input = Paragraph::new(self.input.as_str())
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_type(BorderType::Rounded),
            );

        f.render_widget(input, area);

        // カーソル位置を計算（常に表示）
        let cursor_x = self.calculate_cursor_x_position();
        let cursor_pos_x = area.x + cursor_x as u16 + 1;
        let cursor_pos_y = area.y + 1;

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
                // Visualモードでは棒線カーソル
                f.set_cursor_position((cursor_pos_x, cursor_pos_y));
            }
            InputMode::SessionList => {
                // セッション一覧モードではカーソル非表示
            }
        }
    }

    fn calculate_cursor_x_position(&self) -> usize {
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let mut x_pos = 0;
        
        for (i, grapheme) in graphemes.iter().enumerate() {
            if i >= self.cursor_position {
                break;
            }
            x_pos += UnicodeWidthStr::width(*grapheme);
        }
        
        x_pos
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = match self.input_mode {
            InputMode::Normal => "Normal: i=insert, a=append, hjkl=move, q=quit, n=new session, s=save, S=sessions, Enter=send",
            InputMode::Insert => "Insert: Esc=normal, Enter=send/newline",
            InputMode::Visual => "Visual: Esc=normal",
            InputMode::SessionList => "Sessions: j/k=navigate, Enter=select, d=delete, n=new, q/Esc=back",
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Help")
                    .border_type(BorderType::Rounded),
            );

        f.render_widget(help, area);
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

    fn truncate_string_safe(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            s.to_string()
        } else {
            s.chars().take(max_chars).collect::<String>() + "..."
        }
    }

    fn calculate_display_width(s: &str) -> usize {
        UnicodeWidthStr::width(s)
    }
}
