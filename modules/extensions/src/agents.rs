use std::collections::BTreeMap;
use std::path::Path;

use ccodex_protocol::{AgentId, AgentSpec, ExtensionKind};

use crate::ExtensionError;
use crate::markdown::load_markdown_extensions;
use crate::paths::{ExtensionRoots, agent_dir_name, ordered_root_bases, plugin_dir_name};

pub(crate) fn load_agent_layers(roots: &ExtensionRoots) -> Result<Vec<AgentSpec>, ExtensionError> {
    let mut items = Vec::new();
    for base in ordered_root_bases(roots) {
        merge_agent_specs(&mut items, load_agent_specs(&base.join(agent_dir_name()))?);
        merge_agent_specs(
            &mut items,
            load_agent_specs_from_plugin_dirs(&base.join(plugin_dir_name()), agent_dir_name())?,
        );
    }
    Ok(items)
}

pub(crate) fn load_agent_specs(dir: &Path) -> Result<Vec<AgentSpec>, ExtensionError> {
    let mut agents = Vec::new();
    for markdown in load_markdown_extensions(dir, ExtensionKind::Agent)? {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "source_path".to_string(),
            serde_json::Value::String(markdown.manifest.source_path.display().to_string()),
        );
        agents.push(AgentSpec {
            id: AgentId::from(markdown.manifest.name.clone()),
            name: markdown.manifest.name.clone(),
            description: markdown.manifest.description.clone(),
            instructions: markdown.content,
            metadata,
        });
    }
    Ok(agents)
}

pub(crate) fn load_agent_specs_from_plugin_dirs(
    plugin_root: &Path,
    subdir: &str,
) -> Result<Vec<AgentSpec>, ExtensionError> {
    let mut agents = Vec::new();
    for entry in crate::util::read_dir_if_exists(plugin_root)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        merge_agent_specs(&mut agents, load_agent_specs(&path.join(subdir))?);
    }
    Ok(agents)
}

fn merge_agent_specs(target: &mut Vec<AgentSpec>, items: Vec<AgentSpec>) {
    for item in items {
        if let Some(existing) = target.iter_mut().find(|agent| agent.name == item.name) {
            *existing = item;
        } else {
            target.push(item);
        }
    }
}
