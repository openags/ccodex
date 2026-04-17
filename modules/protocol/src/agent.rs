use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentId;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSpec {
    pub id: AgentId,
    pub name: String,
    pub description: Option<String>,
    pub instructions: String,
    pub metadata: BTreeMap<String, Value>,
}
