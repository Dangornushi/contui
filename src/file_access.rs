use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};

#[derive(Clone)]
pub struct FileAccessManager {
    allowed_directories: Vec<PathBuf>,
}

impl FileAccessManager {
    pub fn new() -> Self {
        Self {
            allowed_directories: Vec::new(),
        }
    }

    /// 許可されたディレクトリを追加
    pub fn add_allowed_directory<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let canonical_path = fs::canonicalize(path)?;
        if !canonical_path.is_dir() {
            return Err(anyhow!("Path is not a directory: {:?}", canonical_path));
        }
        self.allowed_directories.push(canonical_path);
        Ok(())
    }

    /// パスがアクセス許可されているかチェック
    fn is_path_allowed<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path_ref = path.as_ref();
        
        // 絶対パスに変換
        let absolute_path = if path_ref.is_absolute() {
            path_ref.to_path_buf()
        } else {
            std::env::current_dir()?.join(path_ref)
        };
        
        // 可能であれば正規化
        let check_path = absolute_path.canonicalize().unwrap_or(absolute_path);
        
        for allowed_dir in &self.allowed_directories {
            if check_path.starts_with(allowed_dir) {
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    /// ファイルの内容を読み取り
    pub fn read_file<P: AsRef<Path>>(&self, path: P) -> Result<String> {
        if !self.is_path_allowed(&path)? {
            return Err(anyhow!("Access denied to path: {:?}", path.as_ref()));
        }

        let content = fs::read_to_string(path)?;
        Ok(content)
    }

    /// ディレクトリの内容をリスト
    pub fn list_directory<P: AsRef<Path>>(&self, path: P) -> Result<Vec<String>> {
        if !self.is_path_allowed(&path)? {
            return Err(anyhow!("Access denied to path: {:?}", path.as_ref()));
        }

        let mut entries = Vec::new();
        let dir_entries = fs::read_dir(path)?;

        for entry in dir_entries {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata()?;
            
            if metadata.is_dir() {
                entries.push(format!("{}/", file_name));
            } else {
                entries.push(file_name);
            }
        }

        entries.sort();
        Ok(entries)
    }

    /// ファイルを作成（重複チェック付き）- ユニークなファイル名を生成
    pub fn create_file_with_unique_name<P: AsRef<Path>>(&self, path: P, content: &str) -> Result<PathBuf> {
        let original_path = path.as_ref();
        
        // 親ディレクトリを取得
        let parent_dir = match original_path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."), // カレントディレクトリ
        };

        // 親ディレクトリが存在しない場合は作成
        if !parent_dir.exists() {
            if !self.is_path_allowed(parent_dir).unwrap_or(false) {
                return Err(anyhow!("Access denied to directory: {:?}", parent_dir));
            }
            fs::create_dir_all(parent_dir)?;
        }

        // ファイル作成権限をチェック
        if !self.is_path_allowed(parent_dir)? {
            return Err(anyhow!("Access denied to create file in: {:?}", parent_dir));
        }

        // ユニークなファイル名を生成
        let unique_path = self.generate_unique_filename(original_path)?;
        
        // ファイルを作成
        fs::write(&unique_path, content)?;
        Ok(unique_path)
    }

    /// ユニークなファイル名を生成する
    fn generate_unique_filename<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let original_path = path.as_ref();
        
        // 元のファイルが存在しない場合はそのまま返す
        if !original_path.exists() {
            return Ok(original_path.to_path_buf());
        }

        // ファイル名と拡張子を分離
        let file_stem = original_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let extension = original_path.extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{}", s))
            .unwrap_or_else(String::new);
        let parent = original_path.parent().unwrap_or(Path::new("."));

        // 番号付きファイル名を試行
        for i in 1..=9999 {
            let new_filename = format!("{}_{}{}", file_stem, i, extension);
            let new_path = parent.join(new_filename);
            
            if !new_path.exists() {
                return Ok(new_path);
            }
        }

        // 9999回試行しても見つからない場合はタイムスタンプ付きで作成
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let timestamp_filename = format!("{}_{}{}", file_stem, timestamp, extension);
        let timestamp_path = parent.join(timestamp_filename);
        
        Ok(timestamp_path)
    }
    /// 指定ファイルの指定行範囲を新しい内容で置換する
    pub fn edit_file_range<P: AsRef<Path>>(
        &self,
        path: P,
        start_line: usize,
        end_line: usize,
        new_content: &str,
    ) -> Result<()> {
        if !self.is_path_allowed(&path)? {
            return Err(anyhow!("Access denied to path: {:?}", path.as_ref()));
        }
        let file_path = path.as_ref();
        let original = fs::read_to_string(file_path)?;
        let lines: Vec<&str> = original.lines().collect();

        if start_line == 0 || end_line < start_line || end_line > lines.len() {
            return Err(anyhow!("Invalid line range"));
        }

        // 0-based index
        let start = start_line - 1;
        let end = end_line;

        let mut result = Vec::new();
        result.extend_from_slice(&lines[..start]);
        for l in new_content.lines() {
            result.push(l);
        }
        result.extend_from_slice(&lines[end..]);

        fs::write(file_path, result.join("\n"))?;
        Ok(())
    }
}
