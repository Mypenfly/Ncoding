#![allow(dead_code)]

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::command::syntax::CommandResult;
use crate::session::manager::Message;

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    base_url: String,
    api_key: String,
    model: String,
    http: Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ApiMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub temperature: f64,
    pub top_p: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
    pub reasoning_effort: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageInfo {
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum TuiEvent {
    ReasoningChunk(String),
    ContentChunk(String),
    StreamDone { usage: Option<UsageInfo> },
    CommandsCompleted { results: Vec<CommandResult> },
    SessionRenamed(String),
    Error(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub owner: String,
}

impl DeepSeekClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            http: Client::new(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn stream_chat(
        &self,
        messages: Vec<Message>,
        thinking_type: &str,
        reasoning_effort: &str,
        max_tokens: u32,
        temperature: f64,
        top_p: f64,
        tui_tx: mpsc::UnboundedSender<TuiEvent>,
        cmd_tx: mpsc::UnboundedSender<String>,
    ) -> Result<Option<UsageInfo>, anyhow::Error> {
        let api_messages: Vec<ApiMessage> = messages
            .into_iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    crate::session::manager::Role::System => "system".into(),
                    crate::session::manager::Role::User => "user".into(),
                    crate::session::manager::Role::Assistant => "assistant".into(),
                    crate::session::manager::Role::Info => "user".into(),
                },
                reasoning_content: m.reasoning_content,
                content: m.content,
            })
            .collect();

        let request = ChatRequest {
            model: self.model.clone(),
            messages: api_messages,
            stream: true,
            thinking: Some(ThinkingConfig {
                thinking_type: thinking_type.to_string(),
                reasoning_effort: reasoning_effort.to_string(),
            }),
            max_tokens: Some(max_tokens),
            temperature,
            top_p,
        };

        info!(
            "Streaming chat: model={}, {} messages",
            self.model,
            request.messages.len()
        );

        let resp = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let err_msg = format!("API error {}: {}", status, body);
            error!("{}", err_msg);
            let _ = tui_tx.send(TuiEvent::Error(err_msg));
            return Ok(None);
        }

        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut usage_info: Option<UsageInfo> = None;

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(bytes_ref) => {
                    let text = String::from_utf8_lossy(&bytes_ref);
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() || line == "data: [DONE]" {
                            continue;
                        }
                        if let Some(data) = line.strip_prefix("data: ") {
                            match serde_json::from_str::<StreamChunk>(data) {
                                Ok(chunk) => {
                                    if let Some(ref usage) = chunk.usage {
                                        usage_info = Some(usage.clone());
                                    }

                                    for choice in &chunk.choices {
                                        if let Some(ref rc) = choice.delta.reasoning_content {
                                            if !rc.is_empty() {
                                                let _ = tui_tx
                                                    .send(TuiEvent::ReasoningChunk(rc.clone()));
                                            }
                                        }
                                        if let Some(ref c) = choice.delta.content {
                                            if !c.is_empty() {
                                                buffer.push_str(c);
                                                let _ = cmd_tx.send(c.clone());
                                                let _ =
                                                    tui_tx.send(TuiEvent::ContentChunk(c.clone()));
                                            }
                                        }

                                        if choice.finish_reason.as_deref() == Some("stop") {
                                            debug!(
                                                "Stream finished, total content length: {}",
                                                buffer.len()
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    debug!("Failed to parse SSE line: {} — data: {}", e, data);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    let _ = tui_tx.send(TuiEvent::Error(format!("stream error: {}", e)));
                    break;
                }
            }
        }

        let _ = tui_tx.send(TuiEvent::StreamDone {
            usage: usage_info.clone(),
        });

        Ok(usage_info)
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, anyhow::Error> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Failed to list models: {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct ModelListResponse {
            data: Vec<ModelInfo>,
        }

        let body: ModelListResponse = resp.json().await?;
        Ok(body.data)
    }

    pub async fn generate_session_name(
        &self,
        user_input: &str,
    ) -> Result<String, anyhow::Error> {
        let prompt = format!(
            "根据给出的用户输入，猜测用户意图，生成一个session标题，直接给出结果，不要超过15个字：\n{}",
            user_input
        );

        let messages = vec![ApiMessage {
            role: "user".into(),
            reasoning_content: None,
            content: Some(prompt),
        }];

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: false,
            thinking: Some(ThinkingConfig {
                thinking_type: "disabled".into(),
                reasoning_effort: "low".into(),
            }),
            max_tokens: Some(64),
            temperature: 0.3,
            top_p: 1.0,
        };

        let resp = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Name generation failed: {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct NonStreamResp {
            choices: Vec<NonStreamChoice>,
        }
        #[derive(Deserialize)]
        struct NonStreamChoice {
            message: StreamDelta,
        }

        let body: NonStreamResp = resp.json().await?;
        let name = body
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("new-session")
            .trim()
            .chars()
            .filter(|c| !c.is_control() && *c != '/' && *c != '\\' && *c != '\0')
            .take(50)
            .collect::<String>()
            .trim()
            .to_string();

        if name.is_empty() {
            Ok("new-session".into())
        } else {
            Ok(name)
        }
    }
}

impl From<&crate::session::manager::Message> for ApiMessage {
    fn from(m: &crate::session::manager::Message) -> Self {
        Self {
            role: match m.role {
                crate::session::manager::Role::System => "system".into(),
                crate::session::manager::Role::User => "user".into(),
                crate::session::manager::Role::Assistant => "assistant".into(),
                crate::session::manager::Role::Info => "user".into(),
            },
            reasoning_content: m.reasoning_content.clone(),
            content: m.content.clone(),
        }
    }
}
