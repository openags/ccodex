use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use ccodex_brand::{project_state_db_file, DISPLAY_NAME, LOCAL_SERVER_ADDR_ENV, VERSION};
use ccodex_extensions::ExtensionRegistry;
use ccodex_kernel::{Kernel, RunTurnResult};
use ccodex_protocol::{
    LocalServerRequest, LocalServerRequestBody, LocalServerResponse, LocalServerResponseBody,
    LocalServerTurnResult, ServerInfo,
};
use ccodex_runtime::Runtime;
use ccodex_store::{
    JsonlTranscriptExporter, ListSessionsParams, MarkdownTranscriptExporter, SessionStore,
    SQLiteSessionStore, TranscriptExporter,
};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:48765";

struct ServerState {
    workspace_root: std::path::PathBuf,
    store: Arc<SQLiteSessionStore>,
    kernel: Arc<Kernel>,
}

impl ServerState {
    fn bootstrap() -> Result<Self> {
        let workspace_root = std::env::current_dir()?;
        Self::for_workspace(workspace_root)
    }

    fn for_workspace(workspace_root: std::path::PathBuf) -> Result<Self> {
        let db_path = project_state_db_file(&workspace_root);
        let store = Arc::new(SQLiteSessionStore::new(&db_path)?);
        let runtime = Runtime::bootstrap();
        let kernel = Arc::new(Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        ));

        Ok(Self {
            workspace_root,
            store,
            kernel,
        })
    }

    async fn handle_request(&self, request: LocalServerRequest) -> LocalServerResponse {
        let body = match request.body {
            LocalServerRequestBody::Ping => {
                LocalServerResponseBody::Pong(ServerInfo {
                    product: DISPLAY_NAME.to_string(),
                    version: VERSION.to_string(),
                    protocol: "ccodex.local.v1".to_string(),
                })
            }
            LocalServerRequestBody::RunPrompt { prompt } => match self
                .kernel
                .run_prompt(prompt, Some(self.workspace_root.clone()))
                .await
            {
                Ok(result) => LocalServerResponseBody::TurnResult(map_turn_result(result)),
                Err(err) => error_body("kernel_run_failed", err.to_string()),
            },
            LocalServerRequestBody::ResumePrompt { session_id, prompt } => match self
                .kernel
                .resume_prompt(&session_id, prompt)
                .await
            {
                Ok(result) => LocalServerResponseBody::TurnResult(map_turn_result(result)),
                Err(err) => error_body("kernel_resume_failed", err.to_string()),
            },
            LocalServerRequestBody::ListSessions { limit } => match self
                .store
                .list_sessions(ListSessionsParams { limit })
                .await
            {
                Ok(sessions) => LocalServerResponseBody::Sessions { sessions },
                Err(err) => error_body("list_sessions_failed", err.to_string()),
            },
            LocalServerRequestBody::ListExtensions => match ExtensionRegistry::discover_for_workspace(&self.workspace_root) {
                Ok(registry) => LocalServerResponseBody::Extensions {
                    manifests: registry.manifests().to_vec(),
                },
                Err(err) => error_body("list_extensions_failed", err.to_string()),
            },
            LocalServerRequestBody::GetSession { session_id } => match self.store.get_session(&session_id).await {
                Ok(session) => LocalServerResponseBody::Session { session },
                Err(err) => error_body("get_session_failed", err.to_string()),
            },
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
        };

        LocalServerResponse {
            id: request.id,
            body,
        }
    }
}

fn error_body(code: impl Into<String>, message: impl Into<String>) -> LocalServerResponseBody {
    LocalServerResponseBody::Error {
        code: code.into(),
        message: message.into(),
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

async fn handle_connection(stream: TcpStream, state: Arc<ServerState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<LocalServerRequest>(&line) {
            Ok(request) => state.handle_request(request).await,
            Err(err) => LocalServerResponse {
                id: "invalid".to_string(),
                body: error_body("invalid_request", err.to_string()),
            },
        };

        let payload = serde_json::to_string(&response)?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind_addr = std::env::var(LOCAL_SERVER_ADDR_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
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

    use ccodex_protocol::{LocalServerRequest, LocalServerRequestBody, LocalServerResponseBody, TranscriptFormat};

    use super::ServerState;

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
        let state = ServerState::for_workspace(workspace.clone()).expect("state should bootstrap");
        let response = state
            .handle_request(LocalServerRequest {
                id: "1".to_string(),
                body: LocalServerRequestBody::Ping,
            })
            .await;

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
            workspace.join("plugins").join("builtin").join("skills").join("repo_overview.md"),
            "# Repo Overview",
        )
        .expect("builtin skill should write");
        let state = ServerState::for_workspace(workspace.clone()).expect("state should bootstrap");
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

        let state = ServerState::for_workspace(workspace.clone()).expect("state should bootstrap");
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
}
