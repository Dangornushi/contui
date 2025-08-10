use crate::app::{ChatApp, InputMode};
use crate::history::ChatMessage;
use uuid::Uuid;
use chrono::Utc;

impl ChatApp {
    pub fn session_list_next(&mut self) {
        self.select_session_offset(1);
    }

    pub fn session_list_previous(&mut self) {
        self.select_session_offset(-1);
    }

    fn select_session_offset(&mut self, offset: isize) {
        let history_guard = self.history_manager.lock().unwrap();
        let sessions = history_guard.get_history().get_session_list();
        if sessions.is_empty() { return; }
        let len = sessions.len() as isize;
        let current = self.ui.session_list_state.selected().unwrap_or(0) as isize;
        let next = ((current + offset + len) % len) as usize;
        self.ui.session_list_state.select(Some(next));
    }

    pub fn switch_to_selected_session(&mut self) {
        if let Some(i) = self.ui.session_list_state.selected() {
            let session_id = {
                let history_guard = self.history_manager.lock().unwrap();
                let sessions = history_guard.get_history().get_session_list();
                sessions.get(i).map(|s| s.id)
            };

            if let Some(session_id) = session_id {
                let mut history_guard_mut = self.history_manager.lock().unwrap();
                if history_guard_mut.get_history_mut().switch_session(session_id).is_err() {
                    return;
                }
                drop(history_guard_mut); // Explicitly drop the guard

                let _ = self.save_history();
                self.restore_session_messages();
                self.ui.input_mode = InputMode::Normal;
                self.scroll_to_bottom(20);
            }
        }
    }

    pub fn delete_selected_session(&mut self) {
        if let Some(i) = self.ui.session_list_state.selected() {
            let session_id = {
                let history_guard = self.history_manager.lock().unwrap();
                let sessions = history_guard.get_history().get_session_list();
                sessions.get(i).map(|s| s.id)
            };

            if let Some(session_id) = session_id {
                let mut history_guard_mut = self.history_manager.lock().unwrap();
                if history_guard_mut.get_history_mut().delete_session(session_id).is_err() {
                    return;
                }
                drop(history_guard_mut); // Explicitly drop the guard

                let _ = self.save_history();
                
                let current_session_is_none = {
                    let history_guard_check = self.history_manager.lock().unwrap();
                    history_guard_check.get_history().current_session_id.is_none()
                };

                if current_session_is_none {
                    self.create_new_session();
                } else {
                    self.restore_session_messages();
                }
                self.scroll_to_bottom(20);
                self.adjust_session_selection(i);
            }
        }
    }

    fn restore_session_messages(&mut self) {
        self.messages.clear();
        let history_guard = self.history_manager.lock().unwrap();
        if let Some(session) = history_guard.get_history().get_current_session() {
            for hist_msg in &session.messages {
                self.messages.push(ChatMessage {
                    id: hist_msg.id,
                    content: hist_msg.content.clone(),
                    is_user: hist_msg.is_user,
                    timestamp: Utc::now(),
                });
            }
        }
        if self.messages.is_empty() {
            self.messages.push(ChatMessage {
                id: Uuid::new_v4(),
                content: "Welcome to ConTUI!".to_string(),
                is_user: false,
                timestamp: Utc::now(),
            });
        }
    }

    fn adjust_session_selection(&mut self, prev_index: usize) {
        let history_guard = self.history_manager.lock().unwrap();
        let sessions = history_guard.get_history().get_session_list();
        if sessions.is_empty() {
            self.ui.session_list_state.select(None);
        }
        else {
            let new_index = if prev_index >= sessions.len() {
                sessions.len() - 1
            } else {
                prev_index
            };
            self.ui.session_list_state.select(Some(new_index));
        }
    }
}