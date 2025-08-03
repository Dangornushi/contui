use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::Result;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: TodoStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub dependencies: Vec<String>, // 依存するTODOのID
    pub tool_execution_result: Option<String>, // ツール実行結果
    pub error_message: Option<String>, // エラーメッセージ
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoList {
    pub id: String,
    pub title: String,
    pub description: String,
    pub items: HashMap<String, TodoItem>,
    pub order: Vec<String>, // 実行順序
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
}

#[derive(Debug)]
pub struct TodoManager {
    pub current_list: Option<TodoList>,
    pub storage_path: String,
}

impl TodoItem {
    pub fn new(title: String, description: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            description,
            status: TodoStatus::Pending,
            created_at: now,
            updated_at: now,
            dependencies: Vec::new(),
            tool_execution_result: None,
            error_message: None,
        }
    }

    pub fn update_status(&mut self, status: TodoStatus) {
        self.status = status;
        self.updated_at = chrono::Utc::now();
    }

    pub fn set_tool_result(&mut self, result: String) {
        self.tool_execution_result = Some(result);
        self.updated_at = chrono::Utc::now();
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
        self.status = TodoStatus::Failed;
        self.updated_at = chrono::Utc::now();
    }

    pub fn is_ready_to_execute(&self, completed_ids: &Vec<String>) -> bool {
        self.status == TodoStatus::Pending && 
        self.dependencies.iter().all(|dep_id| completed_ids.contains(dep_id))
    }
}

impl TodoList {
    pub fn new(title: String, description: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            description,
            items: HashMap::new(),
            order: Vec::new(),
            created_at: now,
            updated_at: now,
            is_active: true,
        }
    }

    pub fn add_item(&mut self, item: TodoItem) {
        let item_id = item.id.clone();
        self.items.insert(item_id.clone(), item);
        self.order.push(item_id);
        self.updated_at = chrono::Utc::now();
    }

    pub fn get_next_pending_item(&self) -> Option<&TodoItem> {
        let completed_ids: Vec<String> = self.items.values()
            .filter(|item| item.status == TodoStatus::Completed)
            .map(|item| item.id.clone())
            .collect();

        for item_id in &self.order {
            if let Some(item) = self.items.get(item_id) {
                if item.is_ready_to_execute(&completed_ids) {
                    return Some(item);
                }
            }
        }
        None
    }

    pub fn get_current_step_description(&self) -> Option<String> {
        if let Some(item) = self.get_next_pending_item() {
            Some(format!("現在のステップ: {}\n詳細: {}", item.title, item.description))
        } else {
            None
        }
    }

    pub fn update_item_status(&mut self, item_id: &str, status: TodoStatus) -> Result<()> {
        if let Some(item) = self.items.get_mut(item_id) {
            item.update_status(status);
            self.updated_at = chrono::Utc::now();
            Ok(())
        } else {
            Err(anyhow::anyhow!("Todo item not found: {}", item_id))
        }
    }

    pub fn set_item_tool_result(&mut self, item_id: &str, result: String) -> Result<()> {
        if let Some(item) = self.items.get_mut(item_id) {
            item.set_tool_result(result);
            self.updated_at = chrono::Utc::now();
            Ok(())
        } else {
            Err(anyhow::anyhow!("Todo item not found: {}", item_id))
        }
    }

    pub fn set_item_error(&mut self, item_id: &str, error: String) -> Result<()> {
        if let Some(item) = self.items.get_mut(item_id) {
            item.set_error(error);
            self.updated_at = chrono::Utc::now();
            Ok(())
        } else {
            Err(anyhow::anyhow!("Todo item not found: {}", item_id))
        }
    }

    pub fn is_completed(&self) -> bool {
        self.items.values().all(|item| 
            item.status == TodoStatus::Completed || item.status == TodoStatus::Failed
        )
    }

    pub fn get_progress_summary(&self) -> String {
        let total = self.items.len();
        let completed = self.items.values().filter(|item| item.status == TodoStatus::Completed).count();
        let in_progress = self.items.values().filter(|item| item.status == TodoStatus::InProgress).count();
        let failed = self.items.values().filter(|item| item.status == TodoStatus::Failed).count();
        
        format!("進捗: {}/{} 完了, {} 進行中, {} 失敗", completed, total, in_progress, failed)
    }

    pub fn get_display_text(&self) -> String {
        let mut result = String::new();
        result.push_str(&format!("📋 {}\n", self.title));
        result.push_str(&format!("{}\n\n", self.description));
        result.push_str(&format!("{}\n\n", self.get_progress_summary()));
        
        for item_id in &self.order {
            if let Some(item) = self.items.get(item_id) {
                let status_icon = match item.status {
                    TodoStatus::Pending => "⭕",
                    TodoStatus::InProgress => "🔄",
                    TodoStatus::Completed => "✅",
                    TodoStatus::Failed => "❌",
                };
                result.push_str(&format!("{} {}\n", status_icon, item.title));
                
                if !item.description.is_empty() {
                    result.push_str(&format!("   {}\n", item.description));
                }
                
                if let Some(error) = &item.error_message {
                    result.push_str(&format!("   ❌ エラー: {}\n", error));
                }
            }
        }
        
        result
    }
}

