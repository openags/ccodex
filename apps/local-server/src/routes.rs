use ccodex_brand::{DISPLAY_NAME, VERSION};
use ccodex_compat::CompatLayer;
use ccodex_extensions::ExtensionRegistry;
use ccodex_kernel::RunTurnResult;
use ccodex_protocol::McpPort;
use ccodex_protocol::{
    LocalServerMcpServer, LocalServerRequest, LocalServerRequestBody, LocalServerResponse,
    LocalServerResponseBody, LocalServerStoredTurn, LocalServerTurnResult, ServerInfo,
};
use ccodex_runtime::CommandBackedMcpPort;
use ccodex_store::{
    JsonlTranscriptExporter, ListSessionsParams, MarkdownTranscriptExporter, SessionStore,
    TranscriptExporter,
};

use crate::state::{ServerState, error_body};

impl ServerState {
    pub(crate) async fn handle_request(&self, request: LocalServerRequest) -> LocalServerResponse {
        let body = match request.body {
            LocalServerRequestBody::Ping => LocalServerResponseBody::Pong(ServerInfo {
                product: DISPLAY_NAME.to_string(),
                version: VERSION.to_string(),
                protocol: "ccodex.local.v1".to_string(),
            }),
            LocalServerRequestBody::SubscribeEvents => LocalServerResponseBody::Subscribed,
            LocalServerRequestBody::ListPendingInteractions => {
                LocalServerResponseBody::PendingInteractions {
                    approvals: self.interactions.list_pending_approvals().await,
                    ask_user: self.interactions.list_pending_ask_user().await,
                }
            }
            LocalServerRequestBody::ResolveApproval { response } => {
                if self.interactions.resolve_approval(response.clone()).await {
                    LocalServerResponseBody::InteractionResolved {
                        request_item_id: response.request_item_id,
                    }
                } else {
                    error_body(
                        "approval_request_not_found",
                        format!("no pending approval for {}", response.request_item_id),
                    )
                }
            }
            LocalServerRequestBody::ResolveAskUser { response } => {
                if self.interactions.resolve_ask_user(response.clone()).await {
                    LocalServerResponseBody::InteractionResolved {
                        request_item_id: response.request_item_id,
                    }
                } else {
                    error_body(
                        "ask_user_request_not_found",
                        format!(
                            "no pending ask-user request for {}",
                            response.request_item_id
                        ),
                    )
                }
            }
            LocalServerRequestBody::RunPrompt { prompt } => match self
                .kernel
                .agent_runtime()
                .start_session(prompt, Some(self.workspace_root.clone()))
                .await
            {
                Ok(result) => LocalServerResponseBody::TurnResult(map_turn_result(result)),
                Err(err) => error_body("kernel_run_failed", err.to_string()),
            },
            LocalServerRequestBody::ResumePrompt { session_id, prompt } => {
                match self
                    .kernel
                    .agent_runtime()
                    .continue_session(&session_id, prompt)
                    .await
                {
                    Ok(result) => LocalServerResponseBody::TurnResult(map_turn_result(result)),
                    Err(err) => error_body("kernel_resume_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::ForkSession { session_id } => {
                match self.kernel.agent_runtime().fork_session(&session_id).await {
                    Ok(session) => LocalServerResponseBody::Session { session },
                    Err(err) => error_body("kernel_fork_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::ListSessions { limit } => {
                match self.store.list_sessions(ListSessionsParams { limit }).await {
                    Ok(sessions) => LocalServerResponseBody::Sessions { sessions },
                    Err(err) => error_body("list_sessions_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::ListExtensions => {
                let compat = CompatLayer::new();
                match ExtensionRegistry::discover_for_roots(
                    &compat.extension_roots(&self.workspace_root),
                ) {
                    Ok(registry) => LocalServerResponseBody::Extensions {
                        manifests: registry.manifests().to_vec(),
                    },
                    Err(err) => error_body("list_extensions_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::ListMcpServers => {
                let mcp = CommandBackedMcpPort::new(self.workspace_root.clone());
                match mcp.load_servers() {
                    Ok(servers) => LocalServerResponseBody::McpServers {
                        servers: servers.into_iter().map(map_mcp_server).collect(),
                    },
                    Err(err) => error_body("list_mcp_servers_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::GetSession { session_id } => {
                match self.store.get_session(&session_id).await {
                    Ok(session) => LocalServerResponseBody::Session { session },
                    Err(err) => error_body("get_session_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::GetTurns { session_id } => {
                match self.store.list_turns(&session_id).await {
                    Ok(turns) => LocalServerResponseBody::Turns {
                        turns: turns.into_iter().map(map_stored_turn).collect(),
                    },
                    Err(err) => error_body("get_turns_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::ExportSession { session_id, format } => {
                let content = match format {
                    ccodex_protocol::TranscriptFormat::Jsonl => {
                        JsonlTranscriptExporter::new((*self.store).clone())
                            .export_session(&session_id)
                            .await
                    }
                    ccodex_protocol::TranscriptFormat::Markdown => {
                        MarkdownTranscriptExporter::new((*self.store).clone())
                            .export_session(&session_id)
                            .await
                    }
                };

                match content {
                    Ok(content) => LocalServerResponseBody::Transcript { format, content },
                    Err(err) => error_body("export_session_failed", err.to_string()),
                }
            }
            LocalServerRequestBody::CallMcpTool {
                server,
                tool,
                input,
            } => {
                let mcp = CommandBackedMcpPort::new(self.workspace_root.clone());
                match mcp.call_tool(&server, &tool, input).await {
                    Ok(output) => LocalServerResponseBody::McpResult {
                        server,
                        tool,
                        output,
                    },
                    Err(err) => error_body("call_mcp_tool_failed", err.to_string()),
                }
            }
        };

        LocalServerResponse {
            id: request.id,
            body,
        }
    }
}

fn map_turn_result(result: RunTurnResult) -> LocalServerTurnResult {
    LocalServerTurnResult {
        session: result.session,
        turn: result.turn,
        assistant_text: result.assistant_text,
        events: result.events,
    }
}

fn map_stored_turn(turn: ccodex_store::StoredTurn) -> LocalServerStoredTurn {
    LocalServerStoredTurn {
        turn: turn.turn,
        items: turn.items,
    }
}

fn map_mcp_server(server: ccodex_runtime::McpServerDefinition) -> LocalServerMcpServer {
    LocalServerMcpServer {
        name: server.name,
        command: server.command,
        args: server.args,
        cwd: server.cwd.map(|cwd| cwd.display().to_string()),
    }
}
