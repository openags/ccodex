//! Extension discovery and lightweight manifest loading for ccodex.

use std::fs;
use std::path::Path;

use thiserror::Error;

use ccodex_brand::{
    project_dir, user_home_dir, AGENTS_DIR_NAME, HOOKS_DIR_NAME, PLUGINS_DIR_NAME, SKILLS_DIR_NAME,
};
use ccodex_protocol::{ExtensionKind, ExtensionManifest};

#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("failed to read directory {path}: {message}")]
    ReadDirectory { path: String, message: String },
    #[error("failed to inspect extension source {path}: {message}")]
    InspectSource { path: String, message: String },
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    manifests: Vec<ExtensionManifest>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            manifests: Vec::new(),
        }
    }

    pub fn discover_for_workspace(workspace_root: &Path) -> Result<Self, ExtensionError> {
        let mut registry = Self::new();

        let builtin_root = workspace_root.join("plugins").join("builtin");
        let project_root = project_dir(workspace_root);
        let user_root = user_home_dir();

        registry.scan_plugin_dir(&builtin_root.join(PLUGINS_DIR_NAME))?;
        registry.scan_markdown_dir(&builtin_root.join(SKILLS_DIR_NAME), ExtensionKind::Skill)?;
        registry.scan_markdown_dir(&builtin_root.join(AGENTS_DIR_NAME), ExtensionKind::Agent)?;
        registry.scan_hook_dir(&builtin_root.join(HOOKS_DIR_NAME))?;

        registry.scan_plugin_dir(&project_root.join(PLUGINS_DIR_NAME))?;
        registry.scan_markdown_dir(&project_root.join(SKILLS_DIR_NAME), ExtensionKind::Skill)?;
        registry.scan_markdown_dir(&project_root.join(AGENTS_DIR_NAME), ExtensionKind::Agent)?;
        registry.scan_hook_dir(&project_root.join(HOOKS_DIR_NAME))?;

        registry.scan_plugin_dir(&user_root.join(PLUGINS_DIR_NAME))?;
        registry.scan_markdown_dir(&user_root.join(SKILLS_DIR_NAME), ExtensionKind::Skill)?;
        registry.scan_markdown_dir(&user_root.join(AGENTS_DIR_NAME), ExtensionKind::Agent)?;
        registry.scan_hook_dir(&user_root.join(HOOKS_DIR_NAME))?;

        registry
            .manifests
            .sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));

        Ok(registry)
    }

    pub fn manifests(&self) -> &[ExtensionManifest] {
        &self.manifests
    }

    fn scan_plugin_dir(&mut self, dir: &Path) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if path.is_dir() {
                self.manifests.push(ExtensionManifest {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    kind: ExtensionKind::Plugin,
                    version: None,
                    source_path: path,
                    description: None,
                });
            }
        }
        Ok(())
    }

    fn scan_markdown_dir(&mut self, dir: &Path, kind: ExtensionKind) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }

            let description = first_heading_or_line(&path)?;
            self.manifests.push(ExtensionManifest {
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

    fn scan_hook_dir(&mut self, dir: &Path) -> Result<(), ExtensionError> {
        for entry in read_dir_if_exists(dir)? {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }

            self.manifests.push(ExtensionManifest {
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

fn read_dir_if_exists(dir: &Path) -> Result<Vec<fs::DirEntry>, ExtensionError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| ExtensionError::ReadDirectory {
        path: dir.display().to_string(),
        message: err.to_string(),
    })? {
        entries.push(entry.map_err(|err| ExtensionError::ReadDirectory {
            path: dir.display().to_string(),
            message: err.to_string(),
        })?);
    }
    Ok(entries)
}

fn first_heading_or_line(path: &Path) -> Result<Option<String>, ExtensionError> {
    let content = fs::read_to_string(path).map_err(|err| ExtensionError::InspectSource {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            return Ok(Some(heading.trim().to_string()));
        }
        return Ok(Some(trimmed.to_string()));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::ExtensionRegistry;

    #[test]
    fn discovers_project_extensions() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-extensions-{unique}"));
        let project_dir = root.join(".ccodex");

        std::fs::create_dir_all(project_dir.join("plugins").join("demo-plugin"))
            .expect("plugin dir should exist");
        std::fs::create_dir_all(project_dir.join("skills")).expect("skills dir should exist");
        std::fs::create_dir_all(project_dir.join("agents")).expect("agents dir should exist");
        std::fs::create_dir_all(project_dir.join("hooks")).expect("hooks dir should exist");

        std::fs::write(project_dir.join("skills").join("review.md"), "# Review Skill")
            .expect("skill file should write");
        std::fs::write(project_dir.join("agents").join("builder.md"), "# Builder Agent")
            .expect("agent file should write");
        std::fs::write(project_dir.join("hooks").join("format.toml"), "command = \"echo hi\"")
            .expect("hook file should write");

        let registry = ExtensionRegistry::discover_for_workspace(&root).expect("registry should load");
        let names = registry
            .manifests()
            .iter()
            .map(|manifest| format!("{:?}:{}", manifest.kind, manifest.name))
            .collect::<Vec<_>>();

        assert!(names.iter().any(|item| item == "Plugin:demo-plugin"));
        assert!(names.iter().any(|item| item == "Skill:review"));
        assert!(names.iter().any(|item| item == "Agent:builder"));
        assert!(names.iter().any(|item| item == "Hook:format"));

        let _ = std::fs::remove_dir_all(root);
    }
}
