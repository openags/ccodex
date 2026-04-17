use async_trait::async_trait;

use ccodex_protocol::{ItemPayload, SessionId};

use crate::{traits::{SessionStore, StoreError}, TranscriptExporter};

#[derive(Debug, Clone)]
pub struct MarkdownTranscriptExporter {
    store: crate::SQLiteSessionStore,
}

impl MarkdownTranscriptExporter {
    pub fn new(store: crate::SQLiteSessionStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TranscriptExporter for MarkdownTranscriptExporter {
    async fn export_session(&self, session_id: &SessionId) -> Result<String, StoreError> {
        let session = self.store.get_session(session_id).await?;
        let turns = self.store.list_turns(session_id).await?;

        let mut out = vec![format!("# Session {}", session.id)];
        if let Some(title) = session.title {
            out.push(format!("\n## Title\n\n{}", title));
        }

        for stored_turn in turns {
            out.push(format!("\n## Turn {}", stored_turn.turn.id));
            for item in stored_turn.items {
                match item.payload {
                    ItemPayload::UserMessage { content } => out.push(format!("- User: {}", content)),
                    ItemPayload::AssistantMessageDelta { content } => out.push(format!("- Assistant: {}", content)),
                    ItemPayload::ReasoningDelta { content } => out.push(format!("- Reasoning: {}", content)),
                    ItemPayload::ToolCallStarted { call } => {
                        out.push(format!("- ToolCall: {} {}", call.tool_name, call.id));
                    }
                    ItemPayload::ToolCallFinished { result } => {
                        out.push(format!("- ToolResult: {} error={}", result.tool_call_id, result.is_error));
                    }
                    ItemPayload::ApprovalRequested { request } => {
                        out.push(format!("- ApprovalRequested: {}", request.summary));
                    }
                    ItemPayload::ApprovalResolved { response } => {
                        out.push(format!("- ApprovalResolved: {:?}", response.decision));
                    }
                    ItemPayload::AskUserRequested { prompt } => {
                        out.push(format!("- AskUser: {}", prompt.title));
                    }
                    ItemPayload::AskUserResolved { response } => {
                        out.push(format!("- AskUserResponse: {:?}", response.selected_choice_id));
                    }
                    ItemPayload::PlanEntered { .. }
                    | ItemPayload::PlanUpdated { .. }
                    | ItemPayload::PlanExited { .. }
                    | ItemPayload::Warning { .. }
                    | ItemPayload::Error { .. }
                    | ItemPayload::SystemEvent { .. }
                    | ItemPayload::ToolCallDelta { .. } => {
                        out.push("- Event".to_string());
                    }
                }
            }
        }

        Ok(out.join("\n"))
    }
}
