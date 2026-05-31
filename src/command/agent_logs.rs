use std::fs;

use super::syntax::{AgentLogsBlock, AgentLogsMode, CommandOutcome, CommandResult, CommandType};

const LOGS_DIR: &str = ".ncoding/agent_logs";

pub async fn execute(blocks: Vec<AgentLogsBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let _ = fs::create_dir_all(LOGS_DIR);

    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        let outcome = match block.mode {
            AgentLogsMode::Write => cmd_write(block.filename, block.content),
            AgentLogsMode::Read => cmd_read(block.filename),
            AgentLogsMode::List => cmd_list(),
        };

        results.push(CommandResult {
            command_type: CommandType::AgentLogs,
            block_index: i,
            outcome,
        });
    }

    Ok(results)
}

fn sanitize_filename(name: String) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

fn cmd_write(filename: Option<String>, content: Option<String>) -> CommandOutcome {
    let raw_name = filename.unwrap_or_else(|| {
        chrono::Utc::now().format("log_%Y%m%d_%H%M%S.md").to_string()
    });
    let name = sanitize_filename(raw_name);
    let content = content.unwrap_or_default();
    let path = std::path::PathBuf::from(LOGS_DIR).join(&name);

    match fs::write(&path, &content) {
        Ok(()) => CommandOutcome::Success {
            summary: format!("AgentLogs: wrote '{}' ({} bytes)", name, content.len()),
        },
        Err(e) => CommandOutcome::Failure {
            error: format!("Failed to write log '{}': {}", name, e),
        },
    }
}

fn cmd_read(filename: Option<String>) -> CommandOutcome {
    let name = match filename {
        Some(n) => sanitize_filename(n),
        None => {
            return CommandOutcome::Failure {
                error: "filename is required for read".into(),
            };
        }
    };
    let path = std::path::PathBuf::from(LOGS_DIR).join(&name);

    match fs::read_to_string(&path) {
        Ok(content) => {
            let preview = if content.lines().count() > 100 {
                let lines: Vec<&str> = content.lines().collect();
                format!(
                    "{}\n...\n{}",
                    lines[..50].join("\n"),
                    lines[lines.len() - 50..].join("\n"),
                )
            } else {
                content
            };
            CommandOutcome::Success {
                summary: format!("AgentLogs: '{}'\n{}", name, preview),
            }
        }
        Err(e) => CommandOutcome::Failure {
            error: format!("Failed to read log '{}': {}", name, e),
        },
    }
}

fn cmd_list() -> CommandOutcome {
    let dir = match fs::read_dir(LOGS_DIR) {
        Ok(d) => d,
        Err(e) => {
            return CommandOutcome::Failure {
                error: format!("Cannot read logs dir: {}", e),
            };
        }
    };

    let mut files: Vec<String> = dir
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.is_file() {
                path.file_name()?.to_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    files.sort();

    if files.is_empty() {
        return CommandOutcome::Success {
            summary: "AgentLogs: no logs found.".into(),
        };
    }

    let mut out = String::from("AgentLogs:\n");
    for f in &files {
        let path = std::path::PathBuf::from(LOGS_DIR).join(f);
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push_str(&format!("  {} ({} bytes)\n", f, size));
    }

    CommandOutcome::Success { summary: out }
}

