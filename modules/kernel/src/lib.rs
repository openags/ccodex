//! Minimal orchestration kernel for bootstrapping a real end-to-end turn.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use serde_json::{json, Value};
use thiserror::Error;
use time::OffsetDateTime;

use ccodex_compat::{CompatError, CompatLayer};
use ccodex_protocol::{
    ApprovalDecision, ApprovalEnginePort, ApprovalKind, ApprovalRequest, AskUserChoice, AskUserPrompt,
    Item, ItemId, ItemPayload, ModelProviderPort, NotificationPort, PlanId, PlanItem,
    PlanItemStatus, PlanMode, PlanState, PortError, ProtocolEvent, ProviderEvent, Session,
    SessionId, SessionStatus, ToolCall, ToolExecutorPort, ToolResult, ToolSpec, Turn, TurnId,
    TurnRequest, TurnStatus,
};
use ccodex_store::{SessionStore, StoreError};

#[derive(Debug, Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Compat(#[from] CompatError),
}

#[derive(Debug, Clone)]
pub struct RunTurnResult {
    pub session: Session,
    pub turn: Turn,
    pub assistant_text: String,
    pub events: Vec<ProtocolEvent>,
}

pub struct Kernel {
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn ModelProviderPort>,
    notifications: Arc<dyn NotificationPort>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    approval_engine: Arc<dyn ApprovalEnginePort>,
    tool_specs: BTreeMap<String, ToolSpec>,
    compat: CompatLayer,
}

impl Kernel {
    pub fn new(
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn ModelProviderPort>,
        notifications: Arc<dyn NotificationPort>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        approval_engine: Arc<dyn ApprovalEnginePort>,
        tool_specs: impl IntoIterator<Item = ToolSpec>,
    ) -> Self {
        Self {
            store,
            provider,
            notifications,
            tool_executor,
            approval_engine,
            tool_specs: tool_specs
                .into_iter()
                .map(|spec| (spec.name.clone(), spec))
                .collect(),
            compat: CompatLayer::new(),
        }
    }

    pub async fn run_prompt(
        &self,
        prompt: impl Into<String>,
        workspace_root: Option<PathBuf>,
    ) -> Result<RunTurnResult, KernelError> {
        let now = OffsetDateTime::now_utc();
        let session = Session {
            id: SessionId::new(),
            title: Some("Interactive Session".to_string()),
            workspace_root,
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
            active_plan: None,
            metadata: BTreeMap::new(),
        };
        self.store.create_session(&session).await?;

        let mut events = Vec::new();
        self.emit(&mut events, ProtocolEvent::SessionCreated(session.clone()))
            .await?;

        self.run_prompt_in_session(session, prompt.into(), events).await
    }

    pub async fn resume_prompt(
        &self,
        session_id: &SessionId,
        prompt: impl Into<String>,
    ) -> Result<RunTurnResult, KernelError> {
        let session = self.store.get_session(session_id).await?;
        self.run_prompt_in_session(session, prompt.into(), Vec::new()).await
    }

