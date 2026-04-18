use ccodex_brand::{
    ANTHROPIC_API_KEY_ENV, ANTHROPIC_AUTH_TOKEN_ENV, ANTHROPIC_BASE_URL_ENV, ANTHROPIC_MODEL_ENV,
    APPROVAL_ALLOW_COMMANDS_ENV, APPROVAL_ALLOW_PATHS_ENV, APPROVAL_ALLOW_TOOLS_ENV,
    APPROVAL_DENY_COMMANDS_ENV, APPROVAL_DENY_PATHS_ENV, APPROVAL_DENY_TOOLS_ENV,
    APPROVAL_POLICY_ENV, BASE_URL_ENV, DEFAULT_MODEL, LOCAL_BASE_URL_ENV, LOCAL_MODEL_ENV,
    MODEL_ENV, OPENAI_API_KEY_ENV, OPENAI_BASE_URL_ENV, OPENAI_MODEL_ENV, PROVIDER_ENV,
    SANDBOX_MODE_ENV, XAI_API_KEY_ENV, XAI_BASE_URL_ENV, XAI_MODEL_ENV,
};

use crate::config::{ApprovalPolicy, ApprovalRules, ProviderConfig, ProviderKind};
use crate::config_parse::{
    extend_rule_list_csv, infer_provider_kind, parse_approval_policy, parse_provider_kind,
};
use crate::sandbox::SandboxMode;

pub(crate) fn apply_env_provider(target: &mut ProviderConfig) {
    if let Some(kind) = std::env::var(PROVIDER_ENV)
        .ok()
        .as_deref()
        .and_then(parse_provider_kind)
    {
        target.kind = kind;
    } else if matches!(target.kind, ProviderKind::Bootstrap) {
        target.kind = infer_provider_kind();
    }

    match target.kind {
        ProviderKind::Bootstrap => {
            target.base_url = None;
            target.api_key = None;
            if target.model == DEFAULT_MODEL {
                target.model = DEFAULT_MODEL.to_string();
            }
        }
        ProviderKind::Echo => {
            target.base_url = None;
            target.api_key = None;
            if target.model == DEFAULT_MODEL {
                target.model = "echo".to_string();
            }
        }
        ProviderKind::OpenAiCompatible => {
            if let Some(base_url) = std::env::var(BASE_URL_ENV)
                .ok()
                .or_else(|| std::env::var(OPENAI_BASE_URL_ENV).ok())
            {
                target.base_url = Some(base_url);
            } else if target.base_url.is_none() {
                target.base_url = Some("https://api.openai.com/v1".to_string());
            }
            if let Some(model) = std::env::var(MODEL_ENV)
                .ok()
                .or_else(|| std::env::var(OPENAI_MODEL_ENV).ok())
            {
                target.model = model;
            }
            if let Ok(api_key) = std::env::var(OPENAI_API_KEY_ENV) {
                target.api_key = Some(api_key);
            }
        }
        ProviderKind::AnthropicCompatible => {
            if let Some(base_url) = std::env::var(BASE_URL_ENV)
                .ok()
                .or_else(|| std::env::var(ANTHROPIC_BASE_URL_ENV).ok())
            {
                target.base_url = Some(base_url);
            } else if target.base_url.is_none() {
                target.base_url = Some("https://api.anthropic.com".to_string());
            }
            if let Some(model) = std::env::var(MODEL_ENV)
                .ok()
                .or_else(|| std::env::var(ANTHROPIC_MODEL_ENV).ok())
            {
                target.model = model;
            }
            if let Some(api_key) = std::env::var(ANTHROPIC_AUTH_TOKEN_ENV)
                .ok()
                .or_else(|| std::env::var(ANTHROPIC_API_KEY_ENV).ok())
            {
                target.api_key = Some(api_key);
            }
        }
        ProviderKind::XaiCompatible => {
            if let Some(base_url) = std::env::var(BASE_URL_ENV)
                .ok()
                .or_else(|| std::env::var(XAI_BASE_URL_ENV).ok())
            {
                target.base_url = Some(base_url);
            } else if target.base_url.is_none() {
                target.base_url = Some("https://api.x.ai/v1".to_string());
            }
            if let Some(model) = std::env::var(MODEL_ENV)
                .ok()
                .or_else(|| std::env::var(XAI_MODEL_ENV).ok())
            {
                target.model = model;
            }
            if let Ok(api_key) = std::env::var(XAI_API_KEY_ENV) {
                target.api_key = Some(api_key);
            }
        }
        ProviderKind::LocalCompatible => {
            if let Some(base_url) = std::env::var(BASE_URL_ENV)
                .ok()
                .or_else(|| std::env::var(LOCAL_BASE_URL_ENV).ok())
            {
                target.base_url = Some(base_url);
            }
            if let Some(model) = std::env::var(MODEL_ENV)
                .ok()
                .or_else(|| std::env::var(LOCAL_MODEL_ENV).ok())
            {
                target.model = model;
            }
            target.api_key = None;
        }
    }
}

pub(crate) fn apply_env_approval(target: &mut ApprovalPolicy) {
    if let Some(policy) = std::env::var(APPROVAL_POLICY_ENV)
        .ok()
        .as_deref()
        .and_then(parse_approval_policy)
    {
        *target = policy;
    }
}

pub(crate) fn apply_env_approval_rules(target: &mut ApprovalRules) {
    extend_rule_list_csv(
        &mut target.allow_commands,
        std::env::var(APPROVAL_ALLOW_COMMANDS_ENV).ok(),
    );
    extend_rule_list_csv(
        &mut target.deny_commands,
        std::env::var(APPROVAL_DENY_COMMANDS_ENV).ok(),
    );
    extend_rule_list_csv(
        &mut target.allow_paths,
        std::env::var(APPROVAL_ALLOW_PATHS_ENV).ok(),
    );
    extend_rule_list_csv(
        &mut target.deny_paths,
        std::env::var(APPROVAL_DENY_PATHS_ENV).ok(),
    );
    extend_rule_list_csv(
        &mut target.allow_tools,
        std::env::var(APPROVAL_ALLOW_TOOLS_ENV).ok(),
    );
    extend_rule_list_csv(
        &mut target.deny_tools,
        std::env::var(APPROVAL_DENY_TOOLS_ENV).ok(),
    );
}

pub(crate) fn apply_env_sandbox(target: &mut SandboxMode) {
    if let Some(mode) = std::env::var(SANDBOX_MODE_ENV)
        .ok()
        .as_deref()
        .and_then(SandboxMode::parse)
    {
        *target = mode;
    }
}
