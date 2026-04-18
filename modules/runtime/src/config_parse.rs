use crate::config::{ApprovalPolicy, ProviderKind};

pub(crate) fn parse_provider_kind(value: &str) -> Option<ProviderKind> {
    match value {
        "bootstrap" => Some(ProviderKind::Bootstrap),
        "echo" => Some(ProviderKind::Echo),
        "openai" | "openai-compatible" => Some(ProviderKind::OpenAiCompatible),
        "anthropic" | "anthropic-compatible" => Some(ProviderKind::AnthropicCompatible),
        "xai" | "xai-compatible" => Some(ProviderKind::XaiCompatible),
        "local" | "local-compatible" => Some(ProviderKind::LocalCompatible),
        _ => None,
    }
}

pub(crate) fn parse_approval_policy(value: &str) -> Option<ApprovalPolicy> {
    match value {
        "always" | "approve" => Some(ApprovalPolicy::AlwaysApprove),
        "ask" => Some(ApprovalPolicy::Ask),
        "never" | "deny" => Some(ApprovalPolicy::NeverApprove),
        _ => None,
    }
}

pub(crate) fn extend_rule_list(target: &mut Vec<String>, values: Option<&Vec<String>>) {
    if let Some(values) = values {
        target.extend(
            values
                .iter()
                .filter(|value| !value.trim().is_empty())
                .cloned(),
        );
    }
}

pub(crate) fn extend_rule_list_csv(target: &mut Vec<String>, values: Option<String>) {
    if let Some(values) = values {
        target.extend(
            values
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
        );
    }
}

pub(crate) fn infer_provider_kind() -> ProviderKind {
    if std::env::var("OPENAI_API_KEY").is_ok() {
        ProviderKind::OpenAiCompatible
    } else if std::env::var("ANTHROPIC_AUTH_TOKEN").is_ok()
        || std::env::var("ANTHROPIC_API_KEY").is_ok()
    {
        ProviderKind::AnthropicCompatible
    } else if std::env::var("XAI_API_KEY").is_ok() {
        ProviderKind::XaiCompatible
    } else {
        ProviderKind::Bootstrap
    }
}
