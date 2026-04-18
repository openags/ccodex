use std::sync::mpsc::Receiver;

use anyhow::Result;

use ccodex_protocol::{
    ApprovalDecision, ApprovalRequest, AskUserPrompt, ItemId, ItemPayload, ProtocolEvent, SessionId,
};
use ccodex_tui::{
    fork_session, get_session, get_turns, list_extensions, list_pending_interactions,
    list_sessions, ping, resolve_approval, resolve_ask_user, resume_prompt, run_prompt,
    subscribe_events,
};

use crate::render::{
    default_output, default_plan, format_pending_interactions, format_plan, render_stored_turns,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusPanel {
    Input,
    Sessions,
    Extensions,
    Pending,
    Plan,
}

#[derive(Debug, Clone)]
pub(crate) enum PendingInteraction {
    Approval(ApprovalRequest),
    AskUser(AskUserPrompt),
}

pub(crate) struct App {
    pub(crate) status: String,
    pub(crate) input: String,
    pub(crate) output: Vec<String>,
    pub(crate) sessions: Vec<String>,
    pub(crate) session_ids: Vec<SessionId>,
    pub(crate) extensions: Vec<String>,
    pub(crate) pending: Vec<String>,
    pub(crate) pending_interactions: Vec<PendingInteraction>,
    pub(crate) plan: Vec<String>,
    pub(crate) current_session: Option<SessionId>,
    pub(crate) event_rx: Option<Receiver<ProtocolEvent>>,
    pub(crate) focus: FocusPanel,
    pub(crate) selected_session: usize,
    pub(crate) selected_extension: usize,
    pub(crate) selected_pending: usize,
    pub(crate) selected_plan: usize,
    pub(crate) raise_pending_overlay: bool,
}

impl App {
    pub(crate) fn bootstrap() -> Self {
        let mut status = match ping() {
            Ok(status) => format!("connected: {status}"),
            Err(err) => format!("server unavailable: {err}"),
        };

        let recent_sessions = list_sessions(Some(10)).unwrap_or_default();
        let current_session = recent_sessions.first().map(|session| session.id.clone());
        let sessions = recent_sessions
            .iter()
            .map(|session| {
                format!(
                    "{}  {}",
                    session.id,
                    session
                        .title
                        .clone()
                        .unwrap_or_else(|| "Untitled".to_string())
                )
            })
            .collect::<Vec<_>>();
        let session_ids = recent_sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        let extensions = list_extensions()
            .map(|items| {
                items
                    .into_iter()
                    .map(|manifest| format!("{:?}  {}", manifest.kind, manifest.name))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let output = current_session
            .as_ref()
            .and_then(|session_id| get_turns(session_id.clone()).ok())
            .filter(|turns| !turns.is_empty())
            .map(|turns| render_stored_turns(&turns))
            .unwrap_or_else(default_output);
        let event_rx = match subscribe_events() {
            Ok(rx) => Some(rx),
            Err(err) => {
                status = format!("{status} | live updates unavailable: {err}");
                None
            }
        };
        let (pending, pending_interactions) = list_pending_interactions()
            .map(|(approvals, ask_user)| {
                (
                    format_pending_interactions(&approvals, &ask_user),
                    flatten_pending_interactions(approvals, ask_user),
                )
            })
            .unwrap_or_else(|_| (vec!["no pending interactions".to_string()], Vec::new()));
        let has_pending_interactions = !pending_interactions.is_empty();
        let selected_pending = if has_pending_interactions {
            pending.len().saturating_sub(1)
        } else {
            0
        };
        let plan = current_session
            .as_ref()
            .and_then(|session_id| get_session(session_id.clone()).ok())
            .map(|session| format_plan(&session))
            .unwrap_or_else(default_plan);

        Self {
            status,
            input: String::new(),
            output,
            sessions,
            session_ids,
            extensions,
            pending,
            pending_interactions,
            plan,
            current_session,
            event_rx,
            focus: if has_pending_interactions {
                FocusPanel::Pending
            } else {
                FocusPanel::Input
            },
            selected_session: 0,
            selected_extension: 0,
            selected_pending,
            selected_plan: 0,
            raise_pending_overlay: false,
        }
    }

    pub(crate) fn submit(&mut self) {
        let raw = self.input.trim().to_string();
        if raw.is_empty() {
            return;
        }

        self.output.push(format!("> {raw}"));

        let result = if raw == "/new" {
            self.start_new_session();
            self.input.clear();
            return;
        } else if let Some(prompt) = raw.strip_prefix("/new ") {
            self.start_new_session();
            run_prompt(prompt.trim().to_string())
        } else if let Some(rest) = raw.strip_prefix("/resume ") {
            let mut parts = rest.splitn(2, ' ');
            let session_id = parts.next().unwrap_or_default().trim().to_string();
            let prompt = parts.next().unwrap_or_default().trim().to_string();
            if session_id.is_empty() || prompt.is_empty() {
                Err(anyhow::anyhow!("usage: /resume <session_id> <prompt>"))
            } else {
                resume_prompt(SessionId(session_id), prompt)
            }
        } else if let Some(rest) = raw.strip_prefix("/approve ") {
            match handle_approve_command(rest) {
                Ok(message) => {
                    self.status = message;
                    if let Some(session_id) = self.current_session.clone() {
                        let _ = self.reload_transcript(&session_id);
                    }
                    self.refresh_sessions();
                    self.status = format!(
                        "{} | pending {}",
                        self.status,
                        self.pending
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "no pending interactions".to_string())
                    );
                    self.input.clear();
                    return;
                }
                Err(err) => Err(err),
            }
        } else if let Some(rest) = raw.strip_prefix("/answer ") {
            match handle_answer_command(rest) {
                Ok(message) => {
                    self.status = message;
                    if let Some(session_id) = self.current_session.clone() {
                        let _ = self.reload_transcript(&session_id);
                    }
                    self.refresh_sessions();
                    self.status = format!(
                        "{} | pending {}",
                        self.status,
                        self.pending
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "no pending interactions".to_string())
                    );
                    self.input.clear();
                    return;
                }
                Err(err) => Err(err),
            }
        } else if let Some(rest) = raw.strip_prefix("/show ") {
            let session_id = rest.trim();
            if session_id.is_empty() {
                Err(anyhow::anyhow!("usage: /show <session_id>"))
            } else {
                self.load_session(SessionId(session_id.to_string()))
                    .map(|_| {
                        (
                            "loaded session transcript".to_string(),
                            SessionId(session_id.to_string()),
                        )
                    })
            }
        } else if let Some(rest) = raw.strip_prefix("/fork ") {
            let session_id = rest.trim();
            if session_id.is_empty() {
                Err(anyhow::anyhow!("usage: /fork <session_id>"))
            } else {
                match fork_session(SessionId(session_id.to_string())) {
                    Ok(session) => {
                        let forked_id = session.id.clone();
                        match self.load_session(forked_id.clone()) {
                            Ok(()) => Ok((format!("forked session {}", forked_id), forked_id)),
                            Err(err) => Err(err),
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        } else {
            match self.current_session.clone() {
                Some(session_id) => resume_prompt(session_id, raw.clone()),
                None => run_prompt(raw.clone()),
            }
        };

        match result {
            Ok((text, session_id)) => {
                self.current_session = Some(session_id.clone());
                if let Err(err) = self.reload_transcript(&session_id) {
                    self.output.push(text);
                    self.status = format!(
                        "session {} loaded, transcript refresh failed: {err}",
                        session_id
                    );
                } else {
                    self.status = format!("ok: session {}", session_id);
                }
                self.refresh_sessions();
            }
            Err(err) => {
                self.output.push(format!("error: {err}"));
                self.status = format!("error: {err}");
            }
        }

        self.input.clear();
    }

    pub(crate) fn activate(&mut self) {
        match self.focus {
            FocusPanel::Input => self.submit(),
            FocusPanel::Sessions => self.activate_selected_session(),
            FocusPanel::Pending => {
                if let Err(err) = self.activate_selected_pending() {
                    self.status = format!("error: {err}");
                }
            }
            FocusPanel::Extensions | FocusPanel::Plan => {}
        }
    }

    pub(crate) fn has_overlay(&self) -> bool {
        self.focus != FocusPanel::Input
    }

    pub(crate) fn close_overlay(&mut self) {
        self.focus = FocusPanel::Input;
    }

    pub(crate) fn toggle_panel(&mut self, panel: FocusPanel) {
        if panel == FocusPanel::Input || self.focus == panel {
            self.close_overlay();
        } else {
            self.focus = panel;
            if panel == FocusPanel::Pending {
                self.raise_pending_overlay = false;
            }
        }
    }

    pub(crate) fn begin_typing(&mut self, ch: char) {
        self.focus = FocusPanel::Input;
        self.input.push(ch);
    }

    pub(crate) fn refresh_sessions(&mut self) {
        if let Ok(items) = list_sessions(Some(10)) {
            self.session_ids = items.iter().map(|session| session.id.clone()).collect();
            self.sessions = items
                .into_iter()
                .map(|session| {
                    format!(
                        "{}  {}",
                        session.id,
                        session.title.unwrap_or_else(|| "Untitled".to_string())
                    )
                })
                .collect();
        }

        if let Some(current_session) = self.current_session.as_ref()
            && let Some(index) = self
                .session_ids
                .iter()
                .position(|session_id| session_id == current_session)
        {
            self.selected_session = index;
        }

        if let Ok(items) = list_extensions() {
            self.extensions = items
                .into_iter()
                .map(|manifest| format!("{:?}  {}", manifest.kind, manifest.name))
                .collect();
        }

        if let Ok((approvals, ask_user)) = list_pending_interactions() {
            self.pending_interactions =
                flatten_pending_interactions(approvals.clone(), ask_user.clone());
            self.pending = format_pending_interactions(&approvals, &ask_user);
        }

        if let Some(session_id) = self.current_session.clone()
            && let Ok(session) = get_session(session_id)
        {
            self.plan = format_plan(&session);
        }

        self.selected_session = clamp_selection(self.selected_session, self.session_ids.len());
        self.selected_extension = clamp_selection(self.selected_extension, self.extensions.len());
        self.selected_pending =
            clamp_selection(self.selected_pending, self.pending_interactions.len());
        self.selected_plan = clamp_selection(self.selected_plan, self.plan.len());

        if self.raise_pending_overlay && !self.pending_interactions.is_empty() {
            self.focus = FocusPanel::Pending;
            self.selected_pending = self.pending_interactions.len().saturating_sub(1);
            self.raise_pending_overlay = false;
        }
    }

    pub(crate) fn load_session(&mut self, session_id: SessionId) -> Result<()> {
        self.current_session = Some(session_id.clone());
        self.reload_transcript(&session_id)?;
        self.status = format!("loaded session {}", session_id);
        self.focus = FocusPanel::Input;
        self.refresh_sessions();
        Ok(())
    }

    pub(crate) fn reload_transcript(&mut self, session_id: &SessionId) -> Result<()> {
        let turns = get_turns(session_id.clone())?;
        self.output = if turns.is_empty() {
            let mut lines = default_output();
            lines.push(format!("session {} has no turns yet", session_id));
            lines
        } else {
            render_stored_turns(&turns)
        };
        Ok(())
    }

    pub(crate) fn drain_server_events(&mut self) {
        let Some(receiver) = &self.event_rx else {
            return;
        };

        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }

        if events.is_empty() {
            return;
        }

        for event in events {
            self.update_status_from_event(&event);
        }

        if let Some(session_id) = self.current_session.clone() {
            let _ = self.reload_transcript(&session_id);
        }
        self.refresh_sessions();
    }

    pub(crate) fn selected_session_label(&self) -> Option<&str> {
        self.sessions.get(self.selected_session).map(String::as_str)
    }

    pub(crate) fn selected_extension_label(&self) -> Option<&str> {
        self.extensions
            .get(self.selected_extension)
            .map(String::as_str)
    }

    pub(crate) fn selected_plan_line(&self) -> Option<&str> {
        self.plan.get(self.selected_plan).map(String::as_str)
    }

    pub(crate) fn selected_pending_label(&self) -> Option<&str> {
        self.pending.get(self.selected_pending).map(String::as_str)
    }

    pub(crate) fn focus_next(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Input => FocusPanel::Sessions,
            FocusPanel::Sessions => FocusPanel::Extensions,
            FocusPanel::Extensions => FocusPanel::Pending,
            FocusPanel::Pending => FocusPanel::Plan,
            FocusPanel::Plan => FocusPanel::Input,
        };
    }

    pub(crate) fn focus_prev(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Input => FocusPanel::Plan,
            FocusPanel::Sessions => FocusPanel::Input,
            FocusPanel::Extensions => FocusPanel::Sessions,
            FocusPanel::Pending => FocusPanel::Extensions,
            FocusPanel::Plan => FocusPanel::Pending,
        };
    }

    pub(crate) fn move_selection_next(&mut self) {
        self.move_selection(1);
    }

    pub(crate) fn move_selection_prev(&mut self) {
        self.move_selection(-1);
    }

    pub(crate) fn approve_selected_pending(&mut self) {
        if let Err(err) = self.resolve_selected_pending_approval(ApprovalDecision::Approved) {
            self.status = format!("error: {err}");
        }
    }

    pub(crate) fn reject_selected_pending(&mut self) {
        if let Err(err) = self.resolve_selected_pending_approval(ApprovalDecision::Rejected) {
            self.status = format!("error: {err}");
        }
    }

    pub(crate) fn cancel_selected_pending(&mut self) {
        if let Err(err) = self.resolve_selected_pending_approval(ApprovalDecision::Cancelled) {
            self.status = format!("error: {err}");
        }
    }

    pub(crate) fn answer_selected_pending_choice(&mut self, choice_index: usize) {
        if let Err(err) = self.resolve_selected_pending_ask_user(choice_index) {
            self.status = format!("error: {err}");
        }
    }

    pub(crate) fn prepare_selected_pending_freeform(&mut self) {
        let Some(PendingInteraction::AskUser(prompt)) =
            self.pending_interactions.get(self.selected_pending)
        else {
            self.status = "selected pending item is not an ask-user prompt".to_string();
            return;
        };
        if !prompt.allow_freeform {
            self.status = format!("ask-user {} does not allow freeform input", prompt.item_id);
            return;
        }
        self.focus = FocusPanel::Input;
        self.input = format!("/answer {} text:", prompt.item_id);
        self.status = format!("freeform answer prepared for {}", prompt.item_id);
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            FocusPanel::Input => {}
            FocusPanel::Sessions => {
                self.selected_session =
                    next_index(self.selected_session, self.session_ids.len(), delta);
            }
            FocusPanel::Extensions => {
                self.selected_extension =
                    next_index(self.selected_extension, self.extensions.len(), delta);
            }
            FocusPanel::Pending => {
                self.selected_pending = next_index(
                    self.selected_pending,
                    self.pending_interactions.len(),
                    delta,
                );
            }
            FocusPanel::Plan => {
                self.selected_plan = next_index(self.selected_plan, self.plan.len(), delta);
            }
        }
    }

    fn activate_selected_session(&mut self) {
        let Some(session_id) = self.session_ids.get(self.selected_session).cloned() else {
            self.status = "no session selected".to_string();
            return;
        };
        if let Err(err) = self.load_session(session_id.clone()) {
            self.status = format!("error: {err}");
        } else {
            self.status = format!("loaded session {}", session_id);
        }
    }

    pub(crate) fn start_new_session(&mut self) {
        self.current_session = None;
        self.output = default_output();
        self.plan = default_plan();
        self.focus = FocusPanel::Input;
        self.status = "started new session draft".to_string();
    }

    pub(crate) fn fork_selected_session(&mut self) {
        let Some(session_id) = self.session_ids.get(self.selected_session).cloned() else {
            self.status = "no session selected".to_string();
            return;
        };
        match fork_session(session_id.clone()) {
            Ok(session) => {
                let forked_id = session.id.clone();
                if let Err(err) = self.load_session(forked_id.clone()) {
                    self.status = format!("forked {forked_id}, but failed to load: {err}");
                } else {
                    self.status = format!("forked session {}", forked_id);
                }
            }
            Err(err) => {
                self.status = format!("error: {err}");
            }
        }
    }

    fn activate_selected_pending(&mut self) -> Result<()> {
        let Some(selected) = self
            .pending_interactions
            .get(self.selected_pending)
            .cloned()
        else {
            self.status = "no pending interaction selected".to_string();
            return Ok(());
        };

        match selected {
            PendingInteraction::Approval(_) => {
                self.resolve_selected_pending_approval(ApprovalDecision::Approved)
            }
            PendingInteraction::AskUser(prompt) => {
                if let Some(choice) = prompt.choices.first() {
                    let request_item_id =
                        resolve_ask_user(prompt.item_id.clone(), Some(choice.id.clone()), None)?;
                    self.status = format!("resolved ask-user {}", request_item_id);
                    self.after_pending_resolution();
                    Ok(())
                } else if prompt.allow_freeform {
                    self.focus = FocusPanel::Input;
                    self.input = format!("/answer {} text:", prompt.item_id);
                    self.status = format!("freeform answer prepared for {}", prompt.item_id);
                    Ok(())
                } else {
                    self.status = format!("ask-user {} has no selectable choices", prompt.item_id);
                    Ok(())
                }
            }
        }
    }

    fn resolve_selected_pending_approval(&mut self, decision: ApprovalDecision) -> Result<()> {
        let Some(PendingInteraction::Approval(request)) = self
            .pending_interactions
            .get(self.selected_pending)
            .cloned()
        else {
            self.status = "selected pending item is not an approval".to_string();
            return Ok(());
        };
        let request_item_id = resolve_approval(request.item_id.clone(), decision.clone(), None)?;
        self.status = format!("resolved approval {} {:?}", request_item_id, decision);
        self.after_pending_resolution();
        Ok(())
    }

    fn resolve_selected_pending_ask_user(&mut self, choice_index: usize) -> Result<()> {
        let Some(PendingInteraction::AskUser(prompt)) = self
            .pending_interactions
            .get(self.selected_pending)
            .cloned()
        else {
            self.status = "selected pending item is not an ask-user prompt".to_string();
            return Ok(());
        };
        let Some(choice) = prompt.choices.get(choice_index).cloned() else {
            self.status = format!(
                "ask-user {} has no choice {}",
                prompt.item_id,
                choice_index + 1
            );
            return Ok(());
        };
        let request_item_id =
            resolve_ask_user(prompt.item_id.clone(), Some(choice.id.clone()), None)?;
        self.status = format!("resolved ask-user {} -> {}", request_item_id, choice.label);
        self.after_pending_resolution();
        Ok(())
    }

    fn after_pending_resolution(&mut self) {
        if let Some(session_id) = self.current_session.clone() {
            let _ = self.reload_transcript(&session_id);
        }
        self.refresh_sessions();
        if self.pending_interactions.is_empty() {
            self.focus = FocusPanel::Input;
        } else {
            self.focus = FocusPanel::Pending;
            self.selected_pending =
                clamp_selection(self.selected_pending, self.pending_interactions.len());
        }
    }

    fn update_status_from_event(&mut self, event: &ProtocolEvent) {
        match event {
            ProtocolEvent::SessionCreated(session) => {
                self.status = format!("session {} created", session.id);
            }
            ProtocolEvent::SessionUpdated(session) => {
                self.status = format!("session {} updated", session.id);
            }
            ProtocolEvent::TurnStarted(turn) => {
                self.status = format!("turn {} running", turn.id);
            }
            ProtocolEvent::TurnUpdated(turn) => {
                self.status = format!("turn {} updated", turn.id);
            }
            ProtocolEvent::TurnFinished(turn) => {
                self.status = format!("turn {} completed", turn.id);
            }
            ProtocolEvent::ItemAppended(item) => match &item.payload {
                ItemPayload::ToolCallStarted { call } => {
                    self.status = format!("tool {} started", call.tool_name);
                }
                ItemPayload::ToolCallDelta { delta, .. } => {
                    if let Some(phase) = delta.get("phase").and_then(serde_json::Value::as_str) {
                        let tool_name = delta
                            .get("tool_name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("tool");
                        self.status = match phase {
                            "approval_assessed" => format!("approval assessed for {tool_name}"),
                            "waiting_for_user" => format!("waiting for user input for {tool_name}"),
                            "delegating" => format!("delegating to subagent from {tool_name}"),
                            "executing" => format!("executing {tool_name}"),
                            "stream" => format!("streaming output from {tool_name}"),
                            "completed" => format!("{tool_name} completed"),
                            "failed" => format!("{tool_name} failed"),
                            "cancelled" => format!("{tool_name} cancelled"),
                            _ => format!("{tool_name}: {phase}"),
                        };
                    }
                }
                ItemPayload::ToolCallFinished { result } => {
                    self.status = format!(
                        "tool {} {}",
                        result.tool_call_id,
                        if result.is_error {
                            "failed"
                        } else {
                            "finished"
                        }
                    );
                }
                ItemPayload::ApprovalRequested { request } => {
                    self.status = format!(
                        "approval required: {:?}/{:?} {}",
                        request.kind, request.risk, request.summary
                    );
                    self.raise_pending_overlay = true;
                }
                ItemPayload::ApprovalResolved { response } => {
                    self.status = format!(
                        "approval {:?} {}",
                        response.decision, response.request_item_id
                    );
                }
                ItemPayload::AskUserRequested { prompt } => {
                    self.status = format!("question: {}", prompt.title);
                    self.raise_pending_overlay = true;
                }
                ItemPayload::AskUserResolved { response } => {
                    self.status = format!("answer captured for {}", response.request_item_id);
                }
                ItemPayload::PlanEntered { .. } => {
                    self.status = "plan mode entered".to_string();
                }
                ItemPayload::PlanUpdated { .. } => {
                    self.status = "plan updated".to_string();
                }
                ItemPayload::PlanExited { .. } => {
                    self.status = "plan mode exited".to_string();
                }
                ItemPayload::Warning { code, message } => {
                    self.status = format!("warning[{code}] {message}");
                }
                ItemPayload::Error { code, message } => {
                    self.status = format!("error[{code}] {message}");
                }
                ItemPayload::SystemEvent { name, .. } => {
                    self.status = format!("system event: {name}");
                }
                ItemPayload::UserMessage { .. }
                | ItemPayload::AssistantMessageDelta { .. }
                | ItemPayload::ReasoningDelta { .. } => {}
            },
            ProtocolEvent::SessionArchived(session_id) => {
                self.status = format!("session {} archived", session_id);
            }
            ProtocolEvent::Warning { code, message } => {
                self.status = format!("warning[{code}] {message}");
            }
            ProtocolEvent::Error { code, message } => {
                self.status = format!("error[{code}] {message}");
            }
        }
    }
}

fn flatten_pending_interactions(
    approvals: Vec<ApprovalRequest>,
    ask_user: Vec<AskUserPrompt>,
) -> Vec<PendingInteraction> {
    approvals
        .into_iter()
        .map(PendingInteraction::Approval)
        .chain(ask_user.into_iter().map(PendingInteraction::AskUser))
        .collect()
}

fn clamp_selection(index: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        index.min(len.saturating_sub(1))
    }
}

fn next_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current = current.min(len - 1) as isize;
    let next = (current + delta).clamp(0, len as isize - 1);
    next as usize
}

