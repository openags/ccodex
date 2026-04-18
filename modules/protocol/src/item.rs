use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    ApprovalRequest, ApprovalResponse, AskUserPrompt, AskUserResponse, ItemId, PlanState, ToolCall,
    ToolResult,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ItemPayload {
    UserMessage {
        content: String,
    },
    AssistantMessageDelta {
        content: String,
    },
    ReasoningDelta {
        content: String,
    },
    ToolCallStarted {
        call: ToolCall,
    },
    ToolCallDelta {
        tool_call_id: crate::ToolCallId,
        delta: Value,
    },
    ToolCallFinished {
        result: ToolResult,
    },
    ApprovalRequested {
        request: ApprovalRequest,
    },
    ApprovalResolved {
        response: ApprovalResponse,
    },
    AskUserRequested {
        prompt: AskUserPrompt,
    },
    AskUserResolved {
        response: AskUserResponse,
    },
    PlanEntered {
        plan: PlanState,
    },
    PlanUpdated {
        plan: PlanState,
    },
    PlanExited {
        plan_id: crate::PlanId,
    },
    Warning {
        code: String,
        message: String,
    },
    Error {
        code: String,
        message: String,
    },
    SystemEvent {
        name: String,
        payload: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: ItemId,
    pub turn_id: crate::TurnId,
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
    pub payload: ItemPayload,
}
