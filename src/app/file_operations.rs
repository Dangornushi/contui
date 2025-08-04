use std::path::PathBuf;

use crate::app::{ChatApp, InputMode};
use unicode_segmentation::UnicodeSegmentation;

impl ChatApp {
    pub fn refresh_directory_contents(&mut self) {
        match self.gemini_client.list_directory(&self.ui.current_directory) {
            Ok(contents) => {
                self.ui.directory_contents = contents;
            }
            Err(_) => {
                // エラーは無視
                self.ui.directory_contents.clear();
            }
        }
    }

    pub fn file_browser_previous(&mut self) {
        let selected = self.ui.file_browser_state.selected().unwrap_or(0);
        if selected > 0 {
            self.ui.file_browser_state.select(Some(selected - 1));
        }
    }

    pub fn file_browser_next(&mut self) {
        let selected = self.ui.file_browser_state.selected().unwrap_or(0);
        if selected < self.ui.directory_contents.len().saturating_sub(1) {
            self.ui.file_browser_state.select(Some(selected + 1));
        }
    }

    pub fn open_selected_file(&mut self) {
        if let Some(selected) = self.ui.file_browser_state.selected() {
            if let Some(item) = self.ui.directory_contents.get(selected) {
                if item.ends_with('/') {
                    // ディレクトリに移動
                    let mut path = PathBuf::from(&self.ui.current_directory);
                    path.push(item.trim_end_matches('/'));
                    self.ui.current_directory = path.to_string_lossy().to_string();
                    self.refresh_directory_contents();
                    self.ui.file_browser_state.select(Some(0));
                } else {
                    // ファイルを入力フィールドに追加
                    let mut path = PathBuf::from(&self.ui.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    // 入力フィールドにファイル参照を追加
                    if !self.ui.input.is_empty() {
                        self.ui.input.push(' ');
                    }
                    self.ui.input.push_str(&format!("@file:{}", file_path));
                    self.ui.cursor_position = self.ui.input.graphemes(true).count();
                    
                    // ファイルブラウザを閉じて入力モードに切り替え
                    self.ui.input_mode = InputMode::Insert;
                }
            }
        }
    }

    pub fn toggle_file_selection(&mut self) {
        if let Some(selected) = self.ui.file_browser_state.selected() {
            if let Some(item) = self.ui.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = PathBuf::from(&self.ui.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if let Some(pos) = self.ui.selected_files.iter().position(|x| x == &file_path) {
                        // 選択を解除して入力フィールドからも削除
                        self.ui.selected_files.remove(pos);
                        let file_ref = format!("@file:{}", file_path);
                        self.ui.input = self.ui.input.replace(&file_ref, "").trim().to_string();
                        self.ui.cursor_position = self.ui.input.graphemes(true).count();
                    } else {
                        // 選択に追加して入力フィールドにも追加
                        self.ui.selected_files.push(file_path.clone());
                        if !self.ui.input.is_empty() {
                            self.ui.input.push(' ');
                        }
                        self.ui.input.push_str(&format!("@file:{}", file_path));
                        self.ui.cursor_position = self.ui.input.graphemes(true).count();
                    }
                }
            }
        }
    }

    pub fn delete_selected_file(&mut self) {
        if let Some(selected) = self.ui.file_browser_state.selected() {
            if let Some(item) = self.ui.directory_contents.get(selected) {
                if !item.ends_with('/') {
                    let mut path = PathBuf::from(&self.ui.current_directory);
                    path.push(item);
                    let file_path = path.to_string_lossy().to_string();
                    
                    if let Some(pos) = self.ui.selected_files.iter().position(|x| x == &file_path) {
                        self.ui.selected_files.remove(pos);
                    }
                }
            }
        }
    }

    pub fn go_to_parent_directory(&mut self) {
        let path = PathBuf::from(&self.ui.current_directory);
        if let Some(parent) = path.parent() {
            self.ui.current_directory = parent.to_string_lossy().to_string();
            self.refresh_directory_contents();
            self.ui.file_browser_state.select(Some(0));
        }
    }
}