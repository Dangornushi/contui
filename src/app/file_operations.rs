use std::path::PathBuf;

use crate::app::{ChatApp, InputMode};
use unicode_segmentation::UnicodeSegmentation;

impl ChatApp {
    pub fn refresh_directory_contents(&mut self) {
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

    pub fn file_browser_previous(&mut self) {
        let selected = self.file_browser_state.selected().unwrap_or(0);
        if selected > 0 {
            self.file_browser_state.select(Some(selected - 1));
        }
    }

    pub fn file_browser_next(&mut self) {
        let selected = self.file_browser_state.selected().unwrap_or(0);
        if selected < self.directory_contents.len().saturating_sub(1) {
            self.file_browser_state.select(Some(selected + 1));
        }
    }

    pub fn open_selected_file(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if item.ends_with('/') {
                    // ディレクトリに移動
                    let mut path = PathBuf::from(&self.current_directory);
                    path.push(item.trim_end_matches('/'));
                    self.current_directory = path.to_string_lossy().to_string();
                    self.refresh_directory_contents();
                    self.file_browser_state.select(Some(0));
                } else {
                    // ファイルを入力フィールドに追加
                    let mut path = PathBuf::from(&self.current_directory);
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

    pub fn toggle_file_selection(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = PathBuf::from(&self.current_directory);
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

    pub fn delete_selected_file(&mut self) {
        if let Some(selected) = self.file_browser_state.selected() {
            if let Some(item) = self.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = PathBuf::from(&self.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if let Some(pos) = self.selected_files.iter().position(|x| x == &file_path) {
                        self.selected_files.remove(pos);
                    }
                }
            }
        }
    }

    pub fn go_to_parent_directory(&mut self) {
        let path = PathBuf::from(&self.current_directory);
        if let Some(parent) = path.parent() {
            self.current_directory = parent.to_string_lossy().to_string();
            self.refresh_directory_contents();
            self.file_browser_state.select(Some(0));
        }
    }
}