use std::path::Path;

use ccodex_protocol::{ExtensionKind, ExtensionManifest};

use crate::plugins::{load_plugin_config, scan_markdown_dir};
use crate::util::read_dir_if_exists;
use crate::{ExtensionError, ExtensionRegistry};

impl ExtensionRegistry {
    pub(crate) fn scan_plugin_dir(&mut self, dir: &Path) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if path.is_dir() {
                let plugin_config = load_plugin_config(&path)?;
                self.push_manifest(ExtensionManifest {
                    name: plugin_config
                        .as_ref()
                        .and_then(|config| config.name.clone())
                        .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()),
                    kind: ExtensionKind::Plugin,
                    version: plugin_config
                        .as_ref()
                        .and_then(|config| config.version.clone()),
                    source_path: path,
                    description: plugin_config.and_then(|config| config.description),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn scan_plugin_bundle_dir(&mut self, dir: &Path) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            scan_markdown_dir(self, &path.join("skills"), ExtensionKind::Skill)?;
            scan_markdown_dir(self, &path.join("agents"), ExtensionKind::Agent)?;
            self.scan_hook_dir(&path.join("hooks"))?;
        }
        Ok(())
    }

    pub(crate) fn scan_hook_dir(&mut self, dir: &Path) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }

            self.push_manifest(ExtensionManifest {
                name: path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                kind: ExtensionKind::Hook,
                version: None,
                source_path: path,
                description: None,
            });
        }
        Ok(())
    }
}
