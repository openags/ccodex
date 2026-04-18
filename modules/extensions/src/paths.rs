use std::path::{Path, PathBuf};

use ccodex_brand::{
    AGENTS_DIR_NAME, HOOKS_DIR_NAME, PLUGINS_DIR_NAME, SKILLS_DIR_NAME, project_dir, user_home_dir,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionRoots {
    ordered_bases: Vec<PathBuf>,
}

impl ExtensionRoots {
    pub fn new(ordered_bases: Vec<PathBuf>) -> Self {
        Self { ordered_bases }
    }

    pub fn for_ccodex_workspace(workspace_root: &Path) -> Self {
        Self::new(vec![
            workspace_root.join("plugins").join("builtin"),
            project_dir(workspace_root),
            user_home_dir(),
        ])
    }

    pub fn ordered_bases(&self) -> &[PathBuf] {
        &self.ordered_bases
    }
}

pub(crate) fn ordered_root_bases(roots: &ExtensionRoots) -> impl Iterator<Item = &Path> {
    roots.ordered_bases().iter().map(PathBuf::as_path)
}

pub(crate) fn extension_root_paths(roots: &ExtensionRoots, dir_name: &str) -> Vec<PathBuf> {
    roots
        .ordered_bases()
        .iter()
        .map(|base| base.join(dir_name))
        .collect()
}

pub(crate) fn plugin_root_paths(roots: &ExtensionRoots) -> Vec<PathBuf> {
    extension_root_paths(roots, PLUGINS_DIR_NAME)
}

pub(crate) fn hook_root_paths(roots: &ExtensionRoots) -> Vec<PathBuf> {
    extension_root_paths(roots, HOOKS_DIR_NAME)
}

pub(crate) fn skill_dir_name() -> &'static str {
    SKILLS_DIR_NAME
}

pub(crate) fn agent_dir_name() -> &'static str {
    AGENTS_DIR_NAME
}

pub(crate) fn plugin_dir_name() -> &'static str {
    PLUGINS_DIR_NAME
}