    async fn run_prompt_in_session(
        &self,
        mut session: Session,
        prompt: String,
        mut events: Vec<ProtocolEvent>,
    ) -> Result<RunTurnResult, KernelError> {
        let mut turn = Turn {
            id: TurnId::new(),
            session_id: session.id.clone(),
            item_ids: Vec::new(),
            started_at: OffsetDateTime::now_utc(),
            completed_at: None,
            status: TurnStatus::Running,
        };
        self.store.append_turn(&turn).await?;
        self.emit(&mut events, ProtocolEvent::TurnStarted(turn.clone()))
            .await?;

        self.append_item(
            &mut turn,
            &mut events,
            ItemPayload::UserMessage {
                content: prompt.clone(),
            },
        )
        .await?;

        let request = TurnRequest {
            session: session.clone(),
            turn: turn.clone(),
            instructions: prompt,
            project_instructions: session
                .workspace_root
                .as_ref()
                .map(|root| self.compat.load_workspace_instructions(root))
                .transpose()?
                .unwrap_or_default()
                .contents(),
            available_tools: self.tool_specs.values().cloned().collect(),
        };

        let mut provider_stream = self.provider.start_turn(request).await?;
        let mut assistant_text = String::new();
        let mut tool_summary: Option<String> = None;

        while let Some(event) = provider_stream.next().await {
            match event? {
                ProviderEvent::AssistantMessageDelta { content } => {
                    assistant_text.push_str(&content);
                    self.append_item(
                        &mut turn,
                        &mut events,
                        ItemPayload::AssistantMessageDelta { content },
                    )
                    .await?;
                }
                ProviderEvent::ReasoningDelta { content } => {
                    self.append_item(
                        &mut turn,
                        &mut events,
                        ItemPayload::ReasoningDelta { content },
                    )
                    .await?;
                }
                ProviderEvent::ToolCall(call) => {
                    let result = self
                        .handle_tool_call(&mut session, &mut turn, &mut events, call.clone())
                        .await?;
                    tool_summary = Some(self.summarize_tool_outcome(&call, &result, session.active_plan.as_ref()));
                }
                ProviderEvent::Completed => {}
            }
        }

        if assistant_text.is_empty() {
            if let Some(summary) = tool_summary {
                assistant_text = summary.clone();
                self.append_item(
                    &mut turn,
                    &mut events,
                    ItemPayload::AssistantMessageDelta { content: summary },
                )
                .await?;
            }
        }

        turn.status = TurnStatus::Completed;
        turn.completed_at = Some(OffsetDateTime::now_utc());
        self.store.append_turn(&turn).await?;
        self.emit(&mut events, ProtocolEvent::TurnFinished(turn.clone()))
            .await?;

        session.updated_at = OffsetDateTime::now_utc();
        self.store.update_session(&session).await?;
        self.emit(&mut events, ProtocolEvent::SessionUpdated(session.clone()))
            .await?;

        Ok(RunTurnResult {
            session,
            turn,
            assistant_text,
            events,
        })
    }

    async fn handle_tool_call(
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

        if let Some(spec) = self.tool_specs.get(&call.tool_name) {
            if spec.requires_approval {
                let approval_item_id = ItemId::new();
                let request = ApprovalRequest {
                    item_id: approval_item_id.clone(),
                    tool_call_id: Some(call.id.clone()),
                    kind: if call.tool_name == "bash" {
                        ApprovalKind::CommandExecution
                    } else {
                        ApprovalKind::ToolUse
                    },
                    summary: format!("Approve tool execution: {}", call.tool_name),
                    details: Some(call.input.to_string()),
                };
                self.append_item_with_id(
                    turn,
                    events,
                    approval_item_id,
                    ItemPayload::ApprovalRequested {
                        request: request.clone(),
                    },
                )
                .await?;

                let response = self.approval_engine.request_approval(request).await?;
                let approved = response.decision == ApprovalDecision::Approved;
                self.append_item(
                    turn,
                    events,
                    ItemPayload::ApprovalResolved {
                        response: response.clone(),
                    },
                )
                .await?;

                if !approved {
                    let rejected = ToolResult {
                        tool_call_id: call.id.clone(),
                        output: json!({ "error": "tool execution rejected by approval policy" }),
                        is_error: true,
                    };
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::ToolCallFinished {
                            result: rejected.clone(),
                        },
                    )
                    .await?;
                    return Ok(rejected);
                }
            }
        }

        if call.tool_name == "ask_user" {
            let prompt = self.build_ask_user_prompt(&call)?;
            self.append_item(
                turn,
                events,
                ItemPayload::AskUserRequested {
                    prompt: prompt.clone(),
                },
            )
            .await?;

            let response = self.approval_engine.request_user_input(prompt).await?;
            self.append_item(
                turn,
                events,
                ItemPayload::AskUserResolved {
                    response: response.clone(),
                },
            )
            .await?;

            let result = ToolResult {
                tool_call_id: call.id.clone(),
                output: json!({
                    "selected_choice_id": response.selected_choice_id,
                    "freeform_text": response.freeform_text
                }),
                is_error: false,
            };
            self.append_item(
                turn,
                events,
                ItemPayload::ToolCallFinished {
                    result: result.clone(),
                },
            )
            .await?;
            return Ok(result);
        }

