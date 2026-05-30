pub mod agent_logs;
pub mod agent_skills;
pub mod checklist;
pub mod files_operator;
pub mod parser;
pub mod shell;
pub mod sub_agent_task;
pub mod syntax;
pub mod tool_call;

use tokio::sync::Mutex;
#[cfg_attr(not(test), allow(unused))]
pub static CWD_LOCK: Mutex<()> = Mutex::const_new(());

use self::syntax::{CommandResult, CommandType, NCommand};
use tracing::info;

pub struct CommandWatcher {
    buffer: String,
    parser: parser::CommandParser,
    last_scan: usize,
    pending_handles: Vec<tokio::task::JoinHandle<Vec<CommandResult>>>,
}

impl Default for CommandWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandWatcher {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            parser: parser::CommandParser::new(),
            last_scan: 0,
            pending_handles: Vec::new(),
        }
    }

    pub fn feed_token(&mut self, token: &str) {
        self.buffer.push_str(token);

        loop {
            let (cmds, new_scan, warnings) = self.parser.extract_commands_from(&self.buffer, self.last_scan);
            self.last_scan = new_scan;

            for w in &warnings {
                tracing::warn!("Parser warning (mid-stream): {}", w.message);
            }

            if cmds.is_empty() {
                break;
            }

            for cmd in cmds {
                info!("Command parsed: {}", cmd_label(&cmd));
                let handle = tokio::spawn(async move {
                    match cmd {
                        NCommand::Shell { blocks } => shell::execute(blocks).await,
                        NCommand::FilesOperator { blocks } => files_operator::execute(blocks).await,
                        NCommand::ToolCall { blocks } => tool_call::execute(blocks).await,
                        NCommand::SubAgentTask { blocks } => sub_agent_task::execute(blocks).await,
                        NCommand::AgentSkills { blocks } => agent_skills::execute(blocks).await,
                        NCommand::CheckList { blocks } => checklist::execute(blocks).await,
                        NCommand::AgentLogs { blocks } => agent_logs::execute(blocks).await,
                    }
                    .unwrap_or_default()
                });
                self.pending_handles.push(handle);
            }
        }
    }

    pub fn finalize(&mut self) -> (Vec<NCommand>, Vec<String>) {
        let (cmds, _, warnings) = self
            .parser
            .extract_commands_from_final(&self.buffer, self.last_scan);
        let warn_msgs: Vec<String> = warnings.iter().map(|w| w.message.clone()).collect();
        for w in &warn_msgs {
            tracing::warn!("Parser warning (final): {}", w);
        }
        self.buffer.clear();
        self.last_scan = 0;
        (cmds, warn_msgs)
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last_scan = 0;
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_handles.is_empty()
    }

    pub async fn drain_results(&mut self) -> Vec<CommandResult> {
        let handles = std::mem::take(&mut self.pending_handles);
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(mut r) => results.append(&mut r),
                Err(e) => tracing::error!("Command task join error: {}", e),
            }
        }
        results
    }
}

