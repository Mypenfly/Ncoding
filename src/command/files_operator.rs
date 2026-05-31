use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use super::syntax::{CommandOutcome, CommandResult, CommandType, FileMode, FileOpBlock};

const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024;

#[allow(dead_code)]
pub async fn execute(blocks: Vec<FileOpBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    execute_with_backup(blocks, None).await
}

pub async fn execute_with_backup(
    blocks: Vec<FileOpBlock>,
    backup_dir: Option<&Path>,
) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        if block.path.as_os_str().is_empty() {
            let missing_keys = check_missing_keys(&block);
            results.push(CommandResult {
                command_type: CommandType::FilesOperator,
                block_index: i,
                outcome: CommandOutcome::Failure {
                    error: format!(
                        "path is empty. The 【path】key may be malformed or missing a closing 】. {}",
                        missing_keys
                    ),
                },
            });
            continue;
        }
        let base_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let resolved = match resolve_path(&block.path, &base_path) {
            Ok(p) => p,
            Err(e) => {
                results.push(CommandResult {
                    command_type: CommandType::FilesOperator,
                    block_index: i,
                    outcome: CommandOutcome::Failure {
                        error: format!("path resolution error: {}", e),
                    },
                });
                continue;
            }
        };

        if is_path_escape(&block.path) {
            results.push(CommandResult {
                command_type: CommandType::FilesOperator,
                block_index: i,
                outcome: CommandOutcome::Failure {
                    error: "path escape detected: path contains ../ or is outside workspace".into(),
                },
            });
            continue;
        }

        let result = match block.mode {
            FileMode::Read => read_file(&resolved, block.offset, block.limit),
            FileMode::Write => write_file(&resolved, block.content.as_deref(), backup_dir),
            FileMode::Edit => edit_file(
                &resolved,
                block.old_str.as_deref(),
                block.new_str.as_deref(),
                block.old_lines.as_deref(),
                block.new_lines.as_deref(),
                backup_dir,
            ),
        };

        results.push(CommandResult {
            command_type: CommandType::FilesOperator,
            block_index: i,
            outcome: result,
        });
    }

    Ok(results)
}

fn is_path_escape(path: &Path) -> bool {
    path.to_string_lossy().contains("..")
}

fn resolve_path(path: &Path, base: &Path) -> Result<PathBuf, String> {
    let path_str = path.to_string_lossy();

    let expanded = if path_str.starts_with("~/") {
        let home = dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?;
        home.join(path_str.strip_prefix("~/").unwrap())
    } else if path_str.as_ref() == "~" {
        dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?
    } else {
        PathBuf::from(path_str.as_ref())
    };

    let joined = if expanded.is_absolute() {
        expanded
    } else {
        base.join(&expanded)
    };

    Ok(joined)
}

fn tmp_path(path: &Path) -> PathBuf {
    let pid = std::process::id();
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let rand: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        .wrapping_mul(pid as u64);
    let tmp_name = format!(".{}.{}.tmp", file_name, rand);
    if let Some(parent) = path.parent() {
        parent.join(tmp_name)
    } else {
        PathBuf::from(tmp_name)
    }
}

