use std::sync::Arc;

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::accept_async;
use tungstenite::{Message, error::ProtocolError};

use ccodex_protocol::{
    LocalServerRequest, LocalServerRequestBody, LocalServerResponse, LocalServerResponseBody,
};

use crate::state::{ServerState, error_body};

pub(crate) async fn handle_connection(stream: TcpStream, state: Arc<ServerState>) -> Result<()> {
    let mut probe = [0_u8; 3];
    let peeked = stream.peek(&mut probe).await?;
    let is_websocket = peeked >= 3 && &probe == b"GET";
    if is_websocket {
        handle_websocket_connection(stream, state).await
    } else {
        handle_legacy_connection(stream, state).await
    }
}

async fn handle_legacy_connection(stream: TcpStream, state: Arc<ServerState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<LocalServerRequest>(&line) {
            Ok(request) => {
                if matches!(request.body, LocalServerRequestBody::SubscribeEvents) {
                    let subscribed = LocalServerResponse {
                        id: request.id.clone(),
                        body: LocalServerResponseBody::Subscribed,
                    };
                    let payload = serde_json::to_string(&subscribed)?;
                    writer.write_all(payload.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;

                    let mut receiver = state.events.subscribe();
                    loop {
                        match receiver.recv().await {
                            Ok(event) => {
                                let response = LocalServerResponse {
                                    id: request.id.clone(),
                                    body: LocalServerResponseBody::Event { event },
                                };
                                let payload = serde_json::to_string(&response)?;
                                writer.write_all(payload.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await?;
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    break;
                }

                state.handle_request(request).await
            }
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

async fn handle_websocket_connection(stream: TcpStream, state: Arc<ServerState>) -> Result<()> {
    let mut socket = accept_async(stream).await?;

    while let Some(message) = socket.next().await {
        let message = match message {
            Ok(message) => message,
            Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed)
            | Err(tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => {
                break;
            }
            Err(err) => return Err(err.into()),
        };

        match message {
            Message::Text(text) => {
                let request = match serde_json::from_str::<LocalServerRequest>(&text) {
                    Ok(request) => request,
                    Err(err) => {
                        let response = LocalServerResponse {
                            id: "invalid".to_string(),
                            body: error_body("invalid_request", err.to_string()),
                        };
                        socket
                            .send(Message::Text(serde_json::to_string(&response)?))
                            .await?;
                        continue;
                    }
                };

                if matches!(request.body, LocalServerRequestBody::SubscribeEvents) {
                    let subscribed = LocalServerResponse {
                        id: request.id.clone(),
                        body: LocalServerResponseBody::Subscribed,
                    };
                    socket
                        .send(Message::Text(serde_json::to_string(&subscribed)?))
                        .await?;

                    let mut receiver = state.events.subscribe();
                    loop {
                        match receiver.recv().await {
                            Ok(event) => {
                                let response = LocalServerResponse {
                                    id: request.id.clone(),
                                    body: LocalServerResponseBody::Event { event },
                                };
                                socket
                                    .send(Message::Text(serde_json::to_string(&response)?))
                                    .await?;
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    break;
                }

                let response = state.handle_request(request).await;
                socket
                    .send(Message::Text(serde_json::to_string(&response)?))
                    .await?;
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload)).await?;
            }
            Message::Pong(_) => {}
            Message::Close(_) => break,
            Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    Ok(())
}
