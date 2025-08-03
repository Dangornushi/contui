use unicode_segmentation::UnicodeSegmentation;

use crate::app::ChatApp;

impl ChatApp {
    // Visual Modeで使用するヘルパーメソッド
    pub fn move_to_next_word(&mut self) {
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
    
    pub fn move_to_prev_word(&mut self) {
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
    
    pub fn delete_visual_selection(&mut self) {
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
    
    pub fn get_visual_selection_range(&self) -> Option<(usize, usize)> {
        if let Some(start) = self.visual_start {
            let end = self.cursor_position;
            if start <= end {
                Some((start, end + 1))
            } else {
                Some((end, start + 1))
            }
        } else {
            None
        }
    }
}