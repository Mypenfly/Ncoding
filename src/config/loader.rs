use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub api: ApiConfig,
    #[serde(default)]
    pub thinking: ThinkingConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub tools: HashMap<String, ToolDef>,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub character: Option<CharacterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_sub_model")]
    pub sub_model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_effort")]
    pub reasoning_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_sessions_dir")]
    pub sessions_dir: String,
    #[serde(default = "default_backups_dir")]
    pub backups_dir: String,
    #[serde(default = "default_max_ctx_messages")]
    pub max_context_messages: usize,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_skills_local_dir")]
    pub local_dir: String,
    #[serde(default = "default_true")]
    pub auto_load_list: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub description: String,
    pub exec: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub deny_patterns: Vec<String>,
    #[serde(default = "default_shell_timeout")]
    pub shell_timeout_secs: u64,
    #[serde(default = "default_file_max_lines")]
    pub file_max_read_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterConfig {
    pub prompt: String,
}

fn default_base_url() -> String {
    "https://api.deepseek.com".into()
}
fn default_api_key_env() -> String {
    "DEEPSEEK_API_KEY".into()
}
fn default_model() -> String {
    "deepseek-v4-pro".into()
}
fn default_sub_model() -> String {
    "deepseek-v4-flash".into()
}
fn default_max_tokens() -> u32 {
    8192
}
fn default_temperature() -> f64 {
    1.0
}
fn default_top_p() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_effort() -> String {
    "high".into()
}
fn default_sessions_dir() -> String {
    ".ncoding/sessions".into()
}
fn default_backups_dir() -> String {
    ".ncoding/backups".into()
}
fn default_max_ctx_messages() -> usize {
    50
}
fn default_skills_local_dir() -> String {
    ".ncoding/skills".into()
}
fn default_shell_timeout() -> u64 {
    120
}
fn default_file_max_lines() -> usize {
    2000
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api: ApiConfig {
                base_url: default_base_url(),
                api_key: String::new(),
                api_key_env: default_api_key_env(),
                model: default_model(),
                sub_model: default_sub_model(),
                max_tokens: default_max_tokens(),
                temperature: default_temperature(),
                top_p: default_top_p(),
            },
            thinking: ThinkingConfig {
                enabled: true,
                reasoning_effort: default_effort(),
            },
            session: SessionConfig {
                sessions_dir: default_sessions_dir(),
                backups_dir: default_backups_dir(),
                max_context_messages: default_max_ctx_messages(),
                verbose: false,
            },
            skills: SkillsConfig {
                local_dir: default_skills_local_dir(),
                auto_load_list: true,
            },
            tools: HashMap::new(),
            safety: SafetyConfig {
                deny_patterns: vec![
                    "sudo ".into(),
                    "rm -rf /".into(),
                    "rm -rf /*".into(),
                    "chmod 777 /".into(),
                    "dd if=".into(),
                    "> /dev/sda".into(),
                ],
                shell_timeout_secs: 120,
                file_max_read_lines: 2000,
            },
            character: None,
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            api_key_env: default_api_key_env(),
            model: default_model(),
            sub_model: default_sub_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            top_p: default_top_p(),
        }
    }
}

impl Default for ThinkingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reasoning_effort: default_effort(),
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            sessions_dir: default_sessions_dir(),
            backups_dir: default_backups_dir(),
            max_context_messages: default_max_ctx_messages(),
            verbose: false,
        }
    }
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            local_dir: default_skills_local_dir(),
            auto_load_list: true,
        }
    }
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            deny_patterns: vec![
                "sudo ".into(),
                "rm -rf /".into(),
                "rm -rf /*".into(),
                "chmod 777 /".into(),
                "dd if=".into(),
                "> /dev/sda".into(),
            ],
            shell_timeout_secs: 120,
            file_max_read_lines: 2000,
        }
    }
}

pub fn load() -> Result<AppConfig, anyhow::Error> {
    let mut config = AppConfig::default();

    let global_path = dirs::config_dir()
        .map(|d| d.join("ncoding/config.kdl"))
        .unwrap_or_else(|| PathBuf::from("~/.config/ncoding/config.kdl"));

    let mut global_config = AppConfig::default();
    if global_path.exists() {
        debug!("Loading global config: {}", global_path.display());
        match fs::read_to_string(&global_path) {
            Ok(content) => {
                global_config = parse_kdl_config_robust(&content);
                tracing::info!("Global config loaded from {}", global_path.display());
            }
            Err(e) => tracing::warn!("Failed to read global config {}: {}", global_path.display(), e),
        }
    } else {
        tracing::info!("No global config at {}", global_path.display());
    }

    config.api = global_config.api.clone();

    let local_path = PathBuf::from(".ncoding/n_coding.kdl");
    let local_config = if local_path.exists() {
        debug!("Loading local config: {}", local_path.display());
        match fs::read_to_string(&local_path) {
            Ok(content) => {
                let c = parse_kdl_config_robust(&content);
                tracing::info!("Local config loaded from {}", local_path.display());
                Some(c)
            }
            Err(e) => {
                tracing::warn!("Failed to read local config {}: {}", local_path.display(), e);
                None
            }
        }
    } else {
        tracing::info!("No local config at {}", local_path.display());
        None
    };

    merge_non_api_config(&mut config, &global_config);
    if let Some(ref local) = local_config {
        merge_non_api_config(&mut config, local);
    }

    tracing::info!(
        "Config loaded: api_key={}, api_key_env={}, model={}",
        if config.api.api_key.is_empty() { "(none)" } else { "****" },
        config.api.api_key_env,
        config.api.model,
    );

    Ok(config)
}

