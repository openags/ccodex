use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalPolicy {
    AlwaysApprove,
    NeverApprove,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub workspace_root: PathBuf,
    pub approval_policy: ApprovalPolicy,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            approval_policy: ApprovalPolicy::AlwaysApprove,
        }
    }
}
