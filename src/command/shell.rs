#![allow(dead_code, unused_imports)]

use std::process::Command;
use std::time::Duration;
use tracing::info;

use super::syntax::{CommandOutcome, CommandResult, CommandType, ShellBlock};

pub async fn execute(blocks: Vec<ShellBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();
    let mut handles = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        if let Some(rejection) = safety_check(&block.command) {
            results.push(CommandResult {
                command_type: CommandType::Shell,
                block_index: i,
                outcome: CommandOutcome::Failure { error: rejection },
            });
            continue;
        }

        if block.is_async {
            let handle = tokio::spawn(async move { run_shell_async(&block.command, i).await });
            handles.push(handle);
            results.push(CommandResult {
                command_type: CommandType::Shell,
                block_index: i,
                outcome: CommandOutcome::Success {
                    summary: "async exec the command, will notify when done".to_string(),
                },
            });
        } else {
            let handle = tokio::spawn(async move {
                run_shell_sync(&block.command, i, Duration::from_secs(120)).await
            });
            handles.push(handle);
        }
    }

    for handle in handles {
        if let Ok(Some(r)) = handle.await {
            results.push(r);
        }
    }

    Ok(results)
}

pub fn safety_check(command: &str) -> Option<String> {
    let lowered = command.to_lowercase();

    if lowered.contains("sudo") {
        return Some("你不能执行此命令，因为：包含 sudo 提权操作。请自行在终端中执行。".into());
    }

    if lowered.contains("rm -rf /") || lowered.contains("rm -rf /*") {
        return Some("你不能执行此命令，因为：这是一个危险操作。已拦截。".into());
    }

    if lowered.contains("chmod 777")
        && (lowered.contains("/etc")
            || lowered.contains("/usr")
            || lowered.contains("/bin")
            || lowered.contains("/sys"))
    {
        return Some("你不能执行此命令，因为：存在安全风险。建议用户自行评估。".into());
    }

    if lowered.contains("dd if=") || lowered.contains("> /dev/sda") {
        return Some("你不能执行此命令，因为：存在安全风险。建议用户自行评估。".into());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_check_sudo() {
        assert!(safety_check("sudo rm -rf target").is_some());
        assert!(safety_check("sudo cargo build").is_some());
    }

    #[test]
    fn test_safety_check_rm_rf_root() {
        assert!(safety_check("rm -rf /").is_some());
        assert!(safety_check("rm -rf /*").is_some());
    }

    #[test]
    fn test_safety_check_rm_rf_workspace_safe() {
        assert!(safety_check("rm -rf ./target").is_none());
        assert!(safety_check("rm -rf target/").is_none());
        assert!(safety_check("rm -rf node_modules").is_none());
    }

    #[test]
    fn test_safety_check_chmod_system_dir() {
        assert!(safety_check("chmod 777 /usr/bin/something").is_some());
        assert!(safety_check("chmod 777 /etc/passwd").is_some());
        assert!(safety_check("chmod 777 /bin/custom").is_some());
    }

    #[test]
    fn test_safety_check_chmod_workspace_safe() {
        assert!(safety_check("chmod 777 ./script.sh").is_none());
        assert!(safety_check("chmod 777 somefile").is_none());
    }

    #[test]
    fn test_safety_check_dd_and_dev() {
        assert!(safety_check("dd if=/dev/zero").is_some());
        assert!(safety_check("echo hello > /dev/sda").is_some());
    }

    #[test]
    fn test_safety_check_safe_commands() {
        assert!(safety_check("cargo test").is_none());
        assert!(safety_check("ls -la").is_none());
        assert!(safety_check("rg pattern src/").is_none());
        assert!(safety_check("cargo build").is_none());
    }

    #[test]
    fn test_truncate_output_short() {
        let (out, err) = truncate_output("line1\nline2\n".into(), String::new());
        assert_eq!(out, "line1\nline2\n");
        assert!(err.is_empty());
    }

    #[test]
    fn test_truncate_output_long() {
        let lines: Vec<String> = (1..=200).map(|i| format!("line {}", i)).collect();
        let input = lines.join("\n");
        let (out, _) = truncate_output(input, String::new());
        assert!(out.contains("lines trimmed"));
        assert!(out.contains("line 1"));
        assert!(out.contains("line 200"));
        assert!(!out.contains("line 51"));
        assert!(!out.contains("line 150"));
    }
}

async fn run_shell_sync(
    command: &str,
    block_index: usize,
    timeout: Duration,
) -> Option<CommandResult> {
    info!("Executing sync shell: {}", command);

    let child = tokio::task::spawn_blocking({
        let cmd = command.to_string();
        move || Command::new("nu").arg("-c").arg(&cmd).output()
    });

    let result = match tokio::time::timeout(timeout, child).await {
        Ok(Ok(Ok(output))) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let (stdout, stderr) = truncate_output(stdout, stderr);
            CommandResult {
                command_type: CommandType::Shell,
                block_index,
                outcome: CommandOutcome::Success {
                    summary: format!(
                        "exit_code: {}\nstdout: {}\nstderr: {}",
                        output.status.code().unwrap_or(-1),
                        stdout,
                        stderr
                    ),
                },
            }
        }
        Ok(Ok(Err(e))) => CommandResult {
            command_type: CommandType::Shell,
            block_index,
            outcome: CommandOutcome::Failure {
                error: format!("command spawn error: {}", e),
            },
        },
        Err(_) => CommandResult {
            command_type: CommandType::Shell,
            block_index,
            outcome: CommandOutcome::Failure {
                error: "command timed out".into(),
            },
        },
        _ => CommandResult {
            command_type: CommandType::Shell,
            block_index,
            outcome: CommandOutcome::Failure {
                error: "unexpected error".into(),
            },
        },
    };

    Some(result)
}

async fn run_shell_async(command: &str, block_index: usize) -> Option<CommandResult> {
    info!("Executing async shell: {}", command);
    let _ = tokio::process::Command::new("nu")
        .arg("-c")
        .arg(command)
        .spawn()
        .ok()?;

    Some(CommandResult {
        command_type: CommandType::Shell,
        block_index,
        outcome: CommandOutcome::Success {
            summary: "async exec the command, will notify when done".to_string(),
        },
    })
}

fn truncate_output(stdout: String, stderr: String) -> (String, String) {
    const MAX_LINES: usize = 100;

    fn trim(s: String) -> String {
        let lines: Vec<&str> = s.lines().collect();
        if lines.len() <= MAX_LINES {
            return s;
        }
        let head: Vec<&str> = lines.iter().take(50).copied().collect();
        let tail: Vec<&str> = lines.iter().rev().take(50).copied().rev().collect();
        format!(
            "{}\n... ({} lines trimmed, total {} lines) ...\n{}",
            head.join("\n"),
            lines.len() - MAX_LINES,
            lines.len(),
            tail.join("\n")
        )
    }

    (trim(stdout), trim(stderr))
}
