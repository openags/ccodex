use async_trait::async_trait;

use ccodex_protocol::{SessionId, Turn};

use crate::traits::StoreError;

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn initialize(&self, _session_id: &SessionId) -> Result<(), StoreError>;
    async fn recall_context(
        &self,
        _session_id: &SessionId,
        _turn: &Turn,
    ) -> Result<Option<String>, StoreError>;
    async fn sync_turn(&self, _session_id: &SessionId, _turn: &Turn) -> Result<(), StoreError>;
    async fn shutdown(&self, _session_id: &SessionId) -> Result<(), StoreError>;
}
