use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rusqlite::Connection;

use ccodex_protocol::{Item, Session, SessionId, Turn, TurnId};

use crate::sqlite::migrations::MIGRATIONS;
use crate::sqlite::{items, sessions, turns};
use crate::traits::{ListSessionsParams, SessionStore, StoreError, StoredTurn};

#[derive(Debug, Clone)]
pub struct SQLiteSessionStore {
    db_path: PathBuf,
}

impl SQLiteSessionStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let store = Self {
            db_path: db_path.into(),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn initialize(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| StoreError::Database(err.to_string()))?;
        }
        let connection = self.open_connection()?;
        connection
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|err| StoreError::Database(err.to_string()))?;
        for statement in MIGRATIONS {
            connection
                .execute_batch(statement)
                .map_err(|err| StoreError::Database(err.to_string()))?;
        }
        Ok(())
    }

    fn open_connection(&self) -> Result<Connection, StoreError> {
        Connection::open(&self.db_path).map_err(|err| StoreError::Database(err.to_string()))
    }
}

#[async_trait]
impl SessionStore for SQLiteSessionStore {
    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        sessions::upsert_session(&self.open_connection()?, session)
    }

    async fn update_session(&self, session: &Session) -> Result<(), StoreError> {
        sessions::upsert_session(&self.open_connection()?, session)
    }

    async fn get_session(&self, session_id: &SessionId) -> Result<Session, StoreError> {
        sessions::get_session(&self.open_connection()?, session_id)
    }

    async fn list_sessions(&self, params: ListSessionsParams) -> Result<Vec<Session>, StoreError> {
        sessions::list_sessions(&self.open_connection()?, params)
    }

    async fn append_turn(&self, turn: &Turn) -> Result<(), StoreError> {
        turns::upsert_turn(&self.open_connection()?, turn)
    }

    async fn append_item(&self, item: &Item) -> Result<(), StoreError> {
        items::upsert_item(&self.open_connection()?, item)
    }

    async fn get_turn(&self, turn_id: &TurnId) -> Result<StoredTurn, StoreError> {
        let connection = self.open_connection()?;
        let turn = turns::get_turn(&connection, turn_id)?;
        let items = items::list_items_for_turn(&connection, turn_id)?;
        Ok(StoredTurn { turn, items })
    }

    async fn list_turns(&self, session_id: &SessionId) -> Result<Vec<StoredTurn>, StoreError> {
        let connection = self.open_connection()?;
        let turns = turns::list_turns_for_session(&connection, session_id)?;
        let mut stored = Vec::with_capacity(turns.len());
        for turn in turns {
            let items = items::list_items_for_turn(&connection, &turn.id)?;
            stored.push(StoredTurn { turn, items });
        }
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ccodex_protocol::{
        Item, ItemId, ItemPayload, Session, SessionId, SessionStatus, Turn, TurnId, TurnStatus,
    };
    use time::OffsetDateTime;

    use super::*;

    #[tokio::test]
    async fn session_turn_and_item_roundtrip() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("ccodex-store-test-{unique}.sqlite3"));
        let store = SQLiteSessionStore::new(&db_path).expect("store should initialize");

        let session = Session {
            id: SessionId::new(),
            title: Some("demo".to_string()),
            workspace_root: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            status: SessionStatus::Active,
            active_plan: None,
            metadata: BTreeMap::new(),
        };
        store
            .create_session(&session)
            .await
            .expect("session should persist");

        let turn = Turn {
            id: TurnId::new(),
            session_id: session.id.clone(),
            item_ids: Vec::new(),
            started_at: OffsetDateTime::now_utc(),
            completed_at: None,
            status: TurnStatus::Running,
        };
        store.append_turn(&turn).await.expect("turn should persist");

        let item = Item {
            id: ItemId::new(),
            turn_id: turn.id.clone(),
            created_at: OffsetDateTime::now_utc(),
            payload: ItemPayload::UserMessage {
                content: "hello".to_string(),
            },
        };
        store.append_item(&item).await.expect("item should persist");

        let stored_session = store
            .get_session(&session.id)
            .await
            .expect("session should load");
        let stored_turn = store.get_turn(&turn.id).await.expect("turn should load");

        assert_eq!(stored_session.id, session.id);
        assert_eq!(stored_turn.turn.id, turn.id);
        assert_eq!(stored_turn.items.len(), 1);

        let _ = std::fs::remove_file(db_path);
    }
}
