//! Compatibility adapters and instruction import helpers.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use ccodex_brand::PROJECT_INSTRUCTIONS_FILE;

const CLAUDE_INSTRUCTIONS_FILE: &str = "CLAUDE.md";

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
}

#[derive(Debug, Error)]
pub enum CompatError {
    #[error("failed to read instruction file {path}: {message}")]
    ReadInstruction { path: String, message: String },
}

#[derive(Debug, Default, Clone)]
pub struct CompatLayer;

impl CompatLayer {
    pub fn new() -> Self {
        Self
    }

    pub fn load_workspace_instructions(
        &self,
        workspace_root: &Path,
    ) -> Result<WorkspaceInstructions, CompatError> {
        let mut instructions = Vec::new();

        for path in [
            workspace_root.join(PROJECT_INSTRUCTIONS_FILE),
            workspace_root.join(CLAUDE_INSTRUCTIONS_FILE),
        ] {
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path).map_err(|err| CompatError::ReadInstruction {
                path: path.display().to_string(),
                message: err.to_string(),
            })?;

            instructions.push(ImportedInstruction { source: path, content });
        }

        Ok(WorkspaceInstructions { instructions })
    }
}

#[cfg(test)]
mod tests {
    use super::CompatLayer;

    #[test]
    fn loads_ccodex_and_claude_instruction_files_in_priority_order() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-compat-{unique}"));
        std::fs::create_dir_all(&root).expect("temp workspace should exist");
        std::fs::write(root.join("CCODEX.md"), "project-first").expect("ccodex file should write");
        std::fs::write(root.join("CLAUDE.md"), "claude-second").expect("claude file should write");

        let layer = CompatLayer::new();
        let loaded = layer
            .load_workspace_instructions(&root)
            .expect("instructions should load");

        assert_eq!(loaded.contents(), vec!["project-first".to_string(), "claude-second".to_string()]);

        let _ = std::fs::remove_dir_all(root);
    }
}
