use serde_json::json;

use ccodex_protocol::{
    AskUserPrompt, ItemPayload, ProtocolEvent, Session, ToolCall, ToolResult, Turn,
};

use crate::hooks::HookContext;
use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn handle_ask_user_tool(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        call: &ToolCall,
    ) -> Result<ToolResult, KernelError> {
        let prompt: AskUserPrompt = self.build_ask_user_prompt(call)?;
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({ "phase": "waiting_for_user" }),
            },
        )
        .await?;
        self.append_item_with_id(
            turn,
            events,
            prompt.item_id.clone(),
            ItemPayload::AskUserRequested {
                prompt: prompt.clone(),
            },
        )
        .await?;

        let response = self.approval_engine.request_user_input(prompt).await?;
        self.append_item(
            turn,
            events,
            ItemPayload::AskUserResolved {
                response: response.clone(),
            },
        )
        .await?;

        let resolution_mode = if response.freeform_text.is_some() {
            "freeform"
        } else if response.selected_choice_id.is_some() {
            "choice"
        } else {
            "cancelled"
        };
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            output: json!({
                "selected_choice_id": response.selected_choice_id,
                "freeform_text": response.freeform_text,
                "resolution_mode": resolution_mode
            }),
            is_error: false,
        };
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({
                    "phase": "completed",
                    "tool_name": call.tool_name,
                    "status": "resolved",
                    "is_error": false,
                    "resolution_mode": resolution_mode,
                }),
            },
        )
        .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallFinished {
                result: result.clone(),
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
                tool_result: Some(&result),
            },
        )
        .await?;
        Ok(result)
    }

    pub(crate) async fn handle_spawn_agent_tool(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        call: &ToolCall,
    ) -> Result<ToolResult, KernelError> {
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallDelta {
                tool_call_id: call.id.clone(),
                delta: json!({ "phase": "delegating" }),
            },
        )
        .await?;
        let result = self
            .handle_subagent_call(session, turn, events, call)
            .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::ToolCallFinished {
                result: result.clone(),
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
                tool_result: Some(&result),
            },
        )
        .await?;
        Ok(result)
    }
}
