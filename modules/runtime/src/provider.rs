use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use reqwest::Client;
use serde_json::{Value, json};

use ccodex_protocol::{
    ItemPayload, ModelProviderPort, PortError, ProviderEvent, ToolSpec, TurnRequest,
};

use crate::config::{ProviderConfig, ProviderKind};
use crate::provider_bootstrap::BootstrapModelProvider;
use crate::provider_parse::{parse_anthropic_events, parse_openai_events};

#[derive(Debug, Default)]
pub struct EchoModelProvider;

#[async_trait]
impl ModelProviderPort for EchoModelProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        let prompt = request.instructions.trim();
        let content = if prompt.is_empty() {
            "No prompt provided.".to_string()
        } else {
            format!("Echo: {prompt}")
        };

        Ok(Box::pin(stream::iter(vec![
            Ok(ProviderEvent::AssistantMessageDelta { content }),
            Ok(ProviderEvent::Completed),
        ])))
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    client: Client,
    config: ProviderConfig,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }
}

#[async_trait]
impl ModelProviderPort for OpenAiCompatibleProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        let base_url = self.config.base_url.as_deref().ok_or_else(|| {
            PortError::Provider("missing base URL for OpenAI-compatible provider".to_string())
        })?;
        let url = join_endpoint(base_url, "chat/completions");

        let messages = build_openai_messages(&request);
        let tools = build_openai_tools(&request.available_tools);
        let mut payload = json!({
            "model": self.config.model,
            "messages": messages,
            "stream": false,
            "max_completion_tokens": self.config.max_output_tokens,
        });
        if !tools.is_empty() {
            payload["tools"] = Value::Array(tools);
        }

        let mut request_builder = self.client.post(&url).json(&payload);
        match self.config.kind {
            ProviderKind::LocalCompatible => {}
            ProviderKind::OpenAiCompatible | ProviderKind::XaiCompatible => {
                let api_key = self.config.api_key.as_deref().ok_or_else(|| {
                    PortError::Provider(format!(
                        "missing API key for {:?} provider",
                        self.config.kind
                    ))
                })?;
                request_builder = request_builder.bearer_auth(api_key);
            }
            _ => {}
        }
        let response = request_builder.send().await.map_err(|err| {
            PortError::Provider(format!("openai-compatible request failed: {err}"))
        })?;

        let status = response.status();
        let body: Value = response.json().await.map_err(|err| {
            PortError::Provider(format!(
                "failed to decode openai-compatible response: {err}"
            ))
        })?;

        if !status.is_success() {
            return Err(PortError::Provider(format!(
                "openai-compatible provider returned {}: {}",
                status,
                compact_json(&body)
            )));
        }

        Ok(Box::pin(stream::iter(parse_openai_events(&body)?)))
    }
}

#[derive(Debug, Clone)]
pub struct AnthropicCompatibleProvider {
    client: Client,
    config: ProviderConfig,
}

impl AnthropicCompatibleProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }
}

#[async_trait]
impl ModelProviderPort for AnthropicCompatibleProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        let base_url = self.config.base_url.as_deref().ok_or_else(|| {
            PortError::Provider("missing base URL for Anthropic-compatible provider".to_string())
        })?;
        let api_key = self.config.api_key.as_deref().ok_or_else(|| {
            PortError::Provider("missing ANTHROPIC_AUTH_TOKEN/ANTHROPIC_API_KEY".to_string())
        })?;
        let url = join_endpoint(base_url, "v1/messages");

        let messages = build_anthropic_messages(&request);
        let tools = build_anthropic_tools(&request.available_tools);
        let mut payload = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_output_tokens,
            "messages": messages,
            "stream": false
        });
        if !request.project_instructions.is_empty() {
            payload["system"] = Value::String(request.project_instructions.join("\n\n"));
        }
        if !tools.is_empty() {
            payload["tools"] = Value::Array(tools);
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                PortError::Provider(format!("anthropic-compatible request failed: {err}"))
            })?;

        let status = response.status();
        let body: Value = response.json().await.map_err(|err| {
            PortError::Provider(format!(
                "failed to decode anthropic-compatible response: {err}"
            ))
        })?;

        if !status.is_success() {
            return Err(PortError::Provider(format!(
                "anthropic-compatible provider returned {}: {}",
                status,
                compact_json(&body)
            )));
        }

        Ok(Box::pin(stream::iter(parse_anthropic_events(&body)?)))
    }
}

pub fn provider_from_config(config: &ProviderConfig) -> Box<dyn ModelProviderPort> {
    match config.kind {
        ProviderKind::Bootstrap => Box::new(BootstrapModelProvider),
        ProviderKind::Echo => Box::new(EchoModelProvider),
        ProviderKind::OpenAiCompatible
        | ProviderKind::XaiCompatible
        | ProviderKind::LocalCompatible => Box::new(OpenAiCompatibleProvider::new(config.clone())),
        ProviderKind::AnthropicCompatible => {
            Box::new(AnthropicCompatibleProvider::new(config.clone()))
        }
    }
}