fn backup_original(path: &Path, backup_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(backup_dir)?;
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let rel = path
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c == std::path::MAIN_SEPARATOR || c == ':' {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    let backup_name = format!("{}_{}", timestamp, rel);
    let backup_path = backup_dir.join(backup_name);
    fs::copy(path, &backup_path)?;
    info!("Backed up {} to {}", path.display(), backup_path.display());
    Ok(())
}

fn read_file(path: &PathBuf, offset: Option<usize>, limit: Option<usize>) -> CommandOutcome {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return CommandOutcome::Failure {
                error: format!("cannot open {}: {}", path.display(), e),
            }
        }
    };

    let start = offset.unwrap_or(1);
    let max_lines = limit.unwrap_or(2000).min(2000);

    let reader = std::io::BufReader::new(file);
    let mut output = String::new();
    let mut line_num: usize = 0;
    let mut read_start = None;
    let mut read_end = 0;

    for line_result in reader.lines() {
        line_num += 1;
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                warn!("Error reading line {}: {}", line_num, e);
                continue;
            }
        };

        if line_num < start {
            continue;
        }
        if (line_num - start) >= max_lines {
            break;
        }

        if read_start.is_none() {
            read_start = Some(line_num);
        }
        read_end = line_num;
        output.push_str(&format!("{}: {}\n", line_num, line));
    }

    let lines_range = read_start.map(|s| format!("{}..{}", s, read_end));
    let summary = format!(
        "status: OK\npath: {}\nlines: {}{}",
        path.display(),
        lines_range.unwrap_or_else(|| "N/A".into()),
        if read_end > 0 {
            "\n".to_string() + &output
        } else {
            String::new()
        }
    );

    CommandOutcome::Success { summary }
}

fn write_file(path: &PathBuf, content: Option<&str>, backup_dir: Option<&Path>) -> CommandOutcome {
    let content = match content {
        Some(c) => c,
        None => {
            return CommandOutcome::Failure {
                error: "write mode requires content parameter".into(),
            }
        }
    };

    if content.len() > MAX_WRITE_SIZE {
        return CommandOutcome::Failure {
            error: format!(
                "write exceeds size limit ({} bytes allowed, content is {} bytes)",
                MAX_WRITE_SIZE,
                content.len()
            ),
        };
    }

    let existed = path.exists();

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                return CommandOutcome::Failure {
                    error: format!(
                        "failed to create parent directory {}: {}",
                        parent.display(),
                        e
                    ),
                };
            }
        }
    }

    if existed {
        if let Some(backup) = backup_dir {
            let _ = backup_original(path, backup);
        }
    }

    let tmp = tmp_path(path);
    match fs::write(&tmp, content) {
        Ok(()) => match fs::rename(&tmp, path) {
            Ok(()) => {
                match fs::read_to_string(path) {
                    Ok(written) if written != content => CommandOutcome::Failure {
                        error: format!(
                            "write verification failed: file content mismatch (expected {} bytes, got {} bytes). Try re-writing or use a different approach.",
                            content.len(), written.len()
                        ),
                    },
                    Ok(_) => CommandOutcome::Success {
                        summary: format!(
                            "status: OK\npath: {}\naction: {}",
                            path.display(),
                            if existed { "overwritten" } else { "created" }
                        ),
                    },
                    Err(e) => CommandOutcome::Failure {
                        error: format!(
                            "write applied but verification read failed: {}. File may be corrupted, re-check.",
                            e
                        ),
                    },
                }
            },
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                CommandOutcome::Failure {
                    error: format!("failed to rename tmp to {}: {}", path.display(), e),
                }
            }
        },
        Err(e) => CommandOutcome::Failure {
            error: format!("failed to write {}: {}", path.display(), e),
        },
    }
}

fn edit_file(
    path: &PathBuf,
    _old_str: Option<&str>,
    _new_str: Option<&str>,
    old_lines: Option<&str>,
    new_lines: Option<&str>,
    backup_dir: Option<&Path>,
) -> CommandOutcome {
    if old_lines.is_some() || new_lines.is_some() {
        let old_l = old_lines.unwrap_or("");
        let new_l = new_lines.unwrap_or("");
        if old_l.is_empty() {
            return CommandOutcome::Failure {
                error: "edit failed: old_lines cannot be empty".into(),
            };
        }
        return edit_lines(path, old_l, new_l, backup_dir);
    }

    CommandOutcome::Failure {
        error: "edit mode requires old_lines/new_lines parameters. old_str/new_str is no longer supported. Use old_lines/new_lines with ... for skip.".into(),
    }
}



