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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub enum ApprovalRisk {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum ApprovalReasonCode {
    RuleAllow,
    RuleDeny,
    PolicyAlwaysApprove,
    PolicyNeverApprove,
    InteractiveApproved,
    InteractiveRejected,
    InteractiveCancelled,
    InteractiveUnavailable,
}

impl std::fmt::Display for ApprovalReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            ApprovalReasonCode::RuleAllow => "rule_allow",
            ApprovalReasonCode::RuleDeny => "rule_deny",
            ApprovalReasonCode::PolicyAlwaysApprove => "policy_always_approve",
            ApprovalReasonCode::PolicyNeverApprove => "policy_never_approve",
            ApprovalReasonCode::InteractiveApproved => "interactive_approved",
            ApprovalReasonCode::InteractiveRejected => "interactive_rejected",
            ApprovalReasonCode::InteractiveCancelled => "interactive_cancelled",
            ApprovalReasonCode::InteractiveUnavailable => "interactive_unavailable",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ApprovalContext {
    pub tool_name: Option<String>,
    pub command: Option<String>,
    pub path: Option<String>,
    pub touches_workspace: bool,
    pub touches_outside_workspace: bool,
    pub has_network_access: bool,
    pub is_destructive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRequest {
    pub item_id: ItemId,
    pub tool_call_id: Option<ToolCallId>,
    pub kind: ApprovalKind,
    pub risk: ApprovalRisk,
    pub summary: String,
    pub details: Option<String>,
    pub context: ApprovalContext,
}

impl ApprovalRequest {
    pub fn new(
        item_id: ItemId,
        tool_call_id: Option<ToolCallId>,
        kind: ApprovalKind,
        summary: String,
        details: Option<String>,
    ) -> Self {
        Self {
            item_id,
            tool_call_id,
            kind,
            risk: ApprovalRisk::Medium,
            summary,
            details,
            context: ApprovalContext::default(),
        }
    }

    pub fn with_risk(mut self, risk: ApprovalRisk) -> Self {
        self.risk = risk;
        self
    }

    pub fn with_context(mut self, context: ApprovalContext) -> Self {
        self.context = context;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalResponse {
    pub request_item_id: ItemId,
    pub decision: ApprovalDecision,
    pub reason: Option<String>,
    pub reason_code: Option<ApprovalReasonCode>,
}

impl ApprovalResponse {
    pub fn new(
        request_item_id: ItemId,
        decision: ApprovalDecision,
        reason: Option<String>,
        reason_code: Option<ApprovalReasonCode>,
    ) -> Self {
        Self {
            request_item_id,
            decision,
            reason,
            reason_code,
        }
    }

    pub fn with_reason_code(mut self, reason_code: ApprovalReasonCode) -> Self {
        self.reason_code = Some(reason_code);
        self
    }
}