impl TodoManager {
    pub fn new() -> Result<Self> {
        let storage_path = "todo_state.json".to_string();
        let mut manager = Self {
            current_list: None,
            storage_path,
        };
        
        // 既存のTODOリストを読み込み
        if let Err(_) = manager.load() {
            // ファイルが存在しない場合は無視
        }
        
        Ok(manager)
    }

    pub fn create_new_list(&mut self, title: String, description: String) -> Result<String> {
        let mut todo_list = TodoList::new(title, description.clone());
        // プロジェクト内容に基づいてTODO項目を生成
        let todo_items = self.generate_project_specific_todos(&description);
        for item in todo_items {
            todo_list.add_item(item);
        }
        // 最初のアイテムを進行中に設定
        if let Some(first_item_id) = todo_list.order.first() {
            if let Some(first_item) = todo_list.items.get_mut(first_item_id) {
                first_item.update_status(TodoStatus::InProgress);
            }
        }
        let list_id = todo_list.id.clone();
        self.current_list = Some(todo_list);
        self.save()?;
        Ok(list_id)
    }

    pub fn add_todo_item(&mut self, title: String, description: String) -> Result<String> {
        if let Some(ref mut list) = self.current_list {
            let item = TodoItem::new(title, description);
            let item_id = item.id.clone();
            list.add_item(item);
            self.save()?;
            Ok(item_id)
        } else {
            Err(anyhow::anyhow!("No active todo list"))
        }
    }

    pub fn get_next_todo(&self) -> Option<&TodoItem> {
        self.current_list.as_ref()?.get_next_pending_item()
    }

    pub fn get_current_step_context(&self) -> Option<String> {
        self.current_list.as_ref()?.get_current_step_description()
    }

    pub fn update_todo_status(&mut self, item_id: &str, status: TodoStatus) -> Result<()> {
        if let Some(ref mut list) = self.current_list {
            list.update_item_status(item_id, status)?;
            self.save()?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active todo list"))
        }
    }

    pub fn set_todo_tool_result(&mut self, item_id: &str, result: String) -> Result<()> {
        if let Some(ref mut list) = self.current_list {
            list.set_item_tool_result(item_id, result)?;
            self.save()?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active todo list"))
        }
    }

    pub fn set_todo_error(&mut self, item_id: &str, error: String) -> Result<()> {
        if let Some(ref mut list) = self.current_list {
            list.set_item_error(item_id, error)?;
            self.save()?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active todo list"))
        }
    }

    pub fn is_list_completed(&self) -> bool {
        self.current_list.as_ref().map_or(false, |list| list.is_completed())
    }

    pub fn clear_current_list(&mut self) -> Result<()> {
        self.current_list = None;
        self.save()?;
        Ok(())
    }

