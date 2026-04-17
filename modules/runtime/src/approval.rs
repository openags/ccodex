use async_trait::async_trait;

use ccodex_protocol::{
    ApprovalDecision, ApprovalEnginePort, ApprovalRequest, ApprovalResponse, AskUserPrompt,
    AskUserResponse, PortError,
};

use crate::ApprovalPolicy;

#[derive(Debug, Clone)]
pub struct AutoApproveEngine {
    policy: ApprovalPolicy,
}

impl AutoApproveEngine {
    pub fn new(policy: ApprovalPolicy) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl ApprovalEnginePort for AutoApproveEngine {
    async fn request_approval(
        &self,
        request: ApprovalRequest,
    ) -> Result<ApprovalResponse, PortError> {
        let decision = match self.policy {
            ApprovalPolicy::AlwaysApprove => ApprovalDecision::Approved,
            ApprovalPolicy::NeverApprove => ApprovalDecision::Rejected,
        };

        Ok(ApprovalResponse {
            request_item_id: request.item_id,
            decision,
            reason: None,
        })
    }

    async fn request_user_input(
        &self,
        prompt: AskUserPrompt,
    ) -> Result<AskUserResponse, PortError> {
        Ok(AskUserResponse {
            request_item_id: prompt.item_id,
            selected_choice_id: prompt.choices.first().map(|choice| choice.id.clone()),
            freeform_text: None,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct StaticAskUserEngine;

#[async_trait]
impl ApprovalEnginePort for StaticAskUserEngine {
    async fn request_approval(
        &self,
        request: ApprovalRequest,
    ) -> Result<ApprovalResponse, PortError> {
        Ok(ApprovalResponse {
            request_item_id: request.item_id,
            decision: ApprovalDecision::Approved,
            reason: None,
        })
    }

    async fn request_user_input(
        &self,
        prompt: AskUserPrompt,
    ) -> Result<AskUserResponse, PortError> {
        Ok(AskUserResponse {
            request_item_id: prompt.item_id,
            selected_choice_id: prompt.choices.first().map(|choice| choice.id.clone()),
            freeform_text: None,
        })
    }
}
