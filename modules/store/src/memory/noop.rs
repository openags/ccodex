use async_trait::async_trait;

use ccodex_protocol::{SessionId, Turn};

use crate::{traits::StoreError, MemoryProvider};

#[derive(Debug, Clone, Default)]
pub struct NoopMemoryProvider;

#[async_trait]
impl MemoryProvider for NoopMemoryProvider {
    async fn initialize(&self, _session_id: &SessionId) -> Result<(), StoreError> {
        Ok(())
    }

    async fn recall_context(&self, _session_id: &SessionId, _turn: &Turn) -> Result<Option<String>, StoreError> {
        Ok(None)
    }

    async fn sync_turn(&self, _session_id: &SessionId, _turn: &Turn) -> Result<(), StoreError> {
        Ok(())
    }

    async fn shutdown(&self, _session_id: &SessionId) -> Result<(), StoreError> {
        Ok(())
    }
}
