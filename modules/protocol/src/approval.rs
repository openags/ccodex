use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ItemId, ToolCallId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum ApprovalKind {
    ToolUse,
    CommandExecution,
    FileWrite,
    PermissionEscalation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRequest {
    pub item_id: ItemId,
    pub tool_call_id: Option<ToolCallId>,
    pub kind: ApprovalKind,
    pub summary: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalResponse {
    pub request_item_id: ItemId,
    pub decision: ApprovalDecision,
    pub reason: Option<String>,
}
