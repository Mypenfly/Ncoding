#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};
use tracing::{info, warn};

use super::syntax::{CommandOutcome, CommandResult, CommandType, SkillsBlock, SkillsMode};

pub async fn execute(blocks: Vec<SkillsBlock>) -> Result<Vec<CommandResult>, anyhow::Error> {
    let mut results = Vec::new();

    let skills_dirs = vec![
        PathBuf::from(".ncoding/skills"),
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("ncoding/skills"),
    ];

    for (i, block) in blocks.into_iter().enumerate() {
        match block.mode {
            SkillsMode::List => {
                let mut summary = String::from("mode: list\n可用 Skills:\n");
                for dir in &skills_dirs {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    let desc = read_skill_description(&path);
                                    summary.push_str(&format!("  - {}: {}\n", name, desc));
                                }
                            }
                        }
                    }
                }
                results.push(CommandResult {
                    command_type: CommandType::AgentSkills,
                    block_index: i,
                    outcome: CommandOutcome::Success { summary },
                });
            }
            SkillsMode::Load => {
                let skill_name = block.skill_name.as_deref().unwrap_or("");
                let mut loaded = false;
                let mut content = String::new();

                for dir in &skills_dirs {
                    let skill_dir = dir.join(skill_name);
                    let skill_md = skill_dir.join("SKILL.md");
                    if skill_md.exists() {
                        match std::fs::read_to_string(&skill_md) {
                            Ok(c) => {
                                content = c;
                                loaded = true;
                                break;
                            }
                            Err(e) => {
                                warn!("Failed to read skill {} at {}: {}", skill_name, skill_md.display(), e);
                            }
                        }
                    }
                }

                if loaded {
                    results.push(CommandResult {
                        command_type: CommandType::AgentSkills,
                        block_index: i,
                        outcome: CommandOutcome::Success {
                            summary: format!(
                                "mode: load\nskill: {}\ncontent:\n{}",
                                skill_name, content
                            ),
                        },
                    });
                } else {
                    results.push(CommandResult {
                        command_type: CommandType::AgentSkills,
                        block_index: i,
                        outcome: CommandOutcome::Failure {
                            error: format!("skill not found: {}", skill_name),
                        },
                    });
                }
            }
        }
    }

    Ok(results)
}

fn read_skill_description(skill_dir: &Path) -> String {
    let desc_path = skill_dir.join("description.txt");
    if let Ok(desc) = std::fs::read_to_string(&desc_path) {
        desc.trim().to_string()
    } else {
        let skill_md = skill_dir.join("SKILL.md");
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            let first_line = content.lines().next().unwrap_or("");
            first_line.trim_start_matches('#').trim().to_string()
        } else {
            "(no description)".into()
        }
    }
}
