// todo_manager.rs
// TODOリスト管理のための構造体と実装

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone)]
pub struct TodoList {
    pub title: String,
    pub description: String,
    pub items: Vec<TodoItem>,
}

#[derive(Debug, Clone)]
pub struct TodoManager {
    pub current_list: Option<TodoList>,
    pub storage_path: String,
}

impl TodoManager {
    pub fn new() -> Result<Self, String> {
        Ok(TodoManager {
            current_list: None,
            storage_path: "todo_state.json".to_string(),
        })
    }

    pub fn create_new_list(&mut self, title: String, description: String) -> Result<(), String> {
        self.current_list = Some(TodoList {
            title,
            description,
            items: Vec::new(),
        });
        Ok(())
    }

    pub fn should_create_new_list(&self, _message: &str) -> bool {
        // メッセージ内容に応じて新規作成判定（簡易実装）
        self.current_list.is_none()
    }

    pub fn update_from_ai_response(&mut self, _msg: &str) -> Result<Vec<TodoItem>, String> {
        // AIレスポンスからTODOリストを更新するロジック（ダミー実装）
        Ok(Vec::new())
    }

    pub fn get_context_for_llm(&self) -> String {
        if let Some(list) = &self.current_list {
            let mut ctx = format!("{}: {}\n", list.title, list.description);
            for (i, item) in list.items.iter().enumerate() {
                ctx.push_str(&format!("- [{}] {}\n", if item.completed { "x" } else { " " }, item.title));
            }
            ctx
        } else {
            String::new()
        }
    }

    pub fn clear_current_list(&mut self) -> Result<(), String> {
        self.current_list = None;
        Ok(())
    }

    pub fn load(&mut self) -> Result<(), String> {
        // 永続化からロードする処理（ダミー）
        Ok(())
    }
}
