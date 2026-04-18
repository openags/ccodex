use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use serde_json::json;

use ccodex_protocol::TurnStatus;
use ccodex_protocol::{ItemPayload, ModelProviderPort, PortError, ProviderEvent, TurnRequest};
use ccodex_runtime::{ApprovalPolicy, ProviderConfig, ProviderKind, Runtime, RuntimeConfig};
use ccodex_store::{SQLiteSessionStore, SessionStore};

use crate::{Kernel, WorkspaceTrust};

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

    assert_eq!(
        result.assistant_text,
        "Updated the current plan with 3 item(s)."
    );
    assert_eq!(
        session.active_plan.expect("plan should exist").items.len(),
        3
    );
    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("update_plan")
                && delta.get("status").and_then(serde_json::Value::as_str) == Some("entered")
                && delta.get("plan_item_count").and_then(serde_json::Value::as_u64) == Some(3)
    )));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_prompt_can_enter_and_exit_plan_mode() {
    let (store, db_path) = test_store("plan-enter-exit");
    let runtime = Runtime::bootstrap();
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let entered = kernel
        .run_prompt("enter plan alpha, beta", None)
        .await
        .expect("enter plan should succeed");

    let session_after_enter = store
        .get_session(&entered.session.id)
        .await
        .expect("session should load");
    assert!(session_after_enter.active_plan.is_some());

    let exited = kernel
        .resume_prompt(&entered.session.id, "exit plan")
        .await
        .expect("exit plan should succeed");

    let session_after_exit = store
        .get_session(&entered.session.id)
        .await
        .expect("session should load");
    let stored_turn = store
        .get_turn(&exited.turn.id)
        .await
        .expect("turn should load");

    assert_eq!(exited.assistant_text, "Exited plan mode.");
    assert!(session_after_exit.active_plan.is_none());
    assert!(
        stored_turn
            .items
            .iter()
            .any(|item| matches!(item.payload, ItemPayload::PlanExited { .. }))
    );
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("exit_plan_mode")
                && delta.get("status").and_then(serde_json::Value::as_str) == Some("exited")
                && delta.get("plan_item_count").and_then(serde_json::Value::as_u64) == Some(0)
    )));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_prompt_executes_ask_user_tool_and_persists_response() {
    let (store, db_path) = test_store("ask-user");
    let runtime = Runtime::from_config(RuntimeConfig {
        approval_policy: ApprovalPolicy::AlwaysApprove,
        provider: ProviderConfig {
            kind: ProviderKind::Bootstrap,
            base_url: None,
            api_key: None,
            model: "ccodex-test".to_string(),
            max_output_tokens: 16_000,
        },
        ..RuntimeConfig::default()
    });
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
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("waiting_for_user")
    )));
    assert!(
        stored_turn
            .items
            .iter()
            .any(|item| matches!(item.payload, ItemPayload::AskUserRequested { .. }))
    );
    assert!(
        stored_turn
            .items
            .iter()
            .any(|item| matches!(item.payload, ItemPayload::AskUserResolved { .. }))
    );
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("ask_user")
                && delta.get("status").and_then(serde_json::Value::as_str) == Some("resolved")
                && delta.get("resolution_mode").and_then(serde_json::Value::as_str) == Some("choice")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallFinished { result }
            if result
                .output
                .get("resolution_mode")
                .and_then(serde_json::Value::as_str) == Some("choice")
    )));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_prompt_can_delegate_to_subagent() {
    let (store, db_path) = test_store("subagent");
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
        .run_prompt("delegate read Cargo.toml", None)
        .await
        .expect("delegation should succeed");

    let sessions = store
        .list_sessions(ccodex_store::ListSessionsParams { limit: Some(10) })
        .await
        .expect("sessions should list");
    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");

    assert!(
        result
            .assistant_text
            .contains("Subagent delegate completed")
    );
    assert!(result.assistant_text.contains("child turn"));
    assert_eq!(sessions.len(), 2);
    let child_session = sessions
        .iter()
        .find(|session| session.id != result.session.id)
        .expect("child session should exist");
    let child_turns = store
        .list_turns(&child_session.id)
        .await
        .expect("child turns should load");
    assert_eq!(child_turns.len(), 1);
    assert_eq!(child_turns[0].turn.status, TurnStatus::Completed);
    assert!(child_turns[0].items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::UserMessage { content } if content == "read Cargo.toml"
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("delegating")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "subagent_started"
                && payload.get("parent_session_id").and_then(serde_json::Value::as_str)
                    == Some(result.session.id.0.as_str())
                && payload.get("parent_turn_id").and_then(serde_json::Value::as_str)
                    == Some(result.turn.id.0.as_str())
                && payload.get("subagent_depth").and_then(serde_json::Value::as_u64)
                    == Some(1)
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "subagent_finished"
                && payload.get("parent_session_id").and_then(serde_json::Value::as_str)
                    == Some(result.session.id.0.as_str())
                && payload.get("parent_turn_id").and_then(serde_json::Value::as_str)
                    == Some(result.turn.id.0.as_str())
                && payload.get("child_turn_id").and_then(serde_json::Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false)
                && payload.get("subagent_depth").and_then(serde_json::Value::as_u64)
                    == Some(1)
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallFinished { result: tool_result }
            if tool_result
                .output
                .get("parent_session_id")
                .and_then(serde_json::Value::as_str)
                == Some(result.session.id.0.as_str())
                && tool_result
                    .output
                    .get("parent_turn_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(result.turn.id.0.as_str())
                && tool_result
                    .output
                    .get("child_turn_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false)
                && tool_result
                    .output
                    .get("subagent_depth")
                    .and_then(serde_json::Value::as_u64)
                    == Some(1)
    )));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn agent_runtime_facade_can_start_and_continue_sessions() {
    let (store, db_path) = test_store("agent-runtime");
    let runtime = Runtime::echo();
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let agent_runtime = kernel.agent_runtime();
    let started = agent_runtime
        .start_session("hello runtime", None)
        .await
        .expect("session should start");
    let continued = agent_runtime
        .continue_session(&started.session.id, "hello again")
        .await
        .expect("session should continue");

    assert_eq!(started.assistant_text, "Echo: hello runtime");
    assert_eq!(continued.assistant_text, "Echo: hello again");

    let sessions = store
        .list_sessions(ccodex_store::ListSessionsParams { limit: Some(10) })
        .await
        .expect("sessions should list");
    assert_eq!(sessions.len(), 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_prompt_emits_tool_execution_deltas_for_approved_tools() {
    let (store, db_path) = test_store("tool-deltas");
    let runtime = Runtime::from_config(RuntimeConfig {
        approval_policy: ApprovalPolicy::AlwaysApprove,
        provider: ProviderConfig {
            kind: ProviderKind::Bootstrap,
            base_url: None,
            api_key: None,
            model: "ccodex-test".to_string(),
            max_output_tokens: 16_000,
        },
        ..RuntimeConfig::default()
    });
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let result = kernel
        .run_prompt("bash printf lifecycle", None)
        .await
        .expect("turn should succeed");

    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");

    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("approval_assessed")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("executing")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("stream")
                && delta.get("stream").and_then(serde_json::Value::as_str) == Some("stdout")
                && delta.get("content").and_then(serde_json::Value::as_str) == Some("lifecycle")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallDelta { delta, .. }
            if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                && delta.get("is_error").and_then(serde_json::Value::as_bool) == Some(false)
                && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("bash")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::ToolCallFinished { result } if !result.is_error
    )));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_prompt_executes_pre_and_post_turn_hooks() {
    let (store, db_path) = test_store("hooks");
    let runtime = Runtime::echo();
    // Use trusted workspace for hook tests since hooks are in workspace-local .ccodex
    let kernel = Kernel::with_trust(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
        WorkspaceTrust::Trusted,
    );

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("ccodex-kernel-hooks-{unique}"));
    std::fs::create_dir_all(workspace.join(".ccodex").join("hooks"))
        .expect("hooks dir should exist");
    std::fs::write(
        workspace.join(".ccodex").join("hooks").join("prepare.toml"),
        "event = \"pre_turn\"\ncommand = \"echo pre-hook\"",
    )
    .expect("pre hook should write");
    std::fs::write(
        workspace.join(".ccodex").join("hooks").join("finish.toml"),
        "event = \"post_turn\"\ncommand = \"echo post-hook\"",
    )
    .expect("post hook should write");

    let result = kernel
        .run_prompt("hello hooks", Some(workspace.clone()))
        .await
        .expect("turn should succeed");
    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");

    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "hook_executed"
                && payload.get("event").and_then(serde_json::Value::as_str) == Some("PreTurn")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "hook_executed"
                && payload.get("event").and_then(serde_json::Value::as_str) == Some("PostTurn")
    )));

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn run_prompt_executes_pre_and_post_tool_hooks() {
    let (store, db_path) = test_store("tool-hooks");
    let runtime = Runtime::bootstrap();
    // Use trusted workspace for hook tests since hooks are in workspace-local .ccodex
    let kernel = Kernel::with_trust(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
        WorkspaceTrust::Trusted,
    );

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("ccodex-kernel-tool-hooks-{unique}"));
    std::fs::create_dir_all(workspace.join(".ccodex").join("hooks"))
        .expect("hooks dir should exist");
    std::fs::write(
        workspace
            .join(".ccodex")
            .join("hooks")
            .join("pre-tool.toml"),
        "event = \"pre_tool\"\ncommand = \"printf pre:$CCODEX_TOOL_NAME\"",
    )
    .expect("pre-tool hook should write");
    std::fs::write(
        workspace
            .join(".ccodex")
            .join("hooks")
            .join("post-tool.toml"),
        "event = \"post_tool\"\ncommand = \"printf post:$CCODEX_TOOL_NAME:$CCODEX_TOOL_IS_ERROR\"",
    )
    .expect("post-tool hook should write");

    let result = kernel
        .run_prompt("read Cargo.toml", Some(workspace.clone()))
        .await
        .expect("turn should succeed");
    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");

    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "hook_executed"
                && payload.get("event").and_then(serde_json::Value::as_str) == Some("PreTool")
                && payload.get("stdout").and_then(serde_json::Value::as_str) == Some("pre:read_file")
                && payload.get("tool_name").and_then(serde_json::Value::as_str) == Some("read_file")
    )));
    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "hook_executed"
                && payload.get("event").and_then(serde_json::Value::as_str) == Some("PostTool")
                && payload.get("stdout").and_then(serde_json::Value::as_str) == Some("post:read_file:false")
                && payload.get("tool_name").and_then(serde_json::Value::as_str) == Some("read_file")
    )));

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn run_prompt_reports_hook_timeout_without_failing_turn() {
    let (store, db_path) = test_store("hook-timeout");
    let runtime = Runtime::echo();
    // Use trusted workspace for hook tests since hooks are in workspace-local .ccodex
    let kernel = Kernel::with_trust(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
        WorkspaceTrust::Trusted,
    );

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("ccodex-kernel-hook-timeout-{unique}"));
    std::fs::create_dir_all(workspace.join(".ccodex").join("hooks"))
        .expect("hooks dir should exist");
    std::fs::write(
        workspace.join(".ccodex").join("hooks").join("slow.toml"),
        "event = \"pre_turn\"\ntimeout_ms = 10\ncommand = \"sleep 0.2\"",
    )
    .expect("slow hook should write");

    let result = kernel
        .run_prompt("hello hook timeout", Some(workspace.clone()))
        .await
        .expect("turn should still succeed");
    let stored_turn = store
        .get_turn(&result.turn.id)
        .await
        .expect("turn should load");

    assert!(stored_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::Warning { code, message }
            if code == "hook_timed_out" && message.contains("slow")
    )));
    assert_eq!(result.assistant_text, "Echo: hello hook timeout");

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn run_prompt_triggers_compaction_after_many_turns() {
    let (store, db_path) = test_store("compaction");
    let runtime = Runtime::echo();
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let first = kernel
        .run_prompt("turn one", None)
        .await
        .expect("first turn should succeed");
    for idx in 0..6 {
        kernel
            .resume_prompt(&first.session.id, format!("turn {}", idx + 2))
            .await
            .expect("follow-up turn should succeed");
    }

    let session = store
        .get_session(&first.session.id)
        .await
        .expect("session should load");
    let turns = store
        .list_turns(&first.session.id)
        .await
        .expect("turns should list");
    let last_turn = turns.last().expect("last turn should exist");

    let summary = session
        .metadata
        .get("compaction_summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    assert!(!summary.is_empty());
    assert!(summary.contains("compacted 4 turn(s); kept 3 recent turn(s)"));
    assert!(summary.contains("range="));
    assert!(!summary.contains("range=-..-"));
    assert_eq!(
        session
            .metadata
            .get("compacted_turn_count")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert!(
        session
            .metadata
            .get("last_compacted_at")
            .and_then(serde_json::Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    );
    assert!(last_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "session_compacted"
                && payload
                    .get("compacted_turn_count")
                    .and_then(serde_json::Value::as_u64)
                    == Some(4)
                && payload
                    .get("retained_turn_count")
                    .and_then(serde_json::Value::as_u64)
                    == Some(3)
                && payload
                    .get("first_compacted_turn_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false)
                && payload
                    .get("last_compacted_turn_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false)
                && payload
                    .get("last_compacted_at")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false)
                && payload
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value.contains("compacted 4 turn(s); kept 3 recent turn(s)"))
                    .unwrap_or(false)
    )));

    let _ = std::fs::remove_file(db_path);
}

#[derive(Default)]
struct RecordingProvider {
    requests: Arc<Mutex<Vec<TurnRequest>>>,
}

#[derive(Default)]
struct DelegatingRecordingProvider {
    requests: Arc<Mutex<Vec<TurnRequest>>>,
}

#[async_trait]
impl ModelProviderPort for DelegatingRecordingProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        let mut requests = self.requests.lock().expect("lock should succeed");
        let is_first = requests.is_empty();
        requests.push(request);
        drop(requests);

        if is_first {
            Ok(Box::pin(stream::iter(vec![
                Ok(ProviderEvent::ToolCall(ccodex_protocol::ToolCall {
                    id: ccodex_protocol::ToolCallId::new(),
                    tool_name: "spawn_agent".to_string(),
                    input: json!({
                        "name": "reviewer",
                        "prompt": "read Cargo.toml"
                    }),
                })),
                Ok(ProviderEvent::Completed),
            ])))
        } else {
            Ok(Box::pin(stream::iter(vec![
                Ok(ProviderEvent::AssistantMessageDelta {
                    content: "child done".to_string(),
                }),
                Ok(ProviderEvent::Completed),
            ])))
        }
    }
}

