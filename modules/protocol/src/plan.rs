use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{PlanId, SessionId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum PlanItemStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanItem {
    pub id: String,
    pub title: String,
    pub notes: Option<String>,
    pub status: PlanItemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum PlanMode {
    Inactive,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanState {
    pub id: PlanId,
    pub session_id: SessionId,
    pub mode: PlanMode,
    pub summary: Option<String>,
    pub items: Vec<PlanItem>,
    #[schemars(with = "String")]
    pub updated_at: OffsetDateTime,
}
