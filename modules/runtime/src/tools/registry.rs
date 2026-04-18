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
                name: "write_file".to_string(),
                description: "Write UTF-8 text content to a file in the workspace.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "bytes_written": { "type": "integer" }
                    },
                    "required": ["path", "bytes_written"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: true,
                sandbox_profile: Some("workspace-write".to_string()),
            },
            ToolSpec {
                name: "edit_file".to_string(),
                description: "Replace a text snippet in a UTF-8 file in the workspace.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "replacements": { "type": "integer" }
                    },
                    "required": ["path", "replacements"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: true,
                sandbox_profile: Some("workspace-write".to_string()),
            },
            ToolSpec {
                name: "glob".to_string(),
                description: "Find workspace files matching a glob pattern.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    },
                    "required": ["pattern"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "matches": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["matches"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Shared,
                requires_approval: false,
                sandbox_profile: Some("workspace-read".to_string()),
            },
            ToolSpec {
                name: "grep".to_string(),
                description: "Search workspace files for a text pattern.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path_glob": { "type": ["string", "null"] }
                    },
                    "required": ["pattern"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "matches": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "line_number": { "type": "integer" },
                                    "line": { "type": "string" }
                                },
                                "required": ["path", "line_number", "line"]
                            }
                        }
                    },
                    "required": ["matches"]
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
                name: "spawn_agent".to_string(),
                description: "Spawn a same-process subagent to work on a delegated task."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": ["string", "null"] },
                        "prompt": { "type": "string" }
                    },
                    "required": ["prompt"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "agent_name": { "type": "string" },
                        "session_id": { "type": "string" },
                        "assistant_text": { "type": "string" }
                    },
                    "required": ["agent_name", "session_id", "assistant_text"]
                })),
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: false,
                sandbox_profile: None,
            },
            ToolSpec {
                name: "mcp_call".to_string(),
                description: "Invoke a tool exposed by a configured MCP server.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string" },
                        "tool": { "type": "string" },
                        "input": { "type": ["object", "array", "string", "number", "boolean", "null"] }
                    },
                    "required": ["server", "tool"]
                }),
                output_schema: Some(json!({
                    "type": ["object", "array", "string", "number", "boolean", "null"]
                })),
                kind: ToolKind::Mcp,
                concurrency: ToolConcurrency::Exclusive,
                requires_approval: true,
                sandbox_profile: None,
            },
            ToolSpec {
                name: "enter_plan_mode".to_string(),
                description: "Enter plan mode and initialize the current session plan.".to_string(),
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
                    }
                }),
                output_schema: None,
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Shared,
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
            ToolSpec {
                name: "exit_plan_mode".to_string(),
                description: "Exit plan mode and clear the active session plan.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "reason": { "type": ["string", "null"] }
                    }
                }),
                output_schema: None,
                kind: ToolKind::Builtin,
                concurrency: ToolConcurrency::Shared,
                requires_approval: false,
                sandbox_profile: None,
            },
            ToolSpec {
                name: "todo_write".to_string(),
                description: "Write or replace the current structured todo plan.".to_string(),
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
