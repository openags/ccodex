use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use ccodex_brand::{DEFAULT_MODEL, project_config_file, user_config_file};

use crate::config_env::{
    apply_env_approval, apply_env_approval_rules, apply_env_provider, apply_env_sandbox,
};
use crate::config_parse::{
    extend_rule_list, infer_provider_kind, parse_approval_policy, parse_provider_kind,
};
use crate::sandbox::SandboxMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalPolicy {
    AlwaysApprove,
    Ask,
    NeverApprove,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovalRules {
    pub allow_tools: Vec<String>,
    pub deny_tools: Vec<String>,
    pub allow_commands: Vec<String>,
    pub deny_commands: Vec<String>,
    pub allow_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl ApprovalRules {
    pub fn is_empty(&self) -> bool {
        self.allow_tools.is_empty()
            && self.deny_tools.is_empty()
            && self.allow_commands.is_empty()
            && self.deny_commands.is_empty()
            && self.allow_paths.is_empty()
            && self.deny_paths.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Bootstrap,
    Echo,
    OpenAiCompatible,
    AnthropicCompatible,
    XaiCompatible,
    LocalCompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub workspace_root: PathBuf,
    pub approval_policy: ApprovalPolicy,
    pub approval_rules: ApprovalRules,
    pub sandbox_mode: SandboxMode,
    pub provider: ProviderConfig,
}

#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error("failed to read config file {path}: {message}")]
    ReadConfig { path: String, message: String },
    #[error("failed to parse config file {path}: {message}")]
    ParseConfig { path: String, message: String },
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawRuntimeConfig {
    provider: Option<RawProviderConfigValue>,
    approval: Option<RawApprovalConfigValue>,
    sandbox: Option<RawSandboxConfigValue>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawProviderConfig {
    kind: Option<String>,
    base_url: Option<String>,
    api_key_env: Option<String>,
    model: Option<String>,
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawApprovalConfig {
    policy: Option<String>,
    allow_tools: Option<Vec<String>>,
    deny_tools: Option<Vec<String>>,
    allow_commands: Option<Vec<String>>,
    deny_commands: Option<Vec<String>>,
    allow_paths: Option<Vec<String>>,
    deny_paths: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawSandboxConfig {
    mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawProviderConfigValue {
    Name(String),
    Table(RawProviderConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawApprovalConfigValue {
    Name(String),
    Table(RawApprovalConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawSandboxConfigValue {
    Name(String),
    Table(RawSandboxConfig),
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::load_for_workspace(workspace_root)
            .unwrap_or_else(|_| Self::defaults(PathBuf::from(".")))
    }

    pub fn load_for_workspace(workspace_root: PathBuf) -> Result<Self, RuntimeConfigError> {
        let user_raw = load_raw_config(&user_config_file())?;
        let project_raw = load_raw_config(&project_config_file(&workspace_root))?;

        let mut provider = ProviderConfig {
            kind: ProviderKind::Bootstrap,
            base_url: None,
            api_key: None,
            model: DEFAULT_MODEL.to_string(),
            max_output_tokens: 16_000,
        };
        apply_raw_provider(&mut provider, user_raw.provider.as_ref());
        apply_raw_provider(&mut provider, project_raw.provider.as_ref());
        apply_env_provider(&mut provider);

        // Security: Default to Ask rather than AlwaysApprove
        // See Codex adversarial review finding [high] dangerous tool approvals
        let mut approval_policy = ApprovalPolicy::Ask;
        let mut approval_rules = ApprovalRules::default();
        apply_raw_approval(&mut approval_policy, user_raw.approval.as_ref());
        apply_raw_approval_rules(&mut approval_rules, user_raw.approval.as_ref());
        apply_raw_approval(&mut approval_policy, project_raw.approval.as_ref());
        apply_raw_approval_rules(&mut approval_rules, project_raw.approval.as_ref());
        apply_env_approval(&mut approval_policy);
        apply_env_approval_rules(&mut approval_rules);

        let mut sandbox_mode = SandboxMode::WorkspaceWrite;
        apply_raw_sandbox(&mut sandbox_mode, user_raw.sandbox.as_ref());
        apply_raw_sandbox(&mut sandbox_mode, project_raw.sandbox.as_ref());
        apply_env_sandbox(&mut sandbox_mode);

        Ok(Self {
            workspace_root,
            approval_policy,
            approval_rules,
            sandbox_mode,
            provider,
        })
    }

    fn defaults(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            // Security: Default to Ask rather than AlwaysApprove
            approval_policy: ApprovalPolicy::Ask,
            approval_rules: ApprovalRules::default(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            provider: ProviderConfig {
                kind: infer_provider_kind(),
                base_url: None,
                api_key: None,
                model: DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
        }
    }
}

fn load_raw_config(path: &Path) -> Result<RawRuntimeConfig, RuntimeConfigError> {
    if !path.exists() {
        return Ok(RawRuntimeConfig::default());
    }

    let content = fs::read_to_string(path).map_err(|err| RuntimeConfigError::ReadConfig {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    toml::from_str(&content).map_err(|err| RuntimeConfigError::ParseConfig {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn apply_raw_provider(target: &mut ProviderConfig, raw: Option<&RawProviderConfigValue>) {
    let Some(raw) = raw else {
        return;
    };

    let table = match raw {
        RawProviderConfigValue::Name(name) => {
            if let Some(kind) = parse_provider_kind(name) {
                target.kind = kind;
            }
            return;
        }
        RawProviderConfigValue::Table(table) => table,
    };

    if let Some(kind) = table.kind.as_deref().and_then(parse_provider_kind) {
        target.kind = kind;
    }
    if let Some(base_url) = table.base_url.as_ref() {
        target.base_url = Some(base_url.clone());
    }
    if let Some(model) = table.model.as_ref() {
        target.model = model.clone();
    }
    if let Some(max_output_tokens) = table.max_output_tokens {
        target.max_output_tokens = max_output_tokens;
    }
    if let Some(env_name) = table.api_key_env.as_deref() {
        target.api_key = std::env::var(env_name).ok();
    }
}

fn apply_raw_approval(target: &mut ApprovalPolicy, raw: Option<&RawApprovalConfigValue>) {
    let Some(raw) = raw else {
        return;
    };

    let parsed = match raw {
        RawApprovalConfigValue::Name(name) => parse_approval_policy(name),
        RawApprovalConfigValue::Table(table) => {
            table.policy.as_deref().and_then(parse_approval_policy)
        }
    };

    if let Some(policy) = parsed {
        *target = policy;
    }
}

fn apply_raw_approval_rules(target: &mut ApprovalRules, raw: Option<&RawApprovalConfigValue>) {
    let Some(RawApprovalConfigValue::Table(table)) = raw else {
        return;
    };

    extend_rule_list(&mut target.allow_tools, table.allow_tools.as_ref());
    extend_rule_list(&mut target.deny_tools, table.deny_tools.as_ref());
    extend_rule_list(&mut target.allow_commands, table.allow_commands.as_ref());
    extend_rule_list(&mut target.deny_commands, table.deny_commands.as_ref());
    extend_rule_list(&mut target.allow_paths, table.allow_paths.as_ref());
    extend_rule_list(&mut target.deny_paths, table.deny_paths.as_ref());
}

fn apply_raw_sandbox(target: &mut SandboxMode, raw: Option<&RawSandboxConfigValue>) {
    let Some(raw) = raw else {
        return;
    };

    let parsed = match raw {
        RawSandboxConfigValue::Name(name) => SandboxMode::parse(name),
        RawSandboxConfigValue::Table(table) => table.mode.as_deref().and_then(SandboxMode::parse),
    };

    if let Some(mode) = parsed {
        *target = mode;
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::load_for_workspace(workspace_root.clone())
            .unwrap_or_else(|_| Self::defaults(workspace_root))
    }
}

#[cfg(test)]
mod tests {
    use ccodex_brand::{
        APPROVAL_ALLOW_PATHS_ENV, APPROVAL_ALLOW_TOOLS_ENV, APPROVAL_DENY_COMMANDS_ENV,
    };

    use crate::sandbox::SandboxMode;

    use super::{ApprovalPolicy, ProviderKind, RuntimeConfig};

    #[test]
    fn loads_project_config_values() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-runtime-config-{unique}"));
        let config_dir = root.join(".ccodex");
        std::fs::create_dir_all(&config_dir).expect("config dir should exist");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[provider]
kind = "openai"
base_url = "https://example.invalid/v2/coding"
model = "deepseek-v3.2"
max_output_tokens = 2048

[approval]
policy = "ask"
allow_commands = ["git status"]
deny_paths = [".env", "secrets/"]

[sandbox]
mode = "read-only"
"#,
        )
        .expect("config file should write");

        let config = RuntimeConfig::load_for_workspace(root.clone()).expect("config should load");
        assert_eq!(config.provider.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(
            config.provider.base_url.as_deref(),
            Some("https://example.invalid/v2/coding")
        );
        assert_eq!(config.provider.model, "deepseek-v3.2");
        assert_eq!(config.provider.max_output_tokens, 2048);
        assert_eq!(config.approval_policy, ApprovalPolicy::Ask);
        assert_eq!(config.approval_rules.allow_commands, vec!["git status"]);
        assert_eq!(config.approval_rules.deny_paths, vec![".env", "secrets/"]);
        assert_eq!(config.sandbox_mode, SandboxMode::ReadOnly);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_xai_and_local_provider_kinds_from_project_config() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let xai_root = std::env::temp_dir().join(format!("ccodex-runtime-config-xai-{unique}"));
        let xai_dir = xai_root.join(".ccodex");
        std::fs::create_dir_all(&xai_dir).expect("config dir should exist");
        std::fs::write(
            xai_dir.join("config.toml"),
            r#"
[provider]
kind = "xai"
model = "grok-code-fast"
"#,
        )
        .expect("xai config should write");
        let xai = RuntimeConfig::load_for_workspace(xai_root.clone()).expect("config should load");
        assert_eq!(xai.provider.kind, ProviderKind::XaiCompatible);
        let _ = std::fs::remove_dir_all(xai_root);

        let local_root = std::env::temp_dir().join(format!("ccodex-runtime-config-local-{unique}"));
        let local_dir = local_root.join(".ccodex");
        std::fs::create_dir_all(&local_dir).expect("config dir should exist");
        std::fs::write(
            local_dir.join("config.toml"),
            r#"
[provider]
kind = "local"
base_url = "http://127.0.0.1:11434/v1"
"#,
        )
        .expect("local config should write");
        let local =
            RuntimeConfig::load_for_workspace(local_root.clone()).expect("config should load");
        assert_eq!(local.provider.kind, ProviderKind::LocalCompatible);
        assert_eq!(
            local.provider.base_url.as_deref(),
            Some("http://127.0.0.1:11434/v1")
        );
        let _ = std::fs::remove_dir_all(local_root);
    }

    #[test]
    fn loads_approval_rules_from_environment() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-runtime-rules-{unique}"));
        std::fs::create_dir_all(root.join(".ccodex")).expect("config dir should exist");

        unsafe {
            std::env::set_var(APPROVAL_ALLOW_TOOLS_ENV, "grep,glob");
            std::env::set_var(APPROVAL_DENY_COMMANDS_ENV, "rm -rf,curl ");
            std::env::set_var(APPROVAL_ALLOW_PATHS_ENV, "README.md,docs/");
        }

        let config = RuntimeConfig::load_for_workspace(root.clone()).expect("config should load");
        assert_eq!(config.approval_rules.allow_tools, vec!["grep", "glob"]);
        assert_eq!(config.approval_rules.deny_commands, vec!["rm -rf", "curl"]);
        assert_eq!(
            config.approval_rules.allow_paths,
            vec!["README.md", "docs/"]
        );

        unsafe {
            std::env::remove_var(APPROVAL_ALLOW_TOOLS_ENV);
            std::env::remove_var(APPROVAL_DENY_COMMANDS_ENV);
            std::env::remove_var(APPROVAL_ALLOW_PATHS_ENV);
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
