use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use ccodex_protocol::{ItemPayload, ProtocolEvent, Session, Turn};
use ccodex_store::StoredTurn;

use crate::{Kernel, KernelError};

const COMPACTION_TURN_THRESHOLD: usize = 6;
const COMPACTION_KEEP_RECENT_TURNS: usize = 3;
const COMPACTION_SUMMARY_METADATA_KEY: &str = "compaction_summary";
const COMPACTED_TURN_COUNT_METADATA_KEY: &str = "compacted_turn_count";
const LAST_COMPACTED_AT_METADATA_KEY: &str = "last_compacted_at";
const SESSION_COMPACTED_EVENT_NAME: &str = "session_compacted";

impl Kernel {
    pub(crate) async fn maybe_compact_session(
        &self,
        session: &mut Session,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
    ) -> Result<(), KernelError> {
        let turns = self.store.list_turns(&session.id).await?;
        let Some(target_compacted) = target_compacted_turn_count(turns.len()) else {
            return Ok(());
        };

        if target_compacted <= already_compacted_turn_count(session) {
            return Ok(());
        }

        let summary = build_compaction_summary(&turns, target_compacted);
        let now = OffsetDateTime::now_utc();
        apply_compaction_metadata(session, &summary, target_compacted, now);

        self.store.update_session(session).await?;
        self.emit(events, ProtocolEvent::SessionUpdated(session.clone()))
            .await?;
        self.append_item(
            turn,
            events,
            ItemPayload::SystemEvent {
                name: SESSION_COMPACTED_EVENT_NAME.to_string(),
                payload: json!({
                    COMPACTED_TURN_COUNT_METADATA_KEY: target_compacted,
                    "retained_turn_count": turns.len().saturating_sub(target_compacted),
                    "first_compacted_turn_id": turns.first().map(|stored| stored.turn.id.to_string()),
                    "last_compacted_turn_id": turns
                        .get(target_compacted.saturating_sub(1))
                        .map(|stored| stored.turn.id.to_string()),
                    LAST_COMPACTED_AT_METADATA_KEY: session
                        .metadata
                        .get(LAST_COMPACTED_AT_METADATA_KEY)
                        .cloned()
                        .unwrap_or(Value::Null),
                    "summary": summary,
                }),
            },
        )
        .await?;

        Ok(())
    }
}

fn target_compacted_turn_count(total_turns: usize) -> Option<usize> {
    (total_turns > COMPACTION_TURN_THRESHOLD)
        .then(|| total_turns.saturating_sub(COMPACTION_KEEP_RECENT_TURNS))
}

fn already_compacted_turn_count(session: &Session) -> usize {
    session
        .metadata
        .get(COMPACTED_TURN_COUNT_METADATA_KEY)
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}

fn build_compaction_summary(turns: &[StoredTurn], compacted_turn_count: usize) -> String {
    let compacted = turns.iter().take(compacted_turn_count).collect::<Vec<_>>();
    let remaining = turns.len().saturating_sub(compacted_turn_count);
    let first_turn_id = compacted.first().map(|stored| stored.turn.id.to_string());
    let last_turn_id = compacted.last().map(|stored| stored.turn.id.to_string());

    let mut lines = vec![format!(
        "compacted {} turn(s); kept {} recent turn(s); range={}..{}",
        compacted_turn_count,
        remaining,
        first_turn_id.as_deref().unwrap_or("-"),
        last_turn_id.as_deref().unwrap_or("-")
    )];
    lines.extend(compacted.into_iter().map(summarize_turn));
    lines.join("\n")
}

fn summarize_turn(stored: &StoredTurn) -> String {
    let mut chunk = vec![format!("turn {}", stored.turn.id)];
    chunk.extend(
        stored
            .items
            .iter()
            .filter_map(|item| summarize_item(&item.payload)),
    );
    chunk.join(" | ")
}

fn summarize_item(payload: &ItemPayload) -> Option<String> {
    match payload {
        ItemPayload::UserMessage { content } => Some(format!("user: {content}")),
        ItemPayload::AssistantMessageDelta { content } => Some(format!("assistant: {content}")),
        ItemPayload::ToolCallStarted { call } => Some(format!("tool-start: {}", call.tool_name)),
        ItemPayload::ToolCallFinished { result } => {
            Some(format!("tool-finished: {}", result.tool_call_id))
        }
        ItemPayload::PlanEntered { plan } | ItemPayload::PlanUpdated { plan } => {
            Some(format!("plan-items: {}", plan.items.len()))
        }
        ItemPayload::PlanExited { .. } => Some("plan-exited".to_string()),
        ItemPayload::AskUserRequested { prompt } => Some(format!("ask-user: {}", prompt.title)),
        ItemPayload::AskUserResolved { response } => Some(format!(
            "ask-user-response: {}",
            response.selected_choice_id.as_deref().unwrap_or("<none>")
        )),
        ItemPayload::ApprovalRequested { request } => {
            Some(format!("approval: {}", request.summary))
        }
        ItemPayload::ApprovalResolved { response } => {
            Some(format!("approval-result: {:?}", response.decision))
        }
        ItemPayload::Warning { message, .. } => Some(format!("warning: {message}")),
        ItemPayload::Error { message, .. } => Some(format!("error: {message}")),
        ItemPayload::SystemEvent { name, .. } => Some(format!("system: {name}")),
        ItemPayload::ReasoningDelta { .. } | ItemPayload::ToolCallDelta { .. } => None,
    }
}

fn apply_compaction_metadata(
    session: &mut Session,
    summary: &str,
    compacted_turn_count: usize,
    now: OffsetDateTime,
) {
    session.metadata.insert(
        COMPACTION_SUMMARY_METADATA_KEY.to_string(),
        Value::String(summary.to_owned()),
    );
    session.metadata.insert(
        COMPACTED_TURN_COUNT_METADATA_KEY.to_string(),
        json!(compacted_turn_count),
    );
    session.metadata.insert(
        LAST_COMPACTED_AT_METADATA_KEY.to_string(),
        Value::String(format_timestamp(now)),
    );
    session.updated_at = now;
}

fn format_timestamp(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.unix_timestamp().to_string())
}
