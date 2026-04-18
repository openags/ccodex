//! Runtime capability substrate with bootstrap-grade providers, approvals, and tools.

mod approval;
mod config;
mod config_env;
mod config_parse;
mod fs;
mod mcp;
mod permissions;
mod provider;
mod provider_bootstrap;
mod provider_parse;
mod sandbox;
mod shell;
mod tools;

pub use approval::{AutoApproveEngine, StaticAskUserEngine, enrich_approval_request};
pub use config::{
    ApprovalPolicy, ApprovalRules, ProviderConfig, ProviderKind, RuntimeConfig, RuntimeConfigError,
};
pub use fs::WorkspaceFs;
pub use mcp::{CommandBackedMcpPort, McpServerDefinition};
pub use permissions::{ApprovalAssessment, CommandRisk, analyze_tool_call};
pub use provider::{
    AnthropicCompatibleProvider, EchoModelProvider, OpenAiCompatibleProvider, provider_from_config,
};
pub use provider_bootstrap::BootstrapModelProvider;
pub use sandbox::{
    FileAccess, SandboxMode, SandboxPolicy, command_looks_dangerous, command_looks_mutating,
    command_looks_scripted_execution, command_requests_network,
    command_targets_sensitive_locations,
};
pub use shell::{ShellOutput, WorkspaceShell};
pub use tools::{BuiltinToolExecutor, ToolRegistry};

use std::sync::Arc;
use std::{path::PathBuf, result::Result as StdResult};

use async_trait::async_trait;

use ccodex_protocol::{
    ApprovalEnginePort, NotificationPort, PortError, ProtocolEvent, ToolExecutorPort, ToolSpec,
};

#[derive(Default)]
pub struct NoopNotificationPort;

#[async_trait]
impl NotificationPort for NoopNotificationPort {
    async fn notify_event(&self, _event: &ProtocolEvent) -> Result<(), PortError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct Runtime {
    config: RuntimeConfig,
    provider: Arc<dyn ccodex_protocol::ModelProviderPort>,
    notifications: Arc<dyn NotificationPort>,
    approval_engine: Arc<dyn ApprovalEnginePort>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    tool_registry: ToolRegistry,
}

impl Runtime {
    pub fn new(
        config: RuntimeConfig,
        provider: Arc<dyn ccodex_protocol::ModelProviderPort>,
        notifications: Arc<dyn NotificationPort>,
        approval_engine: Arc<dyn ApprovalEnginePort>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_registry: ToolRegistry,
    ) -> Self {
        Self {
            config,
            provider,
            notifications,
            approval_engine,
            tool_executor,
            tool_registry,
        }
    }

    pub fn bootstrap() -> Self {
        let config = RuntimeConfig {
            provider: ProviderConfig {
                kind: ProviderKind::Bootstrap,
                base_url: None,
                api_key: None,
                model: ccodex_brand::DEFAULT_MODEL.to_string(),
                max_output_tokens: 16_000,
            },
            // Use AlwaysApprove for bootstrap mode to enable automated testing
            approval_policy: ApprovalPolicy::AlwaysApprove,
            ..RuntimeConfig::default()
        };
        Self::from_config(config)
    }

    pub fn from_env() -> Self {
        Self::from_config(RuntimeConfig::from_env())
    }

    pub fn for_workspace(workspace_root: PathBuf) -> StdResult<Self, config::RuntimeConfigError> {
        Ok(Self::from_config(RuntimeConfig::load_for_workspace(
            workspace_root,
        )?))
    }

    pub fn from_config(config: RuntimeConfig) -> Self {
        let tool_registry = ToolRegistry::bootstrap_builtin();
        let provider = Arc::from(provider_from_config(&config.provider));
        let notifications = Arc::new(NoopNotificationPort);
        let approval_engine = Arc::new(AutoApproveEngine::new(
            config.approval_policy.clone(),
            config.approval_rules.clone(),
        ));
        let tool_executor = Arc::new(BuiltinToolExecutor::new(
            config.workspace_root.clone(),
            config.sandbox_mode.clone(),
            tool_registry.clone(),
        ));

        Self::new(
            config,
            provider,
            notifications,
            approval_engine,
            tool_executor,
            tool_registry,
        )
    }

    pub fn echo() -> Self {
        let config = RuntimeConfig {
            provider: ProviderConfig {
                kind: ProviderKind::Echo,
                base_url: None,
                api_key: None,
                model: "echo".to_string(),
                max_output_tokens: 16_000,
            },
            ..RuntimeConfig::default()
        };
        Self::from_config(config)
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub fn provider(&self) -> Arc<dyn ccodex_protocol::ModelProviderPort> {
        Arc::clone(&self.provider)
    }

    pub fn notifications(&self) -> Arc<dyn NotificationPort> {
        Arc::clone(&self.notifications)
    }

    pub fn approval_engine(&self) -> Arc<dyn ApprovalEnginePort> {
        Arc::clone(&self.approval_engine)
    }

    pub fn tool_executor(&self) -> Arc<dyn ToolExecutorPort> {
        Arc::clone(&self.tool_executor)
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tool_registry.list().cloned().collect()
    }

    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::from_env()
    }
}
