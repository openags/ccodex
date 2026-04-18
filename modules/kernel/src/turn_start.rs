use time::OffsetDateTime;

use ccodex_protocol::{ProtocolEvent, Session, Turn, TurnId, TurnStatus};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn start_turn(
        &self,
        session: &Session,
        events: &mut Vec<ProtocolEvent>,
    ) -> Result<Turn, KernelError> {
        let turn = Turn {
            id: TurnId::new(),
            session_id: session.id.clone(),
            item_ids: Vec::new(),
            started_at: OffsetDateTime::now_utc(),
            completed_at: None,
            status: TurnStatus::Running,
        };
        self.store.append_turn(&turn).await?;
        self.emit(events, ProtocolEvent::TurnStarted(turn.clone()))
            .await?;
        Ok(turn)
    }
}
