use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use crate::debug_log; // Add this line

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatSession {
    pub id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
}

use crate::gemini::{Content, Part}; // Moved from impl block

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Uuid,
    pub parts: Vec<Part>,
    pub is_user: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatHistory {
    pub sessions: HashMap<Uuid, ChatSession>,
    pub current_session_id: Option<Uuid>,
}

impl ChatHistory {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            current_session_id: None,
        }
    }

    /// 現在のセッションのメッセージを全て削除
    pub fn clear_messages(&mut self) -> Result<()> {
        let session_id = self.current_session_id.ok_or_else(|| {
            anyhow::anyhow!("No active session")
        })?;
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.messages.clear();
            session.updated_at = Utc::now();
            Ok(())
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    }

    pub fn new_session(&mut self, title: Option<String>) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let session = ChatSession {
            id,
            title: title.unwrap_or_else(|| format!("Chat Session {}", now.format("%Y-%m-%d %H:%M"))),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        };
        
        self.sessions.insert(id, session);
        self.current_session_id = Some(id);
        id
    }

    pub fn add_message(&mut self, parts: Vec<Part>, is_user: bool) -> Result<()> {
        let session_id = self.current_session_id.ok_or_else(|| {
            anyhow::anyhow!("No active session")
        })?;

        let message = ChatMessage {
            id: Uuid::new_v4(),
            parts: parts.clone(), // Store parts directly
            is_user,
            timestamp: Utc::now(),
        };

        if let Some(session) = self.sessions.get_mut(&session_id) {
            let message_type = if is_user { "User" } else { "AI" };
            // Adjust debug_log to print a summary of parts or just indicate parts are added
            debug_log!("[DEBUG] ChatHistory: Added {} message to session {}: Parts added", message_type, session_id);
            session.messages.push(message);
            session.updated_at = Utc::now();
        } else {
            return Err(anyhow::anyhow!("Session not found"));
        }

        Ok(())
    }

    pub fn get_current_session(&self) -> Option<&ChatSession> {
        self.current_session_id.and_then(|id| self.sessions.get(&id))
    }

    pub fn switch_session(&mut self, session_id: Uuid) -> Result<()> {
        if self.sessions.contains_key(&session_id) {
            self.current_session_id = Some(session_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    }

    pub fn get_session_list(&self) -> Vec<&ChatSession> {
        let mut sessions: Vec<&ChatSession> = self.sessions.values().collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions
    }

    pub fn delete_session(&mut self, session_id: Uuid) -> Result<()> {
        if self.sessions.remove(&session_id).is_some() {
            if self.current_session_id == Some(session_id) {
                self.current_session_id = None;
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    }
}

pub struct HistoryManager {
    history: ChatHistory,
    file_path: PathBuf,
}

impl HistoryManager {
    pub fn new() -> Result<Self> {
        let mut file_path = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find data directory"))?;
        file_path.push("contui");
        fs::create_dir_all(&file_path)?;
        file_path.push("chat_history.json");

        let history = if file_path.exists() {
            let content = fs::read_to_string(&file_path)?;
            serde_json::from_str(&content).unwrap_or_else(|_| ChatHistory::new())
        } else {
            ChatHistory::new()
        };

        Ok(Self {
            history,
            file_path,
        })
    }

    /// 現在のセッションのメッセージを全て削除
    pub fn clear_messages(&mut self) -> Result<()> {
        self.history.clear_messages()?;
        self.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.history)?;
        fs::write(&self.file_path, content)?;
        Ok(())
    }

    pub fn get_history(&self) -> &ChatHistory {
        &self.history
    }

    pub fn get_history_mut(&mut self) -> &mut ChatHistory {
        &mut self.history
    }

    pub fn ensure_active_session(&mut self) -> Uuid {
        if let Some(session_id) = self.history.current_session_id {
            if self.history.sessions.contains_key(&session_id) {
                return session_id;
            }
        }
        // 新しいセッションを作成
        self.history.new_session(None)
    }

    pub fn get_conversation_context(&self, max_messages: usize) -> Vec<Content> {
        if let Some(session) = self.history.get_current_session() {
            let start_index = session.messages.len().saturating_sub(max_messages);
            
            session.messages[start_index..].iter().map(|msg| {
                let actual_role = if msg.is_user {
                    "user".to_string()
                } else {
                    let has_function_call = msg.parts.iter().any(|p| matches!(p, crate::gemini::Part::FunctionCall { .. }));
                    let has_function_response = msg.parts.iter().any(|p| matches!(p, crate::gemini::Part::FunctionResponse { .. }));

                    if has_function_call {
                        "model".to_string()
                    } else if has_function_response {
                        "function".to_string()
                    } else {
                        "model".to_string()
                    }
                };

                Content {
                    role: actual_role,
                    parts: msg.parts.clone(),
                }
            }).collect()
        } else {
            Vec::new()
        }
    }
}