    pub fn get_display_text(&self) -> Option<String> {
        self.current_list.as_ref().map(|list| list.get_display_text())
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.current_list)?;
        fs::write(&self.storage_path, json)?;
        Ok(())
    }

    pub fn load(&mut self) -> Result<()> {
        if Path::new(&self.storage_path).exists() {
            let json = fs::read_to_string(&self.storage_path)?;
            self.current_list = serde_json::from_str(&json)?;
        }
        Ok(())
    }

    pub fn should_create_new_list(&self, user_message: &str) -> bool {
        // 新しいタスクかどうかを判断するロジック
        
        // 既存のリストが完了している場合
        if self.is_list_completed() {
            return true;
        }
        
        // 新しいプロジェクトを示すキーワード
        let new_project_keywords = [
            "新しい", "別の", "今度は", "次に", "create", "make", "build", 
            "implement", "develop", "design", "プロジェクト", "アプリ", "システム",
            "ツール", "ライブラリ", "サービス", "website", "app", "system", "tool"
        ];
        
        let message_lower = user_message.to_lowercase();
        new_project_keywords.iter().any(|keyword| message_lower.contains(keyword))
    }

    /// LLMに送信するTODOリストのコンテキストを取得
    pub fn get_context_for_llm(&self) -> String {
        if let Some(ref list) = self.current_list {
            let mut context = String::new();
            
            // TODOリストの基本情報
            context.push_str(&format!("**TODO List: {}**\n", list.title));
            context.push_str(&format!("Description: {}\n", list.description));
            context.push_str(&format!("Status: {}\n\n", list.get_progress_summary()));
            
            // 現在のステップ
            if let Some(current_step) = list.get_current_step_description() {
                context.push_str(&format!("**Current Step:**\n{}\n\n", current_step));
            }
            
            // 全TODOアイテムの状況
            context.push_str("**All TODO Items:**\n");
            for item_id in &list.order {
                if let Some(item) = list.items.get(item_id) {
                    let status_text = match item.status {
                        TodoStatus::Pending => "⭕ Pending",
                        TodoStatus::InProgress => "🔄 In Progress",
                        TodoStatus::Completed => "✅ Completed",
                        TodoStatus::Failed => "❌ Failed",
                    };
                    context.push_str(&format!("- {} {}\n", status_text, item.title));
                    
                    if !item.description.is_empty() {
                        context.push_str(&format!("  Description: {}\n", item.description));
                    }
                    
                    if let Some(ref error) = item.error_message {
                        context.push_str(&format!("  Error: {}\n", error));
                    }
                    
                    if let Some(ref result) = item.tool_execution_result {
                        context.push_str(&format!("  Result: {}\n", result));
                    }
                }
            }
            
            context.push_str("\n**Instructions:**\n");
            context.push_str("- Please help with the current step or provide guidance on the next actions\n");
            context.push_str("- If a step fails, suggest corrections or alternative approaches\n");
            context.push_str("- Update TODO status based on the conversation progress\n");
            
            context
        } else {
            String::new()
        }
    }

    /// AIレスポンスを解析してTODOステータスを自動更新
    pub fn update_from_ai_response(&mut self, ai_response: &str) -> Result<Vec<String>> {
        if self.current_list.is_none() {
            return Ok(Vec::new());
        }

        let mut updated_items = Vec::new();

        // ファイル作成の検出
        if let Some(created_files) = self.detect_file_creation(ai_response) {
            if !created_files.is_empty() {
                if let Some(item_id) = self.find_current_pending_item() {
                    if let Err(_) = self.update_todo_status(&item_id, TodoStatus::Completed) {
                        // エラーは無視
                    } else {
                        if let Err(_) = self.set_todo_tool_result(&item_id, format!("ファイル作成完了: {}", created_files.join(", "))) {
                            // エラーは無視
                        }
                        updated_items.push(item_id);
                    }
                }
            }
        }

        // コマンド実行の検出
        if let Some(command_result) = self.detect_command_execution(ai_response) {
            if let Some(item_id) = self.find_current_pending_item() {
                if command_result.success {
                    if let Err(_) = self.update_todo_status(&item_id, TodoStatus::Completed) {
                        // エラーは無視
                    } else {
                        if let Err(_) = self.set_todo_tool_result(&item_id, format!("コマンド実行完了: {}", command_result.description)) {
                            // エラーは無視
                        }
                        updated_items.push(item_id);
                    }
                } else {
                    if let Err(_) = self.set_todo_error(&item_id, format!("コマンド実行失敗: {}", command_result.description)) {
                        // エラーは無視
                    }
                    updated_items.push(item_id);
                }
            }
        }

        // エラーの検出
        if let Some(error_message) = self.detect_error_in_response(ai_response) {
            if let Some(item_id) = self.find_current_pending_item() {
                if let Err(_) = self.set_todo_error(&item_id, error_message) {
                    // エラーは無視
                }
                updated_items.push(item_id);
            }
        }

        // 次のTODOアイテムを自動的に進行中に設定
        if let Some(next_item_id) = self.find_next_pending_item() {
            if let Err(_) = self.update_todo_status(&next_item_id, TodoStatus::InProgress) {
                // エラーは無視
            } else {
                updated_items.push(next_item_id);
            }
        }

        if !updated_items.is_empty() {
            self.save()?;
        }

        Ok(updated_items)
    }

    /// 現在進行中のTODOアイテムのIDを取得
    fn find_current_pending_item(&self) -> Option<String> {
        if let Some(ref list) = self.current_list {
            for item_id in &list.order {
                if let Some(item) = list.items.get(item_id) {
                    if item.status == TodoStatus::InProgress {
                        return Some(item_id.clone());
                    }
                }
            }
            // 進行中がない場合は最初のPendingを返す
            for item_id in &list.order {
                if let Some(item) = list.items.get(item_id) {
                    if item.status == TodoStatus::Pending {
                        return Some(item_id.clone());
                    }
                }
            }
        }
        None
    }

    /// 次のPendingなTODOアイテムのIDを取得
    fn find_next_pending_item(&self) -> Option<String> {
        if let Some(ref list) = self.current_list {
            let completed_ids: Vec<String> = list.items.values()
                .filter(|item| item.status == TodoStatus::Completed)
                .map(|item| item.id.clone())
                .collect();

            for item_id in &list.order {
                if let Some(item) = list.items.get(item_id) {
                    if item.is_ready_to_execute(&completed_ids) {
                        return Some(item_id.clone());
                    }
                }
            }
        }
        None
    }

    /// AIレスポンスからファイル作成を検出
    fn detect_file_creation(&self, response: &str) -> Option<Vec<String>> {
        let mut created_files = Vec::new();
        
        // "✅ File" パターンを検索
        for line in response.lines() {
            if line.contains("✅ File") && (line.contains("created successfully") || line.contains("created as")) {
                // ファイル名を抽出
                if let Some(start) = line.find("'") {
                    if let Some(end) = line[start + 1..].find("'") {
                        let filename = &line[start + 1..start + 1 + end];
                        created_files.push(filename.to_string());
                    }
                }
            }
        }

        if created_files.is_empty() {
            None
        } else {
            Some(created_files)
        }
    }

    /// AIレスポンスからコマンド実行を検出
    fn detect_command_execution(&self, response: &str) -> Option<CommandResult> {
        // コマンド実行の成功/失敗を示すキーワードを検索
        let success_patterns = ["successfully executed", "command completed", "✅"];
        let error_patterns = ["failed to execute", "command failed", "❌", "Error:", "error:"];

        for line in response.lines() {
            let line_lower = line.to_lowercase();
            
            // 成功パターンの検出
            for pattern in &success_patterns {
                if line_lower.contains(&pattern.to_lowercase()) {
                    return Some(CommandResult {
                        success: true,
                        description: line.to_string(),
                    });
                }
            }
            
            // エラーパターンの検出
            for pattern in &error_patterns {
                if line_lower.contains(&pattern.to_lowercase()) {
                    return Some(CommandResult {
                        success: false,
                        description: line.to_string(),
                    });
                }
            }
        }

        None
    }

    /// AIレスポンスからエラーを検出
    fn detect_error_in_response(&self, response: &str) -> Option<String> {
        let error_patterns = ["Error:", "❌", "Failed", "Exception:", "panic!"];
        
        for line in response.lines() {
            for pattern in error_patterns.iter() {
                if line.contains(pattern) {
                    return Some(line.to_string());
                }
            }
        }
        
        None
    }
}

