use serde_json::{Value, json};
use time::OffsetDateTime;

use ccodex_extensions::ExtensionRegistry;
use ccodex_protocol::{ItemPayload, ProtocolEvent, Session, Turn};
use ccodex_runtime::CommandBackedMcpPort;

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn bootstrap_session_context(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
    ) -> Result<(), KernelError> {
        let Some(workspace_root) = session.workspace_root.as_ref() else {
            return Ok(());
        };
        if session.metadata.contains_key("bootstrap_complete") {
            return Ok(());
        }

        let compat = self.compat.load_workspace_instructions(workspace_root)?;
        let extension_roots = self.compat.extension_roots(workspace_root);
        let skills = ExtensionRegistry::load_skill_instructions_for_roots(&extension_roots)?;
        let agents = ExtensionRegistry::load_agents_for_roots(&extension_roots)?;
        let hooks = ExtensionRegistry::load_hooks_for_roots(&extension_roots)?;
        let mcp_servers = CommandBackedMcpPort::new(workspace_root.clone()).load_servers()?;

        session
            .metadata
            .insert("bootstrap_complete".to_string(), json!(true));
        session.metadata.insert(
            "bootstrap_instruction_count".to_string(),
            json!(compat.instructions.len()),
        );
        session.metadata.insert(
            "bootstrap_instruction_sources".to_string(),
            Value::Array(
                compat
                    .sources()
                    .into_iter()
                    .map(|path| Value::String(path.display().to_string()))
                    .collect(),
            ),
        );
        session
            .metadata
            .insert("bootstrap_skill_count".to_string(), json!(skills.len()));
        session.metadata.insert(
            "bootstrap_skill_names".to_string(),
            Value::Array(
                skills
                    .iter()
                    .map(|skill| Value::String(skill.manifest.name.clone()))
                    .collect(),
            ),
        );
        session
            .metadata
            .insert("bootstrap_agent_count".to_string(), json!(agents.len()));
        session.metadata.insert(
            "bootstrap_agent_names".to_string(),
            Value::Array(
                agents
                    .iter()
                    .map(|agent| Value::String(agent.name.clone()))
                    .collect(),
            ),
        );
        session
            .metadata
            .insert("bootstrap_hook_count".to_string(), json!(hooks.len()));
        session.metadata.insert(
            "bootstrap_hook_names".to_string(),
            Value::Array(
                hooks
                    .iter()
                    .map(|hook| Value::String(hook.manifest.name.clone()))
                    .collect(),
            ),
        );
        session.metadata.insert(
            "bootstrap_mcp_server_count".to_string(),
            json!(mcp_servers.len()),
        );
        session.metadata.insert(
            "bootstrap_mcp_server_names".to_string(),
            Value::Array(
                mcp_servers
                    .iter()
                    .map(|server| Value::String(server.name.clone()))
                    .collect(),
            ),
        );
        session.metadata.insert(
            "bootstrapped_at".to_string(),
            Value::String(
                OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string()),
            ),
        );
        session.updated_at = OffsetDateTime::now_utc();
        self.store.update_session(session).await?;
        self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
            .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::SystemEvent {
                name: "session_bootstrap".to_string(),
                payload: json!({
                    "instruction_count": compat.instructions.len(),
                    "instruction_sources": compat
                        .sources()
                        .into_iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>(),
                    "skill_count": skills.len(),
                    "agent_count": agents.len(),
                    "hook_count": hooks.len(),
                    "mcp_server_count": mcp_servers.len(),
                    "skill_names": skills.iter().map(|skill| skill.manifest.name.clone()).collect::<Vec<_>>(),
                    "agent_names": agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
                    "hook_names": hooks.iter().map(|hook| hook.manifest.name.clone()).collect::<Vec<_>>(),
                    "mcp_server_names": mcp_servers.iter().map(|server| server.name.clone()).collect::<Vec<_>>(),
                }),
            },
        )
        .await?;

        Ok(())
    }
}
