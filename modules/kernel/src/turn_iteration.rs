use futures::StreamExt;

use ccodex_protocol::{ItemPayload, ProtocolEvent, ProviderEvent, Session, Turn};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn run_provider_iteration(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        prompt: &str,
        assistant_text: &mut String,
        tool_summary: &mut Option<String>,
    ) -> Result<bool, KernelError> {
        let request = self.build_turn_request(session, turn, prompt).await?;
        let mut provider_stream = self.provider.start_turn(request).await?;
        let mut saw_tool_call = false;

        while let Some(event) = provider_stream.next().await {
            match event? {
                ProviderEvent::AssistantMessageDelta { content } => {
                    assistant_text.push_str(&content);
                    self.append_item(turn, events, ItemPayload::AssistantMessageDelta { content })
                        .await?;
                }
                ProviderEvent::ReasoningDelta { content } => {
                    self.append_item(turn, events, ItemPayload::ReasoningDelta { content })
                        .await?;
                }
                ProviderEvent::ToolCall(call) => {
                    saw_tool_call = true;
                    let result = self
                        .handle_tool_call(session, turn, events, call.clone())
                        .await?;
                    *tool_summary = Some(self.summarize_tool_outcome(
                        &call,
                        &result,
                        session.active_plan.as_ref(),
                    ));
                }
                ProviderEvent::Completed => {}
            }
        }

        Ok(saw_tool_call)
    }
}
