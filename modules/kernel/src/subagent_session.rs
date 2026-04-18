use serde_json::Value;
use time::OffsetDateTime;

use ccodex_extensions::ExtensionRegistry;
use ccodex_protocol::{Item, Session, TurnStatus};

use crate::{Kernel, KernelError};

const SUBAGENT_PARENT_CONTEXT_ITEMS_METADATA_KEY: &str = "subagent_parent_context_items";

impl Kernel {
    pub(crate) fn load_subagent_spec(
        &self,
        parent_session: &Session,
        name: &str,
    ) -> Result<Option<ccodex_protocol::AgentSpec>, KernelError> {
        parent_session
            .workspace_root
            .as_ref()
            .map(|root| {
                let extension_roots = self.compat.extension_roots(root);
                ExtensionRegistry::load_agents_for_roots(&extension_roots)
            })
            .transpose()
            .map_err(KernelError::from)
            .map(|agents| {
                agents.and_then(|agents| agents.into_iter().find(|agent| agent.name == name))
            })
    }

    pub(crate) async fn prepare_subagent_session(
        &self,
        parent_session: &Session,
        name: &str,
        agent_spec: Option<&ccodex_protocol::AgentSpec>,
    ) -> Result<Session, KernelError> {
        let parent_turns = self.store.list_turns(&parent_session.id).await?;
        let parent_context_items = parent_turns
            .iter()
            .filter(|stored| stored.turn.status == TurnStatus::Running)
            .flat_map(|stored| stored.items.iter().cloned())
            .collect::<Vec<Item>>();
        let stable_turns = parent_turns
            .into_iter()
            .filter(|stored| stored.turn.status != TurnStatus::Running)
            .collect::<Vec<_>>();

        let mut child_session = self.create_forked_session(parent_session).await?;
        self.clone_turn_history_into_session(&child_session.id, stable_turns)
            .await?;
        child_session.title = Some(format!("Subagent: {name}"));
        child_session.updated_at = OffsetDateTime::now_utc();
        child_session.metadata.insert(
            "parent_session_id".to_string(),
            Value::String(parent_session.id.to_string()),
        );
        child_session.metadata.insert(
            "subagent_depth".to_string(),
            Value::from(
                parent_session
                    .metadata
                    .get("subagent_depth")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    + 1,
            ),
        );
        child_session.metadata.insert(
            "lineage_root_session_id".to_string(),
            parent_session
                .metadata
                .get("lineage_root_session_id")
                .cloned()
                .unwrap_or_else(|| Value::String(parent_session.id.to_string())),
        );
        child_session.metadata.insert(
            "spawned_by".to_string(),
            Value::String("subagent".to_string()),
        );
        child_session.metadata.insert(
            "parent_agent_name".to_string(),
            parent_session
                .metadata
                .get("agent_name")
                .cloned()
                .unwrap_or_else(|| Value::String("primary".to_string())),
        );
        child_session.metadata.insert(
            "forked_from_session_id".to_string(),
            Value::String(parent_session.id.to_string()),
        );
        child_session.metadata.insert(
            "subagent_parent_session_id".to_string(),
            Value::String(parent_session.id.to_string()),
        );
        child_session.metadata.insert(
            "subagent_lineage".to_string(),
            Value::String(format!("{}>{name}", parent_session.id)),
        );
        child_session
            .metadata
            .insert("agent_name".to_string(), Value::String(name.to_string()));
        child_session
            .metadata
            .insert("subagent_forked".to_string(), Value::Bool(true));
        child_session.metadata.insert(
            SUBAGENT_PARENT_CONTEXT_ITEMS_METADATA_KEY.to_string(),
            serde_json::to_value(parent_context_items).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
        if let Some(agent) = agent_spec {
            child_session.metadata.insert(
                "agent_instructions".to_string(),
                Value::String(agent.instructions.clone()),
            );
            if let Some(description) = agent.description.as_ref() {
                child_session.metadata.insert(
                    "agent_description".to_string(),
                    Value::String(description.clone()),
                );
            }
        }
        self.store.update_session(&child_session).await?;
        Ok(child_session)
    }
}