#[derive(Debug)]
struct CommandResult {
    success: bool,
    description: String,
}

impl TodoManager {
    /// 失敗したTODOアイテムに対する再帰的修正・再実行フロー
    pub fn handle_failed_todo_recursive(&mut self, failed_item_id: &str, error_context: &str) -> Result<String> {
        // 先に必要な情報を取得
        let retry_count = self.get_retry_count(failed_item_id);
        let (failed_title, failed_description) = if let Some(ref list) = self.current_list {
            if let Some(failed_item) = list.items.get(failed_item_id) {
                (failed_item.title.clone(), failed_item.description.clone())
            } else {
                return Err(anyhow::anyhow!("Failed item not found: {}", failed_item_id));
            }
        } else {
            return Err(anyhow::anyhow!("No active todo list"));
        };
        
        if retry_count >= 3 {
            // 最大リトライ回数に達した場合
            return Ok(format!(
                "最大リトライ回数（3回）に達しました。手動での修正が必要です:\n{}",
                failed_title
            ));
        }

        // 修正提案を生成
        let correction_suggestion = Self::generate_correction_suggestion_static(&failed_title, &failed_description, error_context);
        
        // 新しいリトライアイテムを作成
        let retry_item = TodoItem::new(
            format!("{} (修正 {}回目)", failed_title, retry_count + 1),
            format!("修正提案: {}\n\n元のエラー: {}", correction_suggestion, error_context)
        );
        
        let retry_item_id = retry_item.id.clone();
        
        // 失敗したアイテムの後に挿入
        if let Some(ref mut list) = self.current_list {
            if let Some(position) = list.order.iter().position(|id| id == failed_item_id) {
                list.order.insert(position + 1, retry_item_id.clone());
                list.items.insert(retry_item_id.clone(), retry_item);
                
                // 新しいアイテムを進行中に設定
                if let Some(new_item) = list.items.get_mut(&retry_item_id) {
                    new_item.update_status(TodoStatus::InProgress);
                }
                
                self.save()?;
                
                Ok(format!(
                    "修正提案を作成しました: {}\n提案内容: {}",
                    retry_item_id,
                    correction_suggestion
                ))
            } else {
                Err(anyhow::anyhow!("Failed item not found in order"))
            }
        } else {
            Err(anyhow::anyhow!("No active todo list"))
        }
    }

