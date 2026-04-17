//! Small bootstrap harness for end-to-end kernel tests.

use std::path::PathBuf;
use std::sync::Arc;

use ccodex_kernel::{Kernel, KernelError, RunTurnResult};
use ccodex_protocol::SessionId;
use ccodex_runtime::Runtime;
use ccodex_store::{SessionStore, SQLiteSessionStore, StoredTurn};

pub struct BootstrapHarness {
    pub store: Arc<SQLiteSessionStore>,
    pub kernel: Kernel,
    db_path: PathBuf,
}

impl BootstrapHarness {
    pub fn new(label: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("ccodex-testkit-{label}-{unique}.sqlite3"));
        let store = Arc::new(SQLiteSessionStore::new(&db_path).expect("store should initialize"));
        let runtime = Runtime::bootstrap();
        let kernel = Kernel::new(
            store.clone(),
            runtime.provider(),
            runtime.notifications(),
            runtime.tool_executor(),
            runtime.approval_engine(),
            runtime.tool_specs(),
        );

        Self {
            store,
            kernel,
            db_path,
        }
    }

    pub async fn run(&self, prompt: &str) -> Result<RunTurnResult, KernelError> {
        self.kernel
            .run_prompt(prompt.to_string(), std::env::current_dir().ok())
            .await
    }

    pub async fn resume(&self, session_id: &SessionId, prompt: &str) -> Result<RunTurnResult, KernelError> {
        self.kernel.resume_prompt(session_id, prompt.to_string()).await
    }

    pub async fn turns(&self, session_id: &SessionId) -> Vec<StoredTurn> {
        self.store
            .list_turns(session_id)
            .await
            .expect("turns should list")
    }
}

impl Drop for BootstrapHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.db_path);
    }
}

#[cfg(test)]
mod tests {
    use super::BootstrapHarness;

    #[tokio::test]
    async fn harness_runs_bootstrap_plan_scenario() {
        let harness = BootstrapHarness::new("plan");
        let result = harness
            .run("plan define protocol, implement runtime, build tui")
            .await
            .expect("plan run should succeed");

        assert_eq!(result.assistant_text, "Updated the current plan with 3 item(s).");
        assert_eq!(
            result
                .session
                .active_plan
                .as_ref()
                .expect("plan should exist")
                .items
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn harness_resumes_existing_session() {
        let harness = BootstrapHarness::new("resume");
        let first = harness.run("hello harness").await.expect("first run should succeed");
        let second = harness
            .resume(&first.session.id, "hello again")
            .await
            .expect("resume should succeed");

        let turns = harness.turns(&first.session.id).await;
        assert_eq!(first.session.id, second.session.id);
        assert_eq!(turns.len(), 2);
    }
}
