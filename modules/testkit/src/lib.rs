//! Small bootstrap harness for end-to-end kernel tests.

use std::path::PathBuf;
use std::sync::Arc;

use ccodex_kernel::{Kernel, KernelError, RunTurnResult};
use ccodex_protocol::SessionId;
use ccodex_runtime::Runtime;
use ccodex_store::{SQLiteSessionStore, SessionStore, StoredTurn};

pub struct BootstrapHarness {
    pub store: Arc<SQLiteSessionStore>,
    pub kernel: Kernel,
    db_path: PathBuf,
    workspace_root: Option<PathBuf>,
}

impl BootstrapHarness {
    pub fn new(label: &str) -> Self {
        Self::with_runtime_and_workspace(label, Runtime::bootstrap(), std::env::current_dir().ok())
    }

    pub fn with_runtime_and_workspace(
        label: &str,
        runtime: Runtime,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("ccodex-testkit-{label}-{unique}.sqlite3"));
        let store = Arc::new(SQLiteSessionStore::new(&db_path).expect("store should initialize"));
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        Self {
            store,
            kernel,
            db_path,
            workspace_root,
        }
    }

    pub async fn run(&self, prompt: &str) -> Result<RunTurnResult, KernelError> {
        self.kernel
            .agent_runtime()
            .start_session(prompt.to_string(), self.workspace_root.clone())
            .await
    }

    pub async fn resume(
        &self,
        session_id: &SessionId,
        prompt: &str,
    ) -> Result<RunTurnResult, KernelError> {
        self.kernel
            .agent_runtime()
            .continue_session(session_id, prompt.to_string())
            .await
    }

    pub async fn fork(
        &self,
        session_id: &SessionId,
    ) -> Result<ccodex_protocol::Session, KernelError> {
        self.kernel.agent_runtime().fork_session(session_id).await
    }

    pub async fn turns(&self, session_id: &SessionId) -> Vec<StoredTurn> {
        self.store
            .list_turns(session_id)
            .await
            .expect("turns should list")
    }

    pub async fn session(&self, session_id: &SessionId) -> ccodex_protocol::Session {
        self.store
            .get_session(session_id)
            .await
            .expect("session should load")
    }
}

impl Drop for BootstrapHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.db_path);
    }
}

#[cfg(test)]
mod tests {
    use ccodex_protocol::ItemPayload;
    use ccodex_runtime::{
        ApprovalPolicy, ApprovalRules, ProviderConfig, ProviderKind, Runtime, RuntimeConfig,
        SandboxMode,
    };
    use ccodex_store::{ListSessionsParams, SessionStore};

    use super::BootstrapHarness;

