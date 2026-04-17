//! Canonical environment variable names.

/// Environment variable prefix for all ccodex-owned variables.
pub const ENV_PREFIX: &str = "CCODEX";

/// Provider API keys.
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
pub const ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const XAI_API_KEY_ENV: &str = "XAI_API_KEY";

/// Local runtime and mode hints.
pub const CODEX_HOME_ENV: &str = "CCODEX_HOME";
pub const LOG_LEVEL_ENV: &str = "CCODEX_LOG";
pub const LOCAL_SERVER_ADDR_ENV: &str = "CCODEX_LOCAL_SERVER_ADDR";
