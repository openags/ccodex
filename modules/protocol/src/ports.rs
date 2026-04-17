use async_trait::async_trait;
use futures::stream::BoxStream;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{ApprovalRequest, AskUserPrompt, ProtocolEvent, Session, ToolCall, ToolResult, Turn};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnRequest {
    pub session: Session,
    pub turn: Turn,
    pub instructions: String,
    pub project_instructions: Vec<String>,
    pub available_tools: Vec<crate::ToolSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ProviderEvent {
    AssistantMessageDelta { content: String },
    ReasoningDelta { content: String },
    ToolCall(ToolCall),
    Completed,
}

#[derive(Debug, Error)]
pub enum PortError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("approval error: {0}")]
    Approval(String),
    #[error("sandbox error: {0}")]
    Sandbox(String),
    #[error("mcp error: {0}")]
    Mcp(String),
    #[error("notification error: {0}")]
    Notification(String),
}

#[async_trait]
pub trait ModelProviderPort: Send + Sync {
    async fn start_turn(&self, request: TurnRequest) -> Result<BoxStream<'static, Result<ProviderEvent, PortError>>, PortError>;
}

#[async_trait]
pub trait ToolExecutorPort: Send + Sync {
    async fn execute_tool(&self, call: ToolCall) -> Result<ToolResult, PortError>;
}

#[async_trait]
pub trait ApprovalEnginePort: Send + Sync {
    async fn request_approval(&self, request: ApprovalRequest) -> Result<crate::ApprovalResponse, PortError>;
    async fn request_user_input(&self, prompt: AskUserPrompt) -> Result<crate::AskUserResponse, PortError>;
}

#[async_trait]
pub trait SandboxPort: Send + Sync {
    async fn is_command_allowed(&self, command: &str) -> Result<bool, PortError>;
}

#[async_trait]
pub trait McpPort: Send + Sync {
    async fn call_tool(&self, server: &str, tool: &str, input: Value) -> Result<Value, PortError>;
}

#[async_trait]
pub trait NotificationPort: Send + Sync {
    async fn notify_event(&self, event: &ProtocolEvent) -> Result<(), PortError>;
}
