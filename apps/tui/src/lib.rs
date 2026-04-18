use std::sync::mpsc::{self, Receiver};

use anyhow::{Context, Result, anyhow};
use tungstenite::{Message, connect};

use ccodex_brand::LOCAL_SERVER_ADDR_ENV;
use ccodex_protocol::{
    ApprovalDecision, ApprovalResponse, AskUserResponse, ExtensionManifest, ItemId,
    LocalServerRequest, LocalServerRequestBody, LocalServerResponse, LocalServerResponseBody,
    LocalServerStoredTurn, ProtocolEvent, Session, SessionId,
};

const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:48765";

pub fn local_server_addr() -> String {
    std::env::var(LOCAL_SERVER_ADDR_ENV).unwrap_or_else(|_| DEFAULT_SERVER_ADDR.to_string())
}

fn local_server_ws_url() -> String {
    format!("ws://{}", local_server_addr())
}

pub fn send_request(body: LocalServerRequestBody) -> Result<LocalServerResponse> {
    let ws_url = local_server_ws_url();
    let (mut socket, _) = connect(&ws_url)
        .with_context(|| format!("failed to connect to local server at {ws_url}"))?;
    let request = LocalServerRequest {
        id: format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .context("system clock before unix epoch")?
                .as_nanos()
        ),
        body,
    };
    let payload = serde_json::to_string(&request)?;
    socket.send(Message::Text(payload))?;

    loop {
        match socket.read()? {
            Message::Text(text) => {
                let response: LocalServerResponse = serde_json::from_str(&text)?;
                return Ok(response);
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload))?;
            }
            Message::Close(_) => {
                return Err(anyhow!("local server closed the websocket connection"));
            }
            _ => continue,
        }
    }
}

pub fn ping() -> Result<String> {
    match send_request(LocalServerRequestBody::Ping)?.body {
        LocalServerResponseBody::Pong(info) => Ok(format!(
            "{} {} ({})",
            info.product, info.version, info.protocol
        )),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn run_prompt(prompt: String) -> Result<(String, SessionId)> {
    match send_request(LocalServerRequestBody::RunPrompt { prompt })?.body {
        LocalServerResponseBody::TurnResult(result) => {
            Ok((result.assistant_text, result.session.id))
        }
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn resume_prompt(session_id: SessionId, prompt: String) -> Result<(String, SessionId)> {
    match send_request(LocalServerRequestBody::ResumePrompt { session_id, prompt })?.body {
        LocalServerResponseBody::TurnResult(result) => {
            Ok((result.assistant_text, result.session.id))
        }
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn fork_session(session_id: SessionId) -> Result<Session> {
    match send_request(LocalServerRequestBody::ForkSession { session_id })?.body {
        LocalServerResponseBody::Session { session } => Ok(session),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn list_sessions(limit: Option<usize>) -> Result<Vec<Session>> {
    match send_request(LocalServerRequestBody::ListSessions { limit })?.body {
        LocalServerResponseBody::Sessions { sessions } => Ok(sessions),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn list_extensions() -> Result<Vec<ExtensionManifest>> {
    match send_request(LocalServerRequestBody::ListExtensions)?.body {
        LocalServerResponseBody::Extensions { manifests } => Ok(manifests),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn get_turns(session_id: SessionId) -> Result<Vec<LocalServerStoredTurn>> {
    match send_request(LocalServerRequestBody::GetTurns { session_id })?.body {
        LocalServerResponseBody::Turns { turns } => Ok(turns),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn get_session(session_id: SessionId) -> Result<Session> {
    match send_request(LocalServerRequestBody::GetSession { session_id })?.body {
        LocalServerResponseBody::Session { session } => Ok(session),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn list_pending_interactions() -> Result<(
    Vec<ccodex_protocol::ApprovalRequest>,
    Vec<ccodex_protocol::AskUserPrompt>,
)> {
    match send_request(LocalServerRequestBody::ListPendingInteractions)?.body {
        LocalServerResponseBody::PendingInteractions {
            approvals,
            ask_user,
        } => Ok((approvals, ask_user)),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn resolve_approval(
    request_item_id: ItemId,
    decision: ApprovalDecision,
    reason: Option<String>,
) -> Result<ItemId> {
    match send_request(LocalServerRequestBody::ResolveApproval {
        response: ApprovalResponse::new(request_item_id, decision, reason, None),
    })?
    .body
    {
        LocalServerResponseBody::InteractionResolved { request_item_id } => Ok(request_item_id),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn resolve_ask_user(
    request_item_id: ItemId,
    selected_choice_id: Option<String>,
    freeform_text: Option<String>,
) -> Result<ItemId> {
    match send_request(LocalServerRequestBody::ResolveAskUser {
        response: AskUserResponse {
            request_item_id,
            selected_choice_id,
            freeform_text,
        },
    })?
    .body
    {
        LocalServerResponseBody::InteractionResolved { request_item_id } => Ok(request_item_id),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn subscribe_events() -> Result<Receiver<ProtocolEvent>> {
    let ws_url = local_server_ws_url();
    let (mut socket, _) = connect(&ws_url)
        .with_context(|| format!("failed to connect to local server at {ws_url}"))?;
    let request = LocalServerRequest {
        id: format!(
            "sub-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .context("system clock before unix epoch")?
                .as_nanos()
        ),
        body: LocalServerRequestBody::SubscribeEvents,
    };
    let payload = serde_json::to_string(&request)?;
    socket.send(Message::Text(payload))?;

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let parsed = serde_json::from_str::<LocalServerResponse>(&text);
                    match parsed {
                        Ok(LocalServerResponse {
                            body: LocalServerResponseBody::Subscribed,
                            ..
                        }) => {}
                        Ok(LocalServerResponse {
                            body: LocalServerResponseBody::Event { event },
                            ..
                        }) => {
                            if sender.send(event).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                Ok(Message::Ping(payload)) => match socket.send(Message::Pong(payload)) {
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    Ok(receiver)
}
