use rusqlite::{params, Connection};
use serde_json::{from_str, to_string};

use ccodex_protocol::{Turn, TurnId, TurnStatus};

use crate::traits::StoreError;

pub fn upsert_turn(connection: &Connection, turn: &Turn) -> Result<(), StoreError> {
    let item_ids_json = to_string(&turn.item_ids).map_err(|err| StoreError::Serialization(err.to_string()))?;
    let started_at = turn
        .started_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| StoreError::Serialization(err.to_string()))?;
    let completed_at = turn
        .completed_at
        .map(|value| value.format(&time::format_description::well_known::Rfc3339))
        .transpose()
        .map_err(|err| StoreError::Serialization(err.to_string()))?;

    connection
        .execute(
            r#"
            INSERT INTO turns (id, session_id, item_ids_json, started_at, completed_at, status)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                session_id = excluded.session_id,
                item_ids_json = excluded.item_ids_json,
                started_at = excluded.started_at,
                completed_at = excluded.completed_at,
                status = excluded.status
            "#,
            params![
                turn.id.0,
                turn.session_id.0,
                item_ids_json,
                started_at,
                completed_at,
                match turn.status {
                    TurnStatus::Running => "running",
                    TurnStatus::Completed => "completed",
                    TurnStatus::Failed => "failed",
                    TurnStatus::Interrupted => "interrupted",
                }
            ],
        )
        .map_err(|err| StoreError::Database(err.to_string()))?;
    Ok(())
}

pub fn get_turn(connection: &Connection, turn_id: &TurnId) -> Result<Turn, StoreError> {
    let mut statement = connection
        .prepare("SELECT id, session_id, item_ids_json, started_at, completed_at, status FROM turns WHERE id = ?1")
        .map_err(|err| StoreError::Database(err.to_string()))?;

    statement
        .query_row(params![turn_id.0], |row| {
            let item_ids_json: String = row.get(2)?;
            let started_at: String = row.get(3)?;
            let completed_at: Option<String> = row.get(4)?;
            Ok(Turn {
                id: TurnId(row.get::<_, String>(0)?),
                session_id: ccodex_protocol::SessionId(row.get::<_, String>(1)?),
                item_ids: from_str(&item_ids_json)
                    .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))?,
                started_at: time::OffsetDateTime::parse(&started_at, &time::format_description::well_known::Rfc3339)
                    .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))?,
                completed_at: completed_at
                    .map(|value| time::OffsetDateTime::parse(&value, &time::format_description::well_known::Rfc3339))
                    .transpose()
                    .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))?,
                status: match row.get::<_, String>(5)?.as_str() {
                    "completed" => TurnStatus::Completed,
                    "failed" => TurnStatus::Failed,
                    "interrupted" => TurnStatus::Interrupted,
                    _ => TurnStatus::Running,
                },
            })
        })
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound(turn_id.to_string()),
            other => StoreError::Database(other.to_string()),
        })
}

pub fn list_turns_for_session(
    connection: &Connection,
    session_id: &ccodex_protocol::SessionId,
) -> Result<Vec<Turn>, StoreError> {
    let mut statement = connection
        .prepare("SELECT id FROM turns WHERE session_id = ?1 ORDER BY started_at ASC")
        .map_err(|err| StoreError::Database(err.to_string()))?;

    let ids = statement
        .query_map(params![session_id.0], |row| row.get::<_, String>(0))
        .map_err(|err| StoreError::Database(err.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| StoreError::Database(err.to_string()))?;

    ids.iter()
        .map(|id| get_turn(connection, &TurnId(id.clone())))
        .collect()
}
