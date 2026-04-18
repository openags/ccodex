use std::path::{Path, PathBuf};

use ccodex_protocol::PortError;

use crate::sandbox::{FileAccess, SandboxMode, SandboxPolicy};

#[derive(Debug, Clone)]
pub struct WorkspaceFs {
    sandbox: SandboxPolicy,
}

impl WorkspaceFs {
    pub fn new(workspace_root: PathBuf, sandbox_mode: SandboxMode) -> Self {
        Self {
            sandbox: SandboxPolicy::new(workspace_root, sandbox_mode),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        self.sandbox.workspace_root()
    }

    pub fn resolve_path(&self, path: &str, access: FileAccess) -> Result<PathBuf, PortError> {
        self.sandbox.resolve_path(path, access)
    }

    pub fn read_to_string(&self, path: &str) -> Result<(PathBuf, String), PortError> {
        let resolved = self.resolve_path(path, FileAccess::Read)?;
        let content = std::fs::read_to_string(&resolved).map_err(|err| {
            PortError::Tool(format!("failed to read {}: {err}", resolved.display()))
        })?;
        Ok((resolved, content))
    }

    pub fn write_string(&self, path: &str, content: &str) -> Result<PathBuf, PortError> {
        let resolved = self.resolve_path(path, FileAccess::Write)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                PortError::Tool(format!("failed to create {}: {err}", parent.display()))
            })?;
        }
        std::fs::write(&resolved, content).map_err(|err| {
            PortError::Tool(format!("failed to write {}: {err}", resolved.display()))
        })?;
        Ok(resolved)
    }

    pub fn replace_in_file(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
    ) -> Result<(PathBuf, usize), PortError> {
        if old_text.is_empty() {
            return Err(PortError::Tool(
                "edit_file old_text cannot be empty".to_string(),
            ));
        }

        let (resolved, content) = self.read_to_string(path)?;
        let replacements = content.matches(old_text).count();
        if replacements == 0 {
            return Err(PortError::Tool(format!(
                "edit_file could not find target text in {}",
                resolved.display()
            )));
        }

        let updated = content.replace(old_text, new_text);
        let path_display = resolved.display().to_string();
        self.write_string(&path_display, &updated)?;
        Ok((resolved, replacements))
    }

    pub fn walk_files(
        &self,
        visit: &mut dyn FnMut(&PathBuf) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        walk_directory(self.workspace_root(), visit)
    }
}

fn walk_directory(
    root: &Path,
    visit: &mut dyn FnMut(&PathBuf) -> Result<(), PortError>,
) -> Result<(), PortError> {
    if root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "target" || name == ".git" || name == "node_modules")
        .unwrap_or(false)
    {
        return Ok(());
    }

    let entries = std::fs::read_dir(root)
        .map_err(|err| PortError::Tool(format!("failed to read {}: {err}", root.display())))?;

    for entry in entries {
        let entry = entry
            .map_err(|err| PortError::Tool(format!("failed to walk {}: {err}", root.display())))?;
        let path = entry.path();
        if path.is_dir() {
            walk_directory(&path, visit)?;
        } else if path.is_file() {
            visit(&path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceFs;
    use crate::sandbox::SandboxMode;

    #[test]
    fn write_and_read_roundtrip_inside_workspace() {
        let root = std::env::temp_dir().join("ccodex-fs-roundtrip");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("workspace dir should exist");

        let fs = WorkspaceFs::new(root.clone(), SandboxMode::WorkspaceWrite);
        fs.write_string("notes/hello.txt", "hi")
            .expect("write should succeed");
        let (path, content) = fs
            .read_to_string("notes/hello.txt")
            .expect("read should succeed");

        assert_eq!(content, "hi");
        assert!(path.starts_with(&root));

        let _ = std::fs::remove_dir_all(&root);
    }
}
