//! Compatibility adapters and instruction import helpers.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use ccodex_brand::{PROJECT_INSTRUCTIONS_FILE, project_dir, user_home_dir};
use ccodex_extensions::ExtensionRoots;

mod claude;
mod codex;
mod hermes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedInstruction {
    pub source: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceInstructions {
    pub instructions: Vec<ImportedInstruction>,
}

impl WorkspaceInstructions {
    pub fn contents(&self) -> Vec<String> {
        self.instructions
            .iter()
            .map(|item| item.content.clone())
            .collect()
    }

    pub fn sources(&self) -> Vec<PathBuf> {
        self.instructions
            .iter()
            .map(|item| item.source.clone())
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum CompatError {
    #[error("failed to read instruction file {path}: {message}")]
    ReadInstruction { path: String, message: String },
}

/// Workspace trust level for security-sensitive operations like hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceTrust {
    /// Workspace is not trusted. Only load hooks from user-level roots.
    #[default]
    Untrusted,
    /// Workspace is explicitly trusted. Load hooks from all roots including workspace-local.
    Trusted,
}

#[derive(Debug, Default, Clone)]
pub struct CompatLayer {
    trust: WorkspaceTrust,
}

impl CompatLayer {
    pub fn new() -> Self {
        Self {
            trust: WorkspaceTrust::default(),
        }
    }

    /// Create a CompatLayer with explicit workspace trust level.
    pub fn with_trust(trust: WorkspaceTrust) -> Self {
        Self { trust }
    }

    /// Get the current trust level.
    pub fn trust(&self) -> WorkspaceTrust {
        self.trust
    }

    /// Set the trust level.
    pub fn set_trust(&mut self, trust: WorkspaceTrust) {
        self.trust = trust;
    }

    pub fn load_workspace_instructions(
        &self,
        workspace_root: &Path,
    ) -> Result<WorkspaceInstructions, CompatError> {
        let mut instructions = Vec::new();

        load_instruction_file(
            &workspace_root.join(PROJECT_INSTRUCTIONS_FILE),
            &mut instructions,
        )?;
        claude::collect_workspace_instructions(workspace_root, &mut instructions)?;
        hermes::collect_workspace_instructions(workspace_root, &mut instructions)?;
        codex::collect_workspace_instructions(workspace_root, &mut instructions)?;

        Ok(WorkspaceInstructions { instructions })
    }

    /// Get all extension roots for instruction/skill discovery.
    /// This includes both trusted and untrusted roots.
    pub fn extension_roots(&self, workspace_root: &Path) -> ExtensionRoots {
        ExtensionRoots::new(vec![
            workspace_root.join("plugins").join("builtin"),
            project_dir(workspace_root),
            workspace_root.join(".claude"),
            workspace_root.join(".codex"),
            workspace_root.join(".hermes"),
            user_home_dir(),
        ])
    }

    /// Get trusted extension roots only.
    /// Use this for security-sensitive operations like hooks.
    /// Returns only user-level and builtin roots, not workspace-local.
    pub fn trusted_extension_roots(&self, workspace_root: &Path) -> ExtensionRoots {
        // Security: Only return trusted roots for hook execution
        // User home directory and builtin plugins are considered trusted
        ExtensionRoots::new(vec![
            workspace_root.join("plugins").join("builtin"),
            user_home_dir(),
        ])
    }

    /// Get hook extension roots based on trust level.
    /// Untrusted workspaces only get trusted roots.
    /// Trusted workspaces get all roots including workspace-local.
    pub fn hook_roots(&self, workspace_root: &Path) -> ExtensionRoots {
        match self.trust {
            WorkspaceTrust::Untrusted => self.trusted_extension_roots(workspace_root),
            WorkspaceTrust::Trusted => self.extension_roots(workspace_root),
        }
    }
}

pub(crate) fn load_instruction_file(
    path: &Path,
    target: &mut Vec<ImportedInstruction>,
) -> Result<(), CompatError> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|err| CompatError::ReadInstruction {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    target.push(ImportedInstruction {
        source: path.to_path_buf(),
        content,
    });
    Ok(())
}

pub(crate) fn load_markdown_dir(
    dir: &Path,
    target: &mut Vec<ImportedInstruction>,
) -> Result<(), CompatError> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .map_err(|err| CompatError::ReadInstruction {
            path: dir.display().to_string(),
            message: err.to_string(),
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        load_instruction_file(&path, target)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CompatLayer;

    #[test]
    fn loads_ccodex_claude_and_agents_instruction_files_in_priority_order() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-compat-{unique}"));
        std::fs::create_dir_all(&root).expect("temp workspace should exist");
        std::fs::write(root.join("CCODEX.md"), "project-first").expect("ccodex file should write");
        std::fs::write(root.join("CLAUDE.md"), "claude-second").expect("claude file should write");
        std::fs::write(root.join("AGENTS.md"), "agents-third").expect("agents file should write");

        let layer = CompatLayer::new();
        let loaded = layer
            .load_workspace_instructions(&root)
            .expect("instructions should load");

        assert_eq!(
            loaded.contents(),
            vec![
                "project-first".to_string(),
                "claude-second".to_string(),
                "agents-third".to_string()
            ]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_nested_claude_codex_and_hermes_assets() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-compat-nested-{unique}"));
        std::fs::create_dir_all(root.join(".claude").join("instructions"))
            .expect("claude instructions dir should exist");
        std::fs::create_dir_all(root.join(".codex").join("instructions"))
            .expect("codex instructions dir should exist");
        std::fs::create_dir_all(root.join(".hermes")).expect("hermes dir should exist");

        std::fs::write(root.join(".claude").join("CLAUDE.md"), "claude-dir")
            .expect("claude nested file should write");
        std::fs::write(
            root.join(".claude")
                .join("instructions")
                .join("01-context.md"),
            "claude-context",
        )
        .expect("claude instruction should write");
        std::fs::write(root.join(".codex").join("CODEX.md"), "codex-dir")
            .expect("codex nested file should write");
        std::fs::write(
            root.join(".codex")
                .join("instructions")
                .join("02-workflow.md"),
            "codex-workflow",
        )
        .expect("codex instruction should write");
        std::fs::write(root.join(".hermes").join("AGENTS.md"), "hermes-dir")
            .expect("hermes nested file should write");

        let loaded = CompatLayer::new()
            .load_workspace_instructions(&root)
            .expect("instructions should load");

        assert_eq!(
            loaded.contents(),
            vec![
                "claude-dir".to_string(),
                "claude-context".to_string(),
                "hermes-dir".to_string(),
                "codex-dir".to_string(),
                "codex-workflow".to_string(),
            ]
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
