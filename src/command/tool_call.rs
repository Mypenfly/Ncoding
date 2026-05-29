use std::process::Command;

use super::syntax::{CommandOutcome, CommandResult, CommandType, ToolCallBlock};

pub async fn execute(blocks: Vec<ToolCallBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let tools = load_tool_defs();
    execute_with_tools(blocks, &tools).await
}

pub async fn execute_with_tools(
    blocks: Vec<ToolCallBlock>,
    tools: &std::collections::HashMap<String, crate::config::loader::ToolDef>,
) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        let tool_name = block.tool_name.clone();
        let args_json =
            serde_json::to_string(&block.args).unwrap_or_else(|_| "{}".into());

        let tool = match tools.get(&tool_name) {
            Some(t) => t.clone(),
            None => {
                results.push(CommandResult {
                    command_type: CommandType::ToolCall,
                    block_index: i,
                    outcome: CommandOutcome::Failure {
                        error: format!(
                            "unknown tool: {}. Available: {}",
                            tool_name,
                            tools.keys().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                        ),
                    },
                });
                continue;
            }
        };

        let exec = tool.exec.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            if exec.is_empty() {
                return CommandOutcome::Failure {
                    error: format!("no exec command for tool '{}'", tool_name),
                };
            }

            let cmd_name = &exec[0];
            let cmd_args = &exec[1..];

            let output = Command::new(cmd_name)
                .args(cmd_args)
                .arg(&args_json)
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let mut summary = format!(
                        "tool: {}\nexit_code: {}\n",
                        tool_name,
                        out.status.code().unwrap_or(-1),
                    );
                    if !stdout.is_empty() {
                        if stdout.lines().count() > 100 {
                            let lines: Vec<&str> = stdout.lines().collect();
                            summary.push_str(&format!(
                                "stdout (trimmed, {} total):\n{}\n...\n{}",
                                lines.len(),
                                lines[..50].join("\n"),
                                lines[lines.len() - 50..].join("\n"),
                            ));
                        } else {
                            summary.push_str(&format!("stdout:\n{}", stdout));
                        }
                    }
                    if !stderr.is_empty() {
                        summary.push_str(&format!("\nstderr:\n{}", stderr));
                    }
                    CommandOutcome::Success { summary }
                }
                Err(e) => CommandOutcome::Failure {
                    error: format!("failed to execute tool '{}': {}", tool_name, e),
                },
            }
        })
        .await
        .unwrap_or_else(|e| CommandOutcome::Failure {
            error: format!("ToolCall spawn error: {}", e),
        });

        results.push(CommandResult {
            command_type: CommandType::ToolCall,
            block_index: i,
            outcome,
        });
    }

    Ok(results)
}

