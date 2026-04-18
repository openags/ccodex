//! Canonical environment variable names.

/// Environment variable prefix for all ccodex-owned variables.
pub const ENV_PREFIX: &str = "CCODEX";

/// Provider API keys.
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
pub const ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const ANTHROPIC_AUTH_TOKEN_ENV: &str = "ANTHROPIC_AUTH_TOKEN";
pub const XAI_API_KEY_ENV: &str = "XAI_API_KEY";

/// Local runtime and mode hints.
pub const CODEX_HOME_ENV: &str = "CCODEX_HOME";
pub const LOG_LEVEL_ENV: &str = "CCODEX_LOG";
pub const LOCAL_SERVER_ADDR_ENV: &str = "CCODEX_LOCAL_SERVER_ADDR";
pub const PROVIDER_ENV: &str = "CCODEX_PROVIDER";
pub const BASE_URL_ENV: &str = "CCODEX_BASE_URL";
pub const MODEL_ENV: &str = "CCODEX_MODEL";
pub const APPROVAL_POLICY_ENV: &str = "CCODEX_APPROVAL_POLICY";
pub const APPROVAL_ALLOW_COMMANDS_ENV: &str = "CCODEX_APPROVAL_ALLOW_COMMANDS";
pub const APPROVAL_DENY_COMMANDS_ENV: &str = "CCODEX_APPROVAL_DENY_COMMANDS";
pub const APPROVAL_ALLOW_PATHS_ENV: &str = "CCODEX_APPROVAL_ALLOW_PATHS";
pub const APPROVAL_DENY_PATHS_ENV: &str = "CCODEX_APPROVAL_DENY_PATHS";
pub const APPROVAL_ALLOW_TOOLS_ENV: &str = "CCODEX_APPROVAL_ALLOW_TOOLS";
pub const APPROVAL_DENY_TOOLS_ENV: &str = "CCODEX_APPROVAL_DENY_TOOLS";
pub const SANDBOX_MODE_ENV: &str = "CCODEX_SANDBOX";
pub const OPENAI_BASE_URL_ENV: &str = "OPENAI_BASE_URL";
pub const OPENAI_MODEL_ENV: &str = "OPENAI_MODEL";
pub const ANTHROPIC_BASE_URL_ENV: &str = "ANTHROPIC_BASE_URL";
pub const ANTHROPIC_MODEL_ENV: &str = "ANTHROPIC_MODEL";
pub const XAI_BASE_URL_ENV: &str = "XAI_BASE_URL";
pub const XAI_MODEL_ENV: &str = "XAI_MODEL";
pub const LOCAL_BASE_URL_ENV: &str = "CCODEX_LOCAL_BASE_URL";
pub const LOCAL_MODEL_ENV: &str = "CCODEX_LOCAL_MODEL";
