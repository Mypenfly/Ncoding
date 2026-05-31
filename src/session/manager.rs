#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    #[serde(other)]
    Info,
}

pub struct SessionManager {
    sessions_dir: PathBuf,
 backups_dir: PathBuf,
    current_session: Option<Session>,
    is_activated: bool,
}

impl SessionManager {
    pub fn new(sessions_dir: PathBuf, backups_dir: PathBuf) -> Self {
        Self {
            sessions_dir,
            backups_dir,
            current_session: None,
            is_activated: false,
        }
    }

    pub fn init_dirs(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.sessions_dir)?;
        fs::create_dir_all(&self.backups_dir)?;
        Ok(())
    }

    pub fn new_session(&mut self, name: &str) -> std::io::Result<()> {
        let session = Session {
            name: name.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            messages: Vec::new(),
        };
        self.current_session = Some(session);
        self.save_current()
    }

    pub fn save_current(&self) -> std::io::Result<()> {
        if let Some(session) = &self.current_session {
            let path = self.sessions_dir.join(format!("{}.json", session.name));
            let mut save_session = session.clone();
            save_session.messages.retain(|m| m.role != Role::Info);
            let json = serde_json::to_string_pretty(&save_session)?;
            fs::write(&path, json)?;
            debug!("Session saved: {}", path.display());
        }
        Ok(())
    }

    pub fn load_session(&mut self, name: &str) -> std::io::Result<()> {
        let path = self.sessions_dir.join(format!("{}.json", name));
        let json = fs::read_to_string(&path)?;
        let session: Session = serde_json::from_str(&json)?;
        info!("Session loaded: {} ({} messages)", name, session.messages.len());
        self.current_session = Some(session);
        Ok(())
    }

    pub fn list_sessions(&self) -> std::io::Result<Vec<(String, DateTime<Utc>)>> {
        let mut sessions = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(json) = fs::read_to_string(&path) {
                            if let Ok(session) = serde_json::from_str::<Session>(&json) {
                                sessions.push((name.to_string(), session.updated_at));
                            }
                        }
                    }
                }
            }
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.1));
        Ok(sessions)
    }

    pub fn rename_session(&mut self, new_name: &str) -> std::io::Result<()> {
        if let Some(ref session) = self.current_session {
            let old_path = self.sessions_dir.join(format!("{}.json", session.name));
            let new_path = self.sessions_dir.join(format!("{}.json", new_name));
            if new_path.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("session {} already exists", new_name),
                ));
            }
            if old_path.exists() {
                fs::rename(&old_path, &new_path)?;
            }
            let mut new_session = session.clone();
            new_session.name = new_name.to_string();
            new_session.updated_at = Utc::now();
            self.current_session = Some(new_session);
            self.save_current()?;
        }
        Ok(())
    }

    pub fn rename_current(&mut self, new_name: &str) -> std::io::Result<()> {
        self.rename_session(new_name)
    }

    pub fn delete_session(&self, name: &str) -> std::io::Result<()> {
        let path = self.sessions_dir.join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(&path)?;
        }
        let backup_dir = self.backups_dir.join(name);
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir)?;
        }
        info!("Session deleted: {}", name);
        Ok(())
    }

    pub fn current(&self) -> Option<&Session> {
        self.current_session.as_ref()
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current_session.as_ref().map(|s| s.name.as_str())
    }

    pub fn add_message(&mut self, message: Message) -> std::io::Result<()> {
        if self.current_session.is_some() {
            if message.role == Role::Info && !self.is_activated {
                self.add_message_lazy(message);
                return Ok(());
            }
            self.activate()?;
            if let Some(ref mut session) = self.current_session {
                session.messages.push(message);
                session.updated_at = Utc::now();
                self.save_current()?;
            }
        }
        Ok(())
    }

    pub fn add_message_lazy(&mut self, message: Message) {
        if let Some(ref mut session) = self.current_session {
            session.messages.push(message);
            session.updated_at = Utc::now();
        }
    }

    pub fn remove_last_turn(&mut self) -> Option<(Message, Message)> {
        if let Some(ref mut session) = self.current_session {
            let mut user_msg = None;
            let mut assistant_msg = None;

            for i in (0..session.messages.len()).rev() {
                match session.messages[i].role {
                    Role::Assistant if assistant_msg.is_none() => {
                        assistant_msg = Some(session.messages.remove(i));
                    }
                    Role::User if assistant_msg.is_some() && user_msg.is_none() => {
                        user_msg = Some(session.messages.remove(i));
                        break;
                    }
                    _ => continue,
                }
            }

            if let (Some(u), Some(a)) = (user_msg, assistant_msg) {
                Some((u, a))
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn truncate_context(&mut self, max_messages: usize) {
        if let Some(ref mut session) = self.current_session {
            let total = session.messages.len();
            if total <= max_messages {
                return;
            }
            let system_count = session
                .messages
                .iter()
                .take_while(|m| m.role == Role::System)
                .count();
            let keep = max_messages.max(system_count);
            let remove = total.saturating_sub(keep);
            let drain_start = system_count;
            session.messages.drain(drain_start..drain_start + remove);
        }
    }

    pub fn set_messages(&mut self, messages: Vec<Message>) {
        if let Some(ref mut session) = self.current_session {
            session.messages = messages;
            session.updated_at = Utc::now();
        }
    }

    pub fn messages(&self) -> Vec<Message> {
        self.current_session
            .as_ref()
            .map(|s| s.messages.clone())
            .unwrap_or_default()
    }

    pub fn is_activated(&self) -> bool {
        self.is_activated
    }

    pub fn prepare_session(&mut self, name: &str) {
        let session = Session {
            name: name.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            messages: Vec::new(),
        };
        self.current_session = Some(session);
        self.is_activated = false;
    }

    pub fn activate(&mut self) -> std::io::Result<()> {
        if !self.is_activated {
            self.is_activated = true;
            self.save_current()?;
        }
        Ok(())
    }

    pub fn rename_session_file(&mut self, new_name: &str) -> std::io::Result<()> {
        if let Some(ref session) = self.current_session {
            let old_path = self.sessions_dir.join(format!("{}.json", session.name));
            let new_path = self.sessions_dir.join(format!("{}.json", new_name));
            if old_path.exists() && old_path != new_path {
                fs::rename(&old_path, &new_path)?;
            }
            let mut new_session = session.clone();
            new_session.name = new_name.to_string();
            new_session.updated_at = Utc::now();
            self.current_session = Some(new_session);
            if self.is_activated {
                self.save_current()?;
            }
        }
        Ok(())
    }

    pub fn load_session_by_index(&mut self, index: usize) -> std::io::Result<()> {
        let sessions = self.list_sessions()?;
        if index >= sessions.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session index {} not found (total: {})", index, sessions.len()),
            ));
        }
        let name = &sessions[index].0;
        self.load_session(name)?;
        self.is_activated = true;
        Ok(())
    }
}

