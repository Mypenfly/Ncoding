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