    /// TODOアイテムのリトライ回数を取得
    fn get_retry_count(&self, original_item_id: &str) -> usize {
        if let Some(ref list) = self.current_list {
            if let Some(original_item) = list.items.get(original_item_id) {
                let base_title = original_item.title.split(" (修正").next().unwrap_or(&original_item.title);
                
                list.items.values()
                    .filter(|item| item.title.starts_with(base_title) && item.title.contains("修正"))
                    .count()
            } else {
                0
            }
        } else {
            0
        }
    }

    /// エラーコンテキストに基づいて修正提案を生成（静的メソッド）
    fn generate_correction_suggestion_static(failed_title: &str, failed_description: &str, error_context: &str) -> String {
        let error_lower = error_context.to_lowercase();
        
        // 一般的なエラーパターンに基づく修正提案
        if error_lower.contains("file not found") || error_lower.contains("ファイルが見つかりません") {
            "ファイルパスを確認し、正しいディレクトリにファイルが存在するかチェックしてください。相対パスではなく絶対パスを使用することを検討してください。".to_string()
        } else if error_lower.contains("permission denied") || error_lower.contains("アクセスが拒否") {
            "ファイル・ディレクトリのアクセス権限を確認してください。sudo権限が必要な場合があります。".to_string()
        } else if error_lower.contains("syntax error") || error_lower.contains("構文エラー") {
            "コードの構文を確認してください。括弧の対応、セミコロンの有無、インデントなどをチェックしてください。".to_string()
        } else if error_lower.contains("import") || error_lower.contains("module") {
            "必要なモジュールやライブラリがインストールされているかチェックしてください。pip install や npm install などが必要な場合があります。".to_string()
        } else if error_lower.contains("connection") || error_lower.contains("network") {
            "ネットワーク接続を確認してください。プロキシ設定やファイアウォールの設定が原因の可能性があります。".to_string()
        } else if error_lower.contains("timeout") || error_lower.contains("タイムアウト") {
            "処理時間が長すぎる可能性があります。タイムアウト値を増やすか、処理を最適化してください。".to_string()
        } else {
            // 一般的な修正提案
            format!(
                "以下の観点から問題を確認してください:\n\
                1. 入力パラメータの妥当性\n\
                2. 依存関係の充足\n\
                3. 環境設定の確認\n\
                4. リソースの可用性\n\
                \n元のタスク: {}\nエラー詳細を確認し、適切な修正を行ってください。",
                failed_description
            )
        }
    }

