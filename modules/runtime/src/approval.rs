use std::io::{self, IsTerminal, Write};

use async_trait::async_trait;

use ccodex_protocol::{
    ApprovalContext, ApprovalDecision, ApprovalEnginePort, ApprovalReasonCode, ApprovalRequest,
    ApprovalResponse, ApprovalRisk, AskUserPrompt, AskUserResponse, PortError,
};

use crate::sandbox::{
    command_looks_dangerous, command_requests_network, command_targets_sensitive_locations,
};
use crate::{ApprovalPolicy, ApprovalRules};

#[derive(Debug, Clone)]
pub struct AutoApproveEngine {
    policy: ApprovalPolicy,
    rules: ApprovalRules,
    interactive: bool,
}

impl AutoApproveEngine {
    pub fn new(policy: ApprovalPolicy, rules: ApprovalRules) -> Self {
        Self {
            policy,
            rules,
            interactive: io::stdin().is_terminal() && io::stdout().is_terminal(),
        }
    }

    #[cfg(test)]
    pub fn with_interactive(
        policy: ApprovalPolicy,
        rules: ApprovalRules,
        interactive: bool,
    ) -> Self {
        Self {
            policy,
            rules,
            interactive,
        }
    }

    fn prompt_line(&self, prompt: &str) -> Result<String, PortError> {
        print!("{prompt}");
        io::stdout()
            .flush()
            .map_err(|err| PortError::Approval(format!("failed to flush prompt: {err}")))?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|err| PortError::Approval(format!("failed to read response: {err}")))?;
        Ok(input.trim().to_string())
    }
}

#[async_trait]
impl ApprovalEnginePort for AutoApproveEngine {
    async fn request_approval(
        &self,
        request: ApprovalRequest,
    ) -> Result<ApprovalResponse, PortError> {
        let request = enrich_approval_request(request);
        let rule_result = self.apply_rules(&request);
        let response = if let Some((decision, reason, reason_code)) = rule_result {
            ApprovalResponse::new(request.item_id, decision, reason, Some(reason_code))
        } else {
            match self.policy {
                ApprovalPolicy::AlwaysApprove => ApprovalResponse::new(
                    request.item_id,
                    ApprovalDecision::Approved,
                    None,
                    Some(ApprovalReasonCode::PolicyAlwaysApprove),
                ),
                ApprovalPolicy::NeverApprove => ApprovalResponse::new(
                    request.item_id,
                    ApprovalDecision::Rejected,
                    None,
                    Some(ApprovalReasonCode::PolicyNeverApprove),
                ),
                ApprovalPolicy::Ask => {
                    if !self.interactive {
                        ApprovalResponse::new(
                            request.item_id,
                            ApprovalDecision::Cancelled,
                            Some(
                                "interactive approval required but no terminal is attached"
                                    .to_string(),
                            ),
                            Some(ApprovalReasonCode::InteractiveUnavailable),
                        )
                    } else {
                        let input = self.prompt_line(&format!(
                            "Approval required [{:?}/{:?}] {} [y/N]: ",
                            request.kind, request.risk, request.summary
                        ))?;
                        match input.to_ascii_lowercase().as_str() {
                            "y" | "yes" => ApprovalResponse::new(
                                request.item_id,
                                ApprovalDecision::Approved,
                                Some("approved in terminal".to_string()),
                                Some(ApprovalReasonCode::InteractiveApproved),
                            ),
                            "n" | "no" => ApprovalResponse::new(
                                request.item_id,
                                ApprovalDecision::Rejected,
                                Some("rejected in terminal".to_string()),
                                Some(ApprovalReasonCode::InteractiveRejected),
                            ),
                            _ => ApprovalResponse::new(
                                request.item_id,
                                ApprovalDecision::Cancelled,
                                Some("approval cancelled in terminal".to_string()),
                                Some(ApprovalReasonCode::InteractiveCancelled),
                            ),
                        }
                    }
                }
            }
        };

        Ok(response)
    }