#[async_trait]
impl ModelProviderPort for RecordingProvider {
    async fn start_turn(
        &self,
        request: TurnRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError> {
        self.requests
            .lock()
            .expect("lock should succeed")
            .push(request);
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
async fn resume_prompt_includes_prior_session_items_in_turn_request() {
    let (store, db_path) = test_store("resume-history");
    let runtime = Runtime::echo();
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

    let first = kernel
        .run_prompt("hello one", None)
        .await
        .expect("first turn should succeed");

    kernel
        .resume_prompt(&first.session.id, "hello two")
        .await
        .expect("second turn should succeed");

    let requests = recorded.lock().expect("lock should succeed");
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::UserMessage { content } if content == "hello one"
        )),
        "second turn request should include prior user message"
    );
    assert!(
        requests[1].items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::AssistantMessageDelta { content } if content == "recorded"
        )),
        "second turn request should include prior assistant output"
    );
    assert!(
        requests[1].items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::UserMessage { content } if content == "hello two"
        )),
        "second turn request should include current user message"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn fork_session_clones_session_state_and_turns() {
    let (store, db_path) = test_store("fork-session");
    let runtime = Runtime::bootstrap();
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let original = kernel
        .run_prompt("plan alpha, beta", None)
        .await
        .expect("original turn should succeed");

    let forked = kernel
        .fork_session(&original.session.id)
        .await
        .expect("fork should succeed");

    assert_ne!(forked.id, original.session.id);
    assert_eq!(
        forked
            .metadata
            .get("forked_from_session_id")
            .and_then(serde_json::Value::as_str),
        Some(original.session.id.0.as_str())
    );
    assert_eq!(
        forked.active_plan.as_ref().map(|plan| &plan.session_id),
        Some(&forked.id)
    );

    let original_turns = store
        .list_turns(&original.session.id)
        .await
        .expect("original turns should load");
    let forked_turns = store
        .list_turns(&forked.id)
        .await
        .expect("forked turns should load");

    assert_eq!(forked_turns.len(), original_turns.len());
    assert_eq!(forked_turns[0].items.len(), original_turns[0].items.len());
    assert!(forked_turns[0].items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::UserMessage { content } if content == "plan alpha, beta"
    )));
    assert!(
        forked_turns[0]
            .items
            .iter()
            .any(|item| matches!(item.payload, ItemPayload::PlanEntered { .. }))
    );

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
    std::fs::write(workspace.join("CCODEX.md"), "own instructions")
        .expect("ccodex file should write");
    std::fs::write(workspace.join("CLAUDE.md"), "claude instructions")
        .expect("claude file should write");
    std::fs::create_dir_all(workspace.join("plugins").join("builtin").join("skills"))
        .expect("builtin skills dir should exist");
    std::fs::create_dir_all(workspace.join(".ccodex").join("skills"))
        .expect("project skills dir should exist");
    std::fs::write(
        workspace
            .join("plugins")
            .join("builtin")
            .join("skills")
            .join("repo_overview.md"),
        "# Repo Overview\nSummarize the repository.",
    )
    .expect("builtin skill should write");
    std::fs::write(
        workspace
            .join(".ccodex")
            .join("skills")
            .join("local_focus.md"),
        "# Local Focus\nUse project-local conventions first.",
    )
    .expect("project skill should write");

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
        vec![
            "own instructions".to_string(),
            "claude instructions".to_string(),
            "# Repo Overview\nSummarize the repository.".to_string(),
            "# Local Focus\nUse project-local conventions first.".to_string(),
        ]
    );

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn run_prompt_emits_session_bootstrap_summary_once_per_session() {
    let (store, db_path) = test_store("bootstrap");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("ccodex-kernel-bootstrap-{unique}"));
    std::fs::create_dir_all(workspace.join(".ccodex").join("skills"))
        .expect("skills dir should exist");
    std::fs::create_dir_all(workspace.join(".ccodex").join("hooks"))
        .expect("hooks dir should exist");
    std::fs::create_dir_all(workspace.join(".ccodex").join("mcp")).expect("mcp dir should exist");
    std::fs::write(workspace.join("CCODEX.md"), "bootstrap instruction")
        .expect("instruction should write");
    std::fs::write(
        workspace.join(".ccodex").join("skills").join("review.md"),
        "# Review Skill",
    )
    .expect("skill should write");
    std::fs::write(
        workspace.join(".ccodex").join("hooks").join("prepare.toml"),
        "event = \"pre_turn\"\ncommand = \"echo bootstrap-hook\"",
    )
    .expect("hook should write");
    std::fs::write(
        workspace.join(".ccodex").join("mcp").join("echo.toml"),
        "command = \"python3\"\nargs = [\"-c\", \"print('{}')\"]",
    )
    .expect("mcp config should write");

    let runtime = Runtime::echo();
    let kernel = Kernel::new(
        store.clone(),
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    );

    let first = kernel
        .run_prompt("hello bootstrap", Some(workspace.clone()))
        .await
        .expect("turn should succeed");
    let first_turn = store
        .get_turn(&first.turn.id)
        .await
        .expect("turn should load");
    assert!(first_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, payload }
            if name == "session_bootstrap"
                && payload.get("instruction_count").and_then(serde_json::Value::as_u64) == Some(1)
                && payload.get("skill_count").and_then(serde_json::Value::as_u64) == Some(1)
                && payload.get("hook_count").and_then(serde_json::Value::as_u64) == Some(1)
                && payload.get("mcp_server_count").and_then(serde_json::Value::as_u64) == Some(1)
                && payload
                    .get("skill_names")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.iter().any(|value| value.as_str() == Some("review")))
                    .unwrap_or(false)
                && payload
                    .get("hook_names")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.iter().any(|value| value.as_str() == Some("prepare")))
                    .unwrap_or(false)
                && payload
                    .get("mcp_server_names")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.iter().any(|value| value.as_str() == Some("echo")))
                    .unwrap_or(false)
    )));

    let first_session = store
        .get_session(&first.session.id)
        .await
        .expect("session should load");
    assert!(
        first_session
            .metadata
            .get("bootstrap_skill_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().any(|value| value.as_str() == Some("review")))
            .unwrap_or(false)
    );
    assert!(
        first_session
            .metadata
            .get("bootstrap_hook_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().any(|value| value.as_str() == Some("prepare")))
            .unwrap_or(false)
    );
    assert!(
        first_session
            .metadata
            .get("bootstrap_mcp_server_names")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().any(|value| value.as_str() == Some("echo")))
            .unwrap_or(false)
    );
    assert!(
        first_session
            .metadata
            .get("bootstrapped_at")
            .and_then(serde_json::Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    );

    let second = kernel
        .resume_prompt(&first.session.id, "hello again")
        .await
        .expect("resume should succeed");
    let second_turn = store
        .get_turn(&second.turn.id)
        .await
        .expect("turn should load");
    assert!(!second_turn.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::SystemEvent { name, .. } if name == "session_bootstrap"
    )));

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn subagent_named_agent_instructions_are_injected_into_child_turn_request() {
    let (store, db_path) = test_store("named-agent");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("ccodex-kernel-agent-{unique}"));
    std::fs::create_dir_all(workspace.join(".claude").join("agents"))
        .expect("claude agents dir should exist");
    std::fs::write(
        workspace.join(".claude").join("agents").join("reviewer.md"),
        "# Reviewer\nReview carefully and cite concrete file evidence.",
    )
    .expect("agent file should write");

    let runtime = Runtime::bootstrap();
    let provider = DelegatingRecordingProvider::default();
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
        .run_prompt(
            "delegate reviewer | read Cargo.toml",
            Some(workspace.clone()),
        )
        .await
        .expect("turn should succeed");

    let requests = recorded.lock().expect("lock should succeed");
    assert_eq!(requests.len(), 3);
    let child_request = requests
        .iter()
        .find(|request| request.session.metadata.contains_key("parent_session_id"))
        .expect("child request should be recorded");
    assert!(child_request.project_instructions.iter().any(|item| {
        item.contains(
            "Subagent instructions:\n# Reviewer\nReview carefully and cite concrete file evidence.",
        )
    }));
    assert!(child_request.items.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::UserMessage { content } if content == "delegate reviewer | read Cargo.toml"
    )));
    assert_eq!(
        child_request
            .session
            .metadata
            .get("subagent_forked")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(workspace);
}