    /// 自動修正可能なエラーかどうかを判定
    pub fn is_auto_correctable(&self, error_context: &str) -> bool {
        let auto_correctable_patterns = [
            "file not found",
            "ファイルが見つかりません",
            "syntax error",
            "構文エラー",
            "import error",
            "module not found",
            "permission denied",
        ];
        
        let error_lower = error_context.to_lowercase();
        auto_correctable_patterns.iter().any(|pattern| error_lower.contains(pattern))
    }

    /// 失敗したTODOの修正提案をLLMに送信するためのコンテキストを生成
    pub fn generate_retry_context(&self, failed_item_id: &str) -> Option<String> {
        if let Some(ref list) = self.current_list {
            if let Some(failed_item) = list.items.get(failed_item_id) {
                let mut context = String::new();
                context.push_str("## 失敗したタスクの修正要請\n\n");
                context.push_str(&format!("**失敗したタスク**: {}\n", failed_item.title));
                context.push_str(&format!("**タスク詳細**: {}\n", failed_item.description));
                
                if let Some(ref error) = failed_item.error_message {
                    context.push_str(&format!("**エラー内容**: {}\n", error));
                }
                
                if let Some(ref result) = failed_item.tool_execution_result {
                    context.push_str(&format!("**実行結果**: {}\n", result));
                }
                
                context.push_str("\n**修正要請**:\n");
                context.push_str("上記のエラーを分析し、適切な修正方法を提案してください。");
                context.push_str("可能であれば修正されたコードやコマンドを提供してください。\n");
                
                Some(context)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// プロジェクト内容に基づいてTODO項目を生成
    fn generate_project_specific_todos(&self, description: &str) -> Vec<TodoItem> {
        let desc_lower = description.to_lowercase();
        let mut todos = Vec::new();

        // プロジェクトタイプを判定
        let project_type = self.detect_project_type(&desc_lower);
        
        match project_type {
            ProjectType::WebDevelopment => {
                todos.push(TodoItem::new("プロジェクト設定".to_string(), "開発環境とプロジェクト構造を設定".to_string()));
                todos.push(TodoItem::new("フロントエンド実装".to_string(), "HTML/CSS/JavaScriptでUIを実装".to_string()));
                todos.push(TodoItem::new("バックエンド実装".to_string(), "サーバーサイドロジックとAPI実装".to_string()));
                todos.push(TodoItem::new("データベース設計".to_string(), "データモデルとスキーマを設計・実装".to_string()));
                todos.push(TodoItem::new("テスト・デプロイ".to_string(), "動作確認とデプロイメント".to_string()));
            },
            ProjectType::RustDevelopment => {
                todos.push(TodoItem::new("Cargoプロジェクト作成".to_string(), "cargo newでプロジェクト初期化".to_string()));
                todos.push(TodoItem::new("依存関係設定".to_string(), "Cargo.tomlに必要なcrateを追加".to_string()));
                todos.push(TodoItem::new("コア機能実装".to_string(), "メインロジックとモジュール実装".to_string()));
                todos.push(TodoItem::new("エラーハンドリング".to_string(), "Result型を使った適切なエラー処理".to_string()));
                todos.push(TodoItem::new("テスト作成".to_string(), "単体テストと統合テストを作成".to_string()));
            },
            ProjectType::PythonDevelopment => {
                todos.push(TodoItem::new("仮想環境作成".to_string(), "venvまたはcondaで環境を分離".to_string()));
                todos.push(TodoItem::new("依存関係管理".to_string(), "requirements.txtまたはpyproject.toml作成".to_string()));
                todos.push(TodoItem::new("メイン機能実装".to_string(), "コア機能とモジュール実装".to_string()));
                todos.push(TodoItem::new("パッケージ化".to_string(), "setup.pyまたはpyproject.tomlでパッケージ化".to_string()));
                todos.push(TodoItem::new("テスト・ドキュメント".to_string(), "pytestでテスト、docstringでドキュメント".to_string()));
            },
            ProjectType::DataAnalysis => {
                todos.push(TodoItem::new("データ収集".to_string(), "必要なデータソースを特定・収集".to_string()));
                todos.push(TodoItem::new("データクリーニング".to_string(), "欠損値処理と前処理".to_string()));
                todos.push(TodoItem::new("探索的データ分析".to_string(), "データの傾向と特徴を分析".to_string()));
                todos.push(TodoItem::new("モデル構築".to_string(), "機械学習モデルまたは統計モデル作成".to_string()));
                todos.push(TodoItem::new("結果可視化".to_string(), "グラフとレポートで結果を可視化".to_string()));
            },
            ProjectType::MobileDevelopment => {
                todos.push(TodoItem::new("開発環境設定".to_string(), "IDE、SDK、エミュレータの設定".to_string()));
                todos.push(TodoItem::new("UI/UX設計".to_string(), "画面設計とユーザーフロー作成".to_string()));
                todos.push(TodoItem::new("コア機能実装".to_string(), "アプリのメイン機能を実装".to_string()));
                todos.push(TodoItem::new("API連携".to_string(), "外部APIとの連携機能実装".to_string()));
                todos.push(TodoItem::new("テスト・リリース".to_string(), "デバイステストとストア申請".to_string()));
            },
            ProjectType::DevOps => {
                todos.push(TodoItem::new("インフラ設計".to_string(), "サーバー構成とネットワーク設計".to_string()));
                todos.push(TodoItem::new("CI/CD構築".to_string(), "自動ビルド・テスト・デプロイパイプライン".to_string()));
                todos.push(TodoItem::new("監視設定".to_string(), "ログ収集とメトリクス監視".to_string()));
                todos.push(TodoItem::new("セキュリティ設定".to_string(), "アクセス制御とセキュリティ対策".to_string()));
                todos.push(TodoItem::new("ドキュメント作成".to_string(), "運用手順書と構成図作成".to_string()));
            },
            ProjectType::FileOperation => {
                todos.push(TodoItem::new("ファイル分析".to_string(), "対象ファイルの構造と内容を分析".to_string()));
                todos.push(TodoItem::new("処理ロジック実装".to_string(), "ファイル操作の核となる処理を実装".to_string()));
                todos.push(TodoItem::new("エラーハンドリング".to_string(), "ファイルアクセスエラーの適切な処理".to_string()));
                todos.push(TodoItem::new("バックアップ機能".to_string(), "元ファイルの安全な保護機能".to_string()));
                todos.push(TodoItem::new("動作確認".to_string(), "様々なファイルでの動作テスト".to_string()));
            },
            ProjectType::Generic => {
                // 汎用的なTODO項目
                todos.push(TodoItem::new("要件分析".to_string(), "ユーザーのリクエストを分析し、必要な作業を特定".to_string()));
                todos.push(TodoItem::new("設計・計画".to_string(), "実装手順と必要なリソースを計画".to_string()));
                todos.push(TodoItem::new("実装".to_string(), "計画に基づいて実際の実装を行う".to_string()));
                todos.push(TodoItem::new("テスト・検証".to_string(), "実装結果をテストし、要件を満たしているか検証".to_string()));
                todos.push(TodoItem::new("最終調整".to_string(), "細かい調整と最終確認".to_string()));
            }
        }

        todos
    }

    /// プロジェクトタイプを検出
    fn detect_project_type(&self, description: &str) -> ProjectType {
        // Web開発関連
        if description.contains("web") || description.contains("website") || description.contains("html") || 
           description.contains("css") || description.contains("javascript") || description.contains("react") ||
           description.contains("vue") || description.contains("angular") || description.contains("サイト") ||
           description.contains("ウェブ") || description.contains("フロントエンド") || description.contains("バックエンド") {
            return ProjectType::WebDevelopment;
        }

        // Rust開発関連
        if description.contains("rust") || description.contains("cargo") || description.contains("crate") ||
           description.contains(".rs") || description.contains("rustc") {
            return ProjectType::RustDevelopment;
        }

        // Python開発関連
        if description.contains("python") || description.contains(".py") || description.contains("pip") ||
           description.contains("conda") || description.contains("venv") || description.contains("django") ||
           description.contains("flask") || description.contains("fastapi") {
            return ProjectType::PythonDevelopment;
        }

        // データ分析関連
        if description.contains("data") || description.contains("analysis") || description.contains("machine learning") ||
           description.contains("ai") || description.contains("pandas") || description.contains("numpy") ||
           description.contains("データ") || description.contains("分析") || description.contains("機械学習") ||
           description.contains("統計") || description.contains("可視化") {
            return ProjectType::DataAnalysis;
        }

        // モバイル開発関連
        if description.contains("mobile") || description.contains("android") || description.contains("ios") ||
           description.contains("app") || description.contains("flutter") || description.contains("react native") ||
           description.contains("モバイル") || description.contains("アプリ") || description.contains("スマホ") {
            return ProjectType::MobileDevelopment;
        }

        // DevOps関連
        if description.contains("deploy") || description.contains("docker") || description.contains("kubernetes") ||
           description.contains("ci/cd") || description.contains("infrastructure") || description.contains("server") ||
           description.contains("デプロイ") || description.contains("インフラ") || description.contains("サーバー") ||
           description.contains("監視") || description.contains("運用") {
            return ProjectType::DevOps;
        }

        // ファイル操作関連
        if description.contains("file") || description.contains("ファイル") || description.contains("csv") ||
           description.contains("json") || description.contains("xml") || description.contains("処理") ||
           description.contains("変換") || description.contains("整理") {
            return ProjectType::FileOperation;
        }

        ProjectType::Generic
    }
}

#[derive(Debug, PartialEq)]
enum ProjectType {
    WebDevelopment,
    RustDevelopment,
    PythonDevelopment,
    DataAnalysis,
    MobileDevelopment,
    DevOps,
    FileOperation,
    Generic,
}

// TODOリスト作成用のヘルパー関数
pub fn create_todo_list_from_request(user_request: &str) -> Result<TodoList> {
    // ユーザーリクエストからプロジェクト特化型TODOリストを自動生成
    let mut todo_manager = TodoManager::new()?;
    
    let title = format!("タスク: {}", user_request.chars().take(50).collect::<String>());
    let mut todo_list = TodoList::new(title, user_request.to_string());
    
    // プロジェクト内容に基づいてTODO項目を生成
    let todo_items = todo_manager.generate_project_specific_todos(user_request);
    
    for item in todo_items {
        todo_list.add_item(item);
    }
    
    Ok(todo_list)
}