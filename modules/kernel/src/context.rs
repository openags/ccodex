use ccodex_extensions::ExtensionRegistry;
use ccodex_protocol::{Item, Session, ToolSpec, Turn, TurnRequest};

use crate::{Kernel, KernelError};

const COMPACTION_SUMMARY_METADATA_KEY: &str = "compaction_summary";
const AGENT_INSTRUCTIONS_METADATA_KEY: &str = "agent_instructions";
const SUBAGENT_PARENT_CONTEXT_ITEMS_METADATA_KEY: &str = "subagent_parent_context_items";

impl Kernel {
    pub(crate) async fn build_turn_request(
        &self,
        session: &Session,
        turn: &Turn,
        prompt: &str,
    ) -> Result<TurnRequest, KernelError> {
        let mut project_instructions = session
            .workspace_root
            .as_ref()
            .map(|root| self.compat.load_workspace_instructions(root))
            .transpose()?
            .unwrap_or_default()
            .contents();

        if let Some(workspace_root) = session.workspace_root.as_ref() {
            let extension_roots = self.compat.extension_roots(workspace_root);
            project_instructions.extend(
                ExtensionRegistry::load_skill_instructions_for_roots(&extension_roots)?
                    .into_iter()
                    .map(|skill| skill.content),
            );
        }
        if let Some(summary) = session
            .metadata
            .get(COMPACTION_SUMMARY_METADATA_KEY)
            .and_then(serde_json::Value::as_str)
        {
            project_instructions.push(format!("Compacted session summary:\n{}", summary));
        }
        if let Some(agent_instructions) = session
            .metadata
            .get(AGENT_INSTRUCTIONS_METADATA_KEY)
            .and_then(serde_json::Value::as_str)
        {
            project_instructions.push(format!("Subagent instructions:\n{}", agent_instructions));
        }

        let mut items = self
            .store
            .list_turns(&session.id)
            .await?
            .into_iter()
            .flat_map(|stored| stored.items)
            .collect::<Vec<Item>>();

        if let Some(snapshot) = session
            .metadata
            .get(SUBAGENT_PARENT_CONTEXT_ITEMS_METADATA_KEY)
            .cloned()
        {
            let mut inherited_items =
                serde_json::from_value::<Vec<Item>>(snapshot).unwrap_or_default();
            items.append(&mut inherited_items);
        }

        Ok(TurnRequest {
            session: session.clone(),
            turn: turn.clone(),
            instructions: prompt.to_string(),
            project_instructions,
            items,
            available_tools: self.tool_specs.values().cloned().collect::<Vec<ToolSpec>>(),
        })
    }
}
