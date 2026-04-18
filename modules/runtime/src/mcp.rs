use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use ccodex_brand::user_home_dir;
use ccodex_protocol::{McpPort, PortError};

const CLAUDE_DIR_NAME: &str = ".claude";

#[derive(Debug, Clone)]
pub struct McpServerDefinition {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CommandBackedMcpPort {
    workspace_root: PathBuf,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawMcpServerDefinition {
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<String>,
}

impl CommandBackedMcpPort {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    pub fn load_servers(&self) -> Result<Vec<McpServerDefinition>, PortError> {
        let mut servers = Vec::new();
        servers.extend(load_server_dir(
            &self.workspace_root.join(".ccodex").join("mcp"),
            &self.workspace_root,
        )?);
        servers.extend(load_server_dir(
            &self.workspace_root.join(CLAUDE_DIR_NAME).join("mcp"),
            &self.workspace_root,
        )?);
        servers.extend(load_server_dir(
            &user_home_dir().join("mcp"),
            &self.workspace_root,
        )?);
        Ok(servers)
    }

    fn resolve_server(&self, name: &str) -> Result<McpServerDefinition, PortError> {
        self.load_servers()?
            .into_iter()
            .find(|server| server.name == name)
            .ok_or_else(|| PortError::Mcp(format!("unknown MCP server: {name}")))
    }
}

#[async_trait]
impl McpPort for CommandBackedMcpPort {
    async fn call_tool(&self, server: &str, tool: &str, input: Value) -> Result<Value, PortError> {
        let definition = self.resolve_server(server)?;
        let mut command = Command::new(&definition.command);
        command.args(&definition.args);
        command.current_dir(
            definition
                .cwd
                .clone()
                .unwrap_or_else(|| self.workspace_root.clone()),
        );
        command.env("CCODEX_MCP_SERVER", server);
        command.env("CCODEX_MCP_TOOL", tool);
        command.env("CCODEX_MCP_INPUT", input.to_string());
        command.env(
            "CCODEX_WORKSPACE_ROOT",
            self.workspace_root.display().to_string(),
        );
        for (key, value) in definition.env {
            command.env(key, value);
        }

        let output = command
            .output()
            .map_err(|err| PortError::Mcp(format!("failed to spawn MCP server {server}: {err}")))?;

        if !output.status.success() {
            return Err(PortError::Mcp(format!(
                "MCP server {server} exited with {}: {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(json!({ "ok": true, "server": server, "tool": tool, "output": null }));
        }

        serde_json::from_str(&stdout).or_else(|_| {
            Ok(json!({
                "server": server,
                "tool": tool,
                "output": stdout,
            }))
        })
    }
}

fn load_server_dir(
    dir: &Path,
    workspace_root: &Path,
) -> Result<Vec<McpServerDefinition>, PortError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut servers = Vec::new();
    let entries = fs::read_dir(dir).map_err(|err| {
        PortError::Mcp(format!("failed to read MCP dir {}: {err}", dir.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            PortError::Mcp(format!("failed to walk MCP dir {}: {err}", dir.display()))
        })?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let raw = fs::read_to_string(&path).map_err(|err| {
            PortError::Mcp(format!("failed to read MCP file {}: {err}", path.display()))
        })?;
        let parsed: RawMcpServerDefinition = toml::from_str(&raw).map_err(|err| {
            PortError::Mcp(format!(
                "failed to parse MCP file {}: {err}",
                path.display()
            ))
        })?;
        let Some(command) = parsed.command else {
            continue;
        };
        let cwd = parsed.cwd.map(|cwd| {
            let candidate = PathBuf::from(cwd);
            if candidate.is_absolute() {
                candidate
            } else {
                workspace_root.join(candidate)
            }
        });

        servers.push(McpServerDefinition {
            name: path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
                .to_string(),
            command,
            args: parsed.args.unwrap_or_default(),
            env: parsed.env.unwrap_or_default(),
            cwd,
        });
    }

    servers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(servers)
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use serde_json::{Value, json};

    use super::CommandBackedMcpPort;
    use ccodex_protocol::McpPort;

    #[test]
    fn command_backed_mcp_port_executes_server_from_workspace() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-mcp-{unique}"));
        std::fs::create_dir_all(root.join(".ccodex").join("mcp")).expect("mcp dir should exist");
        std::fs::write(
            root.join(".ccodex").join("mcp").join("echo.toml"),
            r#"
command = "python3"
args = ["-c", "import json, os; print(json.dumps({'server': os.environ['CCODEX_MCP_SERVER'], 'tool': os.environ['CCODEX_MCP_TOOL'], 'input': json.loads(os.environ['CCODEX_MCP_INPUT'])}))"]
"#,
        )
        .expect("mcp config should write");

        let port = CommandBackedMcpPort::new(root.clone());
        let result = block_on(port.call_tool("echo", "inspect", json!({"path":"Cargo.toml"})))
            .expect("mcp call should succeed");

        assert_eq!(result.get("server").and_then(Value::as_str), Some("echo"));
        assert_eq!(result.get("tool").and_then(Value::as_str), Some("inspect"));
        assert_eq!(
            result
                .get("input")
                .and_then(Value::as_object)
                .and_then(|obj| obj.get("path"))
                .and_then(Value::as_str),
            Some("Cargo.toml")
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
