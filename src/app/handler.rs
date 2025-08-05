use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, KeyEventKind};
use anyhow::Result;

use crate::app::{ChatApp, InputMode, ChatMessage};
use unicode_segmentation::UnicodeSegmentation;

impl ChatApp {
    pub async fn handle_key_event(&mut self, key_event: KeyEvent, terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> Result<bool> {
        self.ui.notification = None;
        if key_event.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match self.ui.input_mode {
            InputMode::Normal => self.handle_normal_mode_key(key_event, terminal).await,
            InputMode::Insert => self.handle_insert_mode_key(key_event, terminal).await,
            InputMode::Visual => self.handle_visual_mode_key(key_event).await,
            InputMode::SessionList => self.handle_session_list_key(key_event).await,
            InputMode::FileBrowser => self.handle_file_browser_key(key_event).await,
            // InputMode::TodoListは削除
        }
    }

    pub async fn handle_normal_mode_key(&mut self, key_event: KeyEvent, terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> Result<bool> {
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.ui.show_help = !self.ui.show_help;
            return Ok(false);
        }
        
        match key_event.code {
            // 終了
            KeyCode::Char('q') => {
                return Ok(true);
            }
            
            // セッション一覧
            KeyCode::Char('S') => {
                self.ui.input_mode = InputMode::SessionList;
                self.ui.session_list_state.select(Some(0));
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
                self.ui.input_mode = InputMode::Insert;
            }
            KeyCode::Char('a') => {
                self.ui.input_mode = InputMode::Insert;
                self.move_cursor_right();
            }
            KeyCode::Char('A') => {
                self.ui.input_mode = InputMode::Insert;
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
            }
            KeyCode::Char('I') => {
                self.ui.input_mode = InputMode::Insert;
                self.ui.cursor_position = 0;
            }
            KeyCode::Char('o') => {
                self.ui.input_mode = InputMode::Insert;
                self.ui.input.push('\n');
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
            }
            KeyCode::Char('O') => {
                self.ui.input_mode = InputMode::Insert;
                self.ui.input.insert(0, '\n');
                self.ui.cursor_position = 0;
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
                if self.ui.input.trim().is_empty() {
                    self.scroll_messages_up();
                } else if self.ui.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_up();
                }
            }
            KeyCode::Char('0') => {
                self.ui.cursor_position = 0;
            }
            KeyCode::Char('$') => {
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
            }
            
            // Visual Mode
            KeyCode::Char('v') => {
                self.ui.input_mode = InputMode::Visual;
                self.ui.visual_start = Some(self.ui.cursor_position);
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
                self.ui.input.clear();
                self.ui.cursor_position = 0;
                self.ui.input_line_count = 1;
            }
            
            // 送信
            KeyCode::Enter => {
                if !self.ui.input.trim().is_empty() {
                    self.send_message(terminal).await;
                } else {
                    // 入力が空の場合、選択されたメッセージを入力欄に挿入
                    self.insert_selected_message();
                }
            }
            
            // ファイルブラウザ
            KeyCode::Char('f') => {
                self.ui.input_mode = InputMode::FileBrowser;
                self.refresh_directory_contents();
                self.ui.file_browser_state.select(Some(0));
            }
            
            // 選択されたメッセージを入力欄に挿入
            KeyCode::Char('y') => {
                self.insert_selected_message();
            }
            
            _ => {}
        }
        Ok(false)
    }

    pub async fn handle_insert_mode_key(&mut self, key_event: KeyEvent, terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> Result<bool> {
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.ui.show_help = !self.ui.show_help;
            return Ok(false);
        }
        
        match key_event.code {
            KeyCode::Esc => {
                self.ui.input_mode = InputMode::Normal;
                if self.ui.cursor_position > 0 {
                    self.ui.cursor_position -= 1;
                }
            }
            KeyCode::Enter => {
                // クロスターム側でShiftをチェック
                if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_char('\n');
                    self.update_input_line_count();
                    return Ok(false);
                }
                
                // 修飾子が完全に空の場合のみ送信処理
                if key_event.modifiers.is_empty() {
                    if !self.ui.input.trim().is_empty() {
                        self.send_message(terminal).await;
                        // 送信後にUIを即時再描画
                        terminal.draw(|f| self.render(f))?;
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
                if self.ui.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    // 単一行の場合は履歴をナビゲート
                    self.navigate_history_up();
                }
            }
            KeyCode::Down => {
                if self.ui.input.lines().count() > 1 {
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

    pub async fn handle_visual_mode_key(&mut self, key_event: KeyEvent) -> Result<bool> {
        // Ctrl+H でヘルプ表示を切り替え
        if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('h') {
            self.ui.show_help = !self.ui.show_help;
            return Ok(false);
        }
        
        match key_event.code {
            KeyCode::Esc => {
                self.ui.input_mode = InputMode::Normal;
                self.ui.visual_start = None;
            }
            KeyCode::Char('v') => {
                // Visual Modeを終了してNormalモードに戻る
                self.ui.input_mode = InputMode::Normal;
                self.ui.visual_start = None;
            }
            
            // カーソル移動（選択範囲を拡張）
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_cursor_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_cursor_right();
            }
            KeyCode::Char('0') => {
                self.ui.cursor_position = 0;
            }
            KeyCode::Char('$') => {
                self.ui.cursor_position = self.ui.input.graphemes(true).count();
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
                self.ui.input_mode = InputMode::Normal;
                self.ui.visual_start = None;
            }
            
            // ヤンク（選択範囲をコピー）
            KeyCode::Char('y') => {
                // 今回は実装を簡略化してクリップボードに保存しない
                self.ui.input_mode = InputMode::Normal;
                self.ui.visual_start = None;
            }
            
            // 上下移動（複数行の場合は行移動、そうでなければメッセージスクロール）
            KeyCode::Char('j') | KeyCode::Down => {
                if self.ui.input.lines().count() > 1 {
                    self.move_cursor_down();
                } else {
                    self.scroll_messages_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.ui.input.lines().count() > 1 {
                    self.move_cursor_up();
                } else {
                    self.scroll_messages_up();
                }
            }
            
            _ => {}
        }
        Ok(false)
    }

    pub async fn handle_session_list_key(&mut self, key_event: KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc => {
                self.ui.input_mode = InputMode::Normal;
            }
            KeyCode::Char('q') => {
                self.ui.input_mode = InputMode::Normal;
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
                self.ui.input_mode = InputMode::Normal;
                self.create_new_session();
            }
            _ => {}
        }
        Ok(false)
    }

    pub async fn handle_file_browser_key(&mut self, key_event: KeyEvent) -> Result<bool> {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.ui.input_mode = InputMode::Normal;
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
                self.ui.input_mode = InputMode::Insert;
            }
            _ => {}
        }
        Ok(false)
    }

    // handle_todo_list_keyは不要になったため削除
}