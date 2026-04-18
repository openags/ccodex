use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ApprovalRequest, ApprovalResponse, AskUserPrompt, AskUserResponse, ExtensionManifest, Item,
    ItemId, ProtocolEvent, Session, SessionId, Turn,
};

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
    SubscribeEvents,
    ListPendingInteractions,
    ResolveApproval {
        response: ApprovalResponse,
    },
    ResolveAskUser {
        response: AskUserResponse,
    },
    RunPrompt {
        prompt: String,
    },
    ResumePrompt {
        session_id: SessionId,
        prompt: String,
    },
    ForkSession {
        session_id: SessionId,
    },
    ListSessions {
        limit: Option<usize>,
    },
    ListExtensions,
    ListMcpServers,
    GetSession {
        session_id: SessionId,
    },
    GetTurns {
        session_id: SessionId,
    },
    ExportSession {
        session_id: SessionId,
        format: TranscriptFormat,
    },
    CallMcpTool {
        server: String,
        tool: String,
        input: Value,
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
    Subscribed,
    InteractionResolved {
        request_item_id: ItemId,
    },
    PendingInteractions {
        approvals: Vec<ApprovalRequest>,
        ask_user: Vec<AskUserPrompt>,
    },
    Event {
        event: ProtocolEvent,
    },
    TurnResult(LocalServerTurnResult),
    Sessions {
        sessions: Vec<Session>,
    },
    Extensions {
        manifests: Vec<ExtensionManifest>,
    },
    McpServers {
        servers: Vec<LocalServerMcpServer>,
    },
    Session {
        session: Session,
    },
    Turns {
        turns: Vec<LocalServerStoredTurn>,
    },
    Transcript {
        format: TranscriptFormat,
        content: String,
    },
    McpResult {
        server: String,
        tool: String,
        output: Value,
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
pub struct LocalServerStoredTurn {
    pub turn: Turn,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalServerMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
    pub protocol: String,
}
