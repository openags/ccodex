use time::OffsetDateTime;

use ccodex_protocol::{ProtocolEvent, Session, Turn, TurnStatus};

use crate::hooks::HookContext;
use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn finalize_turn(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        prompt: &str,
        assistant_text: &str,
    ) -> Result<(), KernelError> {
        turn.status = TurnStatus::Completed;
        turn.completed_at = Some(OffsetDateTime::now_utc());
        self.store.append_turn(turn).await?;
        self.emit(events, ProtocolEvent::TurnFinished(turn.clone()))
            .await?;

        session.updated_at = OffsetDateTime::now_utc();
        self.store.update_session(session).await?;
        self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
            .await?;
        self.run_hooks(
            session,
            turn,
            events,
            ccodex_extensions::HookEvent::PostTurn,
            HookContext {
                prompt,
                assistant_text,
                tool_call: None,
                tool_result: None,
            },
        )
        .await?;
        self.maybe_compact_session(session, turn, events).await?;
        Ok(())
    }
}
