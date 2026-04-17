use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use serde_json::json;

use ccodex_protocol::{ModelProviderPort, PortError, ProviderEvent, ToolCall, ToolCallId, TurnRequest};

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

#[derive(Debug, Default)]
pub struct BootstrapModelProvider;

#[async_trait]
impl ModelProviderPort for BootstrapModelProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        let prompt = request.instructions.trim();
        let mut events = Vec::new();

        if let Some(path) = prompt.strip_prefix("read ").map(str::trim).filter(|s| !s.is_empty()) {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "read_file".to_string(),
                input: json!({ "path": path }),
            })));
        } else if let Some(command) = prompt.strip_prefix("bash ").map(str::trim).filter(|s| !s.is_empty()) {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "bash".to_string(),
                input: json!({ "command": command }),
            })));
        } else if let Some(rest) = prompt.strip_prefix("ask ").map(str::trim).filter(|s| !s.is_empty()) {
            let mut parts = rest.splitn(2, '|');
            let title = parts.next().unwrap_or("Question").trim();
            let raw_choices = parts.next().unwrap_or("yes, no");
            let choices = raw_choices
                .split(',')
                .map(str::trim)
                .filter(|choice| !choice.is_empty())
                .enumerate()
                .map(|(idx, label)| {
                    json!({
                        "id": format!("choice-{}", idx + 1),
                        "label": label,
                        "description": null
                    })
                })
                .collect::<Vec<_>>();

            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "ask_user".to_string(),
                input: json!({
                    "title": title,
                    "message": title,
                    "choices": choices,
                    "allow_freeform": false
                }),
            })));
        } else if let Some(rest) = prompt.strip_prefix("plan ").map(str::trim).filter(|s| !s.is_empty()) {
            let items = rest
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .enumerate()
                .map(|(idx, title)| {
                    json!({
                        "id": format!("item-{}", idx + 1),
                        "title": title,
                        "notes": null,
                        "status": "Pending"
                    })
                })
                .collect::<Vec<_>>();

            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "update_plan".to_string(),
                input: json!({
                    "summary": "Bootstrap plan",
                    "items": items
                }),
            })));
        } else {
            let content = if prompt.is_empty() {
                "No prompt provided.".to_string()
            } else {
                format!("Echo: {prompt}")
            };
            events.push(Ok(ProviderEvent::AssistantMessageDelta { content }));
        }

        events.push(Ok(ProviderEvent::Completed));
        Ok(Box::pin(stream::iter(events)))
    }
}