fn build_openai_messages(request: &TurnRequest) -> Vec<Value> {
    let mut messages = Vec::new();
    if !request.project_instructions.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": request.project_instructions.join("\n\n"),
        }));
    }

    for item in &request.items {
        match &item.payload {
            ItemPayload::UserMessage { content } => {
                messages.push(json!({ "role": "user", "content": content }));
            }
            ItemPayload::AssistantMessageDelta { content } => {
                if let Some(existing) = messages.last_mut().filter(|m| {
                    m.get("role") == Some(&Value::String("assistant".to_string()))
                        && m.get("tool_calls").is_none()
                }) {
                    let previous = existing
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    existing["content"] = Value::String(format!("{previous}{content}"));
                } else {
                    messages.push(json!({ "role": "assistant", "content": content }));
                }
            }
            ItemPayload::ToolCallStarted { call } => {
                messages.push(json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": [{
                        "id": call.id.0,
                        "type": "function",
                        "function": {
                            "name": call.tool_name,
                            "arguments": compact_json(&call.input)
                        }
                    }]
                }));
            }
            ItemPayload::ToolCallFinished { result } => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": result.tool_call_id.0,
                    "content": compact_json(&result.output),
                }));
            }
            _ => {}
        }
    }

    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": request.instructions,
        }));
    }

    messages
}

fn build_anthropic_messages(request: &TurnRequest) -> Vec<Value> {
    let mut messages = Vec::new();

    for item in &request.items {
        match &item.payload {
            ItemPayload::UserMessage { content } => {
                messages.push(json!({
                    "role": "user",
                    "content": [{ "type": "text", "text": content }]
                }));
            }
            ItemPayload::AssistantMessageDelta { content } => {
                if let Some(existing) = messages
                    .last_mut()
                    .filter(|m| m.get("role") == Some(&Value::String("assistant".to_string())))
                {
                    let array = existing["content"]
                        .as_array_mut()
                        .expect("assistant content should be array");
                    if let Some(last) = array.last_mut().filter(|entry| {
                        entry.get("type") == Some(&Value::String("text".to_string()))
                    }) {
                        let previous = last
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        last["text"] = Value::String(format!("{previous}{content}"));
                    } else {
                        array.push(json!({ "type": "text", "text": content }));
                    }
                } else {
                    messages.push(json!({
                        "role": "assistant",
                        "content": [{ "type": "text", "text": content }]
                    }));
                }
            }
            ItemPayload::ToolCallStarted { call } => {
                messages.push(json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": call.id.0,
                        "name": call.tool_name,
                        "input": call.input
                    }]
                }));
            }
            ItemPayload::ToolCallFinished { result } => {
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": result.tool_call_id.0,
                        "content": compact_json(&result.output),
                        "is_error": result.is_error
                    }]
                }));
            }
            _ => {}
        }
    }

    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": request.instructions }]
        }));
    }

    messages
}

fn build_openai_tools(specs: &[ToolSpec]) -> Vec<Value> {
    specs
        .iter()
        .map(|spec| {
            json!({
                "type": "function",
                "function": {
                    "name": spec.name,
                    "description": spec.description,
                    "parameters": spec.input_schema,
                }
            })
        })
        .collect()
}

fn build_anthropic_tools(specs: &[ToolSpec]) -> Vec<Value> {
    specs
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "description": spec.description,
                "input_schema": spec.input_schema,
            })
        })
        .collect()
}

fn join_endpoint(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with(suffix) {
        base.to_string()
    } else {
        format!("{base}/{suffix}")
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_openai_messages, join_endpoint};
    use ccodex_protocol::{
        Item, ItemId, ItemPayload, Session, SessionId, SessionStatus, ToolCall, ToolCallId,
        ToolResult, Turn, TurnId, TurnRequest, TurnStatus,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use time::OffsetDateTime;

    #[test]
    fn join_endpoint_handles_trailing_slashes_and_existing_suffix() {
        assert_eq!(
            join_endpoint("https://example.com/v2/coding", "chat/completions"),
            "https://example.com/v2/coding/chat/completions"
        );
        assert_eq!(
            join_endpoint("https://example.com/v2/coding/", "chat/completions"),
            "https://example.com/v2/coding/chat/completions"
        );
        assert_eq!(
            join_endpoint(
                "https://example.com/v2/coding/chat/completions",
                "chat/completions"
            ),
            "https://example.com/v2/coding/chat/completions"
        );
    }

    #[test]
    fn build_openai_messages_keeps_tool_roundtrip_order() {
        let now = OffsetDateTime::now_utc();
        let tool_id = ToolCallId::new();
        let request = TurnRequest {
            session: Session {
                id: SessionId::new(),
                title: None,
                workspace_root: None,
                created_at: now,
                updated_at: now,
                status: SessionStatus::Active,
                active_plan: None,
                metadata: BTreeMap::new(),
            },
            turn: Turn {
                id: TurnId::new(),
                session_id: SessionId::new(),
                item_ids: Vec::new(),
                started_at: now,
                completed_at: None,
                status: TurnStatus::Running,
            },
            instructions: "read Cargo.toml".to_string(),
            project_instructions: vec!["system".to_string()],
            items: vec![
                Item {
                    id: ItemId::new(),
                    turn_id: TurnId::new(),
                    created_at: now,
                    payload: ItemPayload::UserMessage {
                        content: "read Cargo.toml".to_string(),
                    },
                },
                Item {
                    id: ItemId::new(),
                    turn_id: TurnId::new(),
                    created_at: now,
                    payload: ItemPayload::ToolCallStarted {
                        call: ToolCall {
                            id: tool_id.clone(),
                            tool_name: "read_file".to_string(),
                            input: json!({"path":"Cargo.toml"}),
                        },
                    },
                },
                Item {
                    id: ItemId::new(),
                    turn_id: TurnId::new(),
                    created_at: now,
                    payload: ItemPayload::ToolCallFinished {
                        result: ToolResult {
                            tool_call_id: tool_id,
                            output: json!({"path":"Cargo.toml","content":"[workspace]"}),
                            is_error: false,
                        },
                    },
                },
            ],
            available_tools: Vec::new(),
        };

        let messages = build_openai_messages(&request);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[3]["role"], "tool");
    }
}
