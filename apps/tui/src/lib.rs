use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::{anyhow, Context, Result};

use ccodex_brand::LOCAL_SERVER_ADDR_ENV;
use ccodex_protocol::{
    ExtensionManifest, LocalServerRequest, LocalServerRequestBody, LocalServerResponse,
    LocalServerResponseBody, Session, SessionId,
};

const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:48765";

pub fn local_server_addr() -> String {
    std::env::var(LOCAL_SERVER_ADDR_ENV).unwrap_or_else(|_| DEFAULT_SERVER_ADDR.to_string())
}

pub fn send_request(body: LocalServerRequestBody) -> Result<LocalServerResponse> {
    let addr = local_server_addr();
    let mut stream = TcpStream::connect(&addr)
        .with_context(|| format!("failed to connect to local server at {addr}"))?;
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
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(anyhow!("local server returned an empty response"));
    }

    let response: LocalServerResponse = serde_json::from_str(&line)?;
    Ok(response)
}

pub fn ping() -> Result<String> {
    match send_request(LocalServerRequestBody::Ping)?.body {
        LocalServerResponseBody::Pong(info) => {
            Ok(format!("{} {} ({})", info.product, info.version, info.protocol))
        }
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn run_prompt(prompt: String) -> Result<(String, SessionId)> {
    match send_request(LocalServerRequestBody::RunPrompt { prompt })?.body {
        LocalServerResponseBody::TurnResult(result) => Ok((result.assistant_text, result.session.id)),
        LocalServerResponseBody::Error { code, message } => Err(anyhow!("{code}: {message}")),
        other => Err(anyhow!("unexpected response: {other:?}")),
    }
}

pub fn resume_prompt(session_id: SessionId, prompt: String) -> Result<(String, SessionId)> {
    match send_request(LocalServerRequestBody::ResumePrompt { session_id, prompt })?.body {
        LocalServerResponseBody::TurnResult(result) => Ok((result.assistant_text, result.session.id)),
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