fn merge_non_api_config(base: &mut AppConfig, overlay: &AppConfig) {
    base.thinking = overlay.thinking.clone();
    base.session = overlay.session.clone();
    base.skills = overlay.skills.clone();
    base.safety = overlay.safety.clone();
    if !overlay.tools.is_empty() {
        base.tools = overlay.tools.clone();
    }
    if let Some(ref ch) = overlay.character {
        base.character = Some(ch.clone());
    }
}

fn parse_kdl_config(content: &str) -> Result<AppConfig, anyhow::Error> {
    let doc: kdl::KdlDocument = content
        .parse()
        .map_err(|e| anyhow::anyhow!("KDL parse error: {:?}", e))?;
    let config = parse_kdl_document(&doc)?;
    Ok(config)
}

fn parse_kdl_config_robust(content: &str) -> AppConfig {
    match parse_kdl_config(content) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "Full KDL parse failed ({}), trying per-section extraction",
                e
            );
            match parse_kdl_by_sections(content) {
                Some(config) => {
                    tracing::info!("Per-section parse succeeded");
                    config
                }
                None => {
                    tracing::warn!("Per-section parse also failed, using defaults");
                    AppConfig::default()
                }
            }
        }
    }
}

fn parse_kdl_by_sections(content: &str) -> Option<AppConfig> {
    let re = regex::Regex::new(r"(?ms)^(\w+)\s*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}").ok()?;
    let mut config = AppConfig::default();

    for caps in re.captures_iter(content) {
        let section = caps.get(1)?.as_str();
        let body = caps.get(2)?.as_str();

        let section_kdl = format!("{} {{ {} }}", section, body);
        if let Ok(doc) = section_kdl.parse::<kdl::KdlDocument>() {
            if let Ok(c) = parse_kdl_document(&doc) {
                merge_config(&mut config, c);
            }
        }
    }

    Some(config)
}

fn parse_kdl_document(doc: &kdl::KdlDocument) -> Result<AppConfig, anyhow::Error> {
    let mut config = AppConfig::default();

    for node in doc.nodes() {
        let section = node.name().to_string();

        if section == "tools" {
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let tool_name = child.name().to_string();
                    let mut description = String::new();
                    let mut exec: Vec<String> = Vec::new();

                    for entry in child.entries() {
                        let name = entry.name().map(|n| n.to_string()).unwrap_or_default();
                        let val = if let Some(s) = entry.value().as_string() {
                            s.to_string()
                        } else {
                            entry.value().to_string()
                        };
                        match name.as_str() {
                            "description" => description = val,
                            "exec" => exec.push(val),
                            _ => exec.push(val),
                        }
                    }

                    if !exec.is_empty() {
                        config.tools.insert(
                            tool_name,
                            ToolDef {
                                description: description.clone(),
                                exec,
                            },
                        );
                    }
                }
            }
            continue;
        }

        for entry in node.entries() {
            if let Some(k) = entry.name() {
                let v = if let Some(s) = entry.value().as_string() {
                    s.to_string()
                } else {
                    entry.value().to_string()
                };
                apply_config_value(&mut config, &section, &k.to_string(), &v);
            }
        }

        if let Some(children) = node.children() {
            for child in children.nodes() {
                let key = child.name().to_string();

                let value = child
                    .entries()
                    .first()
                    .map(|e| {
                        if let Some(s) = e.value().as_string() {
                            s.to_string()
                        } else {
                            e.value().to_string()
                        }
                    })
                    .unwrap_or_default();

                apply_config_value(&mut config, &section, &key, &value);
            }
        }
    }

    Ok(config)
}

fn apply_config_value(config: &mut AppConfig, section: &str, key: &str, value: &str) {
    match section {
        "api" => match key {
            "base_url" => config.api.base_url = value.into(),
            "api_key" => config.api.api_key = value.into(),
            "api_key_env" => config.api.api_key_env = value.into(),
            "model" => config.api.model = value.into(),
            "sub_model" => config.api.sub_model = value.into(),
            "max_tokens" => config.api.max_tokens = value.parse().unwrap_or(8192),
            "temperature" => config.api.temperature = value.parse().unwrap_or(1.0),
            "top_p" => config.api.top_p = value.parse().unwrap_or(1.0),
            _ => {}
        },
        "thinking" => match key {
            "enabled" => config.thinking.enabled = value == "true",
            "reasoning_effort" => config.thinking.reasoning_effort = value.into(),
            _ => {}
        },
        "session" => match key {
            "sessions_dir" => config.session.sessions_dir = value.into(),
            "backups_dir" => config.session.backups_dir = value.into(),
            "max_context_messages" => {
                config.session.max_context_messages = value.parse().unwrap_or(50)
            }
            "verbose" => config.session.verbose = value == "true",
            _ => {}
        },
        "skills" => match key {
            "local_dir" => config.skills.local_dir = value.into(),
            "auto_load_list" => config.skills.auto_load_list = value == "true",
            _ => {}
        },
        "safety" => match key {
            "shell_timeout_secs" => config.safety.shell_timeout_secs = value.parse().unwrap_or(120),
            "file_max_read_lines" => {
                config.safety.file_max_read_lines = value.parse().unwrap_or(2000)
            }
            _ => {}
        },
        "character" if key == "prompt" => {
            config.character = Some(CharacterConfig {
                prompt: value.into(),
            })
        }
        _ => {}
    }
}

