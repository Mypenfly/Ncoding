#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoRound {
    pub user_msg_index: usize,
    pub files: Vec<UndoFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoFileEntry {
    pub original_path: String,
    pub backup_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoLog {
    pub rounds: Vec<UndoRound>,
}

pub struct BackupManager {
    backups_dir: PathBuf,
    session_name: String,
    max_undos: usize,
    current_round_files: Vec<UndoFileEntry>,
}

impl BackupManager {
    pub fn new(backups_dir: PathBuf, session_name: String, max_undos: usize) -> Self {
        Self {
            backups_dir,
            session_name,
            max_undos,
            current_round_files: Vec::new(),
        }
    }

    pub fn session_backup_dir(&self) -> PathBuf {
        self.backups_dir.join(&self.session_name)
    }

    pub fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.session_backup_dir())?;
        Ok(())
    }

    pub fn begin_round(&mut self) {
        self.current_round_files.clear();
    }

    pub fn backup_file(&mut self, original_path: &Path) -> std::io::Result<()> {
        if !original_path.exists() {
            return Ok(());
        }

        let dir = self.session_backup_dir();
        fs::create_dir_all(&dir)?;

        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%3f").to_string();
        let safe_name = original_path
            .to_string_lossy()
            .chars()
            .map(|c| if c == std::path::MAIN_SEPARATOR || c == ':' { '_' } else { c })
            .collect::<String>();
        let backup_name = format!("{}_{}", timestamp, safe_name);
        let backup_path = dir.join(&backup_name);

        fs::copy(original_path, &backup_path)?;
        debug!("Backed up {} -> {}", original_path.display(), backup_path.display());

        self.current_round_files.push(UndoFileEntry {
            original_path: original_path.to_string_lossy().to_string(),
            backup_name,
        });

        self.prune_old_backups(original_path)?;
        Ok(())
    }

    fn prune_old_backups(&self, original_path: &Path) -> std::io::Result<()> {
        let safe_prefix = original_path
            .to_string_lossy()
            .chars()
            .map(|c| if c == std::path::MAIN_SEPARATOR || c == ':' { '_' } else { c })
            .collect::<String>();

        let dir = self.session_backup_dir();
        if !dir.exists() {
            return Ok(());
        }

        let mut matching: Vec<(String, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(&safe_prefix) {
                let ts_part = name.split('_').next().unwrap_or("");
                matching.push((ts_part.to_string(), entry.path()));
            }
        }
        matching.sort_by_key(|(ts, _)| ts.clone());
        while matching.len() > self.max_undos {
            let (_, path) = matching.remove(0);
            let _ = fs::remove_file(&path);
            debug!("Pruned old backup: {}", path.display());
        }
        Ok(())
    }

    pub fn commit_round(&mut self, user_msg_index: usize) {
        if self.current_round_files.is_empty() {
            return;
        }

        let mut log = self.load_undo_log().unwrap_or_else(|_| UndoLog { rounds: Vec::new() });

        log.rounds.push(UndoRound {
            user_msg_index,
            files: std::mem::take(&mut self.current_round_files),
        });

        while log.rounds.len() > self.max_undos {
            let removed = log.rounds.remove(0);
            for entry in removed.files {
                let path = self.session_backup_dir().join(&entry.backup_name);
                let _ = fs::remove_file(&path);
            }
        }

        let _ = self.save_undo_log(&log);
    }

    pub fn pop_last_round(&mut self) -> Option<UndoRound> {
        let mut log = self.load_undo_log().ok()?;
        if log.rounds.is_empty() {
            return None;
        }
        let round = log.rounds.pop().unwrap();
        let _ = self.save_undo_log(&log);

        let backup_dir = self.session_backup_dir();
        let mut restored: Vec<String> = Vec::new();
        for entry in &round.files {
            let backup_path = backup_dir.join(&entry.backup_name);
            if backup_path.exists() {
                let original = PathBuf::from(&entry.original_path);
                if let Some(parent) = original.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = fs::copy(&backup_path, &original) {
                    warn!("Failed to restore {}: {}", original.display(), e);
                } else {
                    info!("Restored: {}", original.display());
                    restored.push(entry.original_path.clone());
                }
                let _ = fs::remove_file(&backup_path);
            }
        }
        if restored.is_empty() {
            None
        } else {
            Some(UndoRound { files: vec![], user_msg_index: round.user_msg_index })
        }
    }

    fn undo_log_path(&self) -> PathBuf {
        self.session_backup_dir().join("undo_log.json")
    }

    fn load_undo_log(&self) -> std::io::Result<UndoLog> {
        let path = self.undo_log_path();
        if !path.exists() {
            return Ok(UndoLog { rounds: Vec::new() });
        }
        let json = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&json).unwrap_or_else(|_| UndoLog { rounds: Vec::new() }))
    }

    fn save_undo_log(&self, log: &UndoLog) -> std::io::Result<()> {
        let path = self.undo_log_path();
        let json = serde_json::to_string_pretty(log)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn cleanup_session_backups(name: &str, backups_dir: &Path) -> std::io::Result<()> {
        let dir = backups_dir.join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
            info!("Cleaned up backups for session: {}", name);
        }
        Ok(())
    }
}
