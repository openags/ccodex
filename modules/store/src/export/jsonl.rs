use async_trait::async_trait;
use serde_json::to_string;

use ccodex_protocol::SessionId;

use crate::{traits::{SessionStore, StoreError}, TranscriptExporter};

#[derive(Debug, Clone)]
pub struct JsonlTranscriptExporter {
    store: crate::SQLiteSessionStore,
}

impl JsonlTranscriptExporter {
    pub fn new(store: crate::SQLiteSessionStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TranscriptExporter for JsonlTranscriptExporter {
    async fn export_session(&self, session_id: &SessionId) -> Result<String, StoreError> {
        let turns = self.store.list_turns(session_id).await?;
        let mut lines = Vec::new();
        for stored_turn in turns {
            lines.push(
                to_string(&stored_turn.turn).map_err(|err| StoreError::Export(err.to_string()))?,
            );
            for item in stored_turn.items {
                lines.push(to_string(&item).map_err(|err| StoreError::Export(err.to_string()))?);
            }
        }
        Ok(lines.join("\n"))
    }
}
