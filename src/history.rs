use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatSession {
    pub id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Uuid,
    pub content: String,
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

    pub fn add_message(&mut self, content: String, is_user: bool) -> Result<()> {
        let session_id = self.current_session_id.ok_or_else(|| {
            anyhow::anyhow!("No active session")
        })?;

        let message = ChatMessage {
            id: Uuid::new_v4(),
            content,
            is_user,
            timestamp: Utc::now(),
        };

        if let Some(session) = self.sessions.get_mut(&session_id) {
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

    pub fn get_conversation_context(&self, max_messages: usize) -> Vec<String> {
        if let Some(session) = self.history.get_current_session() {
            let mut context = Vec::new();
            let start_index = session.messages.len().saturating_sub(max_messages);
            
            for message in &session.messages[start_index..] {
                let role = if message.is_user { "User" } else { "Assistant" };
                context.push(format!("{}: {}", role, message.content));
            }
            
            context
        } else {
            Vec::new()
        }
    }
}
