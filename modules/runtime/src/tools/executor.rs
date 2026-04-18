use async_trait::async_trait;
use glob::Pattern;
use serde_json::{Value, json};

use ccodex_protocol::{
    McpPort, PortError, ToolCall, ToolExecutionOutcome, ToolExecutorPort, ToolResult,
};

use crate::{CommandBackedMcpPort, SandboxMode, WorkspaceFs, WorkspaceShell};

use super::registry::ToolRegistry;

#[derive(Debug, Clone)]
pub struct BuiltinToolExecutor {
    registry: ToolRegistry,
    fs: WorkspaceFs,
    shell: WorkspaceShell,
    mcp: CommandBackedMcpPort,
}

impl BuiltinToolExecutor {
    pub fn new(
        workspace_root: std::path::PathBuf,
        sandbox_mode: SandboxMode,
        registry: ToolRegistry,
    ) -> Self {
        Self {
            registry,
            fs: WorkspaceFs::new(workspace_root.clone(), sandbox_mode.clone()),
            shell: WorkspaceShell::new(workspace_root.clone(), sandbox_mode),
            mcp: CommandBackedMcpPort::new(workspace_root),
        }
    }

    fn read_file(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let path = call
            .input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("read_file requires a string path".to_string()))?;
        let (resolved, content) = self.fs.read_to_string(path)?;

        let bytes = content.len();
        Ok((
            json!({
                "path": resolved.display().to_string(),
                "content": content,
            }),
            vec![json!({
                "phase": "fs_read",
                "path": resolved.display().to_string(),
                "bytes": bytes,
            })],
        ))
    }

    fn write_file(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let path = call
            .input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("write_file requires a string path".to_string()))?;
        let content = call
            .input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("write_file requires string content".to_string()))?;

        let resolved = self.fs.write_string(path, content)?;

        Ok((
            json!({
                "path": resolved.display().to_string(),
                "bytes_written": content.len(),
            }),
            vec![json!({
                "phase": "fs_write",
                "path": resolved.display().to_string(),
                "bytes_written": content.len(),
            })],
        ))
    }

    fn edit_file(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let path = call
            .input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("edit_file requires a string path".to_string()))?;
        let old_text = call
            .input
            .get("old_text")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("edit_file requires old_text".to_string()))?;
        let new_text = call
            .input
            .get("new_text")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("edit_file requires new_text".to_string()))?;

        let (resolved, replacements) = self.fs.replace_in_file(path, old_text, new_text)?;

        Ok((
            json!({
                "path": resolved.display().to_string(),
                "replacements": replacements,
            }),
            vec![json!({
                "phase": "fs_edit",
                "path": resolved.display().to_string(),
                "replacements": replacements,
            })],
        ))
    }

    fn glob_files(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let pattern = call
            .input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("glob requires a string pattern".to_string()))?;
        let matcher = Pattern::new(pattern)
            .map_err(|err| PortError::Tool(format!("invalid glob pattern {pattern:?}: {err}")))?;

        let mut matches = Vec::new();
        self.fs.walk_files(&mut |path| {
            if let Ok(relative) = path.strip_prefix(self.fs.workspace_root()) {
                let candidate = relative.to_string_lossy().replace('\\', "/");
                if matcher.matches(&candidate) {
                    matches.push(candidate);
                }
            }
            Ok(())
        })?;

        let match_count = matches.len();
        Ok((
            json!({ "matches": matches }),
            vec![json!({
                "phase": "glob_scanned",
                "pattern": pattern,
                "match_count": match_count,
            })],
        ))
    }

    fn grep_files(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let pattern = call
            .input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("grep requires a string pattern".to_string()))?;
        let path_glob = call.input.get("path_glob").and_then(Value::as_str);
        let matcher = if let Some(glob) = path_glob {
            Some(Pattern::new(glob).map_err(|err| {
                PortError::Tool(format!("invalid grep path_glob {glob:?}: {err}"))
            })?)
        } else {
            None
        };

        let mut matches = Vec::new();
        self.fs.walk_files(&mut |path| {
            let relative = match path.strip_prefix(self.fs.workspace_root()) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                Err(_) => return Ok(()),
            };

            if let Some(glob) = &matcher {
                if !glob.matches(&relative) {
                    return Ok(());
                }
            }

            let Ok(content) = std::fs::read_to_string(path) else {
                return Ok(());
            };

            for (index, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push(json!({
                        "path": relative,
                        "line_number": index + 1,
                        "line": line,
                    }));
                }
            }

            Ok(())
        })?;

        let match_count = matches.len();
        Ok((
            json!({ "matches": matches }),
            vec![json!({
                "phase": "grep_scanned",
                "pattern": pattern,
                "match_count": match_count,
            })],
        ))
    }

    fn run_bash(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let command = call
            .input
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("bash requires a string command".to_string()))?;

        let (output, deltas) = self.shell.execute_with_deltas(command)?;

        Ok((
            json!({
                "command": command,
                "exit_code": output.exit_code,
                "stdout": output.stdout,
                "stderr": output.stderr,
            }),
            deltas,
        ))
    }

    fn update_plan(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let items = call
            .input
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let item_count = items.len();
        Ok((
            json!({
                "summary": call.input.get("summary").cloned().unwrap_or(Value::Null),
                "items": items,
            }),
            vec![json!({
                "phase": "plan_update",
                "item_count": item_count,
            })],
        ))
    }

    async fn call_mcp(&self, call: &ToolCall) -> Result<(Value, Vec<Value>), PortError> {
        let server = call
            .input
            .get("server")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("mcp_call requires a server".to_string()))?;
        let tool = call
            .input
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| PortError::Tool("mcp_call requires a tool".to_string()))?;
        let input = call
            .input
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let output = self.mcp.call_tool(server, tool, input).await?;
        Ok((
            output,
            vec![json!({
                "phase": "mcp_call",
                "server": server,
                "tool": tool,
            })],
        ))
    }
}