fn load_tool_defs() -> std::collections::HashMap<String, crate::config::loader::ToolDef> {
    let config_paths = [
        std::path::PathBuf::from(".ncoding/n_coding.kdl"),
        dirs::config_dir()
            .unwrap_or_else(|| "~/.config".into())
            .join("ncoding/config.kdl"),
    ];

    let mut tools = std::collections::HashMap::new();

    for path in &config_paths {
        if !path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        let Ok(doc) = content.parse::<kdl::KdlDocument>() else { continue };

        for node in doc.nodes() {
            if node.name().to_string() != "tools" {
                continue;
            }
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let tool_name = child.name().to_string();
                    let mut desc = String::new();
                    let mut exec: Vec<String> = Vec::new();

                    for entry in child.entries() {
                        let name = entry.name().map(|n| n.to_string()).unwrap_or_default();
                        let val = match entry.value().as_string() {
                            Some(s) => s.to_string(),
                            None => entry.value().to_string(),
                        };
                        match name.as_str() {
                            "description" => desc = val,
                            "exec" => exec.push(val),
                            _ => exec.push(val),
                        }
                    }

                    if !exec.is_empty() {
                        tools.insert(tool_name, crate::config::loader::ToolDef {
                            description: desc,
                            exec,
                        });
                    }
                }
            }
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::syntax::ToolCallBlock;
    use std::collections::HashMap;

    #[test]
    fn test_execute_tool_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let block = ToolCallBlock {
            tool_name: "nonexistent".into(),
            args: HashMap::new(),
        };
        let results = rt.block_on(execute(vec![block])).unwrap();
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            CommandOutcome::Failure { error } => {
                assert!(error.contains("unknown tool"));
            }
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn test_execute_tool_with_args() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut tools = HashMap::new();
        tools.insert(
            "echo_test".into(),
            crate::config::loader::ToolDef {
                description: "test echo".into(),
                exec: vec!["echo".into()],
            },
        );

        let mut args = HashMap::new();
        args.insert("query".into(), "hello world".into());
        args.insert("count".into(), "3".into());

        let block = ToolCallBlock {
            tool_name: "echo_test".into(),
            args,
        };

        let results = rt.block_on(execute_with_tools(vec![block], &tools)).unwrap();
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("hello world"));
                assert!(summary.contains("count"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_execute_tool_python() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let tool_path = dir.path().join("test_tool.py");
        std::fs::write(
            &tool_path,
            r#"
import sys, json
args = json.loads(sys.argv[1])
print(f"query={args.get('query','')} count={args.get('count','')}")
"#,
        )
        .unwrap();

        let mut tools = HashMap::new();
        tools.insert(
            "py_test".into(),
            crate::config::loader::ToolDef {
                description: "python test tool".into(),
                exec: vec!["python3".into(), tool_path.to_string_lossy().to_string()],
            },
        );

        let mut args = HashMap::new();
        args.insert("query".into(), "hello world".into());
        args.insert("count".into(), "3".into());

        let block = ToolCallBlock {
            tool_name: "py_test".into(),
            args,
        };

        let results = rt.block_on(execute_with_tools(vec![block], &tools)).unwrap();
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            CommandOutcome::Success { summary } => {
                assert!(summary.contains("query=hello world"));
                assert!(summary.contains("count=3"));
            }
            CommandOutcome::Failure { error } => {
                panic!("unexpected failure: {}", error);
            }
        }
    }

    #[test]
    fn test_parallel_tool_calls_independent() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let tool_path = dir.path().join("tool.py");
        std::fs::write(
            &tool_path,
            r#"
import sys, json
args = json.loads(sys.argv[1])
# Output a unique marker based on args to prove independence
print(f"MARKER:{args.get('marker','none')}")
"#,
        )
        .unwrap();

        let mut tools = HashMap::new();
        tools.insert(
            "parallel_test".into(),
            crate::config::loader::ToolDef {
                description: "parallel test".into(),
                exec: vec!["python3".into(), tool_path.to_string_lossy().to_string()],
            },
        );

        let mut args1 = HashMap::new();
        args1.insert("marker".into(), "AAA".into());
        let mut args2 = HashMap::new();
        args2.insert("marker".into(), "BBB".into());
        let mut args3 = HashMap::new();
        args3.insert("marker".into(), "CCC".into());

        let blocks = vec![
            ToolCallBlock { tool_name: "parallel_test".into(), args: args1 },
            ToolCallBlock { tool_name: "parallel_test".into(), args: args2 },
            ToolCallBlock { tool_name: "parallel_test".into(), args: args3 },
        ];

        let results = rt.block_on(execute_with_tools(blocks, &tools)).unwrap();
        assert_eq!(results.len(), 3);

        let markers: Vec<String> = results.iter().filter_map(|r| {
            match &r.outcome {
                CommandOutcome::Success { summary } => {
                    summary.lines()
                        .find(|l| l.starts_with("MARKER:"))
                        .map(|l| l.to_string())
                }
                _ => None,
            }
        }).collect();

        assert_eq!(markers.len(), 3);
        assert!(markers.iter().any(|m| m.contains("AAA")));
        assert!(markers.iter().any(|m| m.contains("BBB")));
        assert!(markers.iter().any(|m| m.contains("CCC")));
    }
}
