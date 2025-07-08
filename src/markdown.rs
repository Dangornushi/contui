use unicode_width::UnicodeWidthStr;

/// テキストを指定した幅で自動改行する
pub fn wrap_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return text.to_string();
    }

    let mut wrapped_lines = Vec::new();
    let lines = text.lines();

    for line in lines {
        if UnicodeWidthStr::width(line) <= max_width {
            wrapped_lines.push(line.to_string());
        } else {
            // 行を単語単位で分割
            let words: Vec<&str> = line.split_whitespace().collect();
            let mut current_line = String::new();

            for word in words {
                let word_width = UnicodeWidthStr::width(word);
                
                // 単語が最大幅を超える場合、文字単位で強制分割
                if word_width > max_width {
                    // 現在の行に何かあれば保存
                    if !current_line.is_empty() {
                        wrapped_lines.push(current_line.clone());
                        current_line.clear();
                    }
                    
                    // 長い単語を文字単位で分割
                    let mut chars = word.chars();
                    let mut char_line = String::new();
                    
                    while let Some(ch) = chars.next() {
                        let char_width = UnicodeWidthStr::width(ch.to_string().as_str());
                        
                        if UnicodeWidthStr::width(char_line.as_str()) + char_width > max_width {
                            if !char_line.is_empty() {
                                wrapped_lines.push(char_line.clone());
                                char_line.clear();
                            }
                        }
                        char_line.push(ch);
                    }
                    
                    if !char_line.is_empty() {
                        current_line = char_line;
                    }
                    continue;
                }

                let current_width = UnicodeWidthStr::width(current_line.as_str());
                let space_width = if current_line.is_empty() { 0 } else { 1 };

                // 単語を追加しても幅を超えない場合
                if current_width + space_width + word_width <= max_width {
                    if !current_line.is_empty() {
                        current_line.push(' ');
                    }
                    current_line.push_str(word);
                } else {
                    // 現在の行を保存して新しい行を開始
                    if !current_line.is_empty() {
                        wrapped_lines.push(current_line.clone());
                    }
                    current_line = word.to_string();
                }
            }
            
            // 最後の行を追加
            if !current_line.is_empty() {
                wrapped_lines.push(current_line);
            }
        }
    }

    // 空の入力の場合、空行を1つ追加
    if wrapped_lines.is_empty() {
        wrapped_lines.push(String::new());
    }

    wrapped_lines.join("\n")
}