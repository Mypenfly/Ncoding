use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::syntax::{CommandOutcome, CommandResult, CommandType, FileMode, FileOpBlock};

pub async fn execute(blocks: Vec<FileOpBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        let base_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

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

        let full_path = base_path.join(&block.path);
        let result = match block.mode {
            FileMode::Read => read_file(&full_path, block.offset, block.limit),
            FileMode::Write => write_file(&full_path, block.content.as_deref()),
            FileMode::Edit => edit_file(
                &full_path,
                block.old_str.as_deref(),
                block.new_str.as_deref(),
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
        "mode: read\npath: {}\nlines: {}{}",
        path.display(),
        lines_range.unwrap_or_else(|| "N/A".into()),
        if read_end > 0 { "\n".to_string() + &output } else { String::new() }
    );

    CommandOutcome::Success { summary }
}

fn write_file(path: &PathBuf, content: Option<&str>) -> CommandOutcome {
    let content = match content {
        Some(c) => c,
        None => return CommandOutcome::Failure {
            error: "write mode requires content parameter".into(),
        },
    };

    if path.exists() {
        info!("Overwriting existing file: {}", path.display());
    }

    match fs::write(path, content) {
        Ok(()) => CommandOutcome::Success {
            summary: format!(
                "mode: write\npath: {}\nresult: {}",
                path.display(),
                if path.exists() { "overwritten" } else { "created" }
            ),
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
) -> CommandOutcome {
    let (old_str, new_str) = match (old_str, new_str) {
        (Some(o), Some(n)) => (o, n),
        _ => return CommandOutcome::Failure {
            error: "edit mode requires old_str and new_str parameters".into(),
        },
    };

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
            let mut new_content = String::with_capacity(file_content.len());
            new_content.push_str(&file_content[..pos]);
            new_content.push_str(new_str);
            new_content.push_str(&file_content[pos + old_str.len()..]);

            let start_line = file_content[..pos].lines().count() + 1;

            match fs::write(path, &new_content) {
                Ok(()) => CommandOutcome::Success {
                    summary: format!(
                        "mode: edit\npath: {}\nresult: success (1 replacement at line {})",
                        path.display(),
                        start_line
                    ),
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
                    let line = file_content[..*pos].lines().count() + 1;
                    line.to_string()
                })
                .collect();
            CommandOutcome::Failure {
                error: format!(
                    "edit failed: old_str matched {} locations (lines {}). \
                     Provide more context to make old_str unique.",
                    n,
                    locations.join(", ")
                ),
            }
        }
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

        let result = edit_file(&path, Some("fn main() {"), Some("#[tokio::main]\nasync fn main() {"));
        match result {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("success"));
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
    fn test_edit_file_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_match.txt");
        std::fs::write(&path, "hello world\n").unwrap();

        let result = edit_file(&path, Some("not found"), Some("replacement"));
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

        let result = edit_file(&path, Some("hello"), Some("hi"));
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
}