#[async_trait]
impl ToolExecutorPort for BuiltinToolExecutor {
    async fn execute_tool(&self, call: ToolCall) -> Result<ToolExecutionOutcome, PortError> {
        let (output, deltas) = match call.tool_name.as_str() {
            "read_file" => self.read_file(&call)?,
            "write_file" => self.write_file(&call)?,
            "edit_file" => self.edit_file(&call)?,
            "glob" => self.glob_files(&call)?,
            "grep" => self.grep_files(&call)?,
            "bash" => self.run_bash(&call)?,
            "enter_plan_mode" | "update_plan" | "todo_write" => self.update_plan(&call)?,
            "exit_plan_mode" => (
                json!({
                    "reason": call.input.get("reason").cloned().unwrap_or(Value::Null),
                    "closed": true,
                }),
                vec![json!({
                    "phase": "plan_exit",
                })],
            ),
            "mcp_call" => self.call_mcp(&call).await?,
            other => {
                let known = self.registry.get(other).is_some();
                return Err(PortError::Tool(if known {
                    format!("tool {other} is registered but not executable")
                } else {
                    format!("unknown tool: {other}")
                }));
            }
        };

        Ok(ToolExecutionOutcome {
            result: ToolResult {
                tool_call_id: call.id,
                output,
                is_error: false,
            },
            deltas,
        })
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use serde_json::{Value, json};

    use ccodex_protocol::{ToolCall, ToolCallId, ToolExecutorPort};

    use crate::{BuiltinToolExecutor, SandboxMode, ToolRegistry};

    #[test]
    fn read_only_sandbox_denies_write_file() {
        let root = std::env::temp_dir().join("ccodex-executor-readonly");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("workspace should exist");

        let executor = BuiltinToolExecutor::new(
            root.clone(),
            SandboxMode::ReadOnly,
            ToolRegistry::bootstrap_builtin(),
        );
        let error = block_on(executor.execute_tool(ToolCall {
            id: ToolCallId("tool-write".to_string()),
            tool_name: "write_file".to_string(),
            input: json!({"path":"notes/out.txt","content":"hello"}),
        }))
        .expect_err("read-only sandbox should reject writes");

        assert!(error.to_string().contains("read-only"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_write_sandbox_denies_dangerous_bash() {
        let root = std::env::temp_dir().join("ccodex-executor-danger");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("workspace should exist");

        let executor = BuiltinToolExecutor::new(
            root.clone(),
            SandboxMode::WorkspaceWrite,
            ToolRegistry::bootstrap_builtin(),
        );
        let error = block_on(executor.execute_tool(ToolCall {
            id: ToolCallId("tool-bash".to_string()),
            tool_name: "bash".to_string(),
            input: json!({"command":"sudo rm -rf /tmp/ccodex-danger"}),
        }))
        .expect_err("dangerous command should be blocked");

        assert!(error.to_string().contains("dangerous"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn executes_command_backed_mcp_call() {
        let root = std::env::temp_dir().join("ccodex-executor-mcp");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".ccodex").join("mcp")).expect("mcp dir should exist");
        std::fs::write(
            root.join(".ccodex").join("mcp").join("echo.toml"),
            "command = \"python3\"\nargs = [\"-c\", \"import json, os; print(json.dumps({'tool': os.environ['CCODEX_MCP_TOOL'], 'input': json.loads(os.environ['CCODEX_MCP_INPUT'])}))\"]\n",
        )
        .expect("mcp config should write");

        let executor = BuiltinToolExecutor::new(
            root.clone(),
            SandboxMode::WorkspaceWrite,
            ToolRegistry::bootstrap_builtin(),
        );
        let result = block_on(executor.execute_tool(ToolCall {
            id: ToolCallId("tool-mcp".to_string()),
            tool_name: "mcp_call".to_string(),
            input: json!({
                "server": "echo",
                "tool": "inspect",
                "input": { "path": "Cargo.toml" }
            }),
        }))
        .expect("mcp call should succeed");

        assert_eq!(
            result.result.output.get("tool").and_then(Value::as_str),
            Some("inspect")
        );
        assert_eq!(
            result
                .result
                .output
                .get("input")
                .and_then(Value::as_object)
                .and_then(|obj| obj.get("path"))
                .and_then(Value::as_str),
            Some("Cargo.toml")
        );
        assert!(
            result
                .deltas
                .iter()
                .any(|delta| { delta.get("phase").and_then(Value::as_str) == Some("mcp_call") })
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
