use ccodex_protocol::{ItemPayload, ProtocolEvent, Session};

use crate::hooks::HookContext;
use crate::{Kernel, KernelError, RunTurnResult};

impl Kernel {
    pub(crate) async fn run_prompt_in_session(
        &self,
        mut session: Session,
        prompt: String,
        mut events: Vec<ProtocolEvent>,
    ) -> Result<RunTurnResult, KernelError> {
        let mut turn = self.start_turn(&session, &mut events).await?;
        self.bootstrap_session_context(&mut session, &mut turn, &mut events)
            .await?;

        self.append_item(
            &mut turn,
            &mut events,
            ItemPayload::UserMessage {
                content: prompt.clone(),
            },
        )
        .await?;
        self.run_hooks(
            &session,
            &mut turn,
            &mut events,
            ccodex_extensions::HookEvent::PreTurn,
            HookContext {
                prompt: &prompt,
                assistant_text: "",
                tool_call: None,
                tool_result: None,
            },
        )
        .await?;

        let mut assistant_text = String::new();
        let mut tool_summary: Option<String> = None;
        let mut iterations = 0usize;

        loop {
            iterations += 1;
            if iterations > 8 {
                break;
            }

            let saw_tool_call = self
                .run_provider_iteration(
                    &mut session,
                    &mut turn,
                    &mut events,
                    &prompt,
                    &mut assistant_text,
                    &mut tool_summary,
                )
                .await?;

            if !saw_tool_call {
                break;
            }
        }

        if assistant_text.is_empty() {
            if let Some(summary) = tool_summary {
                assistant_text = summary.clone();
                self.append_item(
                    &mut turn,
                    &mut events,
                    ItemPayload::AssistantMessageDelta { content: summary },
                )
                .await?;
            }
        }

        self.finalize_turn(
            &mut session,
            &mut turn,
            &mut events,
            &prompt,
            &assistant_text,
        )
        .await?;

        Ok(RunTurnResult {
            session,
            turn,
            assistant_text,
            events,
        })
    }
}
