use super::syntax::{
    FileMode, FileOpBlock, ShellBlock, SkillsBlock, SkillsMode,
    SubAgentBlock, ToolCallBlock,
    CheckListBlock, CheckListMode,
    AgentLogsBlock, AgentLogsMode,
    NCommand,
};

pub struct CommandParser {
    start_re: regex::Regex,
    key_re: regex::Regex,
    comment_re: regex::Regex,
}

#[derive(Debug)]
pub struct ParseWarning {
    #[allow(dead_code)]
    pub message: String,
}

impl Default for CommandParser { fn default() -> Self { Self::new() } }

impl CommandParser {
    pub fn new() -> Self { Self {
        start_re: regex::Regex::new(r"《\[([^\]]+)\]\|([^|]*)\|》").unwrap(),
        key_re: regex::Regex::new(r"【([^】]+)】").unwrap(),
        comment_re: regex::Regex::new(r"/-(.*?)-/").unwrap(),
    }}

    pub fn normalize_name(&self, name: &str) -> String {
        name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_uppercase()
    }

    #[allow(dead_code)]
    pub fn try_parse(&mut self, buffer: &str) -> Option<Vec<NCommand>> { self.extract_commands(buffer) }

    #[allow(dead_code)]
    pub fn extract_commands(&mut self, buffer: &str) -> Option<Vec<NCommand>> {
        let (cmds, _, _) = self.scan(buffer, 0, true);
        if cmds.is_empty() { None } else { Some(cmds) }
    }

    pub fn extract_commands_from(&mut self, b: &str, f: usize) -> (Vec<NCommand>, usize, Vec<ParseWarning>) { self.scan(b, f, false) }
    pub fn extract_commands_from_final(&mut self, b: &str, f: usize) -> (Vec<NCommand>, usize, Vec<ParseWarning>) { self.scan(b, f, true) }

    fn scan(&self, buffer: &str, from: usize, is_final: bool) -> (Vec<NCommand>, usize, Vec<ParseWarning>) {
        let mut cmds = Vec::new();
        let mut w = Vec::new();
        let mut pos = from;
        while let Some(m) = self.start_re.find_at(buffer, pos) {
            let caps = self.start_re.captures(m.as_str()).unwrap();
            let cmd_name = caps.get(1).unwrap().as_str().trim().to_string();
            let _ch = caps.get(2).unwrap().as_str().trim().to_string();
            let body_start = m.end();
            let end_pat = format!("《[End]|{}|》", cmd_name);
            let body_end = if let Some(end) = self.find_end_with_depth(buffer, body_start, &cmd_name) {
                end
            } else if is_final {
                let mismatched = self.find_any_end_marker(buffer, body_start);
                if let Some((mismatch_pos, mismatch_name)) = mismatched {
                    w.push(ParseWarning {
                        message: format!(
                            "Command '{}' end marker mismatch: found 《[End]|{}|》but expected 《[End]|{}|》",
                            cmd_name, mismatch_name, cmd_name
                        ),
                    });
                    mismatch_pos
                } else {
                    w.push(ParseWarning {
                        message: format!(
                            "Command '{}' is missing a matching 《[End]|{}|》close marker",
                            cmd_name, cmd_name
                        ),
                    });
                    buffer.len()
                }
            } else {
                break;
            };
            let body = &buffer[body_start..body_end];
            let clean = self.comment_re.replace_all(body, "").to_string();
            let (kvs, kw) = self.parse_body(&clean);
            for kwm in kw { w.push(ParseWarning{message:kwm}); }
            let name = self.normalize_name(&cmd_name);
            if let Some(cmd) = self.build_command(&name, kvs) { cmds.push(cmd); }
            pos = (body_end + end_pat.len()).min(buffer.len());
        }
        (cmds, pos, w)
    }

    fn find_end_with_depth(&self, buffer: &str, from: usize, cmd_name: &str) -> Option<usize> {
        let mut depth: i32 = 0;
        let remaining = &buffer[from..];
        let mut search_from = 0usize;
        loop {
            let end = remaining[search_from..].find(&format!("《[End]|{}|》", cmd_name));
            match end {
                Some(rel) => {
                    let abs = from + search_from + rel;
                    let open = remaining[search_from..search_from+rel].matches(&format!("《[{}]|", cmd_name)).count();
                    depth += open as i32;
                    if depth == 0 { return Some(abs); }
                    depth -= 1;
                    search_from += rel + format!("《[End]|{}|》", cmd_name).len();
                }
                None => return None,
            }
        }
    }

