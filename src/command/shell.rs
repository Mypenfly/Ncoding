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
        } else {
            let timeout = if command_uses_find(&block.command) {
                Duration::from_secs(10)
            } else {
                Duration::from_secs(120)
            };
            let handle =
                tokio::spawn(async move { run_shell_sync(&block.command, i, timeout).await });
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

    #[tokio::test]
    async fn test_run_shell_async_captures_output() {
        let blocks = vec![ShellBlock {
            command: "echo hello-async-capture".to_string(),
            is_async: true,
        }];
        let results = execute(blocks).await.unwrap();
        // Wait for all handles to complete - the execute function already does this
        let all_summaries: Vec<String> = results
            .iter()
            .filter_map(|r| match &r.outcome {
                CommandOutcome::Success { summary } => Some(summary.clone()),
                _ => None,
            })
            .collect();
        let combined = all_summaries.join("\n");
        assert!(
            combined.contains("-async-capture"),
            "async shell should capture stdout but got: {}",
            combined
        );
    }

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
    timeout_dur: Duration,
) -> Option<CommandResult> {
    info!("Executing sync shell: {}", command);

    let result = tokio::time::timeout(timeout_dur, async {
        let output = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let (stdout, stderr) = truncate_output(stdout, stderr);
                let exit_code = out.status.code().unwrap_or(-1);
                let status = if exit_code == 0 { "OK" } else { "FAILED" };
                CommandResult {
                    command_type: CommandType::Shell,
                    block_index,
                    outcome: CommandOutcome::Success {
                        summary: format!(
                            "status: {}\nexit_code: {}\nstdout:\n{}\nstderr:\n{}",
                            status, exit_code, stdout, stderr
                        ),
                    },
                }
            }
            Err(e) => CommandResult {
                command_type: CommandType::Shell,
                block_index,
                outcome: CommandOutcome::Failure {
                    error: format!("command spawn error: {}", e),
                },
            },
        }
    })
    .await;

    match result {
        Ok(r) => Some(r),
        Err(_) => Some(CommandResult {
            command_type: CommandType::Shell,
            block_index,
            outcome: CommandOutcome::Failure {
                error: if timeout_dur.as_secs() <= 10 {
                    "status: TIMEOUT (10s). Use rg instead of find for file search, or set is_async=true for slow commands.".into()
                } else {
                    "status: TIMEOUT (120s). Consider using is_async=true for long-running commands."
                        .into()
                },
            },
        }),
    }
}

async fn run_shell_async(command: &str, block_index: usize) -> Option<CommandResult> {
    info!("Executing async shell: {}", command);

    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let (stdout, stderr) = truncate_output(stdout, stderr);
            let exit_code = out.status.code().unwrap_or(-1);
            let status = if exit_code == 0 { "OK" } else { "FAILED" };
            Some(CommandResult {
                command_type: CommandType::Shell,
                block_index,
                outcome: CommandOutcome::Success {
                    summary: format!(
                        "status: {}\nexit_code: {}\nstdout:\n{}\nstderr:\n{}",
                        status, exit_code, stdout, stderr
                    ),
                },
            })
        }
        Err(e) => Some(CommandResult {
            command_type: CommandType::Shell,
            block_index,
            outcome: CommandOutcome::Failure {
                error: format!("command spawn error: {}", e),
            },
        }),
    }
}

fn command_uses_find(cmd: &str) -> bool {
    let lowered = cmd.to_lowercase();
    lowered.starts_with("find ") || lowered.contains("| find ")
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
