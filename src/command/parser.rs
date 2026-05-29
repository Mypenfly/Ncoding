#![allow(dead_code, unused_imports)]

use regex::Regex;

use super::syntax::{
    FileMode, FileOpBlock, ShellBlock, SkillsBlock, SkillsMode,
    SubAgentBlock, ToolCallBlock, NCommand,
};

pub struct CommandParser {
    cmd_start_re: Regex,
    key_value_re: Regex,
}

impl CommandParser {
    pub fn new() -> Self {
        Self {
            cmd_start_re: Regex::new(r"<<<\[([^\]]+)\]>>>").unwrap(),
            key_value_re: Regex::new(r"「([^」]+)」:「「([\s\S]*?)」」").unwrap(),
        }
    }

    pub fn try_parse(&mut self, buffer: &str) -> Option<Vec<NCommand>> {
        self.extract_commands(buffer)
    }

    pub fn normalize_name(&self, name: &str) -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_uppercase()
    }

    pub fn extract_commands(&mut self, buffer: &str) -> Option<Vec<NCommand>> {
        let mut commands = Vec::new();
        let mut start = 0;

        while let Some(m) = self.cmd_start_re.find_at(buffer, start) {
            let name_raw = m.as_str()[4..m.as_str().len() - 3].trim();
            if name_raw == "__END__" {
                start = m.end();
                continue;
            }
            let name = self.normalize_name(name_raw);

            let body_start = m.end();
            let body_end = self.find_block_end(buffer, body_start);

            let body = &buffer[body_start..body_end];
            let kvs = self.parse_key_values(body);

            match name.as_str() {
                "SHELL" => {
                    let blocks: Vec<ShellBlock> = kvs
                        .into_iter()
                        .map(|m| ShellBlock {
                            command: m.get("command").cloned().unwrap_or_default(),
                            is_async: m.get("is_async").map(|v| v == "true").unwrap_or(false),
                        })
                        .collect();
                    if !blocks.is_empty() {
                        commands.push(NCommand::Shell { blocks });
                    }
                }
                "FILESOPERATOR" => {
                    let blocks: Vec<FileOpBlock> = kvs
                        .into_iter()
                        .map(|m| {
                            let mode = match m.get("mode").map(|v| v.as_str()) {
                                Some("read") => FileMode::Read,
                                Some("edit") => FileMode::Edit,
                                _ => FileMode::Write,
                            };
                            FileOpBlock {
                                mode,
                                path: std::path::PathBuf::from(
                                    m.get("path").cloned().unwrap_or_default(),
                                ),
                                content: m.get("content").cloned(),
                                old_str: m.get("old_str").cloned(),
                                new_str: m.get("new_str").cloned(),
                                offset: m.get("offset").and_then(|v| v.parse().ok()),
                                limit: m.get("limit").and_then(|v| v.parse().ok()),
                            }
                        })
                        .collect();
                    if !blocks.is_empty() {
                        commands.push(NCommand::FilesOperator { blocks });
                    }
                }
                "TOOLCALL" => {
                    let blocks: Vec<ToolCallBlock> = kvs
                        .into_iter()
                        .map(|m| {
                            let tool_name = m.get("tool_name").cloned().unwrap_or_default();
                            let args: std::collections::HashMap<String, String> = m
                                .into_iter()
                                .filter(|(k, _)| k != "tool_name")
                                .collect();
                            ToolCallBlock { tool_name, args }
                        })
                        .collect();
                    if !blocks.is_empty() {
                        commands.push(NCommand::ToolCall { blocks });
                    }
                }
                "SUBAGENTTASK" | "SUB_AGENT_TASK" => {
                    let blocks: Vec<SubAgentBlock> = kvs
                        .into_iter()
                        .map(|m| SubAgentBlock {
                            prompt: m.get("prompt").cloned().unwrap_or_default(),
                        })
                        .collect();
                    if !blocks.is_empty() {
                        commands.push(NCommand::SubAgentTask { blocks });
                    }
                }
                "AGENTSKILLS" => {
                    let blocks: Vec<SkillsBlock> = kvs
                        .into_iter()
                        .map(|m| {
                            let mode = match m.get("mode").map(|v| v.as_str()) {
                                Some("load") => SkillsMode::Load,
                                _ => SkillsMode::List,
                            };
                            SkillsBlock {
                                mode,
                                skill_name: m.get("skill_name").cloned(),
                            }
                        })
                        .collect();
                    if !blocks.is_empty() {
                        commands.push(NCommand::AgentSkills { blocks });
                    }
                }
                _ => {}
            }

            start = body_end;
        }

        if commands.is_empty() {
            None
        } else {
            Some(commands)
        }
    }

    fn find_block_end(&self, buffer: &str, from: usize) -> usize {
        let end_re = self.cmd_start_re.find_at(buffer, from);
        match end_re {
            Some(m) => m.start(),
            None => buffer.len(),
        }
    }

    pub fn parse_key_values(&self, body: &str) -> Vec<std::collections::HashMap<String, String>> {
        let mut results = Vec::new();
        let sections: Vec<&str> = body.split("---").collect();

        for section in sections {
            let mut map = std::collections::HashMap::new();
            for caps in self.key_value_re.captures_iter(section) {
                let key = caps[1].trim().to_string();
                let value = caps[2].to_string();
                map.insert(key, value);
            }
            if !map.is_empty() {
                results.push(map);
            }
        }

        results
    }
}

