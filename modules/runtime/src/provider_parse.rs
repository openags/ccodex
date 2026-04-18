use serde_json::{Value, json};

use ccodex_protocol::{PortError, ProviderEvent, ToolCall, ToolCallId};

pub(crate) fn parse_openai_events(
    body: &Value,
) -> Result<Vec<Result<ProviderEvent, PortError>>, PortError> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PortError::Provider(format!(
                "openai-compatible response missing choices: {}",
                compact_json(body)
            ))
        })?;

    let mut events = Vec::new();
    let Some(message) = choices
        .first()
        .and_then(|choice| choice.get("message"))
        .cloned()
    else {
        events.push(Ok(ProviderEvent::Completed));
        return Ok(events);
    };

    if let Some(content) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        events.push(Ok(ProviderEvent::AssistantMessageDelta {
            content: content.to_string(),
        }));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let id = tool_call.get("id").and_then(Value::as_str).ok_or_else(|| {
                PortError::Provider(format!("tool_call missing id: {}", compact_json(tool_call)))
            })?;
            let function = tool_call.get("function").ok_or_else(|| {
                PortError::Provider(format!(
                    "tool_call missing function block: {}",
                    compact_json(tool_call)
                ))
            })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    PortError::Provider(format!(
                        "tool_call missing function name: {}",
                        compact_json(tool_call)
                    ))
                })?;
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input = serde_json::from_str::<Value>(arguments).map_err(|err| {
                PortError::Provider(format!("invalid tool arguments json: {err}"))
            })?;

            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId(id.to_string()),
                tool_name: name.to_string(),
                input,
            })));
        }
    }

    events.push(Ok(ProviderEvent::Completed));
    Ok(events)
}

pub(crate) fn parse_anthropic_events(
    body: &Value,
) -> Result<Vec<Result<ProviderEvent, PortError>>, PortError> {
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PortError::Provider(format!(
                "anthropic-compatible response missing content: {}",
                compact_json(body)
            ))
        })?;

    let mut events = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    events.push(Ok(ProviderEvent::AssistantMessageDelta {
                        content: text.to_string(),
                    }));
                }
            }
            Some("tool_use") => {
                let id = block.get("id").and_then(Value::as_str).ok_or_else(|| {
                    PortError::Provider(format!("tool_use missing id: {}", compact_json(block)))
                })?;
                let name = block.get("name").and_then(Value::as_str).ok_or_else(|| {
                    PortError::Provider(format!("tool_use missing name: {}", compact_json(block)))
                })?;
                let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                    id: ToolCallId(id.to_string()),
                    tool_name: name.to_string(),
                    input,
                })));
            }
            _ => {}
        }
    }

    events.push(Ok(ProviderEvent::Completed));
    Ok(events)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".to_string())
}
