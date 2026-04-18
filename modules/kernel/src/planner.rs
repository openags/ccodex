use serde_json::Value;
use time::OffsetDateTime;

use ccodex_protocol::{PlanItem, PlanItemStatus, PlanMode, PlanState, Session};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) fn build_plan_state(
        &self,
        session: &Session,
        output: &Value,
    ) -> Result<PlanState, KernelError> {
        let summary = output
            .get("summary")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        let items = output
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|item| PlanItem {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("item")
                    .to_string(),
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_string(),
                notes: item
                    .get("notes")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                status: match item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending")
                {
                    "InProgress" | "in_progress" | "in-progress" => PlanItemStatus::InProgress,
                    "Completed" | "completed" => PlanItemStatus::Completed,
                    "Blocked" | "blocked" => PlanItemStatus::Blocked,
                    _ => PlanItemStatus::Pending,
                },
            })
            .collect();

        Ok(PlanState {
            id: session
                .active_plan
                .as_ref()
                .map(|plan| plan.id.clone())
                .unwrap_or_default(),
            session_id: session.id.clone(),
            mode: PlanMode::Active,
            summary,
            items,
            updated_at: OffsetDateTime::now_utc(),
        })
    }
}