impl Default for CommandParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_name_standard() {
        let parser = CommandParser::new();
        assert_eq!(parser.normalize_name("Shell"), "SHELL");
        assert_eq!(parser.normalize_name("FilesOperator"), "FILESOPERATOR");
    }

    #[test]
    fn test_normalize_name_with_special_chars() {
        let parser = CommandParser::new();
        assert_eq!(parser.normalize_name("Example_Command"), "EXAMPLECOMMAND");
        assert_eq!(parser.normalize_name("example-command"), "EXAMPLECOMMAND");
        assert_eq!(parser.normalize_name("exampleCommand"), "EXAMPLECOMMAND");
        assert_eq!(parser.normalize_name("SUB_AGENT_TASK"), "SUBAGENTTASK");
    }

    #[test]
    fn test_normalize_name_end_marker() {
        let parser = CommandParser::new();
        let raw = "__END__";
        let normalized = parser.normalize_name(raw);
        // normalize_name strips non-alphanumeric; __END__ detection
        // is done on the raw name before normalization in try_parse
        assert_eq!(normalized, "END");
    }

    #[test]
    fn test_try_parse_single_shell_command() {
        let mut parser = CommandParser::new();
        let input = "<<<[Shell]>>>
「command」:「「cargo test」」
「is_async」:「「false」」
<<<[__END__]>>>";
        let result = parser.try_parse(input);
        assert!(result.is_some());
        let commands = result.unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].command, "cargo test");
            }
            _ => panic!("expected Shell command"),
        }
    }

    #[test]
    fn test_try_parse_ignores_end_marker() {
        let mut parser = CommandParser::new();
        let input = "<<<[__END__]>>>";
        let result = parser.try_parse(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_try_parse_multiple_commands() {
        let mut parser = CommandParser::new();
        let input = "<<<[Shell]>>>
「command」:「「ls」」
<<<[__END__]>>>
<<<[Shell]>>>
「command」:「「pwd」」
<<<[__END__]>>>";
        let result = parser.try_parse(input);
        assert!(result.is_some());
        let commands = result.unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn test_parse_key_values_shell() {
        let parser = CommandParser::new();
        let body = "「command」:「「cargo test -- --nocapture」」
「is_async」:「「false」」";
        let maps = parser.parse_key_values(body);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].get("command").unwrap(), "cargo test -- --nocapture");
        assert_eq!(maps[0].get("is_async").unwrap(), "false");
    }

    #[test]
    fn test_parse_key_values_with_separator() {
        let parser = CommandParser::new();
        let body = "「command」:「「cargo test」」
「is_async」:「「false」」
---
「command」:「「cargo build」」
「is_async」:「「true」」";
        let maps = parser.parse_key_values(body);
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[0].get("command").unwrap(), "cargo test");
        assert_eq!(maps[1].get("command").unwrap(), "cargo build");
        assert_eq!(maps[1].get("is_async").unwrap(), "true");
    }

    #[test]
    fn test_parse_key_values_multiline_value() {
        let parser = CommandParser::new();
        let body = "「content」:「「pub fn hello() {
    println!(\"hello\");
}」」";
        let maps = parser.parse_key_values(body);
        assert_eq!(maps.len(), 1);
        assert!(maps[0].get("content").unwrap().contains("pub fn hello"));
    }

    #[test]
    fn test_try_parse_empty_input() {
        let mut parser = CommandParser::new();
        let result = parser.try_parse("no command here");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_key_values_empty_input() {
        let parser = CommandParser::new();
        let maps = parser.parse_key_values("no key values here");
        assert!(maps.is_empty());
    }

    #[test]
    fn test_extract_shell_blocks_single() {
        let mut parser = CommandParser::new();
        let input = "<<<[Shell]>>>
「command」:「「cargo test -- --nocapture」」
「is_async」:「「false」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].command, "cargo test -- --nocapture");
                assert!(!blocks[0].is_async);
            }
            _ => panic!("expected Shell command"),
        }
    }

    #[test]
    fn test_extract_shell_blocks_with_separator() {
        let mut parser = CommandParser::new();
        let input = "<<<[Shell]>>>
「command」:「「cargo test」」
「is_async」:「「false」」
---
「command」:「「cargo build」」
「is_async」:「「true」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks.len(), 2);
                assert_eq!(blocks[0].command, "cargo test");
                assert!(!blocks[0].is_async);
                assert_eq!(blocks[1].command, "cargo build");
                assert!(blocks[1].is_async);
            }
            _ => panic!("expected Shell command"),
        }
    }

    #[test]
    fn test_extract_files_operator_blocks() {
        let mut parser = CommandParser::new();
        let input = "<<<[FilesOperator]>>>
「mode」:「「read」」
「path」:「「src/main.rs」」
「offset」:「「30」」
「limit」:「「80」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::FilesOperator { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].mode, FileMode::Read);
                assert_eq!(blocks[0].path, std::path::PathBuf::from("src/main.rs"));
                assert_eq!(blocks[0].offset, Some(30));
                assert_eq!(blocks[0].limit, Some(80));
            }
            _ => panic!("expected FilesOperator command"),
        }
    }

    #[test]
    fn test_extract_tool_call_blocks() {
        let mut parser = CommandParser::new();
        let input = "<<<[ToolCall]>>>
「tool_name」:「「web_search」」
「query」:「「rust testing」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::ToolCall { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].tool_name, "web_search");
                assert_eq!(blocks[0].args.get("query").unwrap(), "rust testing");
            }
            _ => panic!("expected ToolCall command"),
        }
    }

    #[test]
    fn test_extract_sub_agent_blocks() {
        let mut parser = CommandParser::new();
        let input = "<<<[SubAgentTask]>>>
「prompt」:「「review error handling」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::SubAgentTask { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].prompt, "review error handling");
            }
            _ => panic!("expected SubAgentTask command"),
        }
    }

    #[test]
    fn test_extract_agent_skills_blocks() {
        let mut parser = CommandParser::new();
        let input = "<<<[AgentSkills]>>>
「mode」:「「load」」
「skill_name」:「「test-driven-development」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NCommand::AgentSkills { blocks } => {
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0].mode, SkillsMode::Load);
                assert_eq!(blocks[0].skill_name, Some("test-driven-development".into()));
            }
            _ => panic!("expected AgentSkills command"),
        }
    }

    #[test]
    fn test_extract_multiple_command_types() {
        let mut parser = CommandParser::new();
        let input = "<<<[Shell]>>>
「command」:「「ls -la」」
<<<[__END__]>>>
<<<[FilesOperator]>>>
「mode」:「「read」」
「path」:「「Cargo.toml」」
<<<[__END__]>>>";
        let commands = parser.extract_commands(input).unwrap();
        assert_eq!(commands.len(), 2);
    }
}
