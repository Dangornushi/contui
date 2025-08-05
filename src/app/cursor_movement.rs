use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

use crate::app::ChatApp;

impl ChatApp {
    // カーソル移動のヘルパー関数
    pub fn move_cursor_left(&mut self) {
        if self.ui.cursor_position > 0 {
            self.ui.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        let grapheme_count = self.ui.input.graphemes(true).count();
        if self.ui.cursor_position < grapheme_count {
            self.ui.cursor_position += 1;
        }
    }

    // 上方向への移動
    pub fn move_cursor_up(&mut self) {
        let lines: Vec<&str> = self.ui.input.lines().collect();
        if lines.len() <= 1 {
            return;
        }
        
        let (current_line, current_column) = self.calculate_cursor_position();
        if current_line > 0 {
            let target_line = current_line - 1;
            let line_start_pos = self.get_line_start_position(target_line);
            let line_length = lines[target_line].graphemes(true).count();
            let new_column = current_column.min(line_length);
            self.ui.cursor_position = line_start_pos + new_column;
        }
    }

    // 下方向への移動
    pub fn move_cursor_down(&mut self) {
        let lines: Vec<&str> = self.ui.input.lines().collect();
        if lines.len() <= 1 {
            return;
        }
        
        let (current_line, current_column) = self.calculate_cursor_position();
        if current_line < lines.len() - 1 {
            let target_line = current_line + 1;
            let line_start_pos = self.get_line_start_position(target_line);
            let line_length = lines[target_line].graphemes(true).count();
            let new_column = current_column.min(line_length);
            self.ui.cursor_position = line_start_pos + new_column;
        }
    }


    // 文字入力のヘルパー関数
    pub fn insert_char(&mut self, c: char) {
        let graphemes: Vec<&str> = self.ui.input.graphemes(true).collect();
        let mut new_input = String::new();
        
        for (i, grapheme) in graphemes.iter().enumerate() {
            if i == self.ui.cursor_position {
                new_input.push(c);
            }
            new_input.push_str(grapheme);
        }
        
        if self.ui.cursor_position >= graphemes.len() {
            new_input.push(c);
        }
        
        self.ui.input = new_input;
        self.ui.cursor_position += 1;
        self.update_input_line_count();
    }

    // 文字削除のヘルパー関数
    pub fn delete_char_at_cursor(&mut self) {
        let graphemes: Vec<&str> = self.ui.input.graphemes(true).collect();
        if self.ui.cursor_position < graphemes.len() {
            let mut new_input = String::new();
            for (i, grapheme) in graphemes.iter().enumerate() {
                if i != self.ui.cursor_position {
                    new_input.push_str(grapheme);
                }
            }
            self.ui.input = new_input;
            self.update_input_line_count();
        }
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.ui.cursor_position > 0 {
            self.ui.cursor_position -= 1;
            self.delete_char_at_cursor();
        }
    }

    // スクロール関数
    pub fn scroll_messages_up(&mut self) {
        if self.ui.scroll_offset > 0 {
            self.ui.scroll_offset -= 1;
            // list_stateも更新して表示を同期
            self.update_list_state_from_scroll();
        }
    }

    pub fn scroll_messages_down(&mut self) {
        self.ui.scroll_offset += 1;
    }

    // scroll_offsetからlist_stateを更新
    pub fn update_list_state_from_scroll(&mut self) {
        if !self.messages.is_empty() {
            self.ui.list_state.select(Some(self.ui.scroll_offset));
        }
    }
}
