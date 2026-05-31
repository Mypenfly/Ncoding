pub mod agent_logs;
pub mod agent_skills;
pub mod checklist;
pub mod files_operator;
pub mod parser;
pub mod shell;
pub mod sub_agent_task;
pub mod syntax;
pub mod tool_call;

use self::syntax::{CommandResult, CommandType, NCommand};
use tracing::info;

use std::path::PathBuf;

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
    execute_commands_with_backup(commands, None).await
}

pub async fn execute_commands_with_backup(commands: Vec<NCommand>, backup_dir: Option<PathBuf>) -> Vec<CommandResult> {
    let mut tasks = Vec::new();
    for cmd in commands {
        info!("Executing command: {}", cmd_label(&cmd));
        let bd = backup_dir.clone();
        let handle: tokio::task::JoinHandle<Result<Vec<CommandResult>, anyhow::Error>> =
            tokio::spawn(async move {
                match cmd {
                    NCommand::Shell { blocks } => shell::execute(blocks).await,
                    NCommand::FilesOperator { blocks } => files_operator::execute_with_backup(blocks, bd.as_deref()).await,
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

    let mut out = String::from(
        "(你调用的命令执行结果如下)\n【|Command/Tool|】\n",
    );

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

    out.push_str("【|Command/Tool|】");
    out
}

