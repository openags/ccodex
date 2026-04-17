use async_trait::async_trait;
use thiserror::Error;

use ccodex_protocol::{Item, Session, SessionId, Turn, TurnId};

#[derive(Debug, Clone, Default)]
pub struct ListSessionsParams {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct StoredTurn {
    pub turn: Turn,
    pub items: Vec<Item>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("export error: {0}")]
    Export(String),
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create_session(&self, session: &Session) -> Result<(), StoreError>;
    async fn update_session(&self, session: &Session) -> Result<(), StoreError>;
    async fn get_session(&self, session_id: &SessionId) -> Result<Session, StoreError>;
    async fn list_sessions(&self, params: ListSessionsParams) -> Result<Vec<Session>, StoreError>;
    async fn append_turn(&self, turn: &Turn) -> Result<(), StoreError>;
    async fn append_item(&self, item: &Item) -> Result<(), StoreError>;
    async fn get_turn(&self, turn_id: &TurnId) -> Result<StoredTurn, StoreError>;
    async fn list_turns(&self, session_id: &SessionId) -> Result<Vec<StoredTurn>, StoreError>;
}
