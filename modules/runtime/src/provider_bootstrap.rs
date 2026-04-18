use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use serde_json::json;

use ccodex_protocol::{
    Item, ItemPayload, ModelProviderPort, PortError, ProviderEvent, ToolCall, ToolCallId,
    ToolResult, TurnRequest,
};

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

        if let Some(summary) = summarize_last_tool_result(&request.items, &request.turn.id) {
            events.push(Ok(ProviderEvent::AssistantMessageDelta {
                content: summary,
            }));
            events.push(Ok(ProviderEvent::Completed));
            return Ok(Box::pin(stream::iter(events)));
        }

        if let Some(path) = prompt
            .strip_prefix("read ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "read_file".to_string(),
                input: json!({ "path": path }),
            })));
        } else if let Some(pattern) = prompt
            .strip_prefix("glob ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "glob".to_string(),
                input: json!({ "pattern": pattern }),
            })));
        } else if let Some(pattern) = prompt
            .strip_prefix("grep ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "grep".to_string(),
                input: json!({ "pattern": pattern }),
            })));
        } else if let Some(rest) = prompt
            .strip_prefix("write ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut parts = rest.splitn(2, '|');
            let path = parts.next().unwrap_or_default().trim();
            let content = parts.next().unwrap_or_default().trim();
            if !path.is_empty() {
                events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                    id: ToolCallId::new(),
                    tool_name: "write_file".to_string(),
                    input: json!({ "path": path, "content": content }),
                })));
            }
        } else if let Some(rest) = prompt
            .strip_prefix("edit ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut path_and_rest = rest.splitn(2, '|');
            let path = path_and_rest.next().unwrap_or_default().trim();
            let replacement = path_and_rest.next().unwrap_or_default().trim();
            let mut texts = replacement.splitn(2, "=>");
            let old_text = texts.next().unwrap_or_default().trim();
            let new_text = texts.next().unwrap_or_default().trim();
            if !path.is_empty() && !old_text.is_empty() {
                events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                    id: ToolCallId::new(),
                    tool_name: "edit_file".to_string(),
                    input: json!({
                        "path": path,
                        "old_text": old_text,
                        "new_text": new_text
                    }),
                })));
            }
        } else if let Some(command) = prompt
            .strip_prefix("bash ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "bash".to_string(),
                input: json!({ "command": command }),
            })));
        } else if let Some(rest) = prompt
            .strip_prefix("ask ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut parts = rest.splitn(3, '|');
            let title = parts.next().unwrap_or("Question").trim();
            let raw_choices = parts.next().unwrap_or("yes, no");
            let flags = parts.next().unwrap_or("");
            let allow_freeform = flags
                .split(',')
                .map(str::trim)
                .any(|flag| matches!(flag, "freeform" | "text" | "allow_freeform"));
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
                    "allow_freeform": allow_freeform
                }),
            })));
        } else if let Some(rest) = prompt
            .strip_prefix("delegate ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let (name, prompt) = parse_delegate_prompt(rest);
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "spawn_agent".to_string(),
                input: json!({
                    "name": name,
                    "prompt": prompt
                }),
            })));
        } else if let Some(rest) = prompt
            .strip_prefix("mcp ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut parts = rest.splitn(3, ' ');
            let server = parts.next().unwrap_or("").trim();
            let tool = parts.next().unwrap_or("").trim();
            let raw_input = parts.next().unwrap_or("").trim();
            if !server.is_empty() && !tool.is_empty() {
                let input = if raw_input.is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(raw_input).unwrap_or_else(|_| json!({ "raw": raw_input }))
                };
                events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                    id: ToolCallId::new(),
                    tool_name: "mcp_call".to_string(),
                    input: json!({
                        "server": server,
                        "tool": tool,
                        "input": input
                    }),
                })));
            }
        } else if let Some(rest) = prompt
            .strip_prefix("enter plan ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
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
                tool_name: "enter_plan_mode".to_string(),
                input: json!({
                    "summary": "Bootstrap plan mode",
                    "items": items
                }),
            })));
        } else if prompt == "exit plan" {
            events.push(Ok(ProviderEvent::ToolCall(ToolCall {
                id: ToolCallId::new(),
                tool_name: "exit_plan_mode".to_string(),
                input: json!({
                    "reason": "bootstrap requested exit"
                }),
            })));
        } else if let Some(rest) = prompt
            .strip_prefix("plan ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
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

pub fn parse_delegate_prompt(input: &str) -> (Option<String>, String) {
    if let Some((name, prompt)) = input.split_once('|') {
        let name = name.trim();
        let prompt = prompt.trim();
        if !name.is_empty() && !prompt.is_empty() {
            return (Some(name.to_string()), prompt.to_string());
        }
    }

    if let Some((name, prompt)) = input.split_once(':') {
        let name = name.trim();
        let prompt = prompt.trim();
        if !name.is_empty() && !prompt.is_empty() {
            return (Some(name.to_string()), prompt.to_string());
        }
    }

    (Some("delegate".to_string()), input.trim().to_string())
}

pub fn summarize_last_tool_result(
    items: &[Item],
    current_turn_id: &ccodex_protocol::TurnId,
) -> Option<String> {
    items.iter().rev().find_map(|item| match &item.payload {
        ItemPayload::ToolCallFinished { result } if &item.turn_id == current_turn_id => {
            summarize_tool_result(result)
        }
        _ => None,
    })
}

fn summarize_tool_result(result: &ToolResult) -> Option<String> {
    if result.is_error {
        let error = result
            .output
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown tool error");
        return Some(format!("Tool failed: {error}"));
    }

    if let Some(content) = result
        .output
        .get("content")
        .and_then(serde_json::Value::as_str)
    {
        let path = result
            .output
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unknown>");
        return Some(format!("Read {}:\n{}", path, content));
    }
    if let Some(bytes_written) = result
        .output
        .get("bytes_written")
        .and_then(serde_json::Value::as_u64)
    {
        let path = result
            .output
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unknown>");
        return Some(format!("Wrote {} byte(s) to {}.", bytes_written, path));
    }
    if let Some(replacements) = result
        .output
        .get("replacements")
        .and_then(serde_json::Value::as_u64)
    {
        let path = result
            .output
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unknown>");
        return Some(format!(
            "Edited {} with {} replacement(s).",
            path, replacements
        ));
    }
    if let Some(exit_code) = result
        .output
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
    {
        let stdout = result
            .output
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let stderr = result
            .output
            .get("stderr")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        return Some(format!(
            "Command finished with exit code {}.\nstdout:\n{}\nstderr:\n{}",
            exit_code, stdout, stderr
        ));
    }
    if let Some(matches) = result
        .output
        .get("matches")
        .and_then(serde_json::Value::as_array)
    {
        let rendered = matches
            .iter()
            .map(|item| {
                item.as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| compact_json(item))
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !rendered.is_empty() {
            return Some(rendered);
        }
    }
    if let Some(mode) = result
        .output
        .get("resolution_mode")
        .and_then(serde_json::Value::as_str)
    {
        match mode {
            "choice" => {
                if let Some(choice) = result
                    .output
                    .get("selected_choice_id")
                    .and_then(serde_json::Value::as_str)
                {
                    return Some(format!("Captured user choice: {choice}"));
                }
            }
            "freeform" => {
                if let Some(text) = result
                    .output
                    .get("freeform_text")
                    .and_then(serde_json::Value::as_str)
                {
                    return Some(format!("Captured freeform response: {text}"));
                }
            }
            "cancelled" => {
                return Some("Ask-user prompt was cancelled.".to_string());
            }
            _ => {}
        }
    }
    if let Some(choice) = result
        .output
        .get("selected_choice_id")
        .and_then(serde_json::Value::as_str)
    {
        return Some(format!("Captured user choice: {choice}"));
    }
    if let Some(text) = result
        .output
        .get("freeform_text")
        .and_then(serde_json::Value::as_str)
    {
        return Some(format!("Captured freeform response: {text}"));
    }
    if let Some(parent_turn_id) = result
        .output
        .get("parent_turn_id")
        .and_then(serde_json::Value::as_str)
        && let Some(child_turn_id) = result
            .output
            .get("child_turn_id")
            .and_then(serde_json::Value::as_str)
        && let Some(assistant_text) = result
            .output
            .get("assistant_text")
            .and_then(serde_json::Value::as_str)
    {
        let agent_name = result
            .output
            .get("agent_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("subagent");
        return Some(format!(
            "Subagent {} completed (parent turn {}, child turn {}).\n{}",
            agent_name, parent_turn_id, child_turn_id, assistant_text
        ));
    }
    if let Some(assistant_text) = result
        .output
        .get("assistant_text")
        .and_then(serde_json::Value::as_str)
    {
        let agent_name = result
            .output
            .get("agent_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("subagent");
        return Some(format!(
            "Subagent {} completed.\n{}",
            agent_name, assistant_text
        ));
    }
    if result
        .output
        .get("closed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Some("Exited plan mode.".to_string());
    }
    if let Some(count) = result
        .output
        .get("plan_item_count")
        .and_then(serde_json::Value::as_u64)
    {
        let status = result
            .output
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("updated");
        return Some(match status {
            "entered" => format!("Entered plan mode with {} item(s).", count),
            "updated" => format!("Updated the current plan with {} item(s).", count),
            "exited" => "Exited plan mode.".to_string(),
            _ => format!("Plan step completed with {} item(s).", count),
        });
    }
    if result.output.get("items").is_some() {
        let count = result
            .output
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        return Some(format!("Updated the current plan with {} item(s).", count));
    }
    if let Some(text) = result
        .output
        .get("assistant_text")
        .and_then(serde_json::Value::as_str)
        && !text.trim().is_empty()
    {
        return Some(text.to_string());
    }
    if let Some(summary) = result
        .output
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            result
                .output
                .get("message")
                .and_then(serde_json::Value::as_str)
        })
    {
        return Some(summary.to_string());
    }

    Some(format!("Tool result: {}", compact_json(&result.output)))
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".to_string())
}

#[cfg(test)]
mod tests {
    use futures::{StreamExt, executor::block_on};
    use serde_json::json;
    use time::OffsetDateTime;

    use ccodex_protocol::{
        Item, ItemId, ItemPayload, ProviderEvent, Session, SessionId, ToolCallId, ToolResult,
        TurnId, TurnRequest,
    };

    use super::{BootstrapModelProvider, parse_delegate_prompt, summarize_last_tool_result};
    use ccodex_protocol::ModelProviderPort;

    #[test]
    fn summarize_tool_result_reads_path_and_content() {
        let items = vec![Item {
            id: ItemId::new(),
            turn_id: TurnId::new(),
            created_at: OffsetDateTime::now_utc(),
            payload: ItemPayload::ToolCallFinished {
                result: ToolResult {
                    tool_call_id: ToolCallId::new(),
                    output: json!({"path": "/tmp/demo", "content": "hello"}),
                    is_error: false,
                },
            },
        }];

        assert_eq!(
            summarize_last_tool_result(&items, &items[0].turn_id).as_deref(),
            Some("Read /tmp/demo:\nhello")
        );
    }

    #[test]
    fn summarize_tool_result_ignores_previous_turn_results() {
        let previous_turn_id = TurnId::new();
        let current_turn_id = TurnId::new();
        let items = vec![
            Item {
                id: ItemId::new(),
                turn_id: previous_turn_id.clone(),
                created_at: OffsetDateTime::now_utc(),
                payload: ItemPayload::ToolCallFinished {
                    result: ToolResult {
                        tool_call_id: ToolCallId::new(),
                        output: json!({"content": "stale"}),
                        is_error: false,
                    },
                },
            },
            Item {
                id: ItemId::new(),
                turn_id: current_turn_id.clone(),
                created_at: OffsetDateTime::now_utc(),
                payload: ItemPayload::UserMessage {
                    content: "new prompt".to_string(),
                },
            },
        ];

        assert_eq!(summarize_last_tool_result(&items, &current_turn_id), None);
        assert_eq!(
            summarize_last_tool_result(&items, &previous_turn_id).as_deref(),
            Some("Read <unknown>:\nstale")
        );
    }

    #[test]
    fn summarize_tool_result_prefers_freeform_resolution_mode() {
        let items = vec![Item {
            id: ItemId::new(),
            turn_id: TurnId::new(),
            created_at: OffsetDateTime::now_utc(),
            payload: ItemPayload::ToolCallFinished {
                result: ToolResult {
                    tool_call_id: ToolCallId::new(),
                    output: json!({
                        "selected_choice_id": null,
                        "freeform_text": "preview",
                        "resolution_mode": "freeform"
                    }),
                    is_error: false,
                },
            },
        }];

        assert_eq!(
            summarize_last_tool_result(&items, &items[0].turn_id).as_deref(),
            Some("Captured freeform response: preview")
        );
    }

    #[test]
    fn summarize_tool_result_includes_subagent_lineage_when_available() {
        let items = vec![Item {
            id: ItemId::new(),
            turn_id: TurnId::new(),
            created_at: OffsetDateTime::now_utc(),
            payload: ItemPayload::ToolCallFinished {
                result: ToolResult {
                    tool_call_id: ToolCallId::new(),
                    output: json!({
                        "agent_name": "reviewer",
                        "parent_turn_id": "turn-parent-1",
                        "child_turn_id": "turn-child-1",
                        "assistant_text": "child done"
                    }),
                    is_error: false,
                },
            },
        }];

        assert_eq!(
            summarize_last_tool_result(&items, &items[0].turn_id).as_deref(),
            Some(
                "Subagent reviewer completed (parent turn turn-parent-1, child turn turn-child-1).\nchild done"
            )
        );
    }

    #[test]
    fn parse_delegate_prompt_supports_named_agent_syntax() {
        assert_eq!(
            parse_delegate_prompt("reviewer | read Cargo.toml"),
            (Some("reviewer".to_string()), "read Cargo.toml".to_string())
        );
        assert_eq!(
            parse_delegate_prompt("reviewer: read Cargo.toml"),
            (Some("reviewer".to_string()), "read Cargo.toml".to_string())
        );
        assert_eq!(
            parse_delegate_prompt("read Cargo.toml"),
            (Some("delegate".to_string()), "read Cargo.toml".to_string())
        );
    }

    #[test]
    fn bootstrap_provider_supports_freeform_ask_prompt() {
        let provider = BootstrapModelProvider;
        let request = TurnRequest {
            turn: ccodex_protocol::Turn {
                id: TurnId::new(),
                session_id: SessionId::new(),
                item_ids: vec![],
                started_at: OffsetDateTime::now_utc(),
                completed_at: None,
                status: ccodex_protocol::TurnStatus::Running,
            },
            session: Session {
                id: SessionId::new(),
                title: None,
                workspace_root: None,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
                status: ccodex_protocol::SessionStatus::Active,
                active_plan: None,
                metadata: std::collections::BTreeMap::new(),
            },
            instructions: "ask Choose deployment | staging, production | freeform".to_string(),
            project_instructions: vec![],
            items: vec![],
            available_tools: vec![],
        };

        let event = block_on(async {
            let mut stream = provider
                .start_turn(request)
                .await
                .expect("bootstrap start should succeed");
            stream.next().await.expect("first event should exist")
        })
        .expect("provider event should succeed");

        match event {
            ProviderEvent::ToolCall(call) => {
                assert_eq!(call.tool_name, "ask_user");
                assert_eq!(
                    call.input
                        .get("allow_freeform")
                        .and_then(serde_json::Value::as_bool),
                    Some(true)
                );
                assert_eq!(
                    call.input
                        .get("choices")
                        .and_then(serde_json::Value::as_array)
                        .map(|items| items.len()),
                    Some(2)
                );
            }
            other => panic!("unexpected provider event: {other:?}"),
        }
    }
}
