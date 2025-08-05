use crate::app::{ChatApp, ChatMessage, InputMode};

impl ChatApp {
    pub fn session_list_next(&mut self) {
        let sessions = self.history_manager.get_history().get_session_list();
        if !sessions.is_empty() {
            let i = match self.ui.session_list_state.selected() {
                Some(i) => (i + 1) % sessions.len(),
                None => 0,
            };
            self.ui.session_list_state.select(Some(i));
        }
    }

    pub fn session_list_previous(&mut self) {
        let sessions = self.history_manager.get_history().get_session_list();
        if !sessions.is_empty() {
            let i = match self.ui.session_list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        sessions.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.ui.session_list_state.select(Some(i));
        }
    }

    pub fn switch_to_selected_session(&mut self) {
        if let Some(i) = self.ui.session_list_state.selected() {
            let sessions = self.history_manager.get_history().get_session_list();
            if let Some(session) = sessions.get(i) {
                let session_id = session.id;
                if let Err(_) = self.history_manager.get_history_mut().switch_session(session_id) {
                    // エラーは無視
                    return;
                }
                
                // メッセージを再読み込み
                self.messages.clear();
                if let Some(session) = self.history_manager.get_history().get_current_session() {
                    for hist_msg in &session.messages {
                        self.messages.push(ChatMessage {
                            content: hist_msg.content.clone(),
                            is_user: hist_msg.is_user,
                        });
                    }
                }
                
                self.ui.input_mode = InputMode::Normal;
                self.scroll_to_bottom(20);
            }
        }
    }

    pub fn delete_selected_session(&mut self) {
        if let Some(i) = self.ui.session_list_state.selected() {
            let sessions = self.history_manager.get_history().get_session_list();
            if let Some(session) = sessions.get(i) {
                let session_id = session.id;
                
                // セッションを削除
                if let Err(_) = self.history_manager.get_history_mut().delete_session(session_id) {
                    // エラーは無視
                    return;
                }
                
                // 現在のセッションが削除された場合、新しいセッションを作成
                if self.history_manager.get_history().current_session_id.is_none() {
                    self.create_new_session();
                } else {
                    // メッセージを再読み込み
                    self.messages.clear();
                    if let Some(session) = self.history_manager.get_history().get_current_session() {
                        for hist_msg in &session.messages {
                            self.messages.push(ChatMessage {
                                content: hist_msg.content.clone(),
                                is_user: hist_msg.is_user,
                            });
                        }
                    }
                }
                
                self.scroll_to_bottom(20);
                
                // 選択位置を調整
                let remaining_sessions = self.history_manager.get_history().get_session_list();
                if remaining_sessions.is_empty() {
                    self.ui.session_list_state.select(None);
                } else {
                    let new_index = if i >= remaining_sessions.len() {
                        remaining_sessions.len() - 1
                    } else {
                        i
                    };
                    self.ui.session_list_state.select(Some(new_index));
                }
            }
        }
    }
}