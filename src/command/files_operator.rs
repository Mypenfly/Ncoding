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
                    error: "path escape detected: path contains ../ or is outside workspace"
                        .into(),
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
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tmp");
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
        .map(|c| if c == std::path::MAIN_SEPARATOR || c == ':' { '_' } else { c })
        .collect::<String>();
    let backup_name = format!("{}_{}", timestamp, rel);
    let backup_path = backup_dir.join(backup_name);
    fs::copy(path, &backup_path)?;
    info!("Backed up {} to {}", path.display(), backup_path.display());
    Ok(())
}

fn read_file(
    path: &PathBuf,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CommandOutcome {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return CommandOutcome::Failure {
            error: format!("cannot open {}: {}", path.display(), e),
        },
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
        if read_end > 0 { "\n".to_string() + &output } else { String::new() }
    );

    CommandOutcome::Success { summary }
}

fn write_file(path: &PathBuf, content: Option<&str>, backup_dir: Option<&Path>) -> CommandOutcome {
    let content = match content {
        Some(c) => c,
        None => return CommandOutcome::Failure {
            error: "write mode requires content parameter".into(),
        },
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
                    error: format!("failed to create parent directory {}: {}", parent.display(), e),
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
    old_str: Option<&str>,
    new_str: Option<&str>,
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

    let (old_str, new_str) = match (old_str, new_str) {
        (Some(o), Some(n)) => (o, n),
        _ => return CommandOutcome::Failure {
            error: "edit mode requires old_lines/new_lines or old_str/new_str parameters".into(),
        },
    };

    if old_str.is_empty() {
        return CommandOutcome::Failure {
            error: "edit failed: old_str cannot be empty".into(),
        };
    }

    edit_str(path, old_str, new_str, backup_dir)
}

fn edit_str(path: &PathBuf, old_str: &str, new_str: &str, backup_dir: Option<&Path>) -> CommandOutcome {
    let file_content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return CommandOutcome::Failure {
            error: format!("failed to read {}: {}", path.display(), e),
        },
    };

    let matches: Vec<_> = file_content.match_indices(old_str).collect();

    match matches.len() {
        0 => CommandOutcome::Failure {
            error: format!(
                "edit failed: old_str not found in {}. Please re-read the file to verify content.",
                path.display()
            ),
        },
        1 => {
            let (pos, _) = matches[0];
            if let Some(backup) = backup_dir {
                let _ = backup_original(path, backup);
            }
            let mut new_content = String::with_capacity(file_content.len());
            new_content.push_str(&file_content[..pos]);
            new_content.push_str(new_str);
            new_content.push_str(&file_content[pos + old_str.len()..]);

            let start_line = file_content[..pos].bytes().filter(|&b| b == b'\n').count() + 1;

            let tmp = tmp_path(path);
            match fs::write(&tmp, &new_content) {
                Ok(()) => match fs::rename(&tmp, path) {
                    Ok(()) => {
                        match verify_edit(path, new_str, &file_content) {
                            None => CommandOutcome::Success {
                                summary: format!(
                                    "status: OK\npath: {}\nreplaced: 1 occurrence at line {}",
                                    path.display(), start_line
                                ),
                            },
                            Some(err) => CommandOutcome::Failure {
                                error: format!("edit verification failed: {} (line {}). Write was applied but content may be incorrect. Re-read the file.", err, start_line),
                            },
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
        n => {
            let locations: Vec<String> = matches
                .iter()
                .map(|(pos, _)| {
                    let line = file_content[..*pos].bytes().filter(|&b| b == b'\n').count() + 1;
                    line.to_string()
                })
                .collect();
            CommandOutcome::Failure {
                error: format!(
                    "edit failed: old_str matched {} locations (lines {}). \
                     Provide more context to make old_str unique.",
                    n, locations.join(", ")
                ),
            }
        }
    }
}

fn edit_lines(path: &PathBuf, old_lines: &str, new_lines: &str, backup_dir: Option<&Path>) -> CommandOutcome {
    let old_lines = old_lines.trim();
    let new_lines = new_lines.trim();

    let file_content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return CommandOutcome::Failure {
            error: format!("failed to read {}: {}", path.display(), e),
        },
    };

    let file_lines: Vec<&str> = file_content.lines().collect();
    let old_anchored: Vec<&str> = old_lines.lines()
        .filter(|l| l.trim() != "...")
        .filter(|l| !l.trim().is_empty())
        .collect();
    let new_anchored: Vec<&str> = new_lines.lines()
        .filter(|l| l.trim() != "...")
        .filter(|l| !l.trim().is_empty())
        .collect();

    if old_anchored.is_empty() {
        return CommandOutcome::Failure {
            error: "edit failed: old_lines has no anchored content (all lines are '...')".into(),
        };
    }

    if old_anchored.is_empty() {
        return CommandOutcome::Failure {
            error: "edit failed: old_lines has no anchored content (all lines are '...')".into(),
        };
    }

    let first_line = old_anchored[0].trim().to_string();
    let last_line = old_anchored.last().unwrap().trim().to_string();

    let first_matches: Vec<usize> = file_lines.iter().enumerate()
        .filter(|(_, l)| normalized_match(l, &first_line))
        .map(|(i, _)| i)
        .collect();
    let last_matches: Vec<usize> = file_lines.iter().enumerate()
        .filter(|(_, l)| normalized_match(l, &last_line))
        .map(|(i, _)| i)
        .collect();

    if first_matches.is_empty() {
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: first anchored line not found in {}. Searched for (indent-stripped): '{}'",
                path.display(), first_line
            ),
        };
    }
    if last_matches.is_empty() {
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: last anchored line not found in {}. Searched for (indent-stripped): '{}'",
                path.display(), last_line
            ),
        };
    }

    let valid: Vec<(usize, usize)> = first_matches.iter()
        .flat_map(|&f| last_matches.iter().filter(move |&&l| l >= f).map(move |&l| (f, l)))
        .filter(|&(f, l)| match_intermediate(&file_lines, &old_anchored, (f, l)))
        .collect();

    if valid.is_empty() {
        let candidates: Vec<String> = first_matches.iter()
            .flat_map(|&f| last_matches.iter().map(move |&l| (f, l)))
            .map(|(f, l)| format!("first at line {}, last at line {}", f + 1, l + 1))
            .collect();
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: no matching block found in {}. Try re-reading the file. Possible boundaries: {}",
                path.display(),
                candidates.join("; "),
            ),
        };
    }
    if valid.len() > 1 {
        let desc: Vec<String> = valid.iter()
            .map(|(f, l)| format!("lines {}..{}", f + 1, l + 1))
            .collect();
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: ambiguous match — {} possible blocks in {}. Add more context to make the match unique. Candidates: {}",
                valid.len(), path.display(), desc.join(", "),
            ),
        };
    }

    let (start, end) = valid[0];

    if let Some(backup) = backup_dir {
        let _ = backup_original(path, backup);
    }

    let old_segments = split_segments(old_lines);
    let new_segments_needed = split_segments(new_lines);

    let segment_positions = find_segment_positions(&file_lines, &old_segments, start, end);
    if segment_positions.is_empty() {
        return CommandOutcome::Failure {
            error: format!(
                "edit failed: internal matching error in {}. Segments could not be located.",
                path.display()
            ),
        };
    }

    let mut out_lines: Vec<String> = file_lines.iter().take(start).map(|s| s.to_string()).collect();
    let mut cursor = start;

    let n = old_segments.len();
    for seg_idx in 0..n {
        if seg_idx < segment_positions.len() {
            let (seg_start, seg_end) = segment_positions[seg_idx];
            while cursor < seg_start {
                out_lines.push(file_lines[cursor].to_string());
                cursor += 1;
            }

            let new_seg: &[&str] = match new_segments_needed.get(seg_idx) {
                Some(s) if !s.is_empty() => s.as_slice(),
                _ => &[],
            };

            for (j, nl) in new_seg.iter().enumerate() {
                let orig_line_idx = if j < seg_end.saturating_sub(seg_start) + 1 {
                    seg_start + j
                } else {
                    seg_start
                };
                let orig_indent = if orig_line_idx < file_lines.len() {
                    file_lines[orig_line_idx].chars().take_while(|c| c.is_whitespace()).count()
                } else {
                    0
                };
                let stripped = strip_indent_full(nl);
                let indent_str = if orig_indent > 0 && orig_line_idx < file_lines.len() {
                    let fl = file_lines[orig_line_idx];
                    &fl[..orig_indent.min(fl.len())]
                } else {
                    ""
                };
                out_lines.push(format!("{}{}", indent_str, stripped));
            }
            cursor = (seg_end + 1).min(file_lines.len());
        }
    }

    while cursor <= end && cursor < file_lines.len() {
        out_lines.push(file_lines[cursor].to_string());
        cursor += 1;
    }

    out_lines.extend(file_lines.iter().skip(end + 1).map(|s| s.to_string()));

    let new_content = out_lines.join("\n");
    if !file_content.ends_with('\n') {
        let _ = new_content;
    }
    let final_content = if file_content.ends_with('\n') && !new_content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    let tmp = tmp_path(path);
    match fs::write(&tmp, &final_content) {
        Ok(()) => match fs::rename(&tmp, path) {
            Ok(()) => {
                let verif = match fs::read_to_string(path) {
                    Ok(vc) => {
                        let vl: Vec<&str> = vc.lines().collect();
                        let all_found = new_anchored.iter().all(|nl| {
                            vl.iter().any(|fl| normalized_match(fl, nl))
                        });
                        all_found
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
                            path.display(), start + 1, end + 1, new_anchored.len()
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

fn strip_indent_full(line: &str) -> &str {
    line.trim_start()
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

fn find_segment_positions(
    file_lines: &[&str],
    old_segments: &[Vec<&str>],
    search_start: usize,
    search_end: usize,
) -> Vec<(usize, usize)> {
    let mut positions = Vec::new();
    let mut cursor = search_start;
    for seg in old_segments {
        if seg.is_empty() {
            continue;
        }
        let mut seg_start = None;
        let mut seg_end = None;
        let mut si = 0;
        for i in cursor..=search_end {
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
            (Some(s), Some(e)) => {
                positions.push((s, e));
                cursor = e + 1;
            }
            _ => return Vec::new(),
        }
    }
    positions
}

fn match_intermediate(
    file_lines: &[&str],
    anchored: &[&str],
    (start, end): (usize, usize),
) -> bool {
    if end >= file_lines.len() || anchored.is_empty() {
        return false;
    }
    if !normalized_match(file_lines[start], anchored[0]) {
        return false;
    }
    let range_lines: Vec<&str> = file_lines[start..=end].to_vec();
    let mut ai = 0;
    for rl in &range_lines {
        if ai >= anchored.len() {
            break;
        }
        if normalized_match(rl, anchored[ai]) {
            ai += 1;
        }
    }
    ai == anchored.len()
}

fn verify_edit(path: &PathBuf, expected_content: &str, _original: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            if !content.contains(expected_content) {
                Some("new_str content not found in file after write".into())
            } else {
                None
            }
        }
        Err(e) => Some(format!("cannot re-read file for verification: {}", e)),
    }
}

fn check_missing_keys(block: &FileOpBlock) -> String {
    let mut missing = Vec::new();
    if block.path.as_os_str().is_empty() { missing.push("path"); }
    if block.mode != FileMode::Read && block.content.is_none() && block.old_str.is_none() && block.old_lines.is_none() {
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
use std::path::{Path, PathBuf};

    #[test]
    fn test_is_path_escape_detected() {
        assert!(is_path_escape(&PathBuf::from("../etc/passwd")));
        assert!(is_path_escape(&PathBuf::from("../../root/.ssh")));
    }

    #[test]
    fn test_is_path_escape_safe() {
        assert!(!is_path_escape(&PathBuf::from("src/main.rs")));
        assert!(!is_path_escape(&PathBuf::from("target/debug")));
        assert!(!is_path_escape(&PathBuf::from("n_coding.kdl")));
    }

    #[test]
    fn test_read_file_with_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let result = read_file(&path, None, None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("1: line1"));
                assert!(summary.contains("2: line2"));
                assert!(summary.contains("3: line3"));
                assert!(summary.contains("lines: 1..3"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_write_file_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        let result = write_file(&path, Some("hello"), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("action: created"));
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_write_file_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overwrite.txt");
        std::fs::write(&path, "old").unwrap();
        let result = write_file(&path, Some("new"), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("action: overwritten"));
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_write_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/nested/file.txt");
        let result = write_file(&path, Some("deep content"), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("action: created"));
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "deep content");
                assert!(dir.path().join("deep/nested").exists());
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_write_file_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let big = "x".repeat(MAX_WRITE_SIZE + 1);
        let result = write_file(&path, Some(&big), None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("size limit"));
            }
            CommandOutcome::Success { .. } => panic!("expected size limit failure"),
        }
    }

    #[test]
    fn test_write_file_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("with_backup.txt");
        let backup_dir = dir.path().join("backups");
        std::fs::write(&path, "original content").unwrap();
        let result = write_file(&path, Some("new content"), Some(&backup_dir));
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("action: overwritten"));
                assert!(backup_dir.exists());
                let mut backups = std::fs::read_dir(&backup_dir).unwrap();
                let backup_entry = backups.next().unwrap().unwrap();
                let backup_content = std::fs::read_to_string(backup_entry.path()).unwrap();
                assert_eq!(backup_content, "original content");
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_read_file_with_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lines.txt");
        std::fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();

        let result = read_file(&path, Some(3), Some(2));
        match result {
            CommandOutcome::Success { summary } => {
                assert!(!summary.contains("1: a"));
                assert!(!summary.contains("2: b"));
                assert!(summary.contains("3: c"));
                assert!(summary.contains("4: d"));
                assert!(!summary.contains("5: e"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_read_file_missing() {
        let result = read_file(&PathBuf::from("/nonexistent/file.txt"), None, None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("cannot open"));
            }
            CommandOutcome::Success { .. } => {
                panic!("expected failure for missing file");
            }
        }
    }

    #[test]
    fn test_edit_file_single_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edit_test.rs");
        std::fs::write(&path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let result = edit_file(&path, Some("fn main() {"), Some("#[tokio::main]\nasync fn main() {"), None, None, None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("status: OK"));
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("#[tokio::main]"));
                assert!(content.contains("async fn main()"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_edit_file_line_matching_function_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("src.py");
        std::fs::write(&path, "def example_function():\n    num = 10\n    if num <= 20:\n        num += 1\n    return num\n").unwrap();

        let old_lines = "def example_function():\n        ...\n        if num <= 20 :\n        ...\n        return num";
        let new_lines = "def example_function():\n        ...\n        if True:\n            pass\n        ...\n        return num";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("status: OK"));
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("if True:"));
                assert!(!content.contains("if num <= 20"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_edit_file_line_matching_signature() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lib.rs");
        std::fs::write(&path, "fn example_function() -> Option<String> {\n    todo!()\n}\n").unwrap();

        let old_lines = "fn example_function() -> Option<String> {";
        let new_lines = "fn example_function() -> Option<usize> {";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("status: OK"));
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("-> Option<usize>"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_edit_file_line_matching_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.txt");
        std::fs::write(&path, "fn foo() {\n}\n").unwrap();

        let old_lines = "fn bar() {";
        let new_lines = "fn baz() {";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("not found"));
            }
            CommandOutcome::Success { .. } => {
                panic!("expected failure");
            }
        }
    }

    #[test]
    fn test_edit_file_line_matching_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.txt");
        std::fs::write(&path, "fn a() {\n    x()\n}\nfn b() {\n}\nfn a() {\n    x()\n}\n").unwrap();

        let old_lines = "fn a() {\n    x()\n}";
        let new_lines = "fn c() {\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("ambiguous"));
            }
            CommandOutcome::Success { .. } => {
                panic!("expected ambiguous error");
            }
        }
    }

    #[test]
    fn test_edit_lines_multi_level_with_dots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.py");
        std::fs::write(&path, concat!(
            "def outer():\n",
            "    x = 1\n",
            "    def inner():\n",
            "        old_code()\n",
            "        more_old()\n",
            "    y = 2\n",
            "    return x + y\n",
        )).unwrap();

        let old_lines = "def outer():\n        ...\n        old_code()\n        more_old()\n        ...\n        return x + y";
        let new_lines = "def outer():\n        ...\n        new_code()\n        ...\n        return x + y";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("status: OK"));
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("new_code()"), "file should contain new_code:\n{}", content);
                assert!(!content.contains("old_code()"), "old_code should be removed:\n{}", content);
                assert!(!content.contains("more_old()"), "more_old should be removed:\n{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_indentation_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("indent.rs");
        std::fs::write(&path, "fn bar(&self) -> u32 {\n        42\n}\n").unwrap();

        let old_lines = "fn bar(&self) -> u32 {\n        42\n}";
        let new_lines = "fn bar(&self) -> u32\n{\nreturn 84;\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("status: OK"));
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("fn bar(&self) -> u32"), "signature updated:\n{}", content);
                assert!(content.contains("return 84;"), "return added:\n{}", content);
                assert!(!content.contains("42"), "old value removed:\n{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_new_lines_with_dots_preserves_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gaps.rs");
        std::fs::write(&path, "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    println!(\"hi\");\n}\n").unwrap();

        let old_lines = "fn main() {\n        let a = 1;\n        ...\n        let c = 3;\n        ...\n    }";
        let new_lines = "fn main() {\n        let a = 10;\n        ...\n        let c = 30;\n        ...\n    }";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("let a = 10;"), "a should change, got:\n{}", content);
                assert!(content.contains("let b = 2;"), "gap preserved:\n{}", content);
                assert!(content.contains("let c = 30;"), "c should change, got:\n{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_full_function_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full.rs");
        std::fs::write(&path, "fn old_func() {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n\nfn other() {}\n").unwrap();

        let old_lines = "fn old_func() {\n    let x = 1;\n    let y = 2;\n    x + y\n}";
        let new_lines = "fn new_func() -> i32 {\n    42\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { summary } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("fn new_func()"), "function renamed:\n{}", content);
                assert!(!content.contains("fn old_func()"), "old name removed:\n{}", content);
                assert!(content.contains("42"), "new body:\n{}", content);
                assert!(content.contains("fn other()"), "other function preserved:\n{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_possible_boundaries_in_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bound.rs");
        std::fs::write(&path, "fn a() {\n    x()\n}\nfn b() {\n    y()\n}\nfn a() {\n    z()\n}\n").unwrap();

        let old_lines = "fn a() {\n        ...\n    }";
        let new_lines = "fn c() {";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("ambiguous") || error.contains("Possible boundaries"), "should list boundaries or ambiguity, got: {}", error);
            }
            CommandOutcome::Success { .. } => panic!("expected boundary error"),
        }
    }

    #[test]
    fn test_edit_lines_missing_intermediate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing_mid.rs");
        std::fs::write(&path, "fn a() {\n    x();\n    y();\n    z();\n}\n").unwrap();

        let old_lines = "fn a() {\n    x();\n    NOT_HERE();\n    z();\n}";
        let new_lines = "fn a() {\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("no matching block"), "should report no match, got: {}", error);
            }
            CommandOutcome::Success { .. } => panic!("expected failure"),
        }
    }

    #[test]
    fn test_edit_file_line_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lines.txt");
        std::fs::write(&path, "hello\nworld\nhello\n").unwrap();
        let result = edit_file(&path, Some("world"), Some("WORLD"), None, None, None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("line 2"), "expected line 2, got: {}", summary);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_file_line_number_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trailing.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let result = edit_file(&path, Some("c"), Some("C"), None, None, None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("line 3"), "expected line 3, got: {}", summary);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_file_no_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notrail.txt");
        std::fs::write(&path, "a\nb\nc").unwrap();
        let result = edit_file(&path, Some("c"), Some("C"), None, None, None);
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("line 3"), "expected line 3, got: {}", summary);
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_file_empty_old_str() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "hello\n").unwrap();
        let result = edit_file(&path, Some(""), Some("x"), None, None, None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("old_str cannot be empty"));
            }
            CommandOutcome::Success { .. } => panic!("expected failure for empty old_str"),
        }
    }

    #[test]
    fn test_edit_file_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edit_backup.txt");
        let backup_dir = dir.path().join("edit_backups");
        std::fs::write(&path, "hello world\n").unwrap();
        let result = edit_file(&path, Some("world"), Some("WORLD"), None, None, Some(&backup_dir));
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("line 1"));
                assert!(backup_dir.exists());
                let mut backups = std::fs::read_dir(&backup_dir).unwrap();
                let backup_entry = backups.next().unwrap().unwrap();
                let backup_content = std::fs::read_to_string(backup_entry.path()).unwrap();
                assert_eq!(backup_content, "hello world\n");
            }
            CommandOutcome::Failure { error } => panic!("unexpected failure: {}", error),
        }
    }

    #[test]
    fn test_edit_file_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_match.txt");
        std::fs::write(&path, "hello world\n").unwrap();

        let result = edit_file(&path, Some("not found"), Some("replacement"), None, None, None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("old_str not found"));
            }
            CommandOutcome::Success { .. } => {
                panic!("expected failure for no match");
            }
        }
    }

    #[test]
    fn test_edit_file_multiple_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.txt");
        std::fs::write(&path, "hello\nworld\nhello\n").unwrap();

        let result = edit_file(&path, Some("hello"), Some("hi"), None, None, None);
        match result {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("matched"));
                assert!(error.contains("2"));
            }
            CommandOutcome::Success { .. } => {
                panic!("expected failure for multiple matches");
            }
        }
    }

    #[test]
    fn test_edit_lines_leading_newline_stripped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leading.py");
        std::fs::write(&path, "def foo():\n    x = 1\n\ndef bar():\n    y = 2\n").unwrap();
        let old_lines = "\ndef foo():\n    x = 1\n";
        let new_lines = "def foo():\n    x = 42\n";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("x = 42"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_file_end_no_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("end.py");
        std::fs::write(&path, concat!(
            "def a():\n    pass\n\n",
            "def b():\n    pass\n\n",
            "def c():\n    pass\n\n",
            "if __name__ == 'main':\n",
            "    c()\n",
        )).unwrap();
        let old_lines = "if __name__ == 'main':\n    c()";
        let new_lines = "if __name__ == '__main__':\n    c()";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("__main__"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_single_function_not_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mul.py");
        std::fs::write(&path, concat!(
            "def add(a, b):\n    return a + b\n\n",
            "def sub(a, b):\n    return a - b\n\n",
            "def mul(a, b):\n    return a * b\n",
        )).unwrap();
        let old_lines = "def mul(a, b):\n    return a * b";
        let new_lines = "def mul(a, b):\n    return a * c";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("return a * c"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_rust_function_not_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lib.rs");
        std::fs::write(&path, concat!(
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n",
            "pub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n\n",
            "pub fn mul(a: i32, b: i32) -> i32 {\n    a * b\n}\n",
        )).unwrap();
        let old_lines = "pub fn mul(a: i32, b: i32) -> i32 {\n    a * b\n}";
        let new_lines = "pub fn mul(a: i32, b: i32) -> i32 {\n    a * c\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("a * c"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_go_function_not_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("math.go");
        std::fs::write(&path, concat!(
            "func Add(a, b int) int {\n\treturn a + b\n}\n\n",
            "func Sub(a, b int) int {\n\treturn a - b\n}\n\n",
            "func Mul(a, b int) int {\n\treturn a * b\n}\n",
        )).unwrap();
        let old_lines = "func Mul(a, b int) int {\n\treturn a * b\n}";
        let new_lines = "func Mul(a, b int) int {\n\treturn a * c\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("a * c"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }

    #[test]
    fn test_edit_lines_js_function_not_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("math.js");
        std::fs::write(&path, concat!(
            "function add(a, b) {\n    return a + b;\n}\n\n",
            "function sub(a, b) {\n    return a - b;\n}\n\n",
            "function mul(a, b) {\n    return a * b;\n}\n",
        )).unwrap();
        let old_lines = "function mul(a, b) {\n    return a * b;\n}";
        let new_lines = "function mul(a, b) {\n    return a * c;\n}";
        let result = edit_file(&path, None, None, Some(old_lines), Some(new_lines), None);
        match result {
            CommandOutcome::Success { .. } => {
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("a * c"), "{}", content);
            }
            CommandOutcome::Failure { error } => panic!("unexpected: {}", error),
        }
    }
}
