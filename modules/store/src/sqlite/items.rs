use rusqlite::{Connection, params};
use serde_json::{from_str, to_string};

use ccodex_protocol::{Item, TurnId};

use crate::traits::StoreError;

pub fn upsert_item(connection: &Connection, item: &Item) -> Result<(), StoreError> {
    let payload_json =
        to_string(&item.payload).map_err(|err| StoreError::Serialization(err.to_string()))?;
    let created_at = item
        .created_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| StoreError::Serialization(err.to_string()))?;

    connection
        .execute(
            r#"
            INSERT INTO items (id, turn_id, created_at, payload_json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                turn_id = excluded.turn_id,
                created_at = excluded.created_at,
                payload_json = excluded.payload_json
            "#,
            params![item.id.0, item.turn_id.0, created_at, payload_json],
        )
        .map_err(|err| StoreError::Database(err.to_string()))?;
    Ok(())
}

pub fn list_items_for_turn(
    connection: &Connection,
    turn_id: &TurnId,
) -> Result<Vec<Item>, StoreError> {
    let mut statement = connection
        .prepare("SELECT id, turn_id, created_at, payload_json FROM items WHERE turn_id = ?1 ORDER BY created_at ASC")
        .map_err(|err| StoreError::Database(err.to_string()))?;

    statement
        .query_map(params![turn_id.0], |row| {
            let created_at: String = row.get(2)?;
            let payload_json: String = row.get(3)?;
            Ok(Item {
                id: ccodex_protocol::ItemId(row.get::<_, String>(0)?),
                turn_id: ccodex_protocol::TurnId(row.get::<_, String>(1)?),
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
                payload: from_str(&payload_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
            })
        })
        .map_err(|err| StoreError::Database(err.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| StoreError::Database(err.to_string()))
}