fn cmd_label(cmd: &NCommand) -> String {
    match cmd {
        NCommand::Shell { blocks } => {
            let cmd = blocks.first().map(|b| b.command.as_str()).unwrap_or("");
            let preview: String = cmd.chars().take(60).collect();
            if cmd.len() > 60 {
                format!("Shell({preview}...)")
            } else {
                format!("Shell({preview})")
            }
        }
        NCommand::FilesOperator { blocks } => {
            let b = blocks.first();
            let mode = match b.map(|b| b.mode) {
                Some(crate::command::syntax::FileMode::Read) => "read",
                Some(crate::command::syntax::FileMode::Write) => "write",
                Some(crate::command::syntax::FileMode::Edit) => "edit",
                None => "?",
            };
            let path = b.map(|b| b.path.display().to_string()).unwrap_or_default();
            format!("FilesOperator({mode} {path})")
        }
        NCommand::ToolCall { blocks } => {
            let name = blocks.first().map(|b| b.tool_name.as_str()).unwrap_or("");
            let n_args = blocks.first().map(|b| b.args.len()).unwrap_or(0);
            format!("ToolCall({name} args:{n_args})")
        }
        NCommand::SubAgentTask { blocks } => {
            let prompt = blocks.first().map(|b| b.prompt.as_str()).unwrap_or("");
            let preview: String = prompt.chars().take(40).collect();
            format!("SubAgentTask({preview}...)")
        }
        NCommand::AgentSkills { blocks } => {
            let mode = match blocks.first().map(|b| b.mode) {
                Some(crate::command::syntax::SkillsMode::List) => "list",
                Some(crate::command::syntax::SkillsMode::Load) => "load",
                None => "?",
            };
            let name = blocks.first().and_then(|b| b.skill_name.as_deref()).unwrap_or("");
            format!("AgentSkills({mode} {name})")
        }
        NCommand::CheckList { blocks } => {
            let mode = match blocks.first().map(|b| b.mode) {
                Some(crate::command::syntax::CheckListMode::Create) => "create",
                Some(crate::command::syntax::CheckListMode::Update) => "update",
                _ => "list",
            };
            let title = blocks.first().and_then(|b| b.title.as_deref()).unwrap_or("");
            format!("CheckList({mode} {title})")
        }
        NCommand::AgentLogs { blocks } => {
            let mode = match blocks.first().map(|b| b.mode) {
                Some(crate::command::syntax::AgentLogsMode::Write) => "write",
                Some(crate::command::syntax::AgentLogsMode::Read) => "read",
                _ => "list",
            };
            let name = blocks.first().and_then(|b| b.filename.as_deref()).unwrap_or("");
            format!("AgentLogs({mode} {name})")
        }
    }
}

pub async fn execute_commands(commands: Vec<NCommand>) -> Vec<CommandResult> {
    let mut tasks = Vec::new();
    for cmd in commands {
        info!("Executing command: {}", cmd_label(&cmd));
        let handle: tokio::task::JoinHandle<Result<Vec<CommandResult>, anyhow::Error>> =
            tokio::spawn(async move {
                match cmd {
                    NCommand::Shell { blocks } => shell::execute(blocks).await,
                    NCommand::FilesOperator { blocks } => files_operator::execute(blocks).await,
                    NCommand::ToolCall { blocks } => tool_call::execute(blocks).await,
                    NCommand::SubAgentTask { blocks } => sub_agent_task::execute(blocks).await,
                    NCommand::AgentSkills { blocks } => agent_skills::execute(blocks).await,
                    NCommand::CheckList { blocks } => checklist::execute(blocks).await,
                    NCommand::AgentLogs { blocks } => agent_logs::execute(blocks).await,
                }
            });
        tasks.push(handle);
    }

    let mut results = Vec::new();
    for task in tasks {
        match task.await {
            Ok(Ok(mut r)) => results.append(&mut r),
            Ok(Err(e)) => tracing::error!("Command execution error: {}", e),
            Err(e) => tracing::error!("Task join error: {}", e),
        }
    }

    results
}