    async fn request_user_input(
        &self,
        prompt: AskUserPrompt,
    ) -> Result<AskUserResponse, PortError> {
        if matches!(self.policy, ApprovalPolicy::Ask) && self.interactive {
            println!("\n{}: {}", prompt.title, prompt.message);
            for (index, choice) in prompt.choices.iter().enumerate() {
                println!(
                    "{}. {}{}",
                    index + 1,
                    choice.label,
                    choice
                        .description
                        .as_deref()
                        .map(|desc| format!(" - {desc}"))
                        .unwrap_or_default()
                );
            }
            let raw = self.prompt_line("Select a choice number or id (blank to cancel): ")?;
            let selected_choice_id = if raw.is_empty() {
                None
            } else if let Ok(index) = raw.parse::<usize>() {
                prompt
                    .choices
                    .get(index.saturating_sub(1))
                    .map(|choice| choice.id.clone())
            } else {
                prompt
                    .choices
                    .iter()
                    .find(|choice| choice.id == raw)
                    .map(|choice| choice.id.clone())
                    .or_else(|| prompt.allow_freeform.then_some(raw.clone()))
            };

            return Ok(AskUserResponse {
                request_item_id: prompt.item_id,
                selected_choice_id: selected_choice_id
                    .as_ref()
                    .filter(|id| prompt.choices.iter().any(|choice| &choice.id == *id))
                    .cloned(),
                freeform_text: if prompt.allow_freeform
                    && selected_choice_id
                        .as_ref()
                        .map(|id| !prompt.choices.iter().any(|choice| &choice.id == id))
                        .unwrap_or(false)
                {
                    selected_choice_id
                } else {
                    None
                },
            });
        }

        let selected_choice_id = match self.policy {
            ApprovalPolicy::AlwaysApprove => prompt.choices.first().map(|choice| choice.id.clone()),
            ApprovalPolicy::Ask | ApprovalPolicy::NeverApprove => None,
        };

        Ok(AskUserResponse {
            request_item_id: prompt.item_id,
            selected_choice_id,
            freeform_text: None,
        })
    }
}

impl AutoApproveEngine {
    fn apply_rules(
        &self,
        request: &ApprovalRequest,
    ) -> Option<(ApprovalDecision, Option<String>, ApprovalReasonCode)> {
        if self.rules.is_empty() {
            return None;
        }

        let tool_name = request
            .context
            .tool_name
            .clone()
            .or_else(|| extract_tool_name(request));
        let command = request.context.command.as_deref().or_else(|| {
            request.details.as_deref().filter(|_| {
                matches!(
                    request.kind,
                    ccodex_protocol::ApprovalKind::CommandExecution
                )
            })
        });
        let path = request
            .context
            .path
            .clone()
            .or_else(|| extract_path(request));

        if tool_name
            .as_deref()
            .is_some_and(|value| matches_any(value, &self.rules.deny_tools))
            || command.is_some_and(|value| matches_any(value, &self.rules.deny_commands))
            || path
                .as_deref()
                .is_some_and(|value| matches_path(value, &self.rules.deny_paths))
        {
            return Some((
                ApprovalDecision::Rejected,
                Some("rejected by approval rule".to_string()),
                ApprovalReasonCode::RuleDeny,
            ));
        }

        if tool_name
            .as_deref()
            .is_some_and(|value| matches_any(value, &self.rules.allow_tools))
            || command.is_some_and(|value| matches_any(value, &self.rules.allow_commands))
            || path
                .as_deref()
                .is_some_and(|value| matches_path(value, &self.rules.allow_paths))
        {
            return Some((
                ApprovalDecision::Approved,
                Some("approved by approval rule".to_string()),
                ApprovalReasonCode::RuleAllow,
            ));
        }

        None
    }
}

