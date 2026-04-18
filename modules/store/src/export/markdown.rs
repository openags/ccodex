use async_trait::async_trait;

use ccodex_protocol::{ItemPayload, SessionId};

use crate::{
    TranscriptExporter,
    traits::{SessionStore, StoreError},
};

#[derive(Debug, Clone)]
pub struct MarkdownTranscriptExporter {
    store: crate::SQLiteSessionStore,
}

impl MarkdownTranscriptExporter {
    pub fn new(store: crate::SQLiteSessionStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TranscriptExporter for MarkdownTranscriptExporter {
    async fn export_session(&self, session_id: &SessionId) -> Result<String, StoreError> {
        let session = self.store.get_session(session_id).await?;
        let turns = self.store.list_turns(session_id).await?;

        let mut out = vec![format!("# Session {}", session.id)];
        if let Some(title) = session.title {
            out.push(format!("\n## Title\n\n{}", title));
        }

        let bootstrap_complete = session
            .metadata
            .get("bootstrap_complete")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let instruction_count = session
            .metadata
            .get("bootstrap_instruction_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let skill_count = session
            .metadata
            .get("bootstrap_skill_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let agent_count = session
            .metadata
            .get("bootstrap_agent_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let hook_count = session
            .metadata
            .get("bootstrap_hook_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let mcp_count = session
            .metadata
            .get("bootstrap_mcp_server_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let bootstrapped_at = session
            .metadata
            .get("bootstrapped_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("-");
        let skill_names = session
            .metadata
            .get("bootstrap_skill_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "-".to_string());
        let hook_names = session
            .metadata
            .get("bootstrap_hook_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "-".to_string());
        let mcp_server_names = session
            .metadata
            .get("bootstrap_mcp_server_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "-".to_string());
        out.push(format!(
            "\n## Session Context\n\n- Bootstrap: {}\n- BootstrappedAt: {}\n- BootstrapCounts: instructions={} skills={} agents={} hooks={} mcp={}\n- BootstrapSkills: {}\n- BootstrapHooks: {}\n- BootstrapMcpServers: {}",
            if bootstrap_complete { "complete" } else { "pending" },
            bootstrapped_at,
            instruction_count,
            skill_count,
            agent_count,
            hook_count,
            mcp_count,
            skill_names,
            hook_names,
            mcp_server_names,
        ));

        for stored_turn in turns {
            out.push(format!("\n## Turn {}", stored_turn.turn.id));
            for item in stored_turn.items {
                match item.payload {
                    ItemPayload::UserMessage { content } => {
                        out.push(format!("- User: {}", content))
                    }
                    ItemPayload::AssistantMessageDelta { content } => {
                        out.push(format!("- Assistant: {}", content))
                    }
                    ItemPayload::ReasoningDelta { content } => {
                        out.push(format!("- Reasoning: {}", content))
                    }
                    ItemPayload::ToolCallStarted { call } => {
                        out.push(format!("- ToolCall: {} {}", call.tool_name, call.id));
                    }
                    ItemPayload::ToolCallFinished { result } => {
                        let resolution_mode = result
                            .output
                            .get("resolution_mode")
                            .and_then(serde_json::Value::as_str);
                        let plan_item_count = result
                            .output
                            .get("plan_item_count")
                            .and_then(serde_json::Value::as_u64);
                        out.push(match (resolution_mode, plan_item_count) {
                            (Some(mode), _) => format!(
                                "- ToolResult: {} error={} resolution_mode={}",
                                result.tool_call_id, result.is_error, mode
                            ),
                            (_, Some(count)) => format!(
                                "- ToolResult: {} error={} plan_item_count={}",
                                result.tool_call_id, result.is_error, count
                            ),
                            _ => format!(
                                "- ToolResult: {} error={}",
                                result.tool_call_id, result.is_error
                            ),
                        });
                    }
                    ItemPayload::ApprovalRequested { request } => {
                        out.push(format!("- ApprovalRequested: {}", request.summary));
                    }
                    ItemPayload::ApprovalResolved { response } => {
                        out.push(format!(
                            "- ApprovalResolved: {:?} [{}]",
                            response.decision,
                            response
                                .reason_code
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or_else(|| "unknown".to_string())
                        ));
                    }
                    ItemPayload::AskUserRequested { prompt } => {
                        out.push(format!("- AskUser: {}", prompt.title));
                    }
                    ItemPayload::AskUserResolved { response } => {
                        out.push(format!(
                            "- AskUserResponse: choice={:?} freeform={:?}",
                            response.selected_choice_id, response.freeform_text
                        ));
                    }
                    ItemPayload::PlanEntered { plan } => {
                        out.push(format!(
                            "- PlanEntered: {}",
                            plan.items
                                .iter()
                                .map(|item| format!("[{:?}] {}", item.status, item.title))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        ));
                    }
                    ItemPayload::PlanUpdated { plan } => {
                        out.push(format!(
                            "- PlanUpdated: {}",
                            plan.items
                                .iter()
                                .map(|item| format!("[{:?}] {}", item.status, item.title))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        ));
                    }
                    ItemPayload::PlanExited { plan_id } => {
                        out.push(format!("- PlanExited: {}", plan_id));
                    }
                    ItemPayload::Warning { code, message } => {
                        out.push(format!("- Warning[{code}]: {message}"));
                    }
                    ItemPayload::Error { code, message } => {
                        out.push(format!("- Error[{code}]: {message}"));
                    }
                    ItemPayload::SystemEvent { name, payload } => {
                        out.push(format!("- SystemEvent {name}: {}", payload));
                    }
                    ItemPayload::ToolCallDelta {
                        tool_call_id,
                        delta,
                    } => {
                        let phase = delta
                            .get("phase")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        if phase == "approval_assessed" {
                            out.push(format!(
                                "- ToolDelta: {} phase={} kind={} risk={} tool={} path={}",
                                tool_call_id,
                                phase,
                                delta
                                    .get("kind")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("Unknown"),
                                delta
                                    .get("risk")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("Unknown"),
                                delta
                                    .get("tool_name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("-"),
                                delta
                                    .get("path")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("-")
                            ));
                        } else if phase == "completed" {
                            out.push(format!(
                                "- ToolDelta: {} phase={} tool={} status={} error={} resolution_mode={} plan_item_count={}",
                                tool_call_id,
                                phase,
                                delta.get("tool_name").and_then(serde_json::Value::as_str).unwrap_or("-"),
                                delta.get("status").and_then(serde_json::Value::as_str).unwrap_or("-"),
                                delta.get("is_error").and_then(serde_json::Value::as_bool).unwrap_or(false),
                                delta.get("resolution_mode").and_then(serde_json::Value::as_str).unwrap_or("-"),
                                delta.get("plan_item_count").and_then(serde_json::Value::as_u64).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string())
                            ));
                        } else {
                            out.push(format!("- ToolDelta: {} {}", tool_call_id, delta));
                        }
                    }
                }
            }
        }

        Ok(out.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use ccodex_protocol::{
        ApprovalDecision, ApprovalReasonCode, ApprovalResponse, AskUserResponse, Item, ItemId,
        ItemPayload, PlanId, PlanItem, PlanItemStatus, PlanMode, PlanState, Session, SessionId,
        SessionStatus, ToolCallId, ToolResult, Turn, TurnId, TurnStatus,
    };

    use crate::{
        MarkdownTranscriptExporter, SQLiteSessionStore, TranscriptExporter, traits::SessionStore,
    };

    #[tokio::test]
    async fn markdown_export_renders_structured_lifecycle_fields() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("ccodex-store-markdown-{unique}.sqlite3"));
        let store = SQLiteSessionStore::new(&db_path).expect("store should initialize");
        let session_id = SessionId("session-1".to_string());
        let turn_id = TurnId("turn-1".to_string());
        let mut session = Session {
            id: session_id.clone(),
            title: Some("Markdown Export".to_string()),
            workspace_root: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            status: SessionStatus::Active,
            active_plan: None,
            metadata: std::collections::BTreeMap::new(),
        };
        session
            .metadata
            .insert("bootstrap_complete".to_string(), serde_json::json!(true));
        session.metadata.insert(
            "bootstrapped_at".to_string(),
            serde_json::Value::String("2026-04-18T12:00:00Z".to_string()),
        );
        session.metadata.insert(
            "bootstrap_instruction_count".to_string(),
            serde_json::json!(1),
        );
        session
            .metadata
            .insert("bootstrap_skill_count".to_string(), serde_json::json!(2));
        session
            .metadata
            .insert("bootstrap_agent_count".to_string(), serde_json::json!(1));
        session
            .metadata
            .insert("bootstrap_hook_count".to_string(), serde_json::json!(1));
        session.metadata.insert(
            "bootstrap_mcp_server_count".to_string(),
            serde_json::json!(1),
        );
        session.metadata.insert(
            "bootstrap_skill_names".to_string(),
            serde_json::json!(["review", "ship"]),
        );
        session.metadata.insert(
            "bootstrap_hook_names".to_string(),
            serde_json::json!(["prepare"]),
        );
        session.metadata.insert(
            "bootstrap_mcp_server_names".to_string(),
            serde_json::json!(["echo"]),
        );
        store
            .create_session(&session)
            .await
            .expect("session should save");
        store
            .append_turn(&Turn {
                id: turn_id.clone(),
                session_id: session_id.clone(),
                item_ids: vec![],
                started_at: OffsetDateTime::now_utc(),
                completed_at: Some(OffsetDateTime::now_utc()),
                status: TurnStatus::Completed,
            })
            .await
            .expect("turn should save");
        for payload in [
            ItemPayload::ToolCallDelta {
                tool_call_id: ToolCallId("tool-1".to_string()),
                delta: serde_json::json!({
                    "phase": "approval_assessed",
                    "kind": "CommandExecution",
                    "risk": "High",
                    "tool_name": "bash",
                    "path": "/tmp/demo"
                }),
            },
            ItemPayload::ToolCallDelta {
                tool_call_id: ToolCallId("tool-2".to_string()),
                delta: serde_json::json!({
                    "phase": "completed",
                    "tool_name": "ask_user",
                    "status": "resolved",
                    "is_error": false,
                    "resolution_mode": "choice"
                }),
            },
            ItemPayload::ApprovalResolved {
                response: ApprovalResponse::new(
                    ItemId("approval-1".to_string()),
                    ApprovalDecision::Rejected,
                    Some("blocked".to_string()),
                    Some(ApprovalReasonCode::RuleDeny),
                ),
            },
            ItemPayload::AskUserResolved {
                response: AskUserResponse {
                    request_item_id: ItemId("ask-1".to_string()),
                    selected_choice_id: None,
                    freeform_text: Some("preview".to_string()),
                },
            },
            ItemPayload::PlanUpdated {
                plan: PlanState {
                    id: PlanId("plan-1".to_string()),
                    session_id: session_id.clone(),
                    mode: PlanMode::Active,
                    summary: Some("active plan".to_string()),
                    items: vec![PlanItem {
                        id: "step-1".to_string(),
                        title: "Ship lifecycle export".to_string(),
                        notes: None,
                        status: PlanItemStatus::Completed,
                    }],
                    updated_at: OffsetDateTime::now_utc(),
                },
            },
            ItemPayload::ToolCallFinished {
                result: ToolResult {
                    tool_call_id: ToolCallId("tool-2".to_string()),
                    output: serde_json::json!({
                        "selected_choice_id": null,
                        "freeform_text": "preview",
                        "resolution_mode": "freeform"
                    }),
                    is_error: false,
                },
            },
        ] {
            store
                .append_item(&Item {
                    id: ItemId::new(),
                    turn_id: turn_id.clone(),
                    created_at: OffsetDateTime::now_utc(),
                    payload,
                })
                .await
                .expect("item should save");
        }

        let exported = MarkdownTranscriptExporter::new(store.clone())
            .export_session(&session_id)
            .await
            .expect("markdown export should work");

        assert!(exported.contains("## Session Context"));
        assert!(exported.contains("Bootstrap: complete"));
        assert!(exported.contains("BootstrappedAt: 2026-04-18T12:00:00Z"));
        assert!(
            exported.contains("BootstrapCounts: instructions=1 skills=2 agents=1 hooks=1 mcp=1")
        );
        assert!(exported.contains("BootstrapSkills: review, ship"));
        assert!(exported.contains("BootstrapHooks: prepare"));
        assert!(exported.contains("BootstrapMcpServers: echo"));
        assert!(exported.contains("ToolDelta: tool-1 phase=approval_assessed kind=CommandExecution risk=High tool=bash path=/tmp/demo"));
        assert!(exported.contains("ToolDelta: tool-2 phase=completed tool=ask_user status=resolved error=false resolution_mode=choice plan_item_count=-") || exported.contains("ToolDelta: tool-2 phase=completed tool=ask_user status=resolved error=false resolution_mode=freeform plan_item_count=-"));
        assert!(exported.contains("ApprovalResolved: Rejected [rule_deny]"));
        assert!(exported.contains("AskUserResponse: choice=None freeform=Some(\"preview\")"));
        assert!(exported.contains("PlanUpdated: [Completed] Ship lifecycle export"));
        assert!(exported.contains("ToolResult: tool-2 error=false resolution_mode=freeform"));

        let _ = std::fs::remove_file(db_path);
    }
}