pub fn format_command_results(results: &[CommandResult]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut out = String::from("【|SYSTEM|】\n");

    for r in results {
        match r.command_type {
            CommandType::Shell => out.push_str("[ShellResult]\n"),
            CommandType::FilesOperator => out.push_str("[FileResult]\n"),
            CommandType::ToolCall => out.push_str("[ToolCallResult]\n"),
            CommandType::SubAgentTask => out.push_str("[SubAgentResult]\n"),
            CommandType::AgentSkills => out.push_str("[SkillsResult]\n"),
            CommandType::CheckList => out.push_str("[CheckListResult]\n"),
            CommandType::AgentLogs => out.push_str("[AgentLogsResult]\n"),
        }

        match &r.outcome {
            self::syntax::CommandOutcome::Success { summary } => {
                out.push_str(summary);
                out.push('\n');
            }
            self::syntax::CommandOutcome::Failure { error } => {
                out.push_str(&format!("error: {}\n", error));
            }
        }

        if let Some(follower) = results.get(r.block_index.wrapping_add(1)) {
            if follower.block_index != 0 {
                out.push_str("---\n");
            }
        }
    }

    out.push_str("【|SYSTEM|】");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::syntax::{CommandOutcome, NCommand};
    use tracing::debug;

    fn assert_shell_result_has(results: &[CommandResult], expected_contains: &str) {
        for r in results {
            match &r.outcome {
                CommandOutcome::Success { summary } => {
                    if summary.contains(expected_contains) {
                        return;
                    }
                }
                _ => {}
            }
        }
        panic!(
            "No Shell result containing '{}' in {:?}",
            expected_contains, results
        );
    }

    #[tokio::test]
    async fn test_full_pipeline_single_shell_chunked() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token("《[Shell");
        watcher.feed_token("]|Ncoder|》\n");
        watcher.feed_token("【command】echo hello-world【command】\n");
        watcher.feed_token("【is_async】false【is_async】\n");
        watcher.feed_token("《[End]|Shell|》");

        let (remaining, _warnings) = watcher.finalize();
        assert!(remaining.is_empty(), "all commands should be extracted mid-stream");

        let results = watcher.drain_results().await;
        assert!(!results.is_empty(), "expected results but got empty");
        assert_shell_result_has(&results, "hello-world");
    }

    #[tokio::test]
    async fn test_full_pipeline_single_chunk() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token(
            "《[Shell]|Ncoder|》\n\
             【command】echo test-single【command】\n\
             【is_async】false【is_async】\n\
             《[End]|Shell|》",
        );

        let (remaining, _warnings) = watcher.finalize();
        assert!(remaining.is_empty());

        let results = watcher.drain_results().await;
        assert!(!results.is_empty());
        assert_shell_result_has(&results, "test-single");
    }

    #[tokio::test]
    async fn test_full_pipeline_two_shell_commands() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token(
            "《[Shell]|Ncoder|》\n\
             【command】echo first【command】\n\
             《[End]|Shell|》\n\
             《[Shell]|Ncoder|》\n\
             【command】echo second【command】\n\
             《[End]|Shell|》",
        );

        let (remaining, _warnings) = watcher.finalize();
        assert!(remaining.is_empty());

        let results = watcher.drain_results().await;
        assert!(results.len() >= 2, "expected at least 2 results, got {}", results.len());
        assert_shell_result_has(&results, "first");
        assert_shell_result_has(&results, "second");
    }

    #[tokio::test]
    async fn test_full_pipeline_no_end_marker() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token(
            "《[Shell]|Ncoder|》\n\
             【command】echo no-end【command】",
        );

        // No __END__ marker — should still be parsed
        // After feeding, the body extends to end of buffer, parse_key_values should work
        let (remaining, _warnings) = watcher.finalize();
        // The command may have been extracted mid-stream OR left for finalize
        let mut all_cmds = remaining;
        {let (mut extra, _) = watcher.finalize(); all_cmds.append(&mut extra);};

        if !all_cmds.is_empty() {
            let results = execute_commands(all_cmds).await;
            assert_shell_result_has(&results, "no-end");
        } else {
            let results = watcher.drain_results().await;
            assert_shell_result_has(&results, "no-end");
        }
    }

    #[tokio::test]
    async fn test_full_pipeline_shell_separator() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token(
            "《[Shell]|Ncoder|》\n\
             【command】echo block-a【command】\n\
             ---\n\
             【command】echo block-b【command】\n\
             《[End]|Shell|》",
        );

        let (remaining, _warnings) = watcher.finalize();
        assert!(remaining.is_empty());

        let results = watcher.drain_results().await;
        assert!(results.len() >= 2, "expected at least 2 blocks, got {}", results.len());
        assert_shell_result_has(&results, "block-a");
        assert_shell_result_has(&results, "block-b");
    }

    #[tokio::test]
    async fn test_full_pipeline_multiple_types() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token(
            "《[Shell]|Ncoder|》\n\
             【command】echo shell-cmd【command】\n\
             《[End]|Shell|》\n\
             《[Shell]|Ncoder|》\n\
             【command】echo shell-cmd2【command】\n\
             《[End]|Shell|》",
        );

        let (remaining, _warnings) = watcher.finalize();
        assert!(remaining.is_empty());

        let results = watcher.drain_results().await;
        assert!(results.len() >= 2, "expected at least 2 shell results, got {}", results.len());
    }

    #[test]
    fn test_format_empty_results() {
        let result = format_command_results(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_shell_success() {
        let results = vec![CommandResult {
            command_type: CommandType::Shell,
            block_index: 0,
            outcome: CommandOutcome::Success {
                summary: "status: OK\nexit_code: 0\nstdout:\nhello\nstderr:\n".into(),
            },
        }];

        let formatted = format_command_results(&results);
        assert!(formatted.starts_with("【|SYSTEM|】"));
        assert!(formatted.contains("[ShellResult]"));
        assert!(formatted.contains("hello"));
        assert!(formatted.ends_with("【|SYSTEM|】"));
    }

    #[test]
    fn test_format_shell_failure() {
        let results = vec![CommandResult {
            command_type: CommandType::Shell,
            block_index: 0,
            outcome: CommandOutcome::Failure {
                error: "command timed out".into(),
            },
        }];

        let formatted = format_command_results(&results);
        assert!(formatted.contains("error: command timed out"));
    }

    #[tokio::test]
    async fn test_real_world_model_output_shell() {
        let mut watcher = CommandWatcher::new();

        let text = "《[Shell]|Ncoder|》
【command】ls -a【command】
【is_async】false【is_async】
《[End]|Shell|》";

        watcher.feed_token(text);
        let drain = watcher.drain_results().await;
        let (remaining, _warnings) = watcher.finalize();

        assert!(!drain.is_empty() || !remaining.is_empty(),
            "no commands extracted from real-world text");
    }

    #[tokio::test]
    async fn test_real_world_model_output_files_operator() {
        let mut watcher = CommandWatcher::new();

        let text = "Shell 似乎有些问题，我用 FilesOperator 来读取文件：\n\n《[FilesOperator]|Ncoder|》\n【mode】read【mode】\n【path】./flake.nix【path】\n《[End]|FilesOperator|》";
        watcher.feed_token(text);

        let (remaining, _warnings) = watcher.finalize();
        if !remaining.is_empty() {
            let results = execute_commands(remaining).await;
            let _ = format_command_results(&results);
        }

        let results = watcher.drain_results().await;
        assert!(!results.is_empty(),
            "expected FilesOperator results, got empty");
    }

    #[tokio::test]
    async fn test_real_world_interleaved_reasoning_and_commands() {
        let mut watcher = CommandWatcher::new();

        watcher.feed_token("用户想查看目录结构。使用 ls -la 查看。\n\n");
        watcher.feed_token("《[Shell]|Ncoder|》\n");
        watcher.feed_token("【command】ls -a【command】\n");
        watcher.feed_token("《[End]|Shell|》\n");
        watcher.feed_token("现在让我查看文件内容。\n");
        watcher.feed_token("《[Shell]|Ncoder|》\n");
        watcher.feed_token("【command】cat Cargo.toml【command】\n");
        watcher.feed_token("《[End]|Shell|》");

        let (remaining, _warnings) = watcher.finalize();
        if !remaining.is_empty() {
            let results = execute_commands(remaining).await;
            let _ = format_command_results(&results);
        }

        let results = watcher.drain_results().await;
        assert!(results.len() >= 1,
            "expected at least 1 result from interleaved text, got {}", results.len());
    }

    #[tokio::test]
    async fn test_full_pipeline_verify_result_format() {
        let mut watcher = CommandWatcher::new();
        watcher.feed_token("《[Shell]|Ncoder|》\n【command】echo verify-pipeline【command】\n《[End]|Shell|》");

        let (remaining, _warnings) = watcher.finalize();
        let results = if !remaining.is_empty() {
            execute_commands(remaining).await
        } else {
            watcher.drain_results().await
        };

        assert!(!results.is_empty());

        let injection = format_command_results(&results);
        assert!(injection.contains("【|SYSTEM|】"));
        assert!(injection.contains("[ShellResult]"));
        assert!(injection.contains("verify-pipeline"));
    }
}
