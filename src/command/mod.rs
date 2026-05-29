#![allow(dead_code, unused_imports)]

pub mod agent_skills;
pub mod files_operator;
pub mod parser;
pub mod shell;
pub mod sub_agent_task;
pub mod syntax;
pub mod tool_call;

use self::syntax::{CommandResult, CommandType, NCommand};

pub struct CommandWatcher {
    buffer: String,
    parser: parser::CommandParser,
}

impl CommandWatcher {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            parser: parser::CommandParser::new(),
        }
    }

    pub fn feed_token(&mut self, token: &str) {
        self.buffer.push_str(token);
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn finalize(&mut self) -> Vec<NCommand> {
        let result = self.parser.extract_commands(&self.buffer);
        self.buffer.clear();
        result.unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

pub async fn execute_commands(commands: Vec<NCommand>) -> Vec<CommandResult> {
    let mut tasks = Vec::new();

    for cmd in commands {
        let handle: tokio::task::JoinHandle<Result<Vec<CommandResult>, anyhow::Error>> =
            tokio::spawn(async move {
                match cmd {
                    NCommand::Shell { blocks } => shell::execute(blocks).await,
                    NCommand::FilesOperator { blocks } => files_operator::execute(blocks).await,
                    NCommand::ToolCall { blocks } => tool_call::execute(blocks).await,
                    NCommand::SubAgentTask { blocks } => sub_agent_task::execute(blocks).await,
                    NCommand::AgentSkills { blocks } => agent_skills::execute(blocks).await,
                }
            });
        tasks.push(handle);
    }

    let mut results = Vec::new();
    for task in tasks {
        match task.await {
            Ok(Ok(mut r)) => results.append(&mut r),
            Ok(Err(e)) => eprintln!("Command execution error: {}", e),
            Err(e) => eprintln!("Task join error: {}", e),
        }
    }

    results
}

pub fn format_command_results(results: &[CommandResult]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut out = String::from("<(<(SYSTEM\n");

    for r in results {
        match r.command_type {
            CommandType::Shell => out.push_str("[ShellResult]\n"),
            CommandType::FilesOperator => out.push_str("[FileResult]\n"),
            CommandType::ToolCall => out.push_str("[ToolCallResult]\n"),
            CommandType::SubAgentTask => out.push_str("[SubAgentResult]\n"),
            CommandType::AgentSkills => out.push_str("[SkillsResult]\n"),
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

    out.push_str(")>)>");
    out
}
