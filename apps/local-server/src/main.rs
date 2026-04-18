use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::TcpListener;

use ccodex_brand::{DISPLAY_NAME, LOCAL_SERVER_ADDR_ENV};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:48765";

mod routes;
mod state;
mod transport;

use state::ServerState;
use transport::handle_connection;

#[tokio::main]
async fn main() -> Result<()> {
    let bind_addr =
        std::env::var(LOCAL_SERVER_ADDR_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let bind_addr: SocketAddr = bind_addr.parse()?;
    let listener = TcpListener::bind(bind_addr).await?;
    let state = Arc::new(ServerState::bootstrap()?);

    println!("{} local-server listening on {}", DISPLAY_NAME, bind_addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, state).await {
                eprintln!("connection error: {err}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use ccodex_protocol::{
        ApprovalDecision, ApprovalResponse, AskUserResponse, ItemPayload, LocalServerRequest,
        LocalServerRequestBody, LocalServerResponse, LocalServerResponseBody, TranscriptFormat,
    };
    use ccodex_runtime::{
        ApprovalPolicy, ApprovalRules, ProviderConfig, ProviderKind, Runtime, RuntimeConfig,
        SandboxMode,
    };
    use tokio::net::TcpListener;
    use tungstenite::{Message, connect};

    use super::{ServerState, handle_connection};

    fn temp_workspace(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-local-server-{label}-{unique}"));
        std::fs::create_dir_all(&root).expect("temp workspace should exist");
        root
    }

    #[tokio::test]
    async fn ping_request_returns_server_info() {
        let workspace = temp_workspace("ping");
        let state =
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap");
        let response = state
            .handle_request(LocalServerRequest {
                id: "1".to_string(),
                body: LocalServerRequestBody::Ping,
            })
            .await;
        let run = state
            .handle_request(LocalServerRequest {
                id: "1-run".to_string(),
                body: LocalServerRequestBody::RunPrompt {
                    prompt: "hello bootstrap metadata".to_string(),
                },
            })
            .await;

        let session_id = match run.body {
            LocalServerResponseBody::TurnResult(result) => result.session.id,
            other => panic!("unexpected response: {other:?}"),
        };
        let session_response = state
            .handle_request(LocalServerRequest {
                id: "1-session".to_string(),
                body: LocalServerRequestBody::GetSession {
                    session_id: session_id.clone(),
                },
            })
            .await;

        match session_response.body {
            LocalServerResponseBody::Session { session } => {
                assert_eq!(
                    session
                        .metadata
                        .get("bootstrap_complete")
                        .and_then(serde_json::Value::as_bool),
                    Some(true)
                );
                assert!(
                    session
                        .metadata
                        .get("bootstrapped_at")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| !value.is_empty())
                        .unwrap_or(false)
                );
                assert!(
                    session
                        .metadata
                        .get("bootstrap_instruction_sources")
                        .and_then(serde_json::Value::as_array)
                        .is_some()
                );
            }
            other => panic!("unexpected session response: {other:?}"),
        }

        let turns_response = state
            .handle_request(LocalServerRequest {
                id: "1-turns".to_string(),
                body: LocalServerRequestBody::GetTurns { session_id },
            })
            .await;
        match turns_response.body {
            LocalServerResponseBody::Turns { turns } => {
                assert!(turns.iter().flat_map(|turn| turn.items.iter()).any(|item| matches!(
                    &item.payload,
                    ItemPayload::SystemEvent { name, payload }
                        if name == "session_bootstrap"
                            && payload.get("instruction_count").and_then(serde_json::Value::as_u64).is_some()
                            && payload.get("skill_names").and_then(serde_json::Value::as_array).is_some()
                )));
            }
            other => panic!("unexpected turns response: {other:?}"),
        }

        match response.body {
            LocalServerResponseBody::Pong(info) => {
                assert_eq!(info.protocol, "ccodex.local.v1");
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn run_and_export_session_over_request_handler() {
        let workspace = temp_workspace("export");
        std::fs::create_dir_all(workspace.join("plugins").join("builtin").join("skills"))
            .expect("builtin skills dir should exist");
        std::fs::write(
            workspace
                .join("plugins")
                .join("builtin")
                .join("skills")
                .join("repo_overview.md"),
            "# Repo Overview",
        )
        .expect("builtin skill should write");
        let state =
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap");
        let run = state
            .handle_request(LocalServerRequest {
                id: "2".to_string(),
                body: LocalServerRequestBody::RunPrompt {
                    prompt: "plan alpha, beta".to_string(),
                },
            })
            .await;

        let session_id = match run.body {
            LocalServerResponseBody::TurnResult(result) => result.session.id,
            other => panic!("unexpected response: {other:?}"),
        };

        let export = state
            .handle_request(LocalServerRequest {
                id: "3".to_string(),
                body: LocalServerRequestBody::ExportSession {
                    session_id,
                    format: TranscriptFormat::Markdown,
                },
            })
            .await;

        match export.body {
            LocalServerResponseBody::Transcript { content, .. } => {
                assert!(content.contains("Session"));
                assert!(content.contains("PlanEntered"));
                assert!(content.contains("ToolDelta:"));
                assert!(content.contains("phase=completed"));
                assert!(content.contains("plan_item_count=2"));
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn list_extensions_returns_discovered_manifests() {
        let workspace = temp_workspace("extensions");
        std::fs::create_dir_all(workspace.join(".ccodex").join("skills"))
            .expect("project skills dir should exist");
        std::fs::write(
            workspace.join(".ccodex").join("skills").join("review.md"),
            "# Review Skill",
        )
        .expect("project skill should write");

        let state =
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap");
        let response = state
            .handle_request(LocalServerRequest {
                id: "4".to_string(),
                body: LocalServerRequestBody::ListExtensions,
            })
            .await;

        match response.body {
            LocalServerResponseBody::Extensions { manifests } => {
                assert!(manifests.iter().any(|manifest| manifest.name == "review"));
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn list_and_call_mcp_servers_over_request_handler() {
        let workspace = temp_workspace("mcp");
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

        let state =
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap");

        let listed = state
            .handle_request(LocalServerRequest {
                id: "mcp-list".to_string(),
                body: LocalServerRequestBody::ListMcpServers,
            })
            .await;
        match listed.body {
            LocalServerResponseBody::McpServers { servers } => {
                assert_eq!(servers.len(), 1);
                assert_eq!(servers[0].name, "echo");
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let called = state
            .handle_request(LocalServerRequest {
                id: "mcp-call".to_string(),
                body: LocalServerRequestBody::CallMcpTool {
                    server: "echo".to_string(),
                    tool: "inspect".to_string(),
                    input: serde_json::json!({ "path": "Cargo.toml" }),
                },
            })
            .await;
        match called.body {
            LocalServerResponseBody::McpResult {
                server,
                tool,
                output,
            } => {
                assert_eq!(server, "echo");
                assert_eq!(tool, "inspect");
                assert_eq!(
                    output
                        .get("input")
                        .and_then(|value| value.get("path"))
                        .and_then(|value| value.as_str()),
                    Some("Cargo.toml")
                );
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn get_turns_returns_persisted_items() {
        let workspace = temp_workspace("turns");
        let state =
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap");

        let run = state
            .handle_request(LocalServerRequest {
                id: "5".to_string(),
                body: LocalServerRequestBody::RunPrompt {
                    prompt: "read Cargo.toml".to_string(),
                },
            })
            .await;

        let session_id = match run.body {
            LocalServerResponseBody::TurnResult(result) => result.session.id,
            other => panic!("unexpected response: {other:?}"),
        };

        let turns = state
            .handle_request(LocalServerRequest {
                id: "6".to_string(),
                body: LocalServerRequestBody::GetTurns { session_id },
            })
            .await;

        match turns.body {
            LocalServerResponseBody::Turns { turns } => {
                assert!(!turns.is_empty());
                assert!(
                    turns
                        .iter()
                        .flat_map(|turn| turn.items.iter())
                        .any(|item| matches!(
                            &item.payload,
                            ccodex_protocol::ItemPayload::ToolCallDelta { delta, .. }
                                if delta.get("phase").and_then(serde_json::Value::as_str)
                                    == Some("completed")
                                    && delta.get("tool_name").and_then(serde_json::Value::as_str)
                                        == Some("read_file")
                                    && delta.get("is_error").and_then(serde_json::Value::as_bool)
                                        == Some(false)
                        ))
                );
                assert!(
                    turns
                        .iter()
                        .flat_map(|turn| turn.items.iter())
                        .any(|item| matches!(
                            item.payload,
                            ccodex_protocol::ItemPayload::ToolCallFinished { .. }
                        ))
                );
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn fork_session_clones_turn_history() {
        let workspace = temp_workspace("fork");
        let state = ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::echo())
            .expect("state should bootstrap");

        let run = state
            .handle_request(LocalServerRequest {
                id: "fork-run".to_string(),
                body: LocalServerRequestBody::RunPrompt {
                    prompt: "hello fork".to_string(),
                },
            })
            .await;

        let original_session_id = match run.body {
            LocalServerResponseBody::TurnResult(result) => result.session.id,
            other => panic!("unexpected response: {other:?}"),
        };

        let forked = state
            .handle_request(LocalServerRequest {
                id: "fork-session".to_string(),
                body: LocalServerRequestBody::ForkSession {
                    session_id: original_session_id.clone(),
                },
            })
            .await;

        let forked_session = match forked.body {
            LocalServerResponseBody::Session { session } => session,
            other => panic!("unexpected response: {other:?}"),
        };

        assert_ne!(forked_session.id, original_session_id);
        assert_eq!(
            forked_session
                .metadata
                .get("forked_from_session_id")
                .and_then(serde_json::Value::as_str),
            Some(original_session_id.to_string().as_str())
        );

        let turns = state
            .handle_request(LocalServerRequest {
                id: "fork-turns".to_string(),
                body: LocalServerRequestBody::GetTurns {
                    session_id: forked_session.id.clone(),
                },
            })
            .await;

        match turns.body {
            LocalServerResponseBody::Turns { turns } => {
                assert_eq!(turns.len(), 1);
                assert!(turns[0].items.iter().any(|item| matches!(
                    &item.payload,
                    ItemPayload::UserMessage { content } if content == "hello fork"
                )));
                assert!(turns[0].items.iter().any(|item| matches!(
                    &item.payload,
                    ItemPayload::AssistantMessageDelta { content } if content == "Echo: hello fork"
                )));
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn event_broadcast_captures_turn_updates() {
        let workspace = temp_workspace("events");
        let state = ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::echo())
            .expect("state should bootstrap");
        let mut receiver = state.events.subscribe();

        let response = state
            .handle_request(LocalServerRequest {
                id: "events-1".to_string(),
                body: LocalServerRequestBody::RunPrompt {
                    prompt: "hello events".to_string(),
                },
            })
            .await;

        match response.body {
            LocalServerResponseBody::TurnResult(result) => {
                assert_eq!(result.assistant_text, "Echo: hello events");
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let mut saw_turn_finished = false;
        let mut saw_item_appended = false;
        for _ in 0..8 {
            let event = receiver.recv().await.expect("event should broadcast");
            match event {
                ccodex_protocol::ProtocolEvent::TurnFinished(_) => saw_turn_finished = true,
                ccodex_protocol::ProtocolEvent::ItemAppended(_) => saw_item_appended = true,
                _ => {}
            }
            if saw_turn_finished && saw_item_appended {
                break;
            }
        }

        assert!(saw_turn_finished);
        assert!(saw_item_appended);

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn resolve_approval_unblocks_waiting_turn() {
        let workspace = temp_workspace("approval-resolution");
        let runtime = Runtime::from_config(RuntimeConfig {
            workspace_root: workspace.clone(),
            approval_policy: ApprovalPolicy::Ask,
            approval_rules: ApprovalRules::default(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            provider: ProviderConfig {
                kind: ProviderKind::Bootstrap,
                base_url: None,
                api_key: None,
                model: ccodex_brand::DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
        });
        let state = Arc::new(
            ServerState::for_workspace_with_runtime(workspace.clone(), runtime)
                .expect("state should bootstrap"),
        );
        let mut receiver = state.events.subscribe();

        let run_state = state.clone();
        let run = tokio::spawn(async move {
            run_state
                .handle_request(LocalServerRequest {
                    id: "approval-run".to_string(),
                    body: LocalServerRequestBody::RunPrompt {
                        prompt: "bash pwd".to_string(),
                    },
                })
                .await
        });

        let request_item_id = loop {
            if let ccodex_protocol::ProtocolEvent::ItemAppended(item) =
                receiver.recv().await.expect("event should arrive")
            {
                if let ItemPayload::ApprovalRequested { request } = item.payload {
                    break request.item_id;
                }
            }
        };

        let pending = state
            .handle_request(LocalServerRequest {
                id: "approval-list".to_string(),
                body: LocalServerRequestBody::ListPendingInteractions,
            })
            .await;
        match pending.body {
            LocalServerResponseBody::PendingInteractions {
                approvals,
                ask_user,
            } => {
                assert_eq!(approvals.len(), 1);
                assert_eq!(ask_user.len(), 0);
                assert_eq!(approvals[0].item_id, request_item_id);
                assert_eq!(
                    approvals[0].kind,
                    ccodex_protocol::ApprovalKind::CommandExecution
                );
                assert_eq!(approvals[0].risk, ccodex_protocol::ApprovalRisk::Medium);
                assert_eq!(approvals[0].context.tool_name.as_deref(), Some("bash"));
                assert_eq!(approvals[0].context.command.as_deref(), Some("pwd"));
                assert_eq!(approvals[0].context.has_network_access, false);
                assert_eq!(approvals[0].context.is_destructive, false);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let resolved = state
            .handle_request(LocalServerRequest {
                id: "approval-resolve".to_string(),
                body: LocalServerRequestBody::ResolveApproval {
                    response: ApprovalResponse::new(
                        request_item_id.clone(),
                        ApprovalDecision::Approved,
                        Some("approved in local-server test".to_string()),
                        Some(ccodex_protocol::ApprovalReasonCode::InteractiveApproved),
                    ),
                },
            })
            .await;

        match resolved.body {
            LocalServerResponseBody::InteractionResolved {
                request_item_id: id,
            } => {
                assert_eq!(id, request_item_id);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let response = run.await.expect("task should join");
        match response.body {
            LocalServerResponseBody::TurnResult(result) => {
                assert!(!result.assistant_text.is_empty());
                assert!(result.events.iter().any(|event| matches!(
                    event,
                    ccodex_protocol::ProtocolEvent::ItemAppended(item)
                        if matches!(
                            &item.payload,
                            ItemPayload::ApprovalResolved { response }
                                if response.decision == ApprovalDecision::Approved
                                    && response.reason_code
                                        == Some(ccodex_protocol::ApprovalReasonCode::InteractiveApproved)
                                    && response.reason.as_deref()
                                        == Some("approved in local-server test")
                        )
                )));
                assert!(result.events.iter().any(|event| matches!(
                    event,
                    ccodex_protocol::ProtocolEvent::ItemAppended(item)
                        if matches!(
                            &item.payload,
                            ItemPayload::ToolCallFinished { result: tool_result }
                                if !tool_result.is_error
                        )
                )));
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn resolve_ask_user_unblocks_waiting_turn() {
        let workspace = temp_workspace("ask-user-resolution");
        let runtime = Runtime::from_config(RuntimeConfig {
            workspace_root: workspace.clone(),
            approval_policy: ApprovalPolicy::Ask,
            approval_rules: ApprovalRules::default(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            provider: ProviderConfig {
                kind: ProviderKind::Bootstrap,
                base_url: None,
                api_key: None,
                model: ccodex_brand::DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
        });
        let state = Arc::new(
            ServerState::for_workspace_with_runtime(workspace.clone(), runtime)
                .expect("state should bootstrap"),
        );
        let mut receiver = state.events.subscribe();

        let run_state = state.clone();
        let run = tokio::spawn(async move {
            run_state
                .handle_request(LocalServerRequest {
                    id: "ask-run".to_string(),
                    body: LocalServerRequestBody::RunPrompt {
                        prompt: "ask Choose deployment | staging, production".to_string(),
                    },
                })
                .await
        });

        let request_item_id = loop {
            if let ccodex_protocol::ProtocolEvent::ItemAppended(item) =
                receiver.recv().await.expect("event should arrive")
            {
                if let ItemPayload::AskUserRequested { prompt } = item.payload {
                    break prompt.item_id;
                }
            }
        };

        let pending = state
            .handle_request(LocalServerRequest {
                id: "ask-list".to_string(),
                body: LocalServerRequestBody::ListPendingInteractions,
            })
            .await;
        match pending.body {
            LocalServerResponseBody::PendingInteractions {
                approvals,
                ask_user,
            } => {
                assert_eq!(approvals.len(), 0);
                assert_eq!(ask_user.len(), 1);
                assert_eq!(ask_user[0].item_id, request_item_id);
                assert_eq!(ask_user[0].title, "Choose deployment");
                assert_eq!(ask_user[0].allow_freeform, false);
                assert_eq!(ask_user[0].choices.len(), 2);
                assert_eq!(ask_user[0].choices[0].label, "staging");
                assert_eq!(ask_user[0].choices[1].label, "production");
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let resolved = state
            .handle_request(LocalServerRequest {
                id: "ask-resolve".to_string(),
                body: LocalServerRequestBody::ResolveAskUser {
                    response: AskUserResponse {
                        request_item_id: request_item_id.clone(),
                        selected_choice_id: Some("choice-2".to_string()),
                        freeform_text: None,
                    },
                },
            })
            .await;

        match resolved.body {
            LocalServerResponseBody::InteractionResolved {
                request_item_id: id,
            } => {
                assert_eq!(id, request_item_id);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let response = run.await.expect("task should join");
        match response.body {
            LocalServerResponseBody::TurnResult(result) => {
                assert_eq!(result.assistant_text, "Captured user choice: choice-2");
                assert!(result.events.iter().any(|event| matches!(
                    event,
                    ccodex_protocol::ProtocolEvent::ItemAppended(item)
                        if matches!(
                            &item.payload,
                            ItemPayload::ToolCallDelta { delta, .. }
                                if delta.get("phase").and_then(serde_json::Value::as_str)
                                    == Some("completed")
                                    && delta.get("tool_name").and_then(serde_json::Value::as_str)
                                        == Some("ask_user")
                                    && delta.get("status").and_then(serde_json::Value::as_str)
                                        == Some("resolved")
                                    && delta.get("resolution_mode").and_then(serde_json::Value::as_str)
                                        == Some("choice")
                        )
                )));
                assert!(result.events.iter().any(|event| matches!(
                    event,
                    ccodex_protocol::ProtocolEvent::ItemAppended(item)
                        if matches!(
                            &item.payload,
                            ItemPayload::ToolCallFinished { result: tool_result }
                                if tool_result
                                    .output
                                    .get("resolution_mode")
                                    .and_then(serde_json::Value::as_str)
                                    == Some("choice")
                        )
                )));
            }
            other => panic!("unexpected response: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn websocket_ping_roundtrip_succeeds() {
        let workspace = temp_workspace("websocket-ping");
        let state = Arc::new(
            ServerState::for_workspace_with_runtime(workspace.clone(), Runtime::bootstrap())
                .expect("state should bootstrap"),
        );
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!(
                    "skipping websocket ping test: local bind not permitted in this environment"
                );
                let _ = std::fs::remove_dir_all(workspace);
                return;
            }
            Err(error) => panic!("listener should bind: {error}"),
        };
        let addr = listener.local_addr().expect("addr should resolve");
        let server_state = state.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept should work");
            handle_connection(stream, server_state)
                .await
                .expect("websocket connection should succeed");
        });

        let response = tokio::task::spawn_blocking(move || {
            let (mut socket, _) =
                connect(format!("ws://{addr}")).expect("websocket client should connect");
            let request = LocalServerRequest {
                id: "ws-ping".to_string(),
                body: LocalServerRequestBody::Ping,
            };
            socket
                .send(Message::Text(
                    serde_json::to_string(&request).expect("request should serialize"),
                ))
                .expect("request should send");

            loop {
                match socket.read().expect("response should read") {
                    Message::Text(text) => {
                        break serde_json::from_str::<LocalServerResponse>(&text)
                            .expect("response should deserialize");
                    }
                    Message::Ping(payload) => {
                        socket
                            .send(Message::Pong(payload))
                            .expect("pong should send");
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("client task should complete");

        match response.body {
            LocalServerResponseBody::Pong(info) => {
                assert_eq!(info.protocol, "ccodex.local.v1");
            }
            other => panic!("unexpected response: {other:?}"),
        }

        server.await.expect("server should join");
        let _ = std::fs::remove_dir_all(workspace);
    }
}