    fn find_any_end_marker(&self, buffer: &str, from: usize) -> Option<(usize, String)> {
        let end_pat = regex::Regex::new(r"《\[End\]\|([^\]]+)\|》").unwrap();
        let remaining = &buffer[from..];
        if let Some(m) = end_pat.find(remaining) {
            let caps = end_pat.captures(m.as_str()).unwrap();
            let name = caps.get(1).unwrap().as_str().trim().to_string();
            Some((from + m.start(), name))
        } else {
            None
        }
    }

    fn parse_body(&self, body: &str) -> (Vec<std::collections::HashMap<String, String>>, Vec<String>) {
        let mut blocks = Vec::new();
        let mut w = Vec::new();

        let kv_ranges = self.compute_kv_ranges(body);
        let sections = self.split_body_by_separators(body, &kv_ranges);

        for sec in &sections {
            if sec.trim().is_empty() { continue; }
            let (m, sw) = self.parse_section(sec);
            w.extend(sw);
            if !m.is_empty() { blocks.push(m); }
        }
        (blocks, w)
    }

    fn compute_kv_ranges(&self, body: &str) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let markers: Vec<_> = self.key_re.find_iter(body).map(|m| {
            let key = &body[m.start()+3..m.end()-3];
            (m.start(), m.end(), key.to_string())
        }).collect();
        if markers.is_empty() { return ranges; }

