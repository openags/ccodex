use ccodex_protocol::{ApprovalKind, ToolCall, ToolSpec};

use crate::sandbox::{
    command_looks_dangerous, command_looks_mutating, command_looks_scripted_execution,
    command_requests_network, command_targets_sensitive_locations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct ApprovalAssessment {
    pub kind: ApprovalKind,
    pub summary: String,
    pub details: Option<String>,
    pub risk: CommandRisk,
}

pub fn analyze_tool_call(spec: &ToolSpec, call: &ToolCall) -> ApprovalAssessment {
    match call.tool_name.as_str() {
        "bash" => analyze_bash(call),
        "write_file" | "edit_file" => analyze_file_write(call),
        _ => ApprovalAssessment {
            kind: if spec.requires_approval {
                ApprovalKind::ToolUse
            } else {
                ApprovalKind::PermissionEscalation
            },
            summary: format!("Approve tool execution: {}", call.tool_name),
            details: Some(call.input.to_string()),
            risk: CommandRisk::Low,
        },
    }
}

fn analyze_bash(call: &ToolCall) -> ApprovalAssessment {
    let command = call
        .input
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let risk = analyze_command_risk(command);
    let label = match risk {
        CommandRisk::Low => "low",
        CommandRisk::Medium => "medium",
        CommandRisk::High => "high",
    };

    ApprovalAssessment {
        kind: ApprovalKind::CommandExecution,
        summary: format!("Approve {label}-risk shell command"),
        details: Some(command.to_string()),
        risk,
    }
}

fn analyze_file_write(call: &ToolCall) -> ApprovalAssessment {
    let path = call
        .input
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unknown>");

    ApprovalAssessment {
        kind: ApprovalKind::FileWrite,
        summary: format!("Approve file write: {path}"),
        details: Some(call.input.to_string()),
        risk: CommandRisk::Medium,
    }
}

fn analyze_command_risk(command: &str) -> CommandRisk {
    if command_looks_dangerous(command)
        || command_requests_network(command)
        || command_targets_sensitive_locations(command)
    {
        return CommandRisk::High;
    }

    if command_looks_mutating(command) || command_looks_scripted_execution(command) {
        return CommandRisk::Medium;
    }

    CommandRisk::Low
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use ccodex_protocol::{
        ApprovalKind, ToolCall, ToolCallId, ToolConcurrency, ToolKind, ToolSpec,
    };

    use super::{CommandRisk, analyze_tool_call};

    fn bash_spec() -> ToolSpec {
        ToolSpec {
            name: "bash".to_string(),
            description: "bash".to_string(),
            input_schema: json!({}),
            output_schema: None,
            kind: ToolKind::Builtin,
            concurrency: ToolConcurrency::Exclusive,
            requires_approval: true,
            sandbox_profile: Some("workspace-write".to_string()),
        }
    }

    #[test]
    fn classifies_high_risk_bash_commands() {
        let assessment = analyze_tool_call(
            &bash_spec(),
            &ToolCall {
                id: ToolCallId("tool-1".to_string()),
                tool_name: "bash".to_string(),
                input: json!({"command":"rm -rf .ccodex/tmp"}),
            },
        );

        assert_eq!(assessment.kind, ApprovalKind::CommandExecution);
        assert_eq!(assessment.risk, CommandRisk::High);
    }

    #[test]
    fn classifies_network_commands_as_high_risk() {
        let assessment = analyze_tool_call(
            &bash_spec(),
            &ToolCall {
                id: ToolCallId("tool-network".to_string()),
                tool_name: "bash".to_string(),
                input: json!({"command":"curl https://example.com"}),
            },
        );

        assert_eq!(assessment.risk, CommandRisk::High);
        assert!(assessment.summary.contains("high-risk"));
    }

    #[test]
    fn classifies_scripted_execution_as_medium_risk() {
        let assessment = analyze_tool_call(
            &bash_spec(),
            &ToolCall {
                id: ToolCallId("tool-scripted".to_string()),
                tool_name: "bash".to_string(),
                input: json!({"command":"python -c 'print(1)'"}),
            },
        );

        assert_eq!(assessment.risk, CommandRisk::Medium);
        assert!(assessment.summary.contains("medium-risk"));
    }

    #[test]
    fn classifies_sensitive_location_commands_as_high_risk() {
        let assessment = analyze_tool_call(
            &bash_spec(),
            &ToolCall {
                id: ToolCallId("tool-sensitive".to_string()),
                tool_name: "bash".to_string(),
                input: json!({"command":"cat /etc/hosts"}),
            },
        );

        assert_eq!(assessment.risk, CommandRisk::High);
    }

    #[test]
    fn classifies_file_write_as_file_write() {
        let spec = ToolSpec {
            name: "write_file".to_string(),
            description: "write".to_string(),
            input_schema: json!({}),
            output_schema: None,
            kind: ToolKind::Builtin,
            concurrency: ToolConcurrency::Exclusive,
            requires_approval: true,
            sandbox_profile: Some("workspace-write".to_string()),
        };
        let assessment = analyze_tool_call(
            &spec,
            &ToolCall {
                id: ToolCallId("tool-2".to_string()),
                tool_name: "write_file".to_string(),
                input: json!({"path":"src/main.rs","content":"hi"}),
            },
        );

        assert_eq!(assessment.kind, ApprovalKind::FileWrite);
        assert_eq!(assessment.risk, CommandRisk::Medium);
        assert!(assessment.summary.contains("src/main.rs"));
    }
}