pub fn enrich_approval_request(mut request: ApprovalRequest) -> ApprovalRequest {
    let tool_name = extract_tool_name(&request);
    let command = request.details.as_deref().and_then(|details| {
        matches!(
            request.kind,
            ccodex_protocol::ApprovalKind::CommandExecution
        )
        .then(|| details.to_string())
    });
    let path = extract_path(&request);
    let command_text = command.as_deref().unwrap_or_default();
    let has_network_access = command_requests_network(command_text);
    let is_destructive = command_looks_dangerous(command_text)
        || ["rm ", "mv ", "chmod ", "chown ", "git reset", "git clean"]
            .iter()
            .any(|pattern| command_text.to_ascii_lowercase().contains(pattern));
    let touches_outside_workspace = path.as_deref().is_some_and(|value| {
        value.starts_with('/')
            || value.contains("../")
            || command_targets_sensitive_locations(value)
    });

    let risk = match request.kind {
        ccodex_protocol::ApprovalKind::PermissionEscalation => ApprovalRisk::High,
        ccodex_protocol::ApprovalKind::CommandExecution if is_destructive || has_network_access => {
            ApprovalRisk::High
        }
        ccodex_protocol::ApprovalKind::FileWrite if touches_outside_workspace => ApprovalRisk::High,
        ccodex_protocol::ApprovalKind::CommandExecution
        | ccodex_protocol::ApprovalKind::FileWrite => ApprovalRisk::Medium,
        ccodex_protocol::ApprovalKind::ToolUse => ApprovalRisk::Low,
    };

    request = request.with_risk(risk).with_context(ApprovalContext {
        tool_name,
        command,
        path,
        touches_workspace: !touches_outside_workspace,
        touches_outside_workspace,
        has_network_access,
        is_destructive,
    });
    request
}

fn extract_tool_name(request: &ApprovalRequest) -> Option<String> {
    request
        .summary
        .strip_prefix("Approve tool execution: ")
        .map(|value| value.trim().to_string())
        .or_else(|| match request.kind {
            ccodex_protocol::ApprovalKind::CommandExecution => Some("bash".to_string()),
            ccodex_protocol::ApprovalKind::FileWrite => Some("write_file".to_string()),
            _ => None,
        })
}

fn extract_path(request: &ApprovalRequest) -> Option<String> {
    if let Some(path) = request.summary.strip_prefix("Approve file write: ") {
        return Some(path.trim().to_string());
    }

    request.details.as_deref().and_then(|details| {
        serde_json::from_str::<serde_json::Value>(details)
            .ok()
            .and_then(|value| {
                value
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string)
            })
    })
}

fn matches_any(value: &str, patterns: &[String]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    patterns
        .iter()
        .map(|pattern| pattern.trim().to_ascii_lowercase())
        .any(|pattern| !pattern.is_empty() && normalized.contains(&pattern))
}

fn matches_path(value: &str, patterns: &[String]) -> bool {
    let normalized = value.trim().replace('\\', "/");
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim().replace('\\', "/");
        !pattern.is_empty()
            && (normalized == pattern
                || normalized.starts_with(&pattern)
                || normalized.contains(&pattern))
    })
}

#[derive(Debug, Default, Clone)]
pub struct StaticAskUserEngine;

#[async_trait]
impl ApprovalEnginePort for StaticAskUserEngine {
    async fn request_approval(
        &self,
        request: ApprovalRequest,
    ) -> Result<ApprovalResponse, PortError> {
        Ok(ApprovalResponse::new(
            request.item_id,
            ApprovalDecision::Approved,
            None,
            Some(ApprovalReasonCode::PolicyAlwaysApprove),
        ))
    }

