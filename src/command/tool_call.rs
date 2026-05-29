#![allow(dead_code, unused_imports)]

use std::process::Command;
use tracing::info;

use super::syntax::{CommandOutcome, CommandResult, CommandType, ToolCallBlock};

pub async fn execute(blocks: Vec<ToolCallBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        let tool_name = block.tool_name.clone();
        let args_json = serde_json::to_string(&block.args).unwrap_or_else(|_| "{}".into());

        info!("Executing external tool: {} with args: {}", tool_name, args_json);

        let result = tokio::task::spawn_blocking(move || {
            // Tool execution will look up the tool definition from config
            // and run the associated command, passing args_json as an argument.
            // For now, this is a placeholder.
            let _output = Command::new("echo")
                .arg(format!(
                    "ToolCall placeholder: tool={} args={}",
                    tool_name, args_json
                ))
                .output();

            CommandResult {
                command_type: CommandType::ToolCall,
                block_index: i,
                outcome: CommandOutcome::Success {
                    summary: format!(
                        "ToolCall placeholder executed: tool={}",
                        tool_name
                    ),
                },
            }
        })
        .await
        .unwrap_or_else(|e| CommandResult {
            command_type: CommandType::ToolCall,
            block_index: i,
            outcome: CommandOutcome::Failure {
                error: format!("ToolCall spawn error: {}", e),
            },
        });

        results.push(result);
    }

    Ok(results)
}
