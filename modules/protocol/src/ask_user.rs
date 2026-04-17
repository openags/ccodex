use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ItemId;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AskUserChoice {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AskUserPrompt {
    pub item_id: ItemId,
    pub title: String,
    pub message: String,
    pub choices: Vec<AskUserChoice>,
    pub allow_freeform: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AskUserResponse {
    pub request_item_id: ItemId,
    pub selected_choice_id: Option<String>,
    pub freeform_text: Option<String>,
}