    async fn request_user_input(
        &self,
        prompt: AskUserPrompt,
    ) -> Result<AskUserResponse, PortError> {
        Ok(AskUserResponse {
            request_item_id: prompt.item_id,
            selected_choice_id: prompt.choices.first().map(|choice| choice.id.clone()),
            freeform_text: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use ccodex_protocol::{ApprovalKind, ApprovalRequest, AskUserChoice, AskUserPrompt, ItemId};
    use futures::executor::block_on;

    use super::{
        ApprovalDecision, ApprovalEnginePort, ApprovalPolicy, ApprovalReasonCode,
        AutoApproveEngine, enrich_approval_request,
    };
    use crate::ApprovalRules;

    #[test]
    fn ask_policy_without_terminal_cancels_approval() {
        let engine = AutoApproveEngine::with_interactive(
            ApprovalPolicy::Ask,
            ApprovalRules::default(),
            false,
        );
        let response = block_on(engine.request_approval(ApprovalRequest::new(
            ItemId("approval-1".to_string()),
            None,
            ApprovalKind::ToolUse,
            "approve write".to_string(),
            None,
        )))
        .expect("approval response should exist");

        assert_eq!(response.decision, ApprovalDecision::Cancelled);
        assert_eq!(
            response.reason_code,
            Some(ApprovalReasonCode::InteractiveUnavailable)
        );
        assert!(
            response
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("interactive approval required")
        );
    }

    #[test]
    fn ask_policy_without_terminal_returns_empty_user_choice() {
        let engine = AutoApproveEngine::with_interactive(
            ApprovalPolicy::Ask,
            ApprovalRules::default(),
            false,
        );
        let response = block_on(engine.request_user_input(AskUserPrompt {
            item_id: ItemId("ask-1".to_string()),
            title: "Choose".to_string(),
            message: "Pick one".to_string(),
            choices: vec![AskUserChoice {
                id: "choice-1".to_string(),
                label: "First".to_string(),
                description: None,
            }],
            allow_freeform: false,
        }))
        .expect("ask-user response should exist");

        assert_eq!(response.selected_choice_id, None);
        assert_eq!(response.freeform_text, None);
    }

    #[test]
    fn approval_rules_can_auto_reject_command() {
        let engine = AutoApproveEngine::with_interactive(
            ApprovalPolicy::Ask,
            ApprovalRules {
                deny_commands: vec!["rm -rf".to_string()],
                ..ApprovalRules::default()
            },
            false,
        );
        let response = block_on(engine.request_approval(ApprovalRequest::new(
            ItemId("approval-2".to_string()),
            None,
            ApprovalKind::CommandExecution,
            "Approve high-risk shell command".to_string(),
            Some("rm -rf .ccodex/tmp".to_string()),
        )))
        .expect("approval response should exist");

        assert_eq!(response.decision, ApprovalDecision::Rejected);
        assert_eq!(
            response.reason.as_deref(),
            Some("rejected by approval rule")
        );
        assert_eq!(response.reason_code, Some(ApprovalReasonCode::RuleDeny));
    }

    #[test]
    fn approval_rules_can_auto_approve_file_write() {
        let engine = AutoApproveEngine::with_interactive(
            ApprovalPolicy::Ask,
            ApprovalRules {
                allow_paths: vec!["docs/".to_string()],
                ..ApprovalRules::default()
            },
            false,
        );
        let response = block_on(engine.request_approval(ApprovalRequest::new(
            ItemId("approval-3".to_string()),
            None,
            ApprovalKind::FileWrite,
            "Approve file write: docs/guide.md".to_string(),
            Some("{\"path\":\"docs/guide.md\"}".to_string()),
        )))
        .expect("approval response should exist");

        assert_eq!(response.decision, ApprovalDecision::Approved);
        assert_eq!(
            response.reason.as_deref(),
            Some("approved by approval rule")
        );
        assert_eq!(response.reason_code, Some(ApprovalReasonCode::RuleAllow));
    }

    #[test]
    fn enrich_approval_request_marks_network_and_destructive_commands_high_risk() {
        let request = enrich_approval_request(ApprovalRequest::new(
            ItemId("approval-4".to_string()),
            None,
            ApprovalKind::CommandExecution,
            "Approve high-risk shell command".to_string(),
            Some("curl https://example.com && rm -rf target".to_string()),
        ));

        assert_eq!(request.risk, ccodex_protocol::ApprovalRisk::High);
        assert_eq!(request.context.tool_name.as_deref(), Some("bash"));
        assert!(request.context.has_network_access);
        assert!(request.context.is_destructive);
    }
}
