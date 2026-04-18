use serde_json::{Value, json};
use time::OffsetDateTime;

use ccodex_protocol::{ItemPayload, PlanState, ProtocolEvent, Session, ToolCall, ToolResult, Turn};

use crate::hooks::HookContext;
use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn handle_tool_call(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        call: ToolCall,
    ) -> Result<ToolResult, KernelError> {
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallStarted { call: call.clone() },
        )
        .await?;
        self.run_hooks(
            session,
            turn,
            events,
            ccodex_extensions::HookEvent::PreTool,
            HookContext {
                prompt: "",
                assistant_text: "",
                tool_call: Some(&call),
                tool_result: None,
            },
        )
        .await?;

        if let Some(rejected) = self
            .require_tool_approval(session, turn, events, &call)
            .await?
        {
            return Ok(rejected);
        }

        let tool_name = call.tool_name.clone();

        if tool_name == "ask_user" {
            return self
                .handle_ask_user_tool(session, turn, events, &call)
                .await;
        }

        if tool_name == "spawn_agent" {
            return self
                .handle_spawn_agent_tool(session, turn, events, &call)
                .await;
        }

        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({ "phase": "executing" }),
            },
        )
        .await?;

        let execution = match self.tool_executor.execute_tool(call.clone()).await {
            Ok(execution) => execution,
            Err(error) => ccodex_protocol::ToolExecutionOutcome {
                result: ToolResult {
                    tool_call_id: call.id.clone(),
                    output: json!({ "error": error.to_string() }),
                    is_error: true,
                },
                deltas: Vec::new(),
            },
        };
        let result = execution.result;

        for delta in execution.deltas {
            self.append_item(
                turn,
                events,
                ItemPayload::ToolCallDelta {
                    tool_call_id: call.id.clone(),
                    delta,
                },
            )
            .await?;
        }

        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({
                    "phase": "completed",
                    "is_error": result.is_error,
                    "tool_name": tool_name,
                }),
            },
        )
        .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallFinished {
                result: result.clone(),
            },
        )
        .await?;
        self.run_hooks(
            session,
            turn,
            events,
            ccodex_extensions::HookEvent::PostTool,
            HookContext {
                prompt: "",
                assistant_text: "",
                tool_call: Some(&call),
                tool_result: Some(&result),
            },
        )
        .await?;

        if !result.is_error {
            match tool_name.as_str() {
                "enter_plan_mode" => {
                    let plan = self.build_plan_state(session, &result.output)?;
                    session.active_plan = Some(plan.clone());
                    session.updated_at = OffsetDateTime::now_utc();
                    self.store.update_session(session).await?;
                    self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
                        .await?;
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::PlanEntered { plan: plan.clone() },
                    )
                    .await?;
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::ToolCallDelta {
                            tool_call_id: call.id.clone(),
                            delta: json!({
                                "phase": "completed",
                                "tool_name": tool_name,
                                "status": "entered",
                                "is_error": false,
                                "plan_item_count": plan.items.len(),
                            }),
                        },
                    )
                    .await?;
                }
                "update_plan" | "todo_write" => {
                    let plan = self.build_plan_state(session, &result.output)?;
                    let status = if session.active_plan.is_none() {
                        "entered"
                    } else {
                        "updated"
                    };
                    let payload = if session.active_plan.is_none() {
                        ItemPayload::PlanEntered { plan: plan.clone() }
                    } else {
                        ItemPayload::PlanUpdated { plan: plan.clone() }
                    };
                    session.active_plan = Some(plan.clone());
                    session.updated_at = OffsetDateTime::now_utc();
                    self.store.update_session(session).await?;
                    self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
                        .await?;
                    self.append_item(turn, events, payload).await?;
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::ToolCallDelta {
                            tool_call_id: call.id.clone(),
                            delta: json!({
                                "phase": "completed",
                                "tool_name": tool_name,
                                "status": status,
                                "is_error": false,
                                "plan_item_count": plan.items.len(),
                            }),
                        },
                    )
                    .await?;
                }
                "exit_plan_mode" => {
                    if let Some(plan) = session.active_plan.take() {
                        let plan_id = plan.id;
                        session.updated_at = OffsetDateTime::now_utc();
                        self.store.update_session(session).await?;
                        self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
                            .await?;
                        self.append_item(turn, events, ItemPayload::PlanExited { plan_id })
                            .await?;
                        self.append_item(
                            turn,
                            events,
                            ItemPayload::ToolCallDelta {
                                tool_call_id: call.id.clone(),
                                delta: json!({
                                    "phase": "completed",
                                    "tool_name": tool_name,
                                    "status": "exited",
                                    "is_error": false,
                                    "plan_item_count": 0,
                                }),
                            },
                        )
                        .await?;
                    }
                }
                _ => {}
            }
        }

        Ok(result)
    }

    pub(crate) fn summarize_tool_outcome(
        &self,
        call: &ToolCall,
        result: &ToolResult,
        active_plan: Option<&PlanState>,
    ) -> String {
        if result.is_error {
            let error = result
                .output
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown tool error");
            return format!("Tool {} failed: {}", call.tool_name, error);
        }

        match call.tool_name.as_str() {
            "read_file" => {
                let path = result
                    .output
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                let content = result
                    .output
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!("Read {}:\n{}", path, content)
            }
            "write_file" => {
                let path = result
                    .output
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                let bytes = result
                    .output
                    .get("bytes_written")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                format!("Wrote {} byte(s) to {}.", bytes, path)
            }
            "edit_file" => {
                let path = result
                    .output
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                let replacements = result
                    .output
                    .get("replacements")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                format!("Edited {} with {} replacement(s).", path, replacements)
            }
            "glob" => {
                let matches = result
                    .output
                    .get("matches")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let rendered = matches
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Found {} matching file(s).\n{}", matches.len(), rendered)
            }
            "grep" => {
                let matches = result
                    .output
                    .get("matches")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let rendered = matches
                    .iter()
                    .map(|item| {
                        let path = item
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or("<unknown>");
                        let line_number =
                            item.get("line_number").and_then(Value::as_u64).unwrap_or(0);
                        let line = item.get("line").and_then(Value::as_str).unwrap_or("");
                        format!("{path}:{line_number}: {line}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Found {} grep match(es).\n{}", matches.len(), rendered)
            }
            "mcp_call" => {
                let rendered = serde_json::to_string_pretty(&result.output)
                    .unwrap_or_else(|_| result.output.to_string());
                format!("MCP call result:\n{}", rendered)
            }
            "bash" => {
                let exit_code = result
                    .output
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1);
                let stdout = result
                    .output
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let stderr = result
                    .output
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!(
                    "Command finished with exit code {}.\nstdout:\n{}\nstderr:\n{}",
                    exit_code, stdout, stderr
                )
            }
            "enter_plan_mode" => "Entered plan mode.".to_string(),
            "update_plan" | "todo_write" => {
                let count = active_plan.map(|plan| plan.items.len()).unwrap_or(0);
                format!("Updated the current plan with {} item(s).", count)
            }
            "exit_plan_mode" => "Exited plan mode.".to_string(),
            "spawn_agent" => {
                let agent_name = result
                    .output
                    .get("agent_name")
                    .and_then(Value::as_str)
                    .unwrap_or("subagent");
                let assistant_text = result
                    .output
                    .get("assistant_text")
                    .and_then(Value::as_str)
                    .unwrap_or("<no result>");
                format!("Subagent {} completed.\n{}", agent_name, assistant_text)
            }
            "ask_user" => {
                let selected = result
                    .output
                    .get("selected_choice_id")
                    .and_then(Value::as_str)
                    .unwrap_or("<none>");
                format!("Captured user choice: {}", selected)
            }
            _ => format!("Tool {} completed.", call.tool_name),
        }
    }
}
