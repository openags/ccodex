use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;

use ccodex_protocol::PortError;
use serde_json::{Value, json};

use crate::sandbox::{SandboxMode, SandboxPolicy};

#[derive(Debug, Clone)]
pub struct ShellOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceShell {
    workspace_root: PathBuf,
    sandbox: SandboxPolicy,
}

impl WorkspaceShell {
    pub fn new(workspace_root: PathBuf, sandbox_mode: SandboxMode) -> Self {
        Self {
            workspace_root: workspace_root.clone(),
            sandbox: SandboxPolicy::new(workspace_root, sandbox_mode),
        }
    }

    pub fn execute(&self, command: &str) -> Result<ShellOutput, PortError> {
        let (output, _) = self.execute_with_deltas(command)?;
        Ok(output)
    }

    pub fn execute_with_deltas(
        &self,
        command: &str,
    ) -> Result<(ShellOutput, Vec<Value>), PortError> {
        self.sandbox.check_command(command)?;

        let mut child = Command::new("zsh")
            .arg("-lc")
            .arg(command)
            .current_dir(&self.workspace_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| PortError::Tool(format!("failed to execute shell command: {err}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Tool("failed to capture shell stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PortError::Tool("failed to capture shell stderr".to_string()))?;

        let (sender, receiver) = mpsc::channel::<(&'static str, String)>();
        let stdout_sender = sender.clone();
        let stdout_thread = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = stdout_sender.send(("stdout", line));
            }
        });
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = sender.send(("stderr", line));
            }
        });

        let mut stdout_buffer = String::new();
        let mut stderr_buffer = String::new();
        let mut deltas = Vec::new();
        for (stream, line) in receiver {
            deltas.push(json!({
                "phase": "stream",
                "stream": stream,
                "content": line,
            }));
            match stream {
                "stdout" => {
                    if !stdout_buffer.is_empty() {
                        stdout_buffer.push('\n');
                    }
                    stdout_buffer.push_str(
                        deltas
                            .last()
                            .and_then(|delta| delta.get("content"))
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    );
                }
                "stderr" => {
                    if !stderr_buffer.is_empty() {
                        stderr_buffer.push('\n');
                    }
                    stderr_buffer.push_str(
                        deltas
                            .last()
                            .and_then(|delta| delta.get("content"))
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    );
                }
                _ => {}
            }
        }

        stdout_thread
            .join()
            .map_err(|_| PortError::Tool("stdout reader thread panicked".to_string()))?;
        stderr_thread
            .join()
            .map_err(|_| PortError::Tool("stderr reader thread panicked".to_string()))?;

        let status = child
            .wait()
            .map_err(|err| PortError::Tool(format!("failed waiting for shell command: {err}")))?;

        Ok((
            ShellOutput {
                exit_code: status.code().unwrap_or(-1),
                stdout: stdout_buffer,
                stderr: stderr_buffer,
            },
            deltas,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceShell;
    use crate::sandbox::SandboxMode;

    #[test]
    fn executes_safe_command_inside_workspace() {
        let root = std::env::temp_dir().join("ccodex-shell-safe");
        std::fs::create_dir_all(&root).expect("workspace dir should exist");
        let shell = WorkspaceShell::new(root, SandboxMode::WorkspaceWrite);

        let output = shell.execute("printf hello").expect("command should run");
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "hello");
    }

    #[test]
    fn execute_with_deltas_captures_streamed_lines() {
        let root = std::env::temp_dir().join("ccodex-shell-deltas");
        std::fs::create_dir_all(&root).expect("workspace dir should exist");
        let shell = WorkspaceShell::new(root, SandboxMode::WorkspaceWrite);

        let (output, deltas) = shell
            .execute_with_deltas("printf 'one\\ntwo\\n'")
            .expect("command should run");
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "one\ntwo");
        assert!(deltas.iter().any(|delta| {
            delta.get("phase").and_then(serde_json::Value::as_str) == Some("stream")
                && delta.get("stream").and_then(serde_json::Value::as_str) == Some("stdout")
                && delta.get("content").and_then(serde_json::Value::as_str) == Some("one")
        }));
    }
}