        let mut stack: Vec<(String, usize)> = Vec::new();
        for (ms, me, key) in markers {
            if let Some(pos) = stack.iter().rposition(|(k, _)| k == &key) {
                let closers: Vec<_> = stack.drain(pos+1..).collect();
                let start = stack.pop().unwrap().1;
                for (_ck, cvs) in closers {
                    ranges.push((cvs, ms));
                }
                ranges.push((start, ms));
            } else {
                stack.push((key, me));
            }
        }
        let body_end = body.len();
        for (_pk, vs) in stack {
            ranges.push((vs, body_end));
        }
        ranges
    }

    fn split_body_by_separators(&self, body: &str, exclude_ranges: &[(usize, usize)]) -> Vec<String> {
        let mut sections = Vec::new();
        let mut start: usize = 0;
        let sep = "---";
        let sep_len = sep.len();
        let mut pos: usize = 0;
        while let Some(idx) = body[pos..].find(sep) {
            let abs = pos + idx;
            let in_excluded = exclude_ranges.iter().any(|(s, e)| abs > *s && abs < *e);
            if !in_excluded {
                sections.push(body[start..abs].to_string());
                start = abs + sep_len;
            }
            pos = abs + sep_len;
        }
        sections.push(body[start..].to_string());
        sections
    }

    fn parse_section(&self, section: &str) -> (std::collections::HashMap<String, String>, Vec<String>) {
        let mut map = std::collections::HashMap::new();
        let mut w = Vec::new();
        let markers: Vec<_> = self.key_re.find_iter(section).map(|m| {
            let key = &section[m.start()+3..m.end()-3];
            (m.start(), m.end(), key.to_string())
        }).collect();
        if markers.is_empty() { return (map, w); }

        let mut stack: Vec<(String, usize, usize)> = Vec::new();
        for (ms, me, key) in markers {
            if let Some(pos) = stack.iter().rposition(|(k, _, _)| k == &key) {
                let closers: Vec<_> = stack.drain(pos+1..).collect();
                for (ck, cvs, _) in &closers {
                    let val = section[*cvs..ms].to_string();
                    map.insert(ck.clone(), val);
                }
                let (_, vs, _) = stack.pop().unwrap();
                let first_cs = closers.first().map(|(_, _, s)| *s);
                let prefix = section[vs..first_cs.unwrap_or(ms)].to_string();
                map.insert(key.clone(), prefix);
                for (ck, _, _) in closers {
                    w.push(format!("key '{}' not closed by matching marker, auto-closed at key '{}' close", ck, key));
                }
            } else {
                if map.contains_key(&key) { map.remove(&key); }
                stack.push((key, me, ms));
            }
        }
        for (pk, vs, _) in stack {
            let val = section[vs..].to_string();
            w.push(format!("key '{}' not closed. Usage: 【{}】value【{}】", pk, pk, pk));
            map.insert(pk, val);
        }
        (map, w)
    }

    fn build_command(&self, name: &str, kvs: Vec<std::collections::HashMap<String, String>>) -> Option<NCommand> {
        match name {
            "SHELL" => {
                let blocks: Vec<ShellBlock> = kvs.into_iter().map(|m| ShellBlock {
                    command: m.get("command").cloned().unwrap_or_default(),
                    is_async: m.get("is_async").map(|v| v == "true").unwrap_or(false),
                }).collect();
                if !blocks.is_empty() { Some(NCommand::Shell { blocks }) } else { None }
            }
            "FILESOPERATOR" => {
                let blocks: Vec<FileOpBlock> = kvs.into_iter().map(|m| {
                    let mode = match m.get("mode").map(|v| v.as_str()) { Some("read") => FileMode::Read, Some("edit") => FileMode::Edit, _ => FileMode::Write };
                    FileOpBlock { mode, path: std::path::PathBuf::from(m.get("path").cloned().unwrap_or_default()), content: m.get("content").cloned(), old_str: m.get("old_str").cloned(), new_str: m.get("new_str").cloned(), old_lines: m.get("old_lines").cloned(), new_lines: m.get("new_lines").cloned(), offset: m.get("offset").and_then(|v| v.parse().ok()), limit: m.get("limit").and_then(|v| v.parse().ok()) }
                }).collect();
                if !blocks.is_empty() { Some(NCommand::FilesOperator { blocks }) } else { None }
            }
            "TOOLCALL" => {
                let blocks: Vec<ToolCallBlock> = kvs.into_iter().map(|m| {
                    let tool_name = m.get("tool_name").cloned().unwrap_or_default();
                    let args = m.into_iter().filter(|(k,_)| k != "tool_name").collect();
                    ToolCallBlock { tool_name, args }
                }).collect();
                if !blocks.is_empty() { Some(NCommand::ToolCall { blocks }) } else { None }
            }
            "SUBAGENTTASK" | "SUB_AGENT_TASK" => {
                let blocks: Vec<SubAgentBlock> = kvs.into_iter().map(|m| SubAgentBlock { prompt: m.get("prompt").cloned().unwrap_or_default() }).collect();
                if !blocks.is_empty() { Some(NCommand::SubAgentTask { blocks }) } else { None }
            }
            "AGENTSKILLS" => {
                let blocks: Vec<SkillsBlock> = kvs.into_iter().map(|m| {
                    let mode = match m.get("mode").map(|v| v.as_str()) { Some("load") => SkillsMode::Load, _ => SkillsMode::List };
                    SkillsBlock { mode, skill_name: m.get("skill_name").cloned() }
                }).collect();
                if !blocks.is_empty() { Some(NCommand::AgentSkills { blocks }) } else { None }
            }
            "CHECKLIST" => {
                let blocks: Vec<CheckListBlock> = kvs.into_iter().map(|m| {
                    let mode = match m.get("mode").map(|v| v.as_str()) { Some("create") => CheckListMode::Create, Some("update") => CheckListMode::Update, _ => CheckListMode::List };
                    CheckListBlock { mode, id: m.get("id").cloned(), title: m.get("title").cloned(), status: m.get("status").cloned(), content: m.get("content").cloned() }
                }).collect();
                if !blocks.is_empty() { Some(NCommand::CheckList { blocks }) } else { None }
            }
            "AGENTLOGS" => {
                let blocks: Vec<AgentLogsBlock> = kvs.into_iter().map(|m| {
                    let mode = match m.get("mode").map(|v| v.as_str()) { Some("read") => AgentLogsMode::Read, Some("list") => AgentLogsMode::List, _ => AgentLogsMode::Write };
                    AgentLogsBlock { mode, filename: m.get("filename").cloned(), content: m.get("content").cloned() }
                }).collect();
                if !blocks.is_empty() { Some(NCommand::AgentLogs { blocks }) } else { None }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn test_norm() { let p = CommandParser::new(); assert_eq!(p.normalize_name("Shell"), "SHELL"); }

    #[test]
    fn test_shell() {
        let mut p = CommandParser::new();
        let (c, _, w) = p.scan("《[Shell]|Ncoder|》\n【command】ls -la【command】\n《[End]|Shell|》", 0, true);
        assert_eq!(c.len(), 1); assert!(w.is_empty());
        match &c[0] { NCommand::Shell { blocks } => assert_eq!(blocks[0].command, "ls -la"), _ => panic!() }
    }

    #[test]
    fn test_sep() {
        let mut p = CommandParser::new();
        let (c, _, _) = p.scan("《[Shell]|Ncoder|》\n【command】a【command】\n---\n【command】b【command】\n《[End]|Shell|》", 0, true);
        match &c[0] { NCommand::Shell { blocks } => assert_eq!(blocks.len(), 2), _ => panic!() }
    }

    #[test]
    fn test_nested() {
        let mut p = CommandParser::new();
        let (c, _, _) = p.scan("《[FilesOperator]|Ncoder|》\n【mode】write【mode】\n【content】text with 《[Shell]|x|》【cmd】ls【cmd】《[End]|Shell|》 inside【content】\n《[End]|FilesOperator|》", 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] { NCommand::FilesOperator { blocks } => assert!(blocks[0].content.as_deref().unwrap().contains("《[Shell]")), _ => panic!() }
    }

    #[test]
    fn test_no_end() {
        let mut p = CommandParser::new();
        let (c, _, w) = p.scan("《[Shell]|Ncoder|》\n【command】ls【command】\n", 0, true);
        assert_eq!(c.len(), 1); assert!(w.iter().any(|x| x.message.contains("missing a matching 《[End]")));
    }

    #[test]
    fn test_malformed() {
        let mut p = CommandParser::new();
        let (c, _, w) = p.scan("《[Shell]|Ncoder|》\n【command】ls【is_async】true【command】\n《[End]|Shell|》", 0, true);
        assert!(!w.is_empty());
        match &c[0] { NCommand::Shell { blocks } => assert!(!blocks[0].command.contains("is_async")), _ => panic!() }
    }

    #[test]
    fn test_mid() {
        let mut p = CommandParser::new();
        let (c, pos, _) = p.scan("《[Shell]|Ncoder|》\n【command】ls【command】\n", 0, false);
        assert!(c.is_empty()); assert!(pos < 70);
    }

    #[test] fn test_cl() { let mut p = CommandParser::new(); let (c,_,_) = p.scan("《[CheckList]|Ncoder|》\n【mode】create【mode】\n【title】fix【title】\n《[End]|CheckList|》",0,true); match &c[0] { NCommand::CheckList{blocks}=>assert_eq!(blocks[0].title.as_deref(),Some("fix")), _=>panic!() } }

    #[test]
    fn test_write_file_with_full_command_syntax_in_content() {
        let mut p = CommandParser::new();
        let input = r#"
我想帮你写入一个测试文件。

《[FilesOperator]|Ncoder|》
【mode】write【mode】
【path】test_example.md【path】
【content】# Example

Here is how you call a Shell command:

《[Shell]|Ncoder|》
【command】cargo test【command】
【is_async】false【is_async】
《[End]|Shell|》

And here is a FilesOperator example:

《[FilesOperator]|Ncoder|》
【mode】read【mode】
【path】src/main.rs【path】
【offset】1【offset】
【limit】50【limit】
《[End]|FilesOperator|》

The end.【content】
《[End]|FilesOperator|》
"#;
        let (c, _, w) = p.scan(input.trim(), 0, true);
        assert_eq!(c.len(), 1);
        for wm in &w { eprintln!("WARNING: {}", wm.message); }
        match &c[0] {
            NCommand::FilesOperator { blocks } => {
                assert_eq!(blocks.len(), 1);
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("《[Shell]|Ncoder|》"), "should contain Shell command syntax");
                assert!(content.contains("cargo test"), "should contain cargo test");
                assert!(content.contains("《[End]|Shell|》"), "should contain Shell end marker");
                assert!(content.contains("FilesOperator"), "should contain FilesOperator reference");
                assert!(content.contains("The end."), "should contain trailing text");
            }
            _ => panic!("expected FilesOperator, got {:?}", c[0]),
        }
    }

    #[test]
    fn test_interleaved_commands_with_reasoning() {
        let mut p = CommandParser::new();
        let input = r#"
我先读取一下当前的文件结构。

《[Shell]|Ncoder|》
【command】ls -la src/command/【command】
《[End]|Shell|》

现在看看 parser.rs 的内容。

《[FilesOperator]|Ncoder|》
【mode】read【mode】
【path】src/command/parser.rs【path】
【offset】1【offset】
【limit】30【limit】
《[End]|FilesOperator|》

好的，我看到了解析逻辑。现在运行测试确认。

《[Shell]|Ncoder|》
【command】cargo test parser【command】
【is_async】false【is_async】
《[End]|Shell|》

测试通过了，让我创建任务列表。

《[CheckList]|Ncoder|》
【mode】create【mode】
【title】重构解析器【title】
【content】优化 parse_section 的性能，处理嵌套键值对【content】
《[End]|CheckList|》
"#;
        let (c, _, w) = p.scan(input.trim(), 0, true);
        for wm in &w { eprintln!("WARNING: {}", wm.message); }
        assert_eq!(c.len(), 4, "expected 4 commands");
        // Shell 1: ls
        match &c[0] { NCommand::Shell{blocks}=>assert_eq!(blocks[0].command,"ls -la src/command/"), _=>panic!("cmd0") }
        // FilesOperator: read
        match &c[1] { NCommand::FilesOperator{blocks}=>assert_eq!(blocks[0].path, std::path::PathBuf::from("src/command/parser.rs")), _=>panic!("cmd1") }
        // Shell 2: cargo test
        match &c[2] { NCommand::Shell{blocks}=>assert_eq!(blocks[0].command,"cargo test parser"), _=>panic!("cmd2") }
        // CheckList: create
        match &c[3] { NCommand::CheckList{blocks}=>assert_eq!(blocks[0].title.as_deref(),Some("重构解析器")), _=>panic!("cmd3") }
    }

    #[test]
    fn test_multiline_value_with_nested_brackets() {
        let mut p = CommandParser::new();
        let input = "《[AgentLogs]|Ncoder|》\n【mode】write【mode】\n【filename】debug_log.md【filename】\n【content】The parser uses 《[ and 】characters.\n\nWhen content has bracket-like chars, the parser\nmust handle them correctly.【content】\n《[End]|AgentLogs|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::AgentLogs { blocks } => {
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("bracket-like chars"), "full multiline content preserved");
                assert!(content.contains("must handle them"), "full text preserved");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_nested_key_markers_inside_value_are_literal() {
        let mut p = CommandParser::new();
        // 【nested】 inside 【command】 value — since nested is a valid key marker,
        // it gets extracted as a nested key. The command value stops at the nested marker.
        let input = "《[Shell]|Ncoder|》\n【command】echo 【nested】 inside command【command】\n《[End]|Shell|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks[0].command, "echo ",
                    "command value stops at nested 【nested】 marker");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_full_realistic_scenario() {
        let mut p = CommandParser::new();
        let input = r#"
好的，我来为你创建这个功能。首先规划任务：

《[CheckList]|Ncoder|》
【mode】create【mode】
【title】添加日志系统【title】
【content】1. 添加 tracing 依赖 2. 修改 main.rs 3. 测试【content】
---
【mode】create【mode】
【title】添加配置验证【title】
【content】验证用户提供的 API key 是否有效【content】
《[End]|CheckList|》

现在查看项目结构：

《[Shell]|Ncoder|》
【command】ls -la src/【command】
《[End]|Shell|》

《[FilesOperator]|Ncoder|》
【mode】read【mode】
【path】Cargo.toml【path】
《[End]|FilesOperator|》

好的，添加 tracing 依赖。写入新的 main.rs：

《[FilesOperator]|Ncoder|》
【mode】write【mode】
【path】src/new_module.rs【path】
【content】use tracing::info;

pub fn init() {
    info!("module initialized");
}

// Usage example:
// 《[Shell]|Ncoder|》
// 【command】cargo run【command】
// 《[End]|Shell|》
【content】
《[End]|FilesOperator|》

现在构建并测试：

《[Shell]|Ncoder|》
【command】cargo build 2>&1【command】
【is_async】false【is_async】
《[End]|Shell|》

《[Shell]|Ncoder|》
【command】cargo test -- --nocapture【command】
《[End]|Shell|》

构建成功！记录日志：

《[AgentLogs]|Ncoder|》
【mode】write【mode】
【filename】add_logging.md【filename】
【content】Added tracing dependency and logger init. Build passes.【content】
《[End]|AgentLogs|》

《[CheckList]|Ncoder|》
【mode】update【mode】
【id】abc123【id】
【status】done【status】
《[End]|CheckList|》

任务完成！
"#;
        let (c, _, w) = p.scan(input.trim(), 0, true);
        for wm in &w { eprintln!("WARNING: {}", wm.message); }
        assert_eq!(c.len(), 8, "expected 8 commands in realistic scenario");

        let mut cmd_idx = 0;

        // cmd 0: CheckList create × 2
        match &c[cmd_idx] { NCommand::CheckList{blocks}=>{assert_eq!(blocks.len(),2);assert_eq!(blocks[0].title.as_deref(),Some("添加日志系统"));} _=>panic!() }
        cmd_idx += 1;

        // cmd 1: Shell ls
        match &c[cmd_idx] { NCommand::Shell{blocks}=>assert_eq!(blocks[0].command,"ls -la src/"), _=>panic!() }
        cmd_idx += 1;

        // cmd 2: FilesOperator read
        match &c[cmd_idx] { NCommand::FilesOperator{blocks}=>assert_eq!(blocks[0].path,std::path::PathBuf::from("Cargo.toml")), _=>panic!() }
        cmd_idx += 1;

        // cmd 3: FilesOperator write with nested command syntax in content
        match &c[cmd_idx] {
            NCommand::FilesOperator{blocks}=>{
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("《[Shell]|Ncoder|》"), "nested Shell in content preserved");
                assert!(content.contains("cargo run"), "cargo run preserved");
                assert!(content.contains("《[End]|Shell|》"), "nested Shell end preserved");
            }
            _ => panic!("cmd3")
        }
        cmd_idx += 1;

        // cmd 4: Shell cargo build
        match &c[cmd_idx] { NCommand::Shell{blocks}=>{assert_eq!(blocks[0].command,"cargo build 2>&1");assert!(!blocks[0].is_async);} _=>panic!() }
        cmd_idx += 1;

        // cmd 5: Shell cargo test
        match &c[cmd_idx] { NCommand::Shell{blocks}=>assert_eq!(blocks[0].command,"cargo test -- --nocapture"), _=>panic!() }
        cmd_idx += 1;

        // cmd 6: AgentLogs write
        match &c[cmd_idx] { NCommand::AgentLogs{blocks}=>assert_eq!(blocks[0].filename.as_deref(),Some("add_logging.md")), _=>panic!() }
        cmd_idx += 1;

        // cmd 7: CheckList update
        match &c[cmd_idx] { NCommand::CheckList{blocks}=>{assert_eq!(blocks[0].mode,CheckListMode::Update);assert_eq!(blocks[0].status.as_deref(),Some("done"));} _=>panic!() }
        cmd_idx += 1;

        assert_eq!(cmd_idx, 8);
    }

    #[test]
    fn test_separator_inside_content_value_not_split() {
        let mut p = CommandParser::new();
        let input = "《[FilesOperator]|Ncoder|》
【mode】write【mode】
【path】README.md【path】
【content】# Title

---

## Section 1

Some text with --- inline.

---

## Section 2

More text.
【content】
《[End]|FilesOperator|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::FilesOperator { blocks } => {
                assert_eq!(blocks.len(), 1, "only one block, --- inside value not split");
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("---"), "--- should be preserved in value");
                assert!(content.contains("Section 1"), "full content preserved");
                assert!(content.contains("Section 2"), "full content preserved");
                assert_eq!(blocks[0].path, std::path::PathBuf::from("README.md"));
                assert_eq!(blocks[0].mode, crate::command::syntax::FileMode::Write);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_multiple_separators_outside_values_still_split() {
        let mut p = CommandParser::new();
        let input = "《[Shell]|Ncoder|》
【command】echo first【command】
---
【command】echo second【command】
《[End]|Shell|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks.len(), 2, "--- between closed keys should still split");
                assert_eq!(blocks[0].command, "echo first");
                assert_eq!(blocks[1].command, "echo second");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_deeply_nested_content() {
        let mut p = CommandParser::new();
        let input = "《[FilesOperator]|Ncoder|》\n【mode】write【mode】\n【path】nested.md【path】\n【content】Level 0《[Shell]|x|》【command】a【command】《[End]|Shell|》still 0【content】\n《[End]|FilesOperator|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::FilesOperator { blocks } => {
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("Level 0"), "prefix preserved");
                assert!(content.contains("《[Shell]"), "nested Shell start preserved");
                assert!(content.contains("《[End]|Shell|》"), "nested Shell end preserved");
                assert!(content.contains("still 0"), "suffix preserved");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_command_in_content_with_same_command_name() {
        let mut p = CommandParser::new();
        let input = "《[FilesOperator]|Ncoder|》\n【mode】write【mode】\n【content】Look at this: 《[FilesOperator]|inner|》【mode】read【mode】《[End]|FilesOperator|》. That was inner.【content】\n【path】recursive.md【path】\n《[End]|FilesOperator|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::FilesOperator { blocks } => {
                let content = blocks[0].content.as_deref().unwrap();
                assert!(content.contains("《[FilesOperator]|inner|》"), "inner same-type command in value preserved");
                assert!(content.contains("That was inner."), "trailing text preserved");
                assert_eq!(blocks[0].path, std::path::PathBuf::from("recursive.md"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_malformed_key_marker_in_value() {
        let mut p = CommandParser::new();
        let input = "《[Shell]|Ncoder|》\n【command】echo 【badkey oops【command】\n《[End]|Shell|》";
        let (c, _, _) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        match &c[0] {
            NCommand::Shell { blocks } => {
                assert!(blocks[0].command.contains("【badkey oops"), "malformed marker should be in value");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_is_async_not_leaking_into_command() {
        let mut p = CommandParser::new();
        let input = "《[Shell]|Ncoder|》\n【command】cargo test -- --nocapture【command】\n【is_async】true【is_async】\n《[End]|Shell|》";
        let (c, _, w) = p.scan(input, 0, true);
        assert_eq!(c.len(), 1);
        assert!(w.is_empty(), "warnings: {:?}", w);
        match &c[0] {
            NCommand::Shell { blocks } => {
                assert_eq!(blocks[0].command, "cargo test -- --nocapture");
                assert!(blocks[0].is_async, "is_async should be true");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_malformed_key_missing_bracket() {
        let p = CommandParser::new();
        let input = "《[FilesOperator]|Ncoder|》
【mode】read【mode】
【pathsrc/command/files_operator.rs【path】
【limit】80【limit】
《[End]|CommandName|》";
        let (c, _, w) = p.scan(input, 0, true);
        assert!(!w.is_empty(), "should have warnings about mismatched end marker and malformed keys");
        let has_mismatch = w.iter().any(|x| x.message.contains("end marker mismatch"));
        let has_unclosed = w.iter().any(|x| x.message.contains("not closed"));
        assert!(has_mismatch || has_unclosed, "expected mismatch or unclosed key warning, got: {:?}", w);
        if !c.is_empty() {
            match &c[0] {
                NCommand::FilesOperator { blocks } => {
                    assert_eq!(blocks[0].path.as_os_str(), "", "malformed key should result in empty path");
                }
                _ => panic!(),
            }
        }
    }

    #[test]
    fn test_wrong_end_marker_detected() {
        let p = CommandParser::new();
        let input = "《[FilesOperator]|Ncoder|》
【mode】read【mode】
【path】./README.md【path】
《[End]|WrongName|》";
        let (_, _, w) = p.scan(input, 0, true);
        assert!(w.iter().any(|x| x.message.contains("end marker mismatch")), "should detect wrong end marker name, got: {:?}", w);
    }
}
