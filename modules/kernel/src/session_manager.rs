use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use time::OffsetDateTime;

use ccodex_protocol::{
    Item, ItemId, ProtocolEvent, Session, SessionId, SessionStatus, Turn, TurnId,
};
use ccodex_store::StoredTurn;

use crate::{Kernel, KernelError, RunTurnResult};

impl Kernel {
    pub async fn run_prompt(
        &self,
        prompt: impl Into<String>,
        workspace_root: Option<PathBuf>,
    ) -> Result<RunTurnResult, KernelError> {
        let now = OffsetDateTime::now_utc();
        let session = Session {
            id: SessionId::new(),
            title: Some("Interactive Session".to_string()),
            workspace_root,
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
            active_plan: None,
            metadata: BTreeMap::new(),
        };
        self.store.create_session(&session).await?;

        let mut events = Vec::new();
        self.emit(&mut events, ProtocolEvent::SessionCreated(session.clone()))
            .await?;

        self.agent_loop()
            .run_in_session(session, prompt.into(), events)
            .await
    }

    pub async fn resume_prompt(
        &self,
        session_id: &SessionId,
        prompt: impl Into<String>,
    ) -> Result<RunTurnResult, KernelError> {
        let session = self.store.get_session(session_id).await?;
        self.agent_loop()
            .run_in_session(session, prompt.into(), Vec::new())
            .await
    }

    pub async fn fork_session(&self, session_id: &SessionId) -> Result<Session, KernelError> {
        let parent = self.store.get_session(session_id).await?;
        let stored_turns = self.store.list_turns(session_id).await?;
        let forked = self.create_forked_session(&parent).await?;
        self.clone_turn_history_into_session(&forked.id, stored_turns)
            .await?;

        self.notifications
            .notify_event(&ProtocolEvent::SessionCreated(forked.clone()))
            .await?;

        Ok(forked)
    }

    pub(crate) async fn create_forked_session(
        &self,
        parent: &Session,
    ) -> Result<Session, KernelError> {
        let now = OffsetDateTime::now_utc();

        let mut metadata = parent.metadata.clone();
        metadata.insert(
            "forked_from_session_id".to_string(),
            json!(parent.id.to_string()),
        );
        metadata.insert(
            "forked_at".to_string(),
            json!(
                now.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| now.unix_timestamp().to_string())
            ),
        );

        let mut forked = Session {
            id: SessionId::new(),
            title: Some(format!(
                "{} (fork)",
                parent
                    .title
                    .clone()
                    .unwrap_or_else(|| "Interactive Session".to_string())
            )),
            workspace_root: parent.workspace_root.clone(),
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
            active_plan: parent.active_plan.clone().map(|mut plan| {
                plan.session_id = SessionId::new();
                plan
            }),
            metadata,
        };
        if let Some(plan) = forked.active_plan.as_mut() {
            plan.session_id = forked.id.clone();
        }

        self.store.create_session(&forked).await?;
        Ok(forked)
    }

    pub(crate) async fn clone_turn_history_into_session(
        &self,
        target_session_id: &SessionId,
        stored_turns: Vec<StoredTurn>,
    ) -> Result<(), KernelError> {
        for stored in stored_turns {
            let mut cloned_turn = Turn {
                id: TurnId::new(),
                session_id: target_session_id.clone(),
                item_ids: Vec::with_capacity(stored.items.len()),
                started_at: stored.turn.started_at,
                completed_at: stored.turn.completed_at,
                status: stored.turn.status,
            };
            self.store.append_turn(&cloned_turn).await?;

            let cloned_items = stored
                .items
                .into_iter()
                .map(|item| Item {
                    id: ItemId::new(),
                    turn_id: cloned_turn.id.clone(),
                    created_at: item.created_at,
                    payload: item.payload,
                })
                .collect::<Vec<_>>();

            for item in cloned_items {
                cloned_turn.item_ids.push(item.id.clone());
                self.store.append_item(&item).await?;
            }
            self.store.append_turn(&cloned_turn).await?;
        }

        Ok(())
    }
}
