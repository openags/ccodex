use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{ItemId, TurnId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum TurnStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Turn {
    pub id: TurnId,
    pub session_id: crate::SessionId,
    pub item_ids: Vec<ItemId>,
    #[schemars(with = "String")]
    pub started_at: OffsetDateTime,
    #[schemars(with = "Option<String>")]
    pub completed_at: Option<OffsetDateTime>,
    pub status: TurnStatus,
}
