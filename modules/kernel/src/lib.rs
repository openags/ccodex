//! Minimal orchestration kernel for bootstrapping a real end-to-end turn.

use std::collections::BTreeMap;
use std::sync::Arc;

use thiserror::Error;

use ccodex_compat::{CompatError, CompatLayer};
use ccodex_extensions::ExtensionError;
use ccodex_protocol::{
    ApprovalEnginePort, ModelProviderPort, NotificationPort, PortError, ProtocolEvent, Session,
    ToolExecutorPort, ToolSpec, Turn,
};
use ccodex_store::{SessionStore, StoreError};

mod agent_loop;
mod agent_runtime;
mod ask_user;
mod bootstrap;
mod compaction;
mod context;
mod hooks;
mod items;
mod planner;
mod session_manager;
mod subagent_session;
mod subagents;
mod tool_approval;
mod tool_scheduler;
mod tool_special_cases;
mod turn_engine;
mod turn_finalize;
mod turn_iteration;
mod turn_start;

#[cfg(test)]
mod tests;

pub use agent_loop::AgentLoop;
pub use agent_runtime::AgentRuntime;
pub use ccodex_compat::WorkspaceTrust;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Compat(#[from] CompatError),
    #[error(transparent)]
    Extensions(#[from] ExtensionError),
}

#[derive(Debug, Clone)]
pub struct RunTurnResult {
    pub session: Session,
    pub turn: Turn,
    pub assistant_text: String,
    pub events: Vec<ProtocolEvent>,
}

pub struct Kernel {
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn ModelProviderPort>,
    notifications: Arc<dyn NotificationPort>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    approval_engine: Arc<dyn ApprovalEnginePort>,
    tool_specs: BTreeMap<String, ToolSpec>,
    compat: CompatLayer,
}

impl Kernel {
    pub fn new(
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn ModelProviderPort>,
        notifications: Arc<dyn NotificationPort>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        approval_engine: Arc<dyn ApprovalEnginePort>,
        tool_specs: impl IntoIterator<Item = ToolSpec>,
    ) -> Self {
        Self {
            store,
            provider,
            notifications,
            tool_executor,
            approval_engine,
            tool_specs: tool_specs
                .into_iter()
                .map(|spec| (spec.name.clone(), spec))
                .collect(),
            compat: CompatLayer::new(),
        }
    }

    /// Create a kernel with explicit workspace trust level.
    /// Trusted workspaces can load hooks from workspace-local directories.
    pub fn with_trust(
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn ModelProviderPort>,
        notifications: Arc<dyn NotificationPort>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        approval_engine: Arc<dyn ApprovalEnginePort>,
        tool_specs: impl IntoIterator<Item = ToolSpec>,
        trust: WorkspaceTrust,
    ) -> Self {
        Self {
            store,
            provider,
            notifications,
            tool_executor,
            approval_engine,
            tool_specs: tool_specs
                .into_iter()
                .map(|spec| (spec.name.clone(), spec))
                .collect(),
            compat: CompatLayer::with_trust(trust),
        }
    }

    pub fn agent_runtime(&self) -> AgentRuntime<'_> {
        AgentRuntime::new(self)
    }

    pub fn agent_loop(&self) -> AgentLoop<'_> {
        AgentLoop::new(self)
    }
}
