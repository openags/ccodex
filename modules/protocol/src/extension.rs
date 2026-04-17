use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExtensionKind {
    Plugin,
    Skill,
    Agent,
    Hook,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionManifest {
    pub name: String,
    pub kind: ExtensionKind,
    pub version: Option<String>,
    pub source_path: PathBuf,
    pub description: Option<String>,
}
