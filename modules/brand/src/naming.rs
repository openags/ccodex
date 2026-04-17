//! Naming constants used across the product.

/// Canonical binary name.
pub const BINARY_NAME: &str = "ccodex";

/// Display name shown in UI.
pub const DISPLAY_NAME: &str = "CCODEX";

/// User-facing product name.
pub const PRODUCT_NAME: &str = "CCODEX";

/// Version string set by Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Canonical project instructions file.
pub const PROJECT_INSTRUCTIONS_FILE: &str = "CCODEX.md";

/// Project-local configuration directory.
pub const PROJECT_DIR_NAME: &str = ".ccodex";

/// User home data directory.
pub const USER_DIR_NAME: &str = ".ccodex";

/// Canonical config file name.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Canonical state database file name.
pub const STATE_DB_FILE_NAME: &str = "state.sqlite3";

/// Directory names for extension assets.
pub const PLUGINS_DIR_NAME: &str = "plugins";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const AGENTS_DIR_NAME: &str = "agents";
pub const HOOKS_DIR_NAME: &str = "hooks";
pub const EXPORTS_DIR_NAME: &str = "exports";

/// Default model fallback for the very first run.
pub const DEFAULT_MODEL: &str = "gpt-5.4";
