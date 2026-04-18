use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{Mutex, broadcast, oneshot};

use ccodex_brand::project_state_db_file;
use ccodex_kernel::Kernel;
use ccodex_protocol::{
    ApprovalDecision, ApprovalEnginePort, ApprovalRequest, ApprovalResponse, AskUserPrompt,
    AskUserResponse, ItemId, LocalServerResponseBody, NotificationPort, PortError, ProtocolEvent,
};
use ccodex_runtime::{ApprovalPolicy, Runtime};
use ccodex_store::SQLiteSessionStore;

pub(crate) struct ServerState {
    pub(crate) workspace_root: std::path::PathBuf,
    pub(crate) store: Arc<SQLiteSessionStore>,
    pub(crate) kernel: Arc<Kernel>,
    pub(crate) events: broadcast::Sender<ProtocolEvent>,
    pub(crate) interactions: Arc<SharedInteractionEngine>,
}

#[derive(Clone)]
struct LocalServerNotificationPort {
    events: broadcast::Sender<ProtocolEvent>,
}

#[derive(Debug)]
pub(crate) struct SharedInteractionEngine {
    policy: ApprovalPolicy,
    pending_approvals: Mutex<HashMap<ItemId, PendingApproval>>,
    pending_ask_user: Mutex<HashMap<ItemId, PendingAskUser>>,
}

#[derive(Debug)]
struct PendingApproval {
    request: ApprovalRequest,
    sender: oneshot::Sender<ApprovalResponse>,
}

#[derive(Debug)]
struct PendingAskUser {
    prompt: AskUserPrompt,
    sender: oneshot::Sender<AskUserResponse>,
}

impl SharedInteractionEngine {
    fn new(policy: ApprovalPolicy) -> Self {
        Self {
            policy,
            pending_approvals: Mutex::new(HashMap::new()),
            pending_ask_user: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn resolve_approval(&self, response: ApprovalResponse) -> bool {
        let mut pending = self.pending_approvals.lock().await;
        pending
            .remove(&response.request_item_id)
            .map(|pending| pending.sender.send(response).is_ok())
            .unwrap_or(false)
    }

    pub(crate) async fn resolve_ask_user(&self, response: AskUserResponse) -> bool {
        let mut pending = self.pending_ask_user.lock().await;
        pending
            .remove(&response.request_item_id)
            .map(|pending| pending.sender.send(response).is_ok())
            .unwrap_or(false)
    }

    pub(crate) async fn list_pending_approvals(&self) -> Vec<ApprovalRequest> {
        self.pending_approvals
            .lock()
            .await
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }

    pub(crate) async fn list_pending_ask_user(&self) -> Vec<AskUserPrompt> {
        self.pending_ask_user
            .lock()
            .await
            .values()
            .map(|pending| pending.prompt.clone())
            .collect()
    }
}

#[async_trait]
impl ApprovalEnginePort for SharedInteractionEngine {
    async fn request_approval(
        &self,
        request: ApprovalRequest,
    ) -> Result<ApprovalResponse, PortError> {
        match self.policy {
            ApprovalPolicy::AlwaysApprove => Ok(ApprovalResponse::new(
                request.item_id,
                ApprovalDecision::Approved,
                None,
                Some(ccodex_protocol::ApprovalReasonCode::PolicyAlwaysApprove),
            )),
            ApprovalPolicy::NeverApprove => Ok(ApprovalResponse::new(
                request.item_id,
                ApprovalDecision::Rejected,
                None,
                Some(ccodex_protocol::ApprovalReasonCode::PolicyNeverApprove),
            )),
            ApprovalPolicy::Ask => {
                let request_item_id = request.item_id.clone();
                let (sender, receiver) = oneshot::channel();
                self.pending_approvals
                    .lock()
                    .await
                    .insert(request_item_id.clone(), PendingApproval { request, sender });
                match receiver.await {
                    Ok(response) => Ok(response),
                    Err(_) => Ok(ApprovalResponse::new(
                        request_item_id,
                        ApprovalDecision::Cancelled,
                        Some("approval resolution channel closed".to_string()),
                        Some(ccodex_protocol::ApprovalReasonCode::InteractiveCancelled),
                    )),
                }
            }
        }
    }

    async fn request_user_input(
        &self,
        prompt: AskUserPrompt,
    ) -> Result<AskUserResponse, PortError> {
        match self.policy {
            ApprovalPolicy::AlwaysApprove => Ok(AskUserResponse {
                request_item_id: prompt.item_id,
                selected_choice_id: prompt.choices.first().map(|choice| choice.id.clone()),
                freeform_text: None,
            }),
            ApprovalPolicy::NeverApprove => Ok(AskUserResponse {
                request_item_id: prompt.item_id,
                selected_choice_id: None,
                freeform_text: None,
            }),
            ApprovalPolicy::Ask => {
                let request_item_id = prompt.item_id.clone();
                let (sender, receiver) = oneshot::channel();
                self.pending_ask_user
                    .lock()
                    .await
                    .insert(request_item_id.clone(), PendingAskUser { prompt, sender });
                match receiver.await {
                    Ok(response) => Ok(response),
                    Err(_) => Ok(AskUserResponse {
                        request_item_id,
                        selected_choice_id: None,
                        freeform_text: None,
                    }),
                }
            }
        }
    }
}

#[async_trait]
impl NotificationPort for LocalServerNotificationPort {
    async fn notify_event(&self, event: &ProtocolEvent) -> Result<(), PortError> {
        let _ = self.events.send(event.clone());
        Ok(())
    }
}

impl ServerState {
    pub(crate) fn bootstrap() -> Result<Self> {
        let workspace_root = std::env::current_dir()?;
        Self::for_workspace(workspace_root)
    }

    pub(crate) fn for_workspace(workspace_root: std::path::PathBuf) -> Result<Self> {
        let runtime = Runtime::for_workspace(workspace_root.clone())?;
        Self::for_workspace_with_runtime(workspace_root, runtime)
    }

    pub(crate) fn for_workspace_with_runtime(
        workspace_root: std::path::PathBuf,
        runtime: Runtime,
    ) -> Result<Self> {
        let db_path = project_state_db_file(&workspace_root);
        let store = Arc::new(SQLiteSessionStore::new(&db_path)?);
        let (events, _) = broadcast::channel(512);
        let interactions = Arc::new(SharedInteractionEngine::new(
            runtime.config().approval_policy.clone(),
        ));
        let kernel = Arc::new(Kernel::new(
            store.clone(),
            runtime.provider(),
            Arc::new(LocalServerNotificationPort {
                events: events.clone(),
            }),
            runtime.tool_executor(),
            interactions.clone(),
            runtime.tool_specs(),
        ));

        Ok(Self {
            workspace_root,
            store,
            kernel,
            events,
            interactions,
        })
    }
}

pub(crate) fn error_body(
    code: impl Into<String>,
    message: impl Into<String>,
) -> LocalServerResponseBody {
    LocalServerResponseBody::Error {
        code: code.into(),
        message: message.into(),
    }
}
