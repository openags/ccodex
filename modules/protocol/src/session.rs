use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{PlanState, SessionId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    pub id: SessionId,
    pub title: Option<String>,
    pub workspace_root: Option<PathBuf>,
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
    #[schemars(with = "String")]
    pub updated_at: OffsetDateTime,
    pub status: SessionStatus,
    pub active_plan: Option<PlanState>,
    pub metadata: BTreeMap<String, Value>,
}
