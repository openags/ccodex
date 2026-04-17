use std::collections::BTreeMap;

use serde_json::json;

use ccodex_protocol::{ToolConcurrency, ToolKind, ToolSpec};

#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    specs: BTreeMap<String, ToolSpec>,
}

impl ToolRegistry {
    pub fn bootstrap_builtin() -> Self {
        let mut specs = BTreeMap::new();

        let builtins = vec![
            ToolSpec {
                name: "read_file".to_string(),
                description: "Read a UTF-8 text file from the workspace.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Shared,
                requires_approval: false,
                sandbox_profile: Some("workspace-read".to_string()),
            },
            ToolSpec {
                name: "bash".to_string(),
                description: "Execute a shell command in the workspace.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    },
                    "required": ["command", "exit_code", "stdout", "stderr"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: true,
                sandbox_profile: Some("workspace-write".to_string()),
            },
            ToolSpec {
                name: "ask_user".to_string(),
                description: "Prompt the user for a structured choice.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "message": { "type": "string" },
                        "choices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "label": { "type": "string" },
                                    "description": { "type": ["string", "null"] }
                                },
                                "required": ["id", "label"]
                            }
                        },
                        "allow_freeform": { "type": "boolean" }
                    },
                    "required": ["title", "message", "choices", "allow_freeform"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "selected_choice_id": { "type": ["string", "null"] },
                        "freeform_text": { "type": ["string", "null"] }
                    }
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: false,
                sandbox_profile: None,
            },
            ToolSpec {
                name: "update_plan".to_string(),
                description: "Update the current session plan.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": ["string", "null"] },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "title": { "type": "string" },
                                    "notes": { "type": ["string", "null"] },
                                    "status": { "type": "string" }
                                },
                                "required": ["id", "title", "status"]
                            }
                        }
                    },
                    "required": ["items"]
                }),
                output_schema: None,
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Shared,
                requires_approval: false,
                sandbox_profile: None,
            },
        ];

        for spec in builtins {
            specs.insert(spec.name.clone(), spec);
        }

        Self { specs }
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.specs.get(name)
    }

    pub fn list(&self) -> impl Iterator<Item = &ToolSpec> {
        self.specs.values()
    }
}
