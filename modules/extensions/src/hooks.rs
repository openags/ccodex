use std::fs;
use std::path::Path;

use serde::Deserialize;

use ccodex_protocol::{ExtensionKind, ExtensionManifest};

use crate::paths::{ExtensionRoots, hook_root_paths, ordered_root_bases, plugin_dir_name};
use crate::util::read_dir_if_exists;
use crate::{ExtensionError, ExtensionRegistry, HookDefinition, HookEvent};

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct RawHookConfig {
    event: Option<String>,
    command: Option<String>,
    timeout_ms: Option<u64>,
}

pub(crate) fn scan_hook_layers(
    registry: &mut ExtensionRegistry,
    roots: &ExtensionRoots,
) -> Result<(), ExtensionError> {
    for path in hook_root_paths(roots) {
        registry.scan_hook_dir(&path)?;
    }
    Ok(())
}

pub(crate) fn load_hook_layers(
    roots: &ExtensionRoots,
) -> Result<Vec<HookDefinition>, ExtensionError> {
    let mut items = Vec::new();
    for base in ordered_root_bases(roots) {
        merge_hook_definitions(&mut items, load_hook_definitions(&base.join("hooks"))?);
        merge_hook_definitions(
            &mut items,
            load_hook_definitions_from_plugin_dirs(&base.join(plugin_dir_name()), "hooks")?,
        );
    }
    Ok(items)
}

pub(crate) fn load_hook_definitions(dir: &Path) -> Result<Vec<HookDefinition>, ExtensionError> {
    let mut hooks = Vec::new();
    for entry in read_dir_if_exists(dir)? {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let raw = fs::read_to_string(&path).map_err(|err| ExtensionError::InspectSource {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        let parsed: RawHookConfig =
            toml::from_str(&raw).map_err(|err| ExtensionError::InspectSource {
                path: path.display().to_string(),
                message: err.to_string(),
            })?;
        let Some(command) = parsed.command else {
            continue;
        };
        let event = match parsed.event.as_deref().unwrap_or("post_turn") {
            "pre_turn" | "pre-turn" => HookEvent::PreTurn,
            "pre_tool" | "pre-tool" => HookEvent::PreTool,
            "post_tool" | "post-tool" => HookEvent::PostTool,
            _ => HookEvent::PostTurn,
        };

        hooks.push(HookDefinition {
            manifest: ExtensionManifest {
                name: path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                kind: ExtensionKind::Hook,
                version: None,
                source_path: path,
                description: Some(format!("{:?} hook", event)),
            },
            event,
            command,
            timeout_ms: parsed.timeout_ms.unwrap_or(5_000),
        });
    }

    Ok(hooks)
}

pub(crate) fn load_hook_definitions_from_plugin_dirs(
    plugin_root: &Path,
    subdir: &str,
) -> Result<Vec<HookDefinition>, ExtensionError> {
    let mut hooks = Vec::new();
    for entry in read_dir_if_exists(plugin_root)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        merge_hook_definitions(&mut hooks, load_hook_definitions(&path.join(subdir))?);
    }
    Ok(hooks)
}

fn merge_hook_definitions(target: &mut Vec<HookDefinition>, items: Vec<HookDefinition>) {
    for item in items {
        if let Some(existing) = target
            .iter_mut()
            .find(|hook| hook.manifest.name == item.manifest.name)
        {
            *existing = item;
        } else {
            target.push(item);
        }
    }
}
