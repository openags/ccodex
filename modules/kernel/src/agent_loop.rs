use ccodex_protocol::{ProtocolEvent, Session};

use crate::{Kernel, KernelError, RunTurnResult};

/// Explicit façade for the kernel's main agent loop.
///
/// This keeps the internal turn lifecycle split (`turn_start`, `turn_iteration`,
/// `turn_finalize`) while making the orchestration entrypoint discoverable.
pub struct AgentLoop<'a> {
    kernel: &'a Kernel,
}

impl<'a> AgentLoop<'a> {
    pub(crate) fn new(kernel: &'a Kernel) -> Self {
        Self { kernel }
    }

    pub async fn run_in_session(
        &self,
        session: Session,
        prompt: impl Into<String>,
        events: Vec<ProtocolEvent>,
    ) -> Result<RunTurnResult, KernelError> {
        self.kernel
            .run_prompt_in_session(session, prompt.into(), events)
            .await
    }
}
