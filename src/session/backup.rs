#![allow(dead_code, unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::{debug, info, warn};

pub struct BackupManager {
    backups_dir: PathBuf,
    session_name: String,
}

impl BackupManager {
    pub fn new(backups_dir: PathBuf, session_name: String) -> Self {
        Self {
            backups_dir,
            session_name,
        }
    }

    pub fn session_backup_dir(&self) -> PathBuf {
        self.backups_dir.join(&self.session_name)
    }

    pub fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.session_backup_dir())?;
        Ok(())
    }

    pub fn backup_file(&self, original_path: &Path) -> std::io::Result<PathBuf> {
        if !original_path.exists() {
            return Ok(PathBuf::new());
        }

        let timestamp = Utc::now()
            .format("%Y%m%dT%H%M%SZ")
            .to_string();
        let safe_name = original_path
            .to_string_lossy()
            .replace(['/', '\\'], "_");
        let backup_name = format!("{}_{}", timestamp, safe_name);
        let backup_path = self.session_backup_dir().join(&backup_name);

        fs::copy(original_path, &backup_path)?;
        debug!("Backed up {} -> {}", original_path.display(), backup_path.display());
        Ok(backup_path)
    }

    pub fn restore_latest(&self) -> std::io::Result<Vec<String>> {
        let backup_dir = self.session_backup_dir();
        if !backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut restored = Vec::new();
        let mut entries: Vec<_> = fs::read_dir(&backup_dir)?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in &entries {
            let backup_path = entry.path();
            if !backup_path.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if let Some(original_part) = name_str.split_once('_').map(|x| x.1) {
                let original_path = restore_path_from_backup_name(original_part);

                if let Err(e) = fs::copy(&backup_path, &original_path) {
                    warn!("Failed to restore {}: {}", original_path.display(), e);
                } else {
                    fs::remove_file(&backup_path)?;
                    info!("Restored: {}", original_path.display());
                    restored.push(original_path.to_string_lossy().to_string());
                }
            }
        }

        Ok(restored)
    }
}

fn restore_path_from_backup_name(name: &str) -> PathBuf {
    let normalized = name.replace('_', "/");
    PathBuf::from(normalized)
}
