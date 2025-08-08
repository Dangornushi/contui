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
    // --- ファイル作成関連 ---
    pub fn process_file_creation_requests(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        let create_file_pattern = r"(?s)```create_file:([^\n]+)(?:\r?\n(.*?))?```";
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
                            &format!("❌ Failed to create file '{}' : {}", filename, e)
                        );
                        continue;
                    }
                }
            }
        }
        if !files_created.is_empty() {
            self.refresh_directory_contents();
            let summary = format!("📁 ファイル作成: {}", files_created.join(", "));
            self.ui.notification = Some(summary);
        }
        processed_response
    }

    pub fn manual_parse_file_creation(&mut self, response: &str) -> String {
        let mut processed_response = response.to_string();
        let mut files_created = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].starts_with("```create_file:") {
                let filename = lines[i].strip_prefix("```create_file:").unwrap_or("").trim();
                if filename.is_empty() {
                    i += 1;
                    continue;
                }
                let mut content_lines = Vec::new();
                i += 1;
                while i < lines.len() && !lines[i].starts_with("```") {
                    content_lines.push(lines[i]);
                    i += 1;
                }
                let content = content_lines.join("\n");
                match self.gemini_client.create_file_with_unique_name(filename, &content) {
                    Ok(actual_filename) => {
                        files_created.push(actual_filename.clone());
                        let original_block = format!("```create_file:{}\n{}\n```", filename, content);
                        let success_message = if actual_filename == filename {
                            format!("✅ File '{}' created successfully!", filename)
                        } else {
                            format!("✅ File '{}' created as '{}' (original name was taken)", filename, actual_filename)
                        };
                        processed_response = processed_response.replace(&original_block, &success_message);
                    }
                    Err(e) => {
                        let original_block = format!("```create_file:{}\n{}\n```", filename, content);
                        let error_msg = format!("❌ Failed to create file '{}' : {}", filename, e);
                        processed_response = processed_response.replace(&original_block, &error_msg);
                    }
                }
            }
            i += 1;
        }
        if !files_created.is_empty() {
            self.refresh_directory_contents();
            let summary = format!("\n\n📁 Created {} file(s): {}", files_created.len(), files_created.join(", "));
            processed_response.push_str(&summary);
        }
        processed_response
    }
}