fn edit_lines(
    path: &PathBuf,
    old_lines: &str,
    new_lines: &str,
    backup_dir: Option<&Path>,
) -> CommandOutcome {
    let old_lines = old_lines.trim();
    let new_lines = new_lines.trim();

    let file_content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return CommandOutcome::Failure {
                error: format!("failed to read {}: {}", path.display(), e),
            }
        }
    };

    let file_lines: Vec<&str> = file_content.lines().collect();
    let old_anchored: Vec<&str> = old_lines
        .lines()
        .filter(|l| l.trim() != "...")
        .filter(|l| !l.trim().is_empty())
        .collect();
    let new_anchored: Vec<&str> = new_lines
        .lines()
        .filter(|l| l.trim() != "...")
        .filter(|l| !l.trim().is_empty())
        .collect();

    if old_anchored.is_empty() {
        return CommandOutcome::Failure {
            error: "edit failed: old_lines has no anchored content (all lines are '...')".into(),
        };
    }

    let old_segments = split_segments(old_lines);
    let has_dots = old_segments.iter().filter(|s| !s.is_empty()).count() > 1;

    let valid: Vec<(usize, usize)> = if has_dots {
        find_dotted_matches(&file_lines, &old_segments)
    } else {
        find_all_sequential_matches(&file_lines, &old_segments[0])
    };

    if valid.is_empty() {
        let first_line = old_anchored[0].trim().to_string();
        let last_line = old_anchored.last().unwrap().trim().to_string();
        let first_found = file_lines.iter().any(|l| normalized_match(l, &first_line));
        let _last_found = file_lines.iter().any(|l| normalized_match(l, &last_line));
        if !first_found {
            return CommandOutcome::Failure {
                error: format!(
                    "edit failed: first line not found in {}. Searched for (indent-stripped): '{}'. Re-read the file.",
                    path.display(), first_line
                ),
            };
        }
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: no matching block found in {}. The anchored lines could not be matched sequentially. Possible reasons: old_lines content differs from file, or lines are out of order. Try re-reading the file.",
                path.display()
            ),
        };
    }

    if valid.len() > 1 {
        let mut best = valid[0];
        let mut best_score = usize::MAX;
        for &(f, l) in &valid {
            let score = l.saturating_sub(f);
            if score < best_score {
                best_score = score;
                best = (f, l);
            }
        }

        let ctx_start = best.0.saturating_sub(3);
        let ctx_end = (best.1 + 3).min(file_lines.len().saturating_sub(1));
        let block_code: Vec<String> = file_lines[ctx_start..=ctx_end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("    {}: {}", ctx_start + i + 1, line))
            .collect();
        let block_text = block_code.join("\n");

        return CommandOutcome::Failure {
            error: format!(
                "edit failed: old_lines matched {} blocks in {}. The smallest span is lines {}..{}:\n{}\n\n你可能想要修改此处，请重新阅读文件后增加更多相邻行作为上下文来消除歧义。",
                valid.len(), path.display(),
                best.0 + 1, best.1 + 1,
                block_text,
            ),
        };
    }

    let (start, end) = valid[0];

    if let Some(backup) = backup_dir {
        let _ = backup_original(path, backup);
    }

    let new_segments = split_segments(new_lines);

    let final_content = if has_dots {
        apply_dotted_replace(&file_lines, &old_segments, &new_segments, start, end)
    } else {
        apply_simple_replace(&file_lines, &old_segments[0], &new_segments[0], start, end)
    };
    let final_content = preserve_newline(&file_content, final_content);

    let tmp = tmp_path(path);
    match fs::write(&tmp, &final_content) {
        Ok(()) => match fs::rename(&tmp, path) {
            Ok(()) => {
                let verif = match fs::read_to_string(path) {
                    Ok(vc) => {
                        let vl: Vec<&str> = vc.lines().collect();
                        new_anchored
                            .iter()
                            .all(|nl| vl.iter().any(|fl| normalized_match(fl, nl)))
                    }
                    Err(_) => true,
                };
                if !verif {
                    CommandOutcome::Failure {
                        error: format!(
                            "edit verification failed: new_lines not found in edited file {}. Write was applied but content may be incorrect. Re-read the file.",
                            path.display()
                        ),
                    }
                } else {
                    CommandOutcome::Success {
                        summary: format!(
                            "status: OK\npath: {}\nreplaced: lines {}..{} -> {} new lines",
                            path.display(),
                            start + 1,
                            end + 1,
                            new_anchored.len()
                        ),
                    }
                }
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                CommandOutcome::Failure {
                    error: format!("failed to rename tmp to {}: {}", path.display(), e),
                }
            }
        },
        Err(e) => CommandOutcome::Failure {
            error: format!("failed to write {}: {}", path.display(), e),
        },
    }
}

