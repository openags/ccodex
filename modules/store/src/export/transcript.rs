use async_trait::async_trait;

use ccodex_protocol::SessionId;

use crate::traits::StoreError;

#[async_trait]
pub trait TranscriptExporter: Send + Sync {
    async fn export_session(&self, session_id: &SessionId) -> Result<String, StoreError>;
}
