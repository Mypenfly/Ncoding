#![allow(dead_code, unused_imports)]

use tracing::{debug, info};

use super::syntax::{CommandOutcome, CommandResult, CommandType, SubAgentBlock};

pub async fn execute(blocks: Vec<SubAgentBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        info!("Dispatching sub-agent task: {}", &block.prompt[..block.prompt.len().min(80)]);

        results.push(CommandResult {
            command_type: CommandType::SubAgentTask,
            block_index: i,
            outcome: CommandOutcome::Success {
                summary: format!(
                    "[SubAgentResult]\ntask: {}\nresult: subagent execution placeholder",
                    block.prompt
                ),
            },
        });
    }

    Ok(results)
}
