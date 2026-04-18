use std::path::PathBuf;

use ccodex_protocol::{SessionId, Turn};

use crate::{AgentLoop, Kernel, KernelError, RunTurnResult};

/// Product-facing façade over the kernel's session-oriented orchestration.
///
/// Protocol and persistence still speak in terms of `Session / Turn / Item`,
/// but this façade makes the agent runtime entrypoints easier to discover.
pub struct AgentRuntime<'a> {
    kernel: &'a Kernel,
}

impl<'a> AgentRuntime<'a> {
    pub(crate) fn new(kernel: &'a Kernel) -> Self {
        Self { kernel }
    }

    pub fn agent_loop(&self) -> AgentLoop<'a> {
        AgentLoop::new(self.kernel)
    }

    pub async fn start_session(
        &self,
        prompt: impl Into<String>,
        workspace_root: Option<PathBuf>,
    ) -> Result<RunTurnResult, KernelError> {
        self.kernel.run_prompt(prompt, workspace_root).await
    }

    pub async fn continue_session(
        &self,
        session_id: &SessionId,
        prompt: impl Into<String>,
    ) -> Result<RunTurnResult, KernelError> {
        self.kernel.resume_prompt(session_id, prompt).await
    }

    pub async fn fork_session(
        &self,
        session_id: &SessionId,
    ) -> Result<ccodex_protocol::Session, KernelError> {
        self.kernel.fork_session(session_id).await
    }

    pub async fn continue_turn(
        &self,
        session_id: &SessionId,
        turn: &Turn,
        prompt: impl Into<String>,
    ) -> Result<RunTurnResult, KernelError> {
        let session = self.kernel.store.get_session(session_id).await?;
        self.agent_loop()
            .run_in_session(
                session,
                prompt.into(),
                vec![ccodex_protocol::ProtocolEvent::TurnStarted(turn.clone())],
            )
            .await
    }
}
