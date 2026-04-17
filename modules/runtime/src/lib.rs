//! Runtime capability substrate with bootstrap-grade providers, approvals, and tools.

mod approval;
mod config;
mod provider;
mod tools;

pub use approval::{AutoApproveEngine, StaticAskUserEngine};
pub use config::{ApprovalPolicy, RuntimeConfig};
pub use provider::{BootstrapModelProvider, EchoModelProvider};
pub use tools::{BuiltinToolExecutor, ToolRegistry};

use std::sync::Arc;

use async_trait::async_trait;

use ccodex_protocol::{ApprovalEnginePort, NotificationPort, PortError, ProtocolEvent, ToolExecutorPort, ToolSpec};

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
        let config = RuntimeConfig::default();
        let tool_registry = ToolRegistry::bootstrap_builtin();
        let provider = Arc::new(BootstrapModelProvider::default());
        let notifications = Arc::new(NoopNotificationPort);
        let approval_engine = Arc::new(AutoApproveEngine::new(config.approval_policy.clone()));
        let tool_executor = Arc::new(BuiltinToolExecutor::new(
            config.workspace_root.clone(),
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
        let config = RuntimeConfig::default();
        let tool_registry = ToolRegistry::bootstrap_builtin();
        let provider = Arc::new(EchoModelProvider::default());
        let notifications = Arc::new(NoopNotificationPort);
        let approval_engine = Arc::new(AutoApproveEngine::new(config.approval_policy.clone()));
        let tool_executor = Arc::new(BuiltinToolExecutor::new(
            config.workspace_root.clone(),
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
        Self::bootstrap()
    }
}
