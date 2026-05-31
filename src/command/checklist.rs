use std::fs;

use serde::{Deserialize, Serialize};

use super::syntax::{CheckListBlock, CheckListMode, CommandOutcome, CommandResult, CommandType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckListTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckListStore {
    tasks: Vec<CheckListTask>,
}

const CHECKLIST_PATH: &str = ".ncoding/checklist.json";

pub async fn execute(blocks: Vec<CheckListBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        let outcome = match block.mode {
            CheckListMode::List => cmd_list(),
            CheckListMode::Create => cmd_create(block.title, block.content),
            CheckListMode::Update => cmd_update(block.id, block.status),
        };

        results.push(CommandResult {
            command_type: CommandType::CheckList,
            block_index: i,
            outcome,
        });
    }

    Ok(results)
}

fn load_store() -> CheckListStore {
    let Ok(data) = fs::read_to_string(CHECKLIST_PATH) else {
        return CheckListStore { tasks: Vec::new() };
    };
    serde_json::from_str(&data).unwrap_or(CheckListStore { tasks: Vec::new() })
}

fn save_store(store: &CheckListStore) -> std::io::Result<()> {
    let _ = fs::create_dir_all(".ncoding");
    let json = serde_json::to_string_pretty(store)?;
    fs::write(CHECKLIST_PATH, json)
}

pub fn has_unfinished() -> bool {
    let store = load_store();
    store.tasks.iter().any(|t| t.status == "waiting" || t.status == "in_progress")
}

pub fn unfinished_summary() -> Option<String> {
    let store = load_store();
    let pending: Vec<&CheckListTask> = store
        .tasks
        .iter()
        .filter(|t| t.status == "waiting" || t.status == "in_progress")
        .collect();
    if pending.is_empty() {
        return None;
    }
    let mut summary = String::from("[CheckList] 未完成任务:\n");
    for t in pending {
    summary.push_str(&format!("  [{}] {} - {}\n", t.status, t.id, t.title));
    }
    Some(summary)
}

fn cmd_list() -> CommandOutcome {
    let store = load_store();
    if store.tasks.is_empty() {
        return CommandOutcome::Success {
            summary: "status: OK\ntasks: (empty)".into(),
        };
    }
    let mut out = String::from("status: OK\ntasks:\n");
    for t in &store.tasks {
        out.push_str(&format!(
            "  [{}] {} — {}\n",
            t.status, t.id, t.title
        ));
    }
    CommandOutcome::Success { summary: out }
}

fn cmd_create(title: Option<String>, content: Option<String>) -> CommandOutcome {
    let title = title.unwrap_or_else(|| "untitled".into());
    let content = content.unwrap_or_default();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

    let task = CheckListTask {
        id,
        title: title.clone(),
        status: "waiting".into(),
        content,
        created_at: now.clone(),
        updated_at: now,
    };

    let task_id = task.id.clone();
    let mut store = load_store();
    store.tasks.push(task);

    match save_store(&store) {
        Ok(()) => CommandOutcome::Success {
            summary: format!("status: OK\ntask: created '{}' (id: {})", title, task_id),
        },
        Err(e) => CommandOutcome::Failure {
            error: format!("Failed to save checklist: {}", e),
        },
    }
}

fn cmd_update(id: Option<String>, status: Option<String>) -> CommandOutcome {
    let id = match id {
        Some(id) => id,
        None => {
            return CommandOutcome::Failure {
                error: "id is required for update".into(),
            };
        }
    };
    let status = match status {
        Some(s) => s,
        None => {
            return CommandOutcome::Failure {
                error: "status is required for update".into(),
            };
        }
    };

    let valid_status = ["waiting", "in_progress", "done", "failed", "cancelled"];
    if !valid_status.contains(&status.as_str()) {
        return CommandOutcome::Failure {
            error: format!(
                "invalid status '{}'. Valid: {}",
                status,
                valid_status.join(", ")
            ),
        };
    }

    let mut store = load_store();
    if let Some(task) = store.tasks.iter_mut().find(|t| t.id == id) {
        task.status = status;
        task.updated_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let new_status = task.status.clone();
        match save_store(&store) {
            Ok(()) => CommandOutcome::Success {
                summary: format!("status: OK\ntask: '{}' -> {}", id, new_status),
            },
            Err(e) => CommandOutcome::Failure {
                error: format!("Failed to save checklist: {}", e),
            },
        }
    } else {
        CommandOutcome::Failure {
            error: format!("task with id '{}' not found", id),
        }
    }
}