fn find_all_sequential_matches(
    file_lines: &[&str],
    search_lines: &[&str],
) -> Vec<(usize, usize)> {
    let n = file_lines.len();
    let sl = search_lines.len();
    if sl == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let max_start = n.saturating_sub(sl);
    'outer: for start in 0..=max_start {
        for j in 0..sl {
            let old_entry = search_lines[j];
            let file_line = file_lines[start + j];
            if old_entry.trim().is_empty() {
                if !file_line.trim().is_empty() {
                    continue 'outer;
                }
            } else if !normalized_match(file_line, old_entry) {
                continue 'outer;
            }
        }
        matches.push((start, start + sl - 1));
    }
    matches
}

fn find_dotted_matches(
    file_lines: &[&str],
    old_segments: &[Vec<&str>],
) -> Vec<(usize, usize)> {
    let non_empty: Vec<&Vec<&str>> = old_segments.iter().filter(|s| !s.is_empty()).collect();
    if non_empty.is_empty() {
        return Vec::new();
    }

    let head = non_empty[0];
    let head_matches = find_all_sequential_matches(file_lines, head.as_slice());
    if non_empty.len() == 1 {
        return head_matches;
    }

    let tail = non_empty[non_empty.len() - 1];
    let tail_matches = find_all_sequential_matches(file_lines, tail.as_slice());

    let middle: Vec<&Vec<&str>> = non_empty[1..non_empty.len() - 1].to_vec();

    let mut valid = Vec::new();
    for &(hs, he) in &head_matches {
        for &(ts, te) in &tail_matches {
            if ts <= he {
                continue;
            }
            let mut ok = true;
            let mut cursor = he + 1;
            for mseg in &middle {
                if mseg.is_empty() {
                    continue;
                }
                let pos = find_segment_in_range(file_lines, mseg.as_slice(), cursor, ts.saturating_sub(1));
                match pos {
                    Some((_, me)) => cursor = me + 1,
                    None => { ok = false; break; }
                }
            }
            if ok && te >= hs && te >= he && te >= ts {
                valid.push((hs, te));
            }
        }
    }

    if valid.is_empty() && head_matches.len() == 1 && !middle.is_empty() {
        let (hs, he) = head_matches[0];
        for &(ts, te) in &tail_matches {
            if ts > he {
                valid.push((hs, te));
            }
        }
    }

    valid
}

#[allow(clippy::needless_range_loop)]
fn find_segment_in_range(
    file_lines: &[&str],
    seg: &[&str],
    search_start: usize,
    search_end: usize,
) -> Option<(usize, usize)> {
    let mut si = 0;
    let mut seg_start = None;
    let mut seg_end = None;
    for i in search_start..=search_end {
        if i >= file_lines.len() {
            break;
        }
        if si < seg.len() && normalized_match(file_lines[i], seg[si]) {
            if seg_start.is_none() {
                seg_start = Some(i);
            }
            si += 1;
            if si == seg.len() {
                seg_end = Some(i);
                break;
            }
        }
    }
    match (seg_start, seg_end) {
        (Some(s), Some(e)) => Some((s, e)),
        _ => None,
    }
}