        let result = match self.tool_executor.execute_tool(call.clone()).await {
            Ok(result) => result,
            Err(error) => ToolResult {
                tool_call_id: call.id.clone(),
                output: json!({ "error": error.to_string() }),
                is_error: true,
            },
        };

        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallFinished {
                result: result.clone(),
            },
        )
        .await?;

        if !result.is_error && call.tool_name == "update_plan" {
            let plan = self.build_plan_state(session, &result.output)?;
            let payload = if session.active_plan.is_none() {
                ItemPayload::PlanEntered { plan: plan.clone() }
            } else {
                ItemPayload::PlanUpdated { plan: plan.clone() }
            };
            session.active_plan = Some(plan);
            session.updated_at = OffsetDateTime::now_utc();
            self.store.update_session(session).await?;
            self.emit(events, ProtocolEvent::SessionUpdated(session.clone())).await?;
            self.append_item(turn, events, payload).await?;
        }

        Ok(result)
    }

    fn build_plan_state(&self, session: &Session, output: &Value) -> Result<PlanState, KernelError> {
        let summary = output
            .get("summary")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        let items = output
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|item| PlanItem {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("item")
                    .to_string(),
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_string(),
                notes: item.get("notes").and_then(Value::as_str).map(ToOwned::to_owned),
                status: match item.get("status").and_then(Value::as_str).unwrap_or("pending") {
                    "InProgress" | "in_progress" | "in-progress" => PlanItemStatus::InProgress,
                    "Completed" | "completed" => PlanItemStatus::Completed,
                    "Blocked" | "blocked" => PlanItemStatus::Blocked,
                    _ => PlanItemStatus::Pending,
                },
            })
            .collect();

        Ok(PlanState {
            id: session
                .active_plan
                .as_ref()
                .map(|plan| plan.id.clone())
                .unwrap_or_else(PlanId::new),
            session_id: session.id.clone(),
            mode: PlanMode::Active,
            summary,
            items,
            updated_at: OffsetDateTime::now_utc(),
        })
    }

    fn build_ask_user_prompt(&self, call: &ToolCall) -> Result<AskUserPrompt, KernelError> {
        let title = call
            .input
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Question")
            .to_string();
        let message = call
            .input
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(title.as_str())
            .to_string();
        let allow_freeform = call
            .input
            .get("allow_freeform")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let choices = call
            .input
            .get("choices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|choice| AskUserChoice {
                id: choice
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("choice")
                    .to_string(),
                label: choice
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("Choice")
                    .to_string(),
                description: choice
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            })
            .collect();

        Ok(AskUserPrompt {
            item_id: ItemId::new(),
            title,
            message,
            choices,
            allow_freeform,
        })
    }

    fn summarize_tool_outcome(
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
                let path = result.output.get("path").and_then(Value::as_str).unwrap_or("<unknown>");
                let content = result.output.get("content").and_then(Value::as_str).unwrap_or("");
                format!("Read {}:\n{}", path, content)
            }
            "bash" => {
                let exit_code = result.output.get("exit_code").and_then(Value::as_i64).unwrap_or(-1);
                let stdout = result.output.get("stdout").and_then(Value::as_str).unwrap_or("");
                let stderr = result.output.get("stderr").and_then(Value::as_str).unwrap_or("");
                format!(
                    "Command finished with exit code {}.\nstdout:\n{}\nstderr:\n{}",
                    exit_code, stdout, stderr
                )
            }
            "update_plan" => {
                let count = active_plan.map(|plan| plan.items.len()).unwrap_or(0);
                format!("Updated the current plan with {} item(s).", count)
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

    async fn append_item(
        &self,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        payload: ItemPayload,
    ) -> Result<Item, KernelError> {
        self.append_item_with_id(turn, events, ItemId::new(), payload).await
    }

    async fn append_item_with_id(
        &self,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        item_id: ItemId,
        payload: ItemPayload,
    ) -> Result<Item, KernelError> {
        let item = Item {
            id: item_id,
            turn_id: turn.id.clone(),
            created_at: OffsetDateTime::now_utc(),
            payload,
        };
        self.store.append_item(&item).await?;
        turn.item_ids.push(item.id.clone());
        self.store.append_turn(turn).await?;
        self.emit(events, ProtocolEvent::ItemAppended(item.clone())).await?;
        Ok(item)
    }

    async fn emit(
        &self,
        events: &mut Vec<ProtocolEvent>,
        event: ProtocolEvent,
    ) -> Result<(), KernelError> {
        self.notifications.notify_event(&event).await?;
        events.push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};

    use ccodex_protocol::{ItemPayload, ModelProviderPort, PortError, ProviderEvent, TurnRequest};
    use ccodex_runtime::Runtime;
    use ccodex_store::{SessionStore, SQLiteSessionStore};

    use super::Kernel;

    fn test_store(name: &str) -> (Arc<SQLiteSessionStore>, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("ccodex-{name}-{unique}.sqlite3"));
        (
            Arc::new(SQLiteSessionStore::new(&db_path).expect("store should initialize")),
            db_path,
        )
    }

    #[tokio::test]
    async fn run_prompt_persists_user_and_assistant_items() {
        let (store, db_path) = test_store("kernel");
        let runtime = Runtime::echo();
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        let result = kernel
            .run_prompt("hello kernel", None)
            .await
            .expect("turn should succeed");

        let stored_turn = store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should roundtrip");

        assert_eq!(result.assistant_text, "Echo: hello kernel");
        assert_eq!(stored_turn.items.len(), 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn run_prompt_executes_plan_tool_and_persists_plan() {
        let (store, db_path) = test_store("plan");
        let runtime = Runtime::bootstrap();
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        let result = kernel
            .run_prompt("plan define protocol, implement runtime, build tui", None)
            .await
            .expect("turn should succeed");

        let session = store
            .get_session(&result.session.id)
            .await
            .expect("session should load");

        assert_eq!(result.assistant_text, "Updated the current plan with 3 item(s).");
        assert_eq!(session.active_plan.expect("plan should exist").items.len(), 3);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn run_prompt_executes_ask_user_tool_and_persists_response() {
        let (store, db_path) = test_store("ask-user");
        let runtime = Runtime::bootstrap();
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        let result = kernel
            .run_prompt("ask Choose deployment | staging, production", None)
            .await
            .expect("turn should succeed");

        let stored_turn = store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");

        assert_eq!(result.assistant_text, "Captured user choice: choice-1");
        assert!(stored_turn.items.iter().any(|item| matches!(item.payload, ItemPayload::AskUserRequested { .. })));
        assert!(stored_turn.items.iter().any(|item| matches!(item.payload, ItemPayload::AskUserResolved { .. })));

        let _ = std::fs::remove_file(db_path);
    }

    #[derive(Default)]
    struct RecordingProvider {
        requests: Arc<Mutex<Vec<TurnRequest>>>,
    }

    #[async_trait]
    impl ModelProviderPort for RecordingProvider {
        async fn start_turn(
            &self,
            request: TurnRequest,
        ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
            self.requests.lock().expect("lock should succeed").push(request);
            Ok(Box::pin(stream::iter(vec![
                Ok(ProviderEvent::AssistantMessageDelta {
                    content: "recorded".to_string(),
                }),
                Ok(ProviderEvent::Completed),
            ])))
        }
    }

    #[tokio::test]
    async fn resume_prompt_adds_a_new_turn_to_existing_session() {
        let (store, db_path) = test_store("resume");
        let runtime = Runtime::bootstrap();
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        let first = kernel
            .run_prompt("hello one", None)
            .await
            .expect("first turn should succeed");

        let second = kernel
            .resume_prompt(&first.session.id, "hello two")
            .await
            .expect("second turn should succeed");

        let turns = store
            .list_turns(&first.session.id)
            .await
            .expect("turns should list");

        assert_eq!(first.session.id, second.session.id);
        assert_eq!(turns.len(), 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn run_prompt_loads_workspace_instruction_files_into_turn_request() {
        let (store, db_path) = test_store("compat");
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("ccodex-kernel-compat-{unique}"));
        std::fs::create_dir_all(&workspace).expect("workspace should exist");
        std::fs::write(workspace.join("CCODEX.md"), "own instructions").expect("ccodex file should write");
        std::fs::write(workspace.join("CLAUDE.md"), "claude instructions").expect("claude file should write");

        let runtime = Runtime::bootstrap();
        let provider = RecordingProvider::default();
        let recorded = provider.requests.clone();
        let kernel = Kernel::new(
            store,
            Arc::new(provider),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        kernel
            .run_prompt("hello compat", Some(workspace.clone()))
            .await
            .expect("turn should succeed");

        let requests = recorded.lock().expect("lock should succeed");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].project_instructions,
            vec!["own instructions".to_string(), "claude instructions".to_string()]
        );

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_dir_all(workspace);
    }
}
