use serde_json::Value;

use ccodex_protocol::{AskUserChoice, AskUserPrompt, ItemId, ToolCall};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) fn build_ask_user_prompt(
        &self,
        call: &ToolCall,
    ) -> Result<AskUserPrompt, KernelError> {
        let title = call
            .input
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Question")
            .to_string();
        let message = call
            .input
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(title.as_str())
            .to_string();
        let allow_freeform = call
            .input
            .get("allow_freeform")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let choices = call
            .input
            .get("choices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|choice| AskUserChoice {
                id: choice
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("choice")
                    .to_string(),
                label: choice
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("Choice")
                    .to_string(),
                description: choice
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            })
            .collect();

        Ok(AskUserPrompt {
            item_id: ItemId::new(),
            title,
            message,
            choices,
            allow_freeform,
        })
    }
}
