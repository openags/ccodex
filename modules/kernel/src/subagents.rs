use serde_json::{Value, json};

use ccodex_protocol::{ItemPayload, ProtocolEvent, Session, ToolCall, ToolResult, Turn};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn handle_subagent_call(
        &self,
        parent_session: &Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        call: &ToolCall,
    ) -> Result<ToolResult, KernelError> {
        let name = call
            .input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("subagent")
            .to_string();
        let prompt = call
            .input
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let agent_spec = self.load_subagent_spec(parent_session, &name)?;

        let child_session = self
            .prepare_subagent_session(parent_session, &name, agent_spec.as_ref())
            .await?;
        self.emit(events, ProtocolEvent::SessionUpdated(child_session.clone()))
            .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::SystemEvent {
                name: "subagent_started".to_string(),
                payload: json!({
                    "agent_name": name,
                    "session_id": child_session.id,
                    "parent_session_id": parent_session.id,
                    "parent_turn_id": turn.id,
                    "prompt": prompt,
                    "agent_loaded": agent_spec.is_some(),
                    "subagent_depth": child_session
                        .metadata
                        .get("subagent_depth")
                        .cloned()
                        .unwrap_or(Value::Null),
                }),
            },
        )
        .await?;

        let child_result =
            Box::pin(self.run_prompt_in_session(child_session.clone(), prompt, Vec::new())).await?;

        self.append_item(
            turn,
            events,
            ItemPayload::SystemEvent {
                name: "subagent_finished".to_string(),
                payload: json!({
                    "agent_name": name,
                    "session_id": child_result.session.id,
                    "parent_session_id": parent_session.id,
                    "parent_turn_id": turn.id,
                    "child_turn_id": child_result.turn.id,
                    "assistant_text": child_result.assistant_text,
                    "subagent_depth": child_result
                        .session
                        .metadata
                        .get("subagent_depth")
                        .cloned()
                        .unwrap_or(Value::Null),
                }),
            },
        )
        .await?;

        Ok(ToolResult {
            tool_call_id: call.id.clone(),
            output: json!({
                "agent_name": name,
                "session_id": child_result.session.id,
                "parent_session_id": parent_session.id,
                "parent_turn_id": turn.id,
                "child_turn_id": child_result.turn.id,
                "assistant_text": child_result.assistant_text,
                "subagent_depth": child_result
                    .session
                    .metadata
                    .get("subagent_depth")
                    .cloned()
                    .unwrap_or(Value::Null),
            }),
            is_error: false,
        })
    }
}
