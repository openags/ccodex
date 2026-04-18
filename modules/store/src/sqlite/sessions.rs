use rusqlite::{Connection, params};
use serde_json::{from_str, to_string};

use ccodex_protocol::{Session, SessionId, SessionStatus};

use crate::traits::{ListSessionsParams, StoreError};

pub fn upsert_session(connection: &Connection, session: &Session) -> Result<(), StoreError> {
    let active_plan_json = session
        .active_plan
        .as_ref()
        .map(to_string)
        .transpose()
        .map_err(|err| StoreError::Serialization(err.to_string()))?;
    let metadata_json =
        to_string(&session.metadata).map_err(|err| StoreError::Serialization(err.to_string()))?;

    connection
        .execute(
            r#"
            INSERT INTO sessions (
                id, title, workspace_root, created_at, updated_at, status, active_plan_json, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                workspace_root = excluded.workspace_root,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                status = excluded.status,
                active_plan_json = excluded.active_plan_json,
                metadata_json = excluded.metadata_json
            "#,
            params![
                session.id.0,
                session.title,
                session.workspace_root.as_ref().map(|path| path.display().to_string()),
                session.created_at.format(&time::format_description::well_known::Rfc3339).map_err(|err| StoreError::Serialization(err.to_string()))?,
                session.updated_at.format(&time::format_description::well_known::Rfc3339).map_err(|err| StoreError::Serialization(err.to_string()))?,
                match session.status {
                    SessionStatus::Active => "active",
                    SessionStatus::Archived => "archived",
                },
                active_plan_json,
                metadata_json,
            ],
        )
        .map_err(|err| StoreError::Database(err.to_string()))?;

    Ok(())
}

pub fn get_session(connection: &Connection, session_id: &SessionId) -> Result<Session, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, title, workspace_root, created_at, updated_at, status, active_plan_json, metadata_json FROM sessions WHERE id = ?1",
        )
        .map_err(|err| StoreError::Database(err.to_string()))?;

    statement
        .query_row(params![session_id.0], |row| {
            let created_at: String = row.get(3)?;
            let updated_at: String = row.get(4)?;
            let active_plan_json: Option<String> = row.get(6)?;
            let metadata_json: String = row.get(7)?;

            Ok(Session {
                id: SessionId(row.get::<_, String>(0)?),
                title: row.get(1)?,
                workspace_root: row.get::<_, Option<String>>(2)?.map(Into::into),
                created_at: time::OffsetDateTime::parse(
                    &created_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                updated_at: time::OffsetDateTime::parse(
                    &updated_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                status: match row.get::<_, String>(5)?.as_str() {
                    "archived" => SessionStatus::Archived,
                    _ => SessionStatus::Active,
                },
                active_plan: active_plan_json
                    .map(|json| from_str(&json))
                    .transpose()
                    .map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(err),
                        )
                    })?,
                metadata: from_str(&metadata_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
            })
        })
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound(session_id.to_string()),
            other => StoreError::Database(other.to_string()),
        })
}

pub fn list_sessions(
    connection: &Connection,
    params_cfg: ListSessionsParams,
) -> Result<Vec<Session>, StoreError> {
    let limit = params_cfg.limit.unwrap_or(100) as i64;
    let mut statement = connection
        .prepare("SELECT id FROM sessions ORDER BY updated_at DESC LIMIT ?1")
        .map_err(|err| StoreError::Database(err.to_string()))?;

    let ids = statement
        .query_map(params![limit], |row| row.get::<_, String>(0))
        .map_err(|err| StoreError::Database(err.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| StoreError::Database(err.to_string()))?;

    ids.iter()
        .map(|id| get_session(connection, &SessionId(id.clone())))
        .collect()
}
