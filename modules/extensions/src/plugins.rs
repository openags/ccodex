use std::path::Path;

use serde::Deserialize;

use ccodex_protocol::{ExtensionKind, ExtensionManifest};

use crate::markdown::{first_heading_or_line, scan_markdown_layers};
use crate::paths::{ExtensionRoots, agent_dir_name, plugin_root_paths, skill_dir_name};
use crate::util::read_dir_if_exists;
use crate::{ExtensionError, ExtensionRegistry};

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct RawPluginConfig {
    pub(crate) name: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) description: Option<String>,
}

pub(crate) fn scan_plugin_layers(
    registry: &mut ExtensionRegistry,
    roots: &ExtensionRoots,
) -> Result<(), ExtensionError> {
    for path in plugin_root_paths(roots) {
        registry.scan_plugin_dir(&path)?;
        registry.scan_plugin_bundle_dir(&path)?;
    }
    Ok(())
}

pub(crate) fn scan_skill_layers(
    registry: &mut ExtensionRegistry,
    roots: &ExtensionRoots,
) -> Result<(), ExtensionError> {
    scan_markdown_layers(registry, roots, skill_dir_name(), ExtensionKind::Skill)
}

pub(crate) fn scan_agent_layers(
    registry: &mut ExtensionRegistry,
    roots: &ExtensionRoots,
) -> Result<(), ExtensionError> {
    scan_markdown_layers(registry, roots, agent_dir_name(), ExtensionKind::Agent)
}

pub(crate) fn load_plugin_config(dir: &Path) -> Result<Option<RawPluginConfig>, ExtensionError> {
    let path = dir.join("plugin.toml");
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path).map_err(|err| ExtensionError::InspectSource {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    let parsed: RawPluginConfig =
        toml::from_str(&raw).map_err(|err| ExtensionError::InspectSource {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
    Ok(Some(parsed))
}

pub(crate) fn scan_markdown_dir(
    registry: &mut ExtensionRegistry,
    dir: &Path,
    kind: ExtensionKind,
) -> Result<(), ExtensionError> {
    for entry in read_dir_if_exists(dir)? {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let description = first_heading_or_line(&path)?;
        registry.push_manifest(ExtensionManifest {
            name: path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string(),
            kind: kind.clone(),
            version: None,
            source_path: path,
            description,
        });
    }
    Ok(())
}
