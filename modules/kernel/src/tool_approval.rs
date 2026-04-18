use serde_json::json;

use ccodex_protocol::{
    ApprovalDecision, ApprovalRequest, ItemId, ItemPayload, ProtocolEvent, Session, ToolCall,
    ToolResult, Turn,
};
use ccodex_runtime::{analyze_tool_call, enrich_approval_request};

use crate::hooks::HookContext;
use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn require_tool_approval(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        call: &ToolCall,
    ) -> Result<Option<ToolResult>, KernelError> {
        let Some(spec) = self.tool_specs.get(&call.tool_name) else {
            return Ok(None);
        };
        if !spec.requires_approval {
            return Ok(None);
        }

        let approval_item_id = ItemId::new();
        let assessment = analyze_tool_call(spec, call);
        let request = enrich_approval_request(ApprovalRequest::new(
            approval_item_id.clone(),
            Some(call.id.clone()),
            assessment.kind,
            assessment.summary,
            assessment.details,
        ));
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({
                    "phase": "approval_assessed",
                    "kind": format!("{:?}", request.kind),
                    "risk": format!("{:?}", request.risk),
                    "tool_name": request.context.tool_name,
                    "command": request.context.command,
                    "path": request.context.path,
                    "touches_workspace": request.context.touches_workspace,
                    "touches_outside_workspace": request.context.touches_outside_workspace,
                    "has_network_access": request.context.has_network_access,
                    "is_destructive": request.context.is_destructive,
                }),
            },
        )
        .await?;
        self.append_item_with_id(
            turn,
            events,
            approval_item_id,
            ItemPayload::ApprovalRequested {
                request: request.clone(),
            },
        )
        .await?;

        let response = self.approval_engine.request_approval(request).await?;
        let approved = response.decision == ApprovalDecision::Approved;
        self.append_item(
            turn,
            events,
            ItemPayload::ApprovalResolved {
                response: response.clone(),
            },
        )
        .await?;

        if approved {
            return Ok(None);
        }

        let rejected = ToolResult {
            tool_call_id: call.id.clone(),
            output: json!({ "error": "tool execution rejected by approval policy" }),
            is_error: true,
        };
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({
                    "phase": "completed",
                    "is_error": true,
                    "tool_name": call.tool_name,
                    "status": "rejected",
                }),
            },
        )
        .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallFinished {
                result: rejected.clone(),
            },
        )
        .await?;
        self.run_hooks(
            session,
            turn,
            events,
            ccodex_extensions::HookEvent::PostTool,
            HookContext {
                prompt: "",
                assistant_text: "",
                tool_call: Some(call),
                tool_result: Some(&rejected),
            },
        )
        .await?;
        Ok(Some(rejected))
    }
}
