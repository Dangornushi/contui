use crate::app::ChatApp;

impl ChatApp {
    pub fn create_new_todo_list(&mut self) {
        if let Err(e) = self.todo_manager.create_new_list("New Todo List".to_string(), "".to_string()) {
            self.show_notification(&format!("Error creating todo list: {}", e));
        } else {
            self.show_notification("New todo list created");
        }
    }

    pub fn show_notification(&mut self, message: &str) {
        self.ui.notification = Some(message.to_string());
    }

    pub fn check_and_handle_failed_todos(&mut self, ai_response: &str) {
        // TODO: Implement logic to check for failed TODOs and initiate recursive correction flow
        // This might involve parsing the AI response for specific failure indicators
        // and then generating a new message to the AI to correct the issue.
        // For now, this is a placeholder.
    }

    pub fn append_todo_summary_to_response(&self, mut response: String) -> String {
        if let Some(ref list) = self.todo_manager.current_list {
            let completed_count = list.items.iter().filter(|item| item.completed).count();
            let total_count = list.items.len();
            if total_count > 0 {
                response.push_str(&format!("\n\n📋 TODO Progress: {}/{}", completed_count, total_count));
            }
        }
        response
    }
}