fn handle_approve_command(input: &str) -> Result<String> {
    let mut parts = input.trim().splitn(3, ' ');
    let request_item_id = parts.next().unwrap_or_default().trim();
    let decision_raw = parts.next().unwrap_or_default().trim();
    let reason = parts.next().map(|value| value.trim().to_string());
    if request_item_id.is_empty() || decision_raw.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /approve <item_id> <approve|reject|cancel> [reason]"
        ));
    }

    let decision = match decision_raw.to_ascii_lowercase().as_str() {
        "approve" | "approved" | "yes" | "y" => ApprovalDecision::Approved,
        "reject" | "rejected" | "no" | "n" => ApprovalDecision::Rejected,
        "cancel" | "cancelled" | "canceled" => ApprovalDecision::Cancelled,
        _ => {
            return Err(anyhow::anyhow!(
                "approval decision must be approve, reject, or cancel"
            ));
        }
    };

    let request_item_id = resolve_approval(ItemId::from(request_item_id), decision, reason)?;
    Ok(format!("resolved approval {}", request_item_id))
}

fn handle_answer_command(input: &str) -> Result<String> {
    let mut parts = input.trim().splitn(2, ' ');
    let request_item_id = parts.next().unwrap_or_default().trim();
    let answer = parts.next().unwrap_or_default().trim();
    if request_item_id.is_empty() || answer.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /answer <item_id> <choice_id|text:...>"
        ));
    }

    let (selected_choice_id, freeform_text) = if let Some(text) = answer.strip_prefix("text:") {
        (None, Some(text.trim().to_string()))
    } else {
        (Some(answer.to_string()), None)
    };

    let request_item_id = resolve_ask_user(
        ItemId::from(request_item_id),
        selected_choice_id,
        freeform_text,
    )?;
    Ok(format!("resolved ask-user {}", request_item_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_command_requires_valid_decision() {
        let error = handle_approve_command("item-1 maybe")
            .expect_err("invalid decision should fail")
            .to_string();
        assert!(error.contains("approval decision must be approve, reject, or cancel"));
    }

    #[test]
    fn answer_command_requires_value() {
        let error = handle_answer_command("item-1")
            .expect_err("missing answer should fail")
            .to_string();
        assert!(error.contains("usage: /answer <item_id> <choice_id|text:...>"));
    }

    #[test]
    fn next_index_clamps_to_bounds() {
        assert_eq!(next_index(0, 0, 1), 0);
        assert_eq!(next_index(0, 3, 1), 1);
        assert_eq!(next_index(2, 3, 1), 2);
        assert_eq!(next_index(2, 3, -1), 1);
        assert_eq!(next_index(0, 3, -1), 0);
    }
}
