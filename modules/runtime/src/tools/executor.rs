use std::path::PathBuf;
use std::process::Command;

use async_trait::async_trait;
use serde_json::{json, Value};

use ccodex_protocol::{PortError, ToolCall, ToolExecutorPort, ToolResult};

use super::registry::ToolRegistry;

#[derive(Debug, Clone)]
pub struct BuiltinToolExecutor {
    workspace_root: PathBuf,
    registry: ToolRegistry,
}

impl BuiltinToolExecutor {
    pub fn new(workspace_root: PathBuf, registry: ToolRegistry) -> Self {
        Self {
            workspace_root,
            registry,
        }
    }

    fn resolve_workspace_path(&self, path: &str) -> Result<PathBuf, PortError> {
        let candidate = PathBuf::from(path);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root.join(candidate)
        };

        if !resolved.starts_with(&self.workspace_root) {
            return Err(PortError::Tool(format!(
                "path escapes workspace: {}",
                resolved.display()
            )));
        }

        Ok(resolved)
    }

    fn read_file(&self, call: &ToolCall) -> Result<Value, PortError> {
        let path = call
            .input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("read_file requires a string path".to_string()))?;
        let resolved = self.resolve_workspace_path(path)?;
        let content = std::fs::read_to_string(&resolved)
            .map_err(|err| PortError::Tool(format!("failed to read {}: {err}", resolved.display())))?;

        Ok(json!({
            "path": resolved.display().to_string(),
            "content": content,
        }))
    }

    fn run_bash(&self, call: &ToolCall) -> Result<Value, PortError> {
        let command = call
            .input
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("bash requires a string command".to_string()))?;

        let output = Command::new("zsh")
            .arg("-lc")
            .arg(command)
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|err| PortError::Tool(format!("failed to execute shell command: {err}")))?;

        Ok(json!({
            "command": command,
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
    }

    fn update_plan(&self, call: &ToolCall) -> Result<Value, PortError> {
        let items = call
            .input
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| PortError::Tool("update_plan requires an items array".to_string()))?;

        Ok(json!({
            "summary": call.input.get("summary").cloned().unwrap_or(Value::Null),
            "items": items,
        }))
    }
}

#[async_trait]
impl ToolExecutorPort for BuiltinToolExecutor {
    async fn execute_tool(&self, call: ToolCall) -> Result<ToolResult, PortError> {
        let output = match call.tool_name.as_str() {
            "read_file" => self.read_file(&call)?,
            "bash" => self.run_bash(&call)?,
            "update_plan" => self.update_plan(&call)?,
            other => {
                let known = self.registry.get(other).is_some();
                return Err(PortError::Tool(if known {
                    format!("tool {other} is registered but not executable")
                } else {
                    format!("unknown tool: {other}")
                }));
            }
        };

        Ok(ToolResult {
            tool_call_id: call.id,
            output,
            is_error: false,
        })
    }
}
