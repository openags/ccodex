//! Path helpers derived from canonical naming.

use std::path::{Path, PathBuf};

use crate::naming::{
    CONFIG_FILE_NAME, EXPORTS_DIR_NAME, PROJECT_DIR_NAME, PROJECT_INSTRUCTIONS_FILE, STATE_DB_FILE_NAME,
    USER_DIR_NAME,
};

/// Returns the canonical user home directory for ccodex.
pub fn user_home_dir() -> PathBuf {
    let home = dirs::home_dir().expect("failed to determine home directory");
    home.join(USER_DIR_NAME)
}

/// Returns the canonical user config file.
pub fn user_config_file() -> PathBuf {
    user_home_dir().join(CONFIG_FILE_NAME)
}

/// Returns the canonical user state database file.
pub fn user_state_db_file() -> PathBuf {
    user_home_dir().join(STATE_DB_FILE_NAME)
}

/// Returns the project-local ccodex directory.
pub fn project_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(PROJECT_DIR_NAME)
}

/// Returns the project-local config file.
pub fn project_config_file(workspace_root: &Path) -> PathBuf {
    project_dir(workspace_root).join(CONFIG_FILE_NAME)
}

/// Returns the project instructions file.
pub fn project_instructions_file(workspace_root: &Path) -> PathBuf {
    workspace_root.join(PROJECT_INSTRUCTIONS_FILE)
}

/// Returns the project exports directory.
pub fn project_exports_dir(workspace_root: &Path) -> PathBuf {
    project_dir(workspace_root).join(EXPORTS_DIR_NAME)
}

/// Returns the project-local state database file.
pub fn project_state_db_file(workspace_root: &Path) -> PathBuf {
    project_dir(workspace_root).join(STATE_DB_FILE_NAME)
}