    #[tokio::test]
    async fn harness_runs_bootstrap_plan_scenario() {
        let harness = BootstrapHarness::new("plan");
        let result = harness
            .run("plan define protocol, implement runtime, build tui")
            .await
            .expect("plan run should succeed");

        assert_eq!(
            result.assistant_text,
            "Updated the current plan with 3 item(s)."
        );
        assert_eq!(
            result
                .session
                .active_plan
                .as_ref()
                .expect("plan should exist")
                .items
                .len(),
            3
        );
        let turn = harness
            .store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                    && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("update_plan")
                    && delta.get("status").and_then(serde_json::Value::as_str) == Some("entered")
                    && delta.get("plan_item_count").and_then(serde_json::Value::as_u64) == Some(3)
        )));
    }

    #[tokio::test]
    async fn harness_resumes_existing_session() {
        let harness = BootstrapHarness::new("resume");
        let first = harness
            .run("hello harness")
            .await
            .expect("first run should succeed");
        let second = harness
            .resume(&first.session.id, "hello again")
            .await
            .expect("resume should succeed");

        let turns = harness.turns(&first.session.id).await;
        assert_eq!(first.session.id, second.session.id);
        assert_eq!(turns.len(), 2);
    }

    #[tokio::test]
    async fn harness_covers_ask_user_flow() {
        let harness = BootstrapHarness::new("ask-user");
        let result = harness
            .run("ask Choose deployment | staging, production")
            .await
            .expect("ask-user run should succeed");

        let turn = harness
            .store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");

        assert_eq!(result.assistant_text, "Captured user choice: choice-1");
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("waiting_for_user")
        )));
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                    && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("ask_user")
                    && delta.get("status").and_then(serde_json::Value::as_str) == Some("resolved")
                    && delta.get("resolution_mode").and_then(serde_json::Value::as_str) == Some("choice")
        )));
        assert!(
            turn.items
                .iter()
                .any(|item| matches!(item.payload, ItemPayload::AskUserRequested { .. }))
        );
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallFinished { result }
                if result
                    .output
                    .get("resolution_mode")
                    .and_then(serde_json::Value::as_str) == Some("choice")
        )));
    }

    #[tokio::test]
    async fn harness_covers_subagent_flow() {
        let harness = BootstrapHarness::new("subagent");
        let result = harness
            .run("delegate read Cargo.toml")
            .await
            .expect("delegate should succeed");

        let sessions = harness
            .store
            .list_sessions(ListSessionsParams { limit: Some(10) })
            .await
            .expect("sessions should list");
        let turn = harness
            .store
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
        assert_eq!(
            child_session
                .metadata
                .get("parent_session_id")
                .and_then(serde_json::Value::as_str),
            Some(result.session.id.0.as_str())
        );
        assert_eq!(
            child_session
                .metadata
                .get("subagent_forked")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            child_session
                .metadata
                .get("subagent_depth")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            child_session
                .metadata
                .get("lineage_root_session_id")
                .and_then(serde_json::Value::as_str),
            Some(result.session.id.0.as_str())
        );
        assert_eq!(
            child_session
                .metadata
                .get("parent_agent_name")
                .and_then(serde_json::Value::as_str),
            Some("primary")
        );
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("delegating")
        )));
        assert!(turn.items.iter().any(|item| matches!(
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
        assert!(turn.items.iter().any(|item| matches!(
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
        assert!(turn.items.iter().any(|item| matches!(
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
        let child_turns = harness.turns(&child_session.id).await;
        assert_eq!(child_turns.len(), 1);
        assert!(child_turns[0].items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::UserMessage { content } if content == "read Cargo.toml"
        )));
        assert!(
            child_turns
                .iter()
                .any(|stored| stored.items.iter().any(|item| matches!(
                    &item.payload,
                    ItemPayload::AssistantMessageDelta { .. }
                        | ItemPayload::ToolCallFinished { .. }
                )))
        );
    }

    #[tokio::test]
    async fn harness_covers_compaction_flow() {
        let harness = BootstrapHarness::with_runtime_and_workspace(
            "compaction",
            Runtime::echo(),
            std::env::current_dir().ok(),
        );
        let first = harness
            .run("hello compact")
            .await
            .expect("first turn should succeed");

        for index in 0..6 {
            let prompt = format!("continue compact {index}");
            harness
                .resume(&first.session.id, &prompt)
                .await
                .expect("resume should succeed");
        }

        let session = harness
            .store
            .get_session(&first.session.id)
            .await
            .expect("session should load");
        let summary = session
            .metadata
            .get("compaction_summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        assert!(!summary.is_empty());
        assert!(summary.contains("compacted 4 turn(s); kept 3 recent turn(s)"));
    }

    #[tokio::test]
    async fn harness_covers_session_fork_flow() {
        let harness = BootstrapHarness::new("fork");
        let first = harness
            .run("plan alpha, beta")
            .await
            .expect("first run should succeed");

        let forked = harness
            .fork(&first.session.id)
            .await
            .expect("fork should succeed");
        let forked_turns = harness.turns(&forked.id).await;

        assert_ne!(forked.id, first.session.id);
        assert_eq!(
            forked
                .metadata
                .get("forked_from_session_id")
                .and_then(serde_json::Value::as_str),
            Some(first.session.id.0.as_str())
        );
        assert_eq!(forked_turns.len(), 1);
        assert!(forked_turns[0].items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::UserMessage { content } if content == "plan alpha, beta"
        )));
    }

    #[tokio::test]
    async fn harness_covers_approval_rule_rejection() {
        let workspace = std::env::current_dir().ok();
        let runtime = Runtime::from_config(RuntimeConfig {
            workspace_root: workspace.clone().unwrap_or_else(|| ".".into()),
            approval_policy: ApprovalPolicy::Ask,
            approval_rules: ApprovalRules {
                deny_commands: vec!["pwd".to_string()],
                ..ApprovalRules::default()
            },
            sandbox_mode: SandboxMode::WorkspaceWrite,
            provider: ProviderConfig {
                kind: ProviderKind::Bootstrap,
                base_url: None,
                api_key: None,
                model: ccodex_brand::DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
        });
        let harness =
            BootstrapHarness::with_runtime_and_workspace("approval-rules", runtime, workspace);

        let result = harness.run("bash pwd").await.expect("run should succeed");
        let turn = harness
            .store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");

        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("approval_assessed")
                    && delta.get("risk").and_then(serde_json::Value::as_str).is_some()
        )));
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ApprovalResolved { response }
                if response.reason.as_deref() == Some("rejected by approval rule")
                    && response.reason_code == Some(ccodex_protocol::ApprovalReasonCode::RuleDeny)
        )));
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallFinished { result } if result.is_error
        )));
    }

    #[tokio::test]
    async fn harness_captures_structured_approval_context() {
        let workspace = std::env::current_dir().ok();
        let runtime = Runtime::from_config(RuntimeConfig {
            workspace_root: workspace.clone().unwrap_or_else(|| ".".into()),
            approval_policy: ApprovalPolicy::Ask,
            approval_rules: ApprovalRules {
                deny_commands: vec!["curl".to_string()],
                ..ApprovalRules::default()
            },
            sandbox_mode: SandboxMode::WorkspaceWrite,
            provider: ProviderConfig {
                kind: ProviderKind::Bootstrap,
                base_url: None,
                api_key: None,
                model: ccodex_brand::DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
        });
        let harness =
            BootstrapHarness::with_runtime_and_workspace("approval-context", runtime, workspace);

        let result = harness
            .run("bash curl https://example.com")
            .await
            .expect("run should succeed");
        let turn = harness
            .store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");

        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("approval_assessed")
                    && delta.get("command").and_then(serde_json::Value::as_str)
                        == Some("curl https://example.com")
                    && delta.get("has_network_access").and_then(serde_json::Value::as_bool)
                        == Some(true)
        )));
        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallDelta { delta, .. }
                if delta.get("phase").and_then(serde_json::Value::as_str) == Some("completed")
                    && delta.get("is_error").and_then(serde_json::Value::as_bool) == Some(true)
                    && delta.get("tool_name").and_then(serde_json::Value::as_str) == Some("bash")
                    && delta.get("status").and_then(serde_json::Value::as_str) == Some("rejected")
        )));
    }

    #[tokio::test]
    async fn harness_covers_mcp_roundtrip() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("ccodex-testkit-mcp-{unique}"));
        std::fs::create_dir_all(workspace.join(".ccodex").join("mcp"))
            .expect("mcp dir should exist");
        std::fs::write(
            workspace.join(".ccodex").join("mcp").join("echo.toml"),
            r#"
command = "python3"
args = ["-c", "import json, os; print(json.dumps({'server': os.environ['CCODEX_MCP_SERVER'], 'tool': os.environ['CCODEX_MCP_TOOL'], 'input': json.loads(os.environ['CCODEX_MCP_INPUT'])}))"]
"#,
        )
        .expect("mcp config should write");

        let harness = BootstrapHarness::with_runtime_and_workspace(
            "mcp",
            Runtime::from_config(RuntimeConfig {
                workspace_root: workspace.clone(),
                approval_policy: ApprovalPolicy::AlwaysApprove,
                approval_rules: ApprovalRules::default(),
                sandbox_mode: SandboxMode::WorkspaceWrite,
                provider: ProviderConfig {
                    kind: ProviderKind::Bootstrap,
                    base_url: None,
                    api_key: None,
                    model: ccodex_brand::DEFAULT_MODEL.to_string(),
                    max_output_tokens: 16_000,
                },
            }),
            Some(workspace.clone()),
        );
        let result = harness
            .run(r#"mcp echo inspect {"path":"Cargo.toml"}"#)
            .await
            .expect("mcp run should succeed");
        let turn = harness
            .store
            .get_turn(&result.turn.id)
            .await
            .expect("turn should load");

        assert!(turn.items.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolCallFinished { result }
                if result
                    .output
                    .get("input")
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str)
                    == Some("Cargo.toml")
        )));

        let _ = std::fs::remove_dir_all(workspace);
    }
}