#[allow(clippy::needless_range_loop)]
fn apply_simple_replace(
    file_lines: &[&str],
    old_seg: &[&str],
    new_seg: &[&str],
    start: usize,
    end: usize,
) -> String {
    let old_len = old_seg.len();
    let new_len = new_seg.len();
    let last_old_indent = if end < file_lines.len() {
        file_lines[end].chars().take_while(|c| c.is_whitespace()).count()
    } else {
        0
    };

    let mut out: Vec<String> = Vec::with_capacity(file_lines.len() + new_len);
    out.extend(file_lines[..start].iter().map(|s| s.to_string()));

    let min_len = old_len.min(new_len);
    for j in 0..min_len {
        let orig_line_idx = start + j;
        let orig_indent = if orig_line_idx < file_lines.len() {
            file_lines[orig_line_idx].chars().take_while(|c| c.is_whitespace()).count()
        } else {
            0
        };
        let indent_str = if orig_indent > 0 && orig_line_idx < file_lines.len() {
            &file_lines[orig_line_idx][..orig_indent.min(file_lines[orig_line_idx].len())]
        } else {
            ""
        };
        let body = new_seg[j].trim_start();
        out.push(format!("{}{}", indent_str, body));
    }

    if new_len > old_len {
        let first_extra_body = new_seg[old_len].trim_start();
        let first_extra_indent = new_seg[old_len].len() - first_extra_body.len();
        for j in old_len..new_len {
            let body = new_seg[j].trim_start();
            let this_indent = new_seg[j].len() - body.len();
            let rel_indent = this_indent.saturating_sub(first_extra_indent);
            let total_indent = last_old_indent + rel_indent;
            out.push(format!("{:indent$}{}", "", body, indent = total_indent));
        }
    }

    out.extend(file_lines[end + 1..].iter().map(|s| s.to_string()));
    out.join("\n")
}

#[allow(clippy::needless_range_loop)]
fn apply_dotted_replace(
    file_lines: &[&str],
    old_segments: &[Vec<&str>],
    new_segments: &[Vec<&str>],
    start: usize,
    end: usize,
) -> String {
    let mut out: Vec<String> = file_lines[..start].iter().map(|s| s.to_string()).collect();
    let mut cursor = start;
    let n = old_segments.len();

    let old_positions: Vec<Option<(usize, usize)>> = {
        let mut pos = Vec::new();
        let mut c = start;
        for seg in old_segments {
            if seg.is_empty() {
                pos.push(None);
                continue;
            }
            let p = find_segment_in_range(file_lines, seg.as_slice(), c, end);
            if let Some((s, e)) = p {
                c = e + 1;
                pos.push(Some((s, e)));
            } else {
                pos.push(None);
            }
        }
        pos
    };

    for seg_idx in 0..n {
        if let Some(Some((seg_start, seg_end))) = old_positions.get(seg_idx) {
            while cursor < *seg_start {
                out.push(file_lines[cursor].to_string());
                cursor += 1;
            }

            let new_seg: &[&str] = match new_segments.get(seg_idx) {
                Some(s) if !s.is_empty() => s.as_slice(),
                _ => &[],
            };

            let old_len = seg_end.saturating_sub(*seg_start) + 1;
            let new_len = new_seg.len();
            let min_len = old_len.min(new_len);

            for j in 0..min_len {
                let orig_line_idx = *seg_start + j;
                let orig_indent = file_lines[orig_line_idx]
                    .chars().take_while(|c| c.is_whitespace()).count();
                let indent_str = if orig_indent > 0 {
                    &file_lines[orig_line_idx][..orig_indent.min(file_lines[orig_line_idx].len())]
                } else { "" };
                out.push(format!("{}{}", indent_str, new_seg[j].trim_start()));
            }

            if new_len > old_len {
                let last_indent = file_lines[*seg_end]
                    .chars().take_while(|c| c.is_whitespace()).count();
                let first_extra_body = new_seg[old_len].trim_start();
                let first_extra_indent = new_seg[old_len].len() - first_extra_body.len();
                for j in old_len..new_len {
                    let body = new_seg[j].trim_start();
                    let this_indent = new_seg[j].len() - body.len();
                    let rel = this_indent.saturating_sub(first_extra_indent);
                    out.push(format!("{:indent$}{}", "", body, indent = last_indent + rel));
                }
            }

            cursor = seg_end + 1;
        }
    }

    while cursor <= end && cursor < file_lines.len() {
        out.push(file_lines[cursor].to_string());
        cursor += 1;
    }

    out.extend(file_lines[end + 1..].iter().map(|s| s.to_string()));
    out.join("\n")
}

