use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ExtensionManifest, ProtocolEvent, Session, SessionId, Turn};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum TranscriptFormat {
    Jsonl,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalServerRequest {
    pub id: String,
    pub body: LocalServerRequestBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum LocalServerRequestBody {
    Ping,
    RunPrompt {
        prompt: String,
    },
    ResumePrompt {
        session_id: SessionId,
        prompt: String,
    },
    ListSessions {
        limit: Option<usize>,
    },
    ListExtensions,
    GetSession {
        session_id: SessionId,
    },
    ExportSession {
        session_id: SessionId,
        format: TranscriptFormat,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalServerResponse {
    pub id: String,
    pub body: LocalServerResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum LocalServerResponseBody {
    Pong(ServerInfo),
    TurnResult(LocalServerTurnResult),
    Sessions {
        sessions: Vec<Session>,
    },
    Extensions {
        manifests: Vec<ExtensionManifest>,
    },
    Session {
        session: Session,
    },
    Transcript {
        format: TranscriptFormat,
        content: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalServerTurnResult {
    pub session: Session,
    pub turn: Turn,
    pub assistant_text: String,
    pub events: Vec<ProtocolEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
    pub protocol: String,
}
