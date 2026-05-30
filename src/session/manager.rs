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
}

impl SessionManager {
    pub fn new(sessions_dir: PathBuf, backups_dir: PathBuf) -> Self {
        Self {
            sessions_dir,
            backups_dir,
            current_session: None,
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
        if let Some(ref mut session) = self.current_session {
            session.messages.push(message);
            session.updated_at = Utc::now();
            self.save_current()?;
        }
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_manager() -> SessionManager {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let backups = dir.path().join("backups");
        SessionManager::new(sessions, backups)
    }

    fn make_msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: Some(content.to_string()),
            reasoning_content: None,
        }
    }

    #[test]
    fn test_new_session_creates_file() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("test-session").unwrap();
        assert_eq!(mgr.current_name(), Some("test-session"));

        let path = mgr.sessions_dir.join("test-session.json");
        assert!(path.exists());
        let json = std::fs::read_to_string(&path).unwrap();
        let session: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(session.name, "test-session");
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_add_and_get_messages() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("test").unwrap();
        mgr.add_message(make_msg(Role::System, "system")).unwrap();
        mgr.add_message(make_msg(Role::User, "hello")).unwrap();
        mgr.add_message(make_msg(Role::Assistant, "hi")).unwrap();

        let msgs = mgr.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[1].content.as_deref(), Some("hello"));
    }

    #[test]
    fn test_remove_last_turn() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("test").unwrap();
        mgr.add_message(make_msg(Role::System, "system")).unwrap();
        mgr.add_message(make_msg(Role::User, "q1")).unwrap();
        mgr.add_message(make_msg(Role::Assistant, "a1")).unwrap();
        mgr.add_message(make_msg(Role::User, "q2")).unwrap();
        mgr.add_message(make_msg(Role::Assistant, "a2")).unwrap();

        let removed = mgr.remove_last_turn();
        assert!(removed.is_some());
        let (u, a) = removed.unwrap();
        assert_eq!(u.content.as_deref(), Some("q2"));
        assert_eq!(a.content.as_deref(), Some("a2"));

        let msgs = mgr.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].content.as_deref(), Some("a1"));
    }

    #[test]
    fn test_truncate_context_keeps_system() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("test").unwrap();
        mgr.add_message(make_msg(Role::System, "system")).unwrap();
        for i in 0..10 {
            mgr.add_message(make_msg(Role::User, &format!("u{}", i))).unwrap();
            mgr.add_message(make_msg(Role::Assistant, &format!("a{}", i))).unwrap();
        }
        assert_eq!(mgr.messages().len(), 21);

        mgr.truncate_context(5);
        let msgs = mgr.messages();
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[0].content.as_deref(), Some("system"));
    }

    #[test]
    fn test_list_sessions() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("alpha").unwrap();
        mgr.add_message(make_msg(Role::User, "hi")).unwrap();
        mgr.new_session("beta").unwrap();
        mgr.add_message(make_msg(Role::User, "hello")).unwrap();

        let list = mgr.list_sessions().unwrap();
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn test_rename_session() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("old-name").unwrap();
        mgr.rename_session("new-name").unwrap();
        assert_eq!(mgr.current_name(), Some("new-name"));
        assert!(!mgr.sessions_dir.join("old-name.json").exists());
        assert!(mgr.sessions_dir.join("new-name.json").exists());
    }

    #[test]
    fn test_delete_session() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("to-delete").unwrap();
        let path = mgr.sessions_dir.join("to-delete.json");
        assert!(path.exists());
        mgr.delete_session("to-delete").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_load_session() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("load-test").unwrap();
        mgr.add_message(make_msg(Role::User, "persisted")).unwrap();

        let mut mgr2 = make_test_manager();
        mgr2.sessions_dir = mgr.sessions_dir.clone();
        mgr2.load_session("load-test").unwrap();
        let msgs = mgr2.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content.as_deref(), Some("persisted"));
    }

    #[test]
    fn test_set_messages() {
        let mut mgr = make_test_manager();
        mgr.init_dirs().unwrap();
        mgr.new_session("test").unwrap();
        mgr.set_messages(vec![
            make_msg(Role::System, "s"),
            make_msg(Role::User, "u"),
        ]);
        assert_eq!(mgr.messages().len(), 2);
    }
}