fn preserve_newline(original: &str, content: String) -> String {
    if original.ends_with('\n') && !content.ends_with('\n') {
        content + "\n"
    } else {
        content
    }
}

fn normalized_match(a: &str, b: &str) -> bool {
    let na: String = a.chars().filter(|c| !c.is_whitespace()).collect();
    let nb: String = b.chars().filter(|c| !c.is_whitespace()).collect();
    na == nb
}

fn split_segments(text: &str) -> Vec<Vec<&str>> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        if line.trim() == "..." {
            segments.push(std::mem::take(&mut current));
        } else {
            current.push(line);
        }
    }
    segments.push(current);
    segments
}

fn check_missing_keys(block: &FileOpBlock) -> String {
    let mut missing = Vec::new();
    if block.path.as_os_str().is_empty() {
        missing.push("path");
    }
    if block.mode != FileMode::Read
        && block.content.is_none()
        && block.old_str.is_none()
        && block.old_lines.is_none()
    {
        missing.push("content or old_str/new_str or old_lines/new_lines");
    }
    if missing.is_empty() {
        String::from("Check key syntax: 【key】value【key】")
    } else {
        format!("Missing or malformed keys: {}", missing.join(", "))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

 struct Cleanup(Option<PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(ref dir) = self.0 {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    fn make_temp_dir() -> (PathBuf, Cleanup) {
        let mut dir = std::env::temp_dir();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap()
            .as_nanos();
        dir.push(format!("ncoding_test_{}", ts));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.clone();
        (path, Cleanup(Some(dir)))
    }

    fn write_test_file(path: &PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).ok();
            }
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_read_file_basic() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("test.txt");
        write_test_file(&p, "line1\nline2\nline3\n");
        let outcome = read_file(&p, None, None);
        match outcome {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("1: line1"));
                assert!(summary.contains("3: line3"));
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_write_file_create_and_overwrite() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("new.txt");

        let outcome = write_file(&p, Some("hello world"), None);
        match outcome {
            CommandOutcome::Success { ref summary } => {
                assert!(summary.contains("created"));
            }
            _ => panic!("expected created"),
        }

        let outcome2 = write_file(&p, Some("updated"), None);
        match outcome2 {
            CommandOutcome::Success { ref summary } => {
                assert!(summary.contains("overwritten"));
            }
            _ => panic!("expected overwritten"),
        }
    }

    #[test]
    fn test_edit_lines_basic_replace() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("edit.txt");
        write_test_file(&p, "fn main() {\n    println!(\"hello\");\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("fn main() {\n    ...\n}"),
            Some("fn main() {\n    ...\n    println!(\"world\");\n}"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("edit failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("println!(\"world\");"));
    }

    #[test]
    fn test_edit_lines_ambiguous_error_shows_code() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("ambig.txt");
        write_test_file(&p, "\
fn foo() {
    do_a()
    do_b()
}
fn bar() {
    do_a()
    do_b()
}
");

        let outcome = edit_file(
            &p, None, None,
            Some("do_a()\n    do_b()"),
            Some("do_a()\n    do_b()"),
            None,
        );
        match outcome {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("do_a"), "error should contain code: {}", error);
                assert!(error.contains("matched 2 blocks"), "error should mention ambiguity: {}", error);
            }
            _ => panic!("expected failure for ambiguous match, but got success"),
        }
    }

    #[test]
    fn test_edit_file_rejects_old_str() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("reject.txt");
        write_test_file(&p, "hello old world\n");

        let outcome = edit_file(
            &p,
            Some("hello old world"),
            Some("hello new world"),
            None, None,
            None,
        );
        match outcome {
            CommandOutcome::Failure { error } => {
                assert!(
                    error.contains("old_lines"),
                    "error should mention old_lines: {}",
                    error
                );
            }
            _ => panic!("expected failure when using old_str without old_lines"),
        }
    }

    #[test]
    fn test_edit_lines_no_match_reports_boundary() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("nomatch.txt");
       write_test_file(&p, "fn a() {}\nfn b() {}\nfn c() {}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("fn z() {}"),
            Some("fn z() {}"),
            None,
        );
        match outcome {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("not found"));
            }
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn test_edit_single_line_unique_match() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("single.txt");
        write_test_file(&p, "fn foo() {\n    old_code()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("old_code()"),
            Some("new_code()"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("single-line match failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("new_code()"));
        assert!(!content.contains("old_code()"));
    }

    #[test]
    fn test_edit_single_line_to_multi_increment() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("incr.txt");
        write_test_file(&p, "fn main() {\n    old()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("old()"),
            Some("new_a()\n    new_b()\n    new_c()"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("increment edit failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("new_a()"));
        assert!(content.contains("new_b()"));
        assert!(content.contains("new_c()"));
        assert!(!content.contains("old()"));
    }

    #[test]
    fn test_edit_multi_to_single_decrement() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("decr.txt");
        write_test_file(&p, "fn main() {\n    x()\n    y()\n    z()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("x()\n    y()\n    z()"),
            Some("done()"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("decrement edit failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("done()"));
        assert!(!content.contains("x()"));
        assert!(!content.contains("y()"));
        assert!(!content.contains("z()"));
    }

    #[test]
    fn test_edit_indentation_preserved() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("indent.txt");
        write_test_file(&p, "fn outer() {\n        fn inner() {\n            42\n        }\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("42"),
            Some("84"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("indent edit failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("            84"), "indentation should be preserved");
    }

    #[test]
    fn test_edit_dotted_match() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("dots.txt");
        write_test_file(&p, "fn calc() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("fn calc() {\n        ...\n        let c = 3;\n        ...\n    a + b + c\n}"),
            Some("fn calc() {\n        ...\n        let c = 99;\n        ...\n    a * b * c\n}"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("dotted match failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("let c = 99"), "c should change: {}", content);
        assert!(content.contains("a * b * c"), "return should change: {}", content);
    }

    #[test]
    fn test_edit_new_segment_empty_removes_lines() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("remove.txt");
        write_test_file(&p, "fn main() {\n    keep_me()\n    remove_me()\n    also_keep()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("keep_me()\n    remove_me()\n    also_keep()"),
            Some("keep_me()\n    also_keep()"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("removal edit failed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("keep_me"));
        assert!(!content.contains("remove_me"));
        assert!(content.contains("also_keep"));
    }

    #[test]
    fn test_edit_lines_no_gaps_allowed() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("nogaps.txt");
        write_test_file(&p, "fn a() {\n    // comment\n    x()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("fn a() {\n    x()\n}"),
            Some("fn a() {\n    y()\n}"),
            None,
        );
        match outcome {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("no matching block"), "should fail on gap: {}", error);
            }
            _ => panic!("expected failure due to // comment gap between old_lines"),
        }
    }

    #[test]
    fn test_edit_empty_lines_must_match() {
        let (dir, _cleanup) = make_temp_dir();
        let p = dir.join("emptymatch.txt");
        write_test_file(&p, "fn a() {\n\n    x()\n}\n");

        let outcome = edit_file(
            &p, None, None,
            Some("fn a() {\n\n    x()\n}"),
            Some("fn a() {\n\n    y()\n}"),
            None,
        );
        match outcome {
            CommandOutcome::Success { .. } => {},
            CommandOutcome::Failure { error } => panic!("empty line match should succeed: {}", error),
        }
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("y()"));
        assert!(!content.contains("x()"));
    }
}
