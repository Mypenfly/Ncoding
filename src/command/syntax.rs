use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone)]
pub enum NCommand {
    Shell { blocks: Vec<ShellBlock> },
    FilesOperator { blocks: Vec<FileOpBlock> },
    ToolCall { blocks: Vec<ToolCallBlock> },
    SubAgentTask { blocks: Vec<SubAgentBlock> },
    AgentSkills { blocks: Vec<SkillsBlock> },
    CheckList { blocks: Vec<CheckListBlock> },
    AgentLogs { blocks: Vec<AgentLogsBlock> },
}

#[derive(Debug, Clone)]
pub struct ShellBlock {
    pub command: String,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct FileOpBlock {
    pub mode: FileMode,
    pub path: PathBuf,
    pub content: Option<String>,
    pub old_str: Option<String>,
    pub new_str: Option<String>,
    pub old_lines: Option<String>,
    pub new_lines: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ToolCallBlock {
    pub tool_name: String,
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SubAgentBlock {
    pub prompt: String,
}

#[derive(Debug, Clone)]
pub struct SkillsBlock {
    pub mode: SkillsMode,
    pub skill_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckListBlock {
    pub mode: CheckListMode,
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckListMode {
    Create,
    Update,
    List,
}

#[derive(Debug, Clone)]
pub struct AgentLogsBlock {
    pub mode: AgentLogsMode,
    pub filename: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLogsMode {
    Write,
    Read,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Read,
    Write,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillsMode {
    List,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    Shell,
    FilesOperator,
    ToolCall,
    SubAgentTask,
    AgentSkills,
    CheckList,
    AgentLogs,
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub command_type: CommandType,
    pub block_index: usize,
    pub outcome: CommandOutcome,
}

#[derive(Debug, Clone)]
pub enum CommandOutcome {
    Success { summary: String },
    Failure { error: String },
}
