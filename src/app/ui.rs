use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Text},
    widgets::{
        Block, BorderType, Borders, List, ListItem, Paragraph,
    },
    Frame,
};
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use std::io::stdout;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{ChatApp, InputMode, ChatMessage};
use crate::markdown::wrap_text;

impl ChatApp {
    /// AI進行状態メッセージを逐次追加し即時描画する
    pub fn push_ai_progress_message<B: ratatui::backend::Backend>(
        &mut self,
        msg: String,
        terminal: &mut Terminal<B>,
    ) {
        self.messages.push(ChatMessage {
            is_user: false,
            content: msg,
        });
        // 再描画（run_appから呼ばれる場合のみ即時反映）
        let _ = terminal.draw(|f| self.render(f));
    }
    pub fn render(&mut self, f: &mut Frame) {
        if self.input_mode == InputMode::SessionList {
            self.render_session_list(f);
        } else if self.input_mode == InputMode::FileBrowser {
            self.render_file_browser(f);
        } else {
            let input_height = (self.input_line_count + 2).clamp(3, 10) as u16;
            let notification_height = if self.notification.is_some() { 2 } else { 0 };
            
            // 通常表示（TODOパネル分割は削除）
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

    pub fn render_messages(&mut self, f: &mut Frame, area: Rect) {
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
                let content = format!(/* "{}: {{}}" */ "{}: {}", prefix, msg.content);
                
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

    pub fn render_input(&self, f: &mut Frame, area: Rect) {
        let input_style = match self.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Insert => Style::default().fg(Color::Yellow),
            InputMode::Visual => Style::default().fg(Color::Magenta),
            InputMode::SessionList => Style::default().fg(Color::Cyan),
            InputMode::FileBrowser => Style::default().fg(Color::Cyan),
            // InputMode::TodoListは削除済み
        };

        let title = match self.input_mode {
            InputMode::Normal => "Input (Press 'i' to insert, 'v' for visual, 'q' to quit)",
            InputMode::Insert => "Insert Mode (Shift+Enter: new line, Enter: send, Esc: normal mode)",
            InputMode::Visual => "Visual Mode (Select text, press 'd' to delete, 'y' to yank, Esc to exit)",
            InputMode::SessionList => "Session List (Press Enter to select, 'd' to delete, 'n' for new)",
            InputMode::FileBrowser => "File Browser (Press Enter to open, 'd' to delete, 'n' for new)",
            // InputMode::TodoListは削除済み
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
            // InputMode::TodoListは削除済み
        }
    }

    pub fn render_floating_help(&self, f: &mut Frame) {
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
                "  t                   - Toggle TODO panel",
                "  T                   - TODO list management",
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
            // InputMode::TodoListは削除済み
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

    pub fn render_session_list(&mut self, f: &mut Frame) {
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

    pub fn render_file_browser(&mut self, f: &mut Frame) {
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

    // render_todo_listは不要になったため削除

    // render_todo_panelは不要になったため削除

    pub fn render_notification(&self, f: &mut Frame, area: Rect, note: &str) {
        let notification_paragraph = Paragraph::new(note)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Notification")
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(notification_paragraph, area);
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use std::io::stdout;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

}

}
