use serde_json::json;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use ccodex_extensions::{ExtensionRegistry, HookDefinition, HookEvent};
use ccodex_protocol::{ItemPayload, ProtocolEvent, Session, ToolCall, ToolResult, Turn};

use crate::{Kernel, KernelError};

pub(crate) struct HookContext<'a> {
    pub prompt: &'a str,
    pub assistant_text: &'a str,
    pub tool_call: Option<&'a ToolCall>,
    pub tool_result: Option<&'a ToolResult>,
}

impl Kernel {
    pub(crate) async fn run_hooks(
        &self,
        session: &Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        hook_event: HookEvent,
        context: HookContext<'_>,
    ) -> Result<(), KernelError> {
        let Some(workspace_root) = session.workspace_root.as_ref() else {
            return Ok(());
        };
        // Security: Use hook_roots() which respects workspace trust level
        // Untrusted workspaces only load hooks from trusted roots (user home, builtin)
        let extension_roots = self.compat.hook_roots(workspace_root);
        let hooks = ExtensionRegistry::load_hooks_for_roots(&extension_roots)?;
        for hook in hooks.into_iter().filter(|hook| hook.event == hook_event) {
            self.run_single_hook(session, turn, events, &hook, &context)
                .await?;
        }
        Ok(())
    }

    async fn run_single_hook(
        &self,
        session: &Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        hook: &HookDefinition,
        context: &HookContext<'_>,
    ) -> Result<(), KernelError> {
        let Some(workspace_root) = session.workspace_root.as_ref() else {
            return Ok(());
        };
        let mut command = Command::new("zsh");
        command.kill_on_drop(true);
        command
            .arg("-lc")
            .arg(&hook.command)
            .current_dir(workspace_root)
            .env("CCODEX_SESSION_ID", session.id.to_string())
            .env("CCODEX_TURN_ID", turn.id.to_string())
            .env("CCODEX_HOOK_NAME", &hook.manifest.name)
            .env("CCODEX_HOOK_EVENT", format!("{:?}", hook.event))
            .env("CCODEX_PROMPT", context.prompt)
            .env("CCODEX_ASSISTANT_TEXT", context.assistant_text);
        if let Some(call) = context.tool_call {
            command
                .env("CCODEX_TOOL_NAME", &call.tool_name)
                .env("CCODEX_TOOL_CALL_ID", call.id.to_string())
                .env("CCODEX_TOOL_INPUT", call.input.to_string());
        }
        if let Some(result) = context.tool_result {
            command
                .env("CCODEX_TOOL_RESULT", result.output.to_string())
                .env(
                    "CCODEX_TOOL_IS_ERROR",
                    if result.is_error { "true" } else { "false" },
                );
        }
        let output = timeout(Duration::from_millis(hook.timeout_ms), command.output()).await;

        match output {
            Ok(Ok(output)) => {
                if output.status.success() {
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::SystemEvent {
                            name: "hook_executed".to_string(),
                            payload: json!({
                                "hook_name": hook.manifest.name,
                                "event": format!("{:?}", hook.event),
                                "exit_code": output.status.code().unwrap_or(0),
                                "stdout": String::from_utf8_lossy(&output.stdout),
                                "stderr": String::from_utf8_lossy(&output.stderr),
                                "tool_name": context.tool_call.map(|call| call.tool_name.as_str()),
                                "tool_call_id": context.tool_call.map(|call| call.id.to_string()),
                            }),
                        },
                    )
                    .await?;
                } else {
                    self.append_item(
                        turn,
                        events,
                        ItemPayload::Warning {
                            code: "hook_failed".to_string(),
                            message: format!(
                                "{} failed with exit code {}",
                                hook.manifest.name,
                                output.status.code().unwrap_or(-1)
                            ),
                        },
                    )
                    .await?;
                }
            }
            Ok(Err(err)) => {
                self.append_item(
                    turn,
                    events,
                    ItemPayload::Warning {
                        code: "hook_error".to_string(),
                        message: format!("{} failed to start: {}", hook.manifest.name, err),
                    },
                )
                .await?;
            }
            Err(_) => {
                self.append_item(
                    turn,
                    events,
                    ItemPayload::Warning {
                        code: "hook_timed_out".to_string(),
                        message: format!("{} timed out", hook.manifest.name),
                    },
                )
                .await?;
            }
        }

        Ok(())
    }
}