fn merge_config(base: &mut AppConfig, overlay: AppConfig) {
    if !overlay.api.api_key.is_empty() {
        base.api.api_key = overlay.api.api_key;
    }
    if overlay.api.api_key_env != default_api_key_env() {
        base.api.api_key_env = overlay.api.api_key_env;
    }
    if overlay.api.base_url != default_base_url() {
        base.api.base_url = overlay.api.base_url;
    }
    if overlay.api.model != default_model() {
        base.api.model = overlay.api.model;
    }
    if overlay.api.sub_model != default_sub_model() {
        base.api.sub_model = overlay.api.sub_model;
    }
    if overlay.api.max_tokens != default_max_tokens() {
        base.api.max_tokens = overlay.api.max_tokens;
    }
    if overlay.api.temperature != 1.0 {
        base.api.temperature = overlay.api.temperature;
    }
    if overlay.api.top_p != 1.0 {
        base.api.top_p = overlay.api.top_p;
    }

    base.thinking = overlay.thinking;
    base.session = overlay.session;
    base.skills = overlay.skills;
    base.safety = overlay.safety;

    if !overlay.tools.is_empty() {
        base.tools = overlay.tools;
    }

    if let Some(ref ch) = overlay.character {
        base.character = Some(ch.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = AppConfig::default();
        assert_eq!(config.api.model, "deepseek-v4-pro");
        assert_eq!(config.api.sub_model, "deepseek-v4-flash");
        assert_eq!(config.api.max_tokens, 8192);
        assert_eq!(config.api.temperature, 1.0);
        assert_eq!(config.thinking.enabled, true);
        assert_eq!(config.thinking.reasoning_effort, "high");
        assert_eq!(config.session.max_context_messages, 50);
        assert_eq!(config.safety.shell_timeout_secs, 120);
        assert_eq!(config.safety.file_max_read_lines, 2000);
    }

    #[test]
    fn test_parse_kdl_api_config() {
        let kdl =
            "api {\n    model \"deepseek-v4-flash\"\n    max_tokens 4096\n    temperature 0.7\n}\n";
        let config = parse_kdl_config(kdl).unwrap();
        assert_eq!(config.api.model, "deepseek-v4-flash");
        assert_eq!(config.api.max_tokens, 4096);
        assert_eq!(config.api.temperature, 0.7);
        assert_eq!(config.api.base_url, "https://api.deepseek.com");
        assert!(config.api.api_key.is_empty());
    }

    #[test]
    fn test_parse_kdl_api_key() {
        let kdl = "api {\n    api_key \"sk-test-key-12345\"\n}\n";
        let config = parse_kdl_config(kdl).unwrap();
        assert_eq!(config.api.api_key, "sk-test-key-12345");
    }

    #[test]
    fn test_parse_kdl_session_config() {
        let kdl = "session {\n    sessions_dir \".ncoding/sessions\"\n    max_context_messages 30\n    verbose \"true\"\n}\n";
        let config = parse_kdl_config(kdl).unwrap();
        assert_eq!(config.session.sessions_dir, ".ncoding/sessions");
        assert_eq!(config.session.max_context_messages, 30);
        assert_eq!(config.session.verbose, true);
    }

    #[test]
    fn test_parse_kdl_boolean_values() {
        let kdl = "thinking {\n    enabled \"true\"\n    reasoning_effort \"high\"\n}\n";
        let config = parse_kdl_config(kdl).unwrap();
        assert_eq!(config.thinking.enabled, true);
        assert_eq!(config.thinking.reasoning_effort, "high");
    }

    #[test]
    fn test_parse_kdl_safety_config() {
        let kdl = r#"
safety {
    shell_timeout_secs 60
    file_max_read_lines 500
}
"#;
        let config = parse_kdl_config(kdl).unwrap();
        assert_eq!(config.safety.shell_timeout_secs, 60);
        assert_eq!(config.safety.file_max_read_lines, 500);
    }

    #[test]
    fn test_parse_empty_kdl_returns_defaults() {
        let config = parse_kdl_config("").unwrap();
        let defaults = AppConfig::default();
        assert_eq!(config.api.model, defaults.api.model);
        assert_eq!(
            config.session.max_context_messages,
            defaults.session.max_context_messages
        );
        assert_eq!(
            config.safety.shell_timeout_secs,
            defaults.safety.shell_timeout_secs
        );
    }
}
