use std::io::{self, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use serde_json::Value;

use ccodex_protocol::{ItemPayload, LocalServerStoredTurn};

use crate::app::{App, FocusPanel, PendingInteraction};

pub(crate) fn default_output() -> Vec<String> {
    vec![
        "CCODEX TUI".to_string(),
        "Input continues the current session by default. Ctrl-N starts a new one.".to_string(),
        "Ctrl-S sessions  Ctrl-E extensions  Ctrl-A pending  Ctrl-L plan".to_string(),
        "Esc closes overlays. Type anytime to return to the composer.".to_string(),
        "In Sessions: Enter=load  f=fork selected session.".to_string(),
        "In Pending: a=approve  r=reject  c=cancel  1-9=choose  i=freeform".to_string(),
        "Use /resume <session_id> <prompt> to continue a session.".to_string(),
        "Use /show <session_id> to load a stored transcript.".to_string(),
        "Use /fork <session_id> to fork a stored session.".to_string(),
        "Use /approve <item_id> <approve|reject|cancel> [reason] to resolve approvals.".to_string(),
        "Use /answer <item_id> <choice_id|text:...> to resolve ask-user prompts.".to_string(),
        "Press Ctrl-C to quit.".to_string(),
    ]
}

pub(crate) fn render_stored_turns(turns: &[LocalServerStoredTurn]) -> Vec<String> {
    let mut lines = Vec::new();
    for stored in turns {
        lines.push(format!("turn {}", stored.turn.id));
        for item in &stored.items {
            match &item.payload {
                ItemPayload::UserMessage { content } => lines.push(format!("user> {content}")),
                ItemPayload::AssistantMessageDelta { content } => {
                    lines.push(format!("assistant> {content}"))
                }
                ItemPayload::ReasoningDelta { content } => {
                    lines.push(format!("reasoning> {content}"))
                }
                ItemPayload::ToolCallStarted { call } => lines.push(format!(
                    "tool:start {} {}",
                    call.tool_name,
                    compact_json(&call.input)
                )),
                ItemPayload::ToolCallDelta {
                    tool_call_id,
                    delta,
                } if delta.get("phase").and_then(serde_json::Value::as_str)
                    == Some("approval_assessed") =>
                {
                    let kind = delta
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Unknown");
                    let risk = delta
                        .get("risk")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Unknown");
                    let tool_name = delta
                        .get("tool_name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    let path = delta
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    lines.push(format!(
                        "tool:approval {} kind={} risk={} tool={} path={}",
                        tool_call_id, kind, risk, tool_name, path
                    ));
                }
                ItemPayload::ToolCallDelta {
                    tool_call_id,
                    delta,
                } if delta.get("phase").and_then(serde_json::Value::as_str)
                    == Some("completed") =>
                {
                    let tool_name = delta
                        .get("tool_name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    let is_error = delta
                        .get("is_error")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let status = delta
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(if is_error { "error" } else { "ok" });
                    let resolution_mode = delta
                        .get("resolution_mode")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| format!(" resolution_mode={value}"))
                        .unwrap_or_default();
                    let plan_item_count = delta
                        .get("plan_item_count")
                        .and_then(serde_json::Value::as_u64)
                        .map(|value| format!(" plan_items={value}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "tool:completed {} tool={} status={} error={}{}{}",
                        tool_call_id, tool_name, status, is_error, resolution_mode, plan_item_count
                    ));
                }
                ItemPayload::ToolCallDelta {
                    tool_call_id,
                    delta,
                } => lines.push(format!(
                    "tool:delta {} {}",
                    tool_call_id,
                    compact_json(delta)
                )),
                ItemPayload::ToolCallFinished { result } => lines.push(format!(
                    "tool:done {} {}",
                    result.tool_call_id,
                    compact_json(&result.output)
                )),
                ItemPayload::ApprovalRequested { request } => lines.push(format!(
                    "approval:request {} {:?}/{:?} {}",
                    request.item_id, request.kind, request.risk, request.summary
                )),
                ItemPayload::ApprovalResolved { response } => lines.push(format!(
                    "approval:resolved {} {:?} {} [{}]",
                    response.request_item_id,
                    response.decision,
                    response.reason.as_deref().unwrap_or(""),
                    response
                        .reason_code
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "unknown".to_string())
                )),
                ItemPayload::AskUserRequested { prompt } => lines.push(format!(
                    "ask-user> {} {} [{}]",
                    prompt.item_id,
                    prompt.title,
                    prompt
                        .choices
                        .iter()
                        .map(|choice| choice.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                ItemPayload::AskUserResolved { response } => lines.push(format!(
                    "ask-user:resolved {} choice={} freeform={}",
                    response.request_item_id,
                    response.selected_choice_id.as_deref().unwrap_or("<none>"),
                    response.freeform_text.as_deref().unwrap_or("<none>")
                )),
                ItemPayload::PlanEntered { plan } | ItemPayload::PlanUpdated { plan } => lines
                    .push(format!(
                        "plan> {}",
                        plan.items
                            .iter()
                            .map(|item| format!("[{:?}] {}", item.status, item.title))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )),
                ItemPayload::PlanExited { plan_id } => {
                    lines.push(format!("plan:exited {}", plan_id))
                }
                ItemPayload::Warning { code, message } => {
                    lines.push(format!("warning[{code}] {message}"))
                }
                ItemPayload::Error { code, message } => {
                    lines.push(format!("error[{code}] {message}"))
                }
                ItemPayload::SystemEvent { name, payload } if name == "subagent_finished" => {
                    let agent_name = payload
                        .get("agent_name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("subagent");
                    let parent_turn_id = payload
                        .get("parent_turn_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    let child_turn_id = payload
                        .get("child_turn_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    let assistant_text = payload
                        .get("assistant_text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    lines.push(format!(
                        "system:subagent_finished agent={} parent_turn={} child_turn={} {}",
                        agent_name, parent_turn_id, child_turn_id, assistant_text
                    ));
                }
                ItemPayload::SystemEvent { name, payload } => {
                    lines.push(format!("system:{} {}", name, compact_json(payload)))
                }
            }
        }
        lines.push(String::new());
    }
    lines
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

pub(crate) fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &App,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .split(frame.area());

        let header = Paragraph::new(app.status.clone()).block(
            Block::default()
                .title(transcript_header_title(app))
                .borders(Borders::ALL),
        );
        frame.render_widget(header, chunks[0]);

        let body = Paragraph::new(app.output.join("\n"))
            .block(
                Block::default()
                    .title(transcript_title(app))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(body, chunks[1]);

        let input = Paragraph::new(app.input.clone()).block(
            Block::default()
                .title(Line::from(vec![
                    if !app.has_overlay() {
                        Span::styled(
                            "> Composer",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::styled("Composer", Style::default().add_modifier(Modifier::BOLD))
                    },
                    Span::raw("  "),
                    Span::raw("Enter=send"),
                ]))
                .borders(Borders::ALL),
        );
        frame.render_widget(input, chunks[2]);

        let footer = Paragraph::new(help_line(app)).block(Block::default().borders(Borders::ALL));
        frame.render_widget(footer, chunks[3]);

        if app.has_overlay() {
            render_overlay(frame, app);
        }
    })?;
    Ok(())
}

fn transcript_header_title(app: &App) -> Line<'static> {
    let session = app
        .current_session
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "new-session".to_string());
    let pending_count = app.pending_interactions.len();
    let mode = if pending_count > 0 {
        "awaiting-input"
    } else if app.has_overlay() {
        "browsing"
    } else {
        "ready"
    };
    Line::from(format!(
        "CCODEX  session={session}  mode={mode}  pending={pending_count}"
    ))
}

fn transcript_title(app: &App) -> Line<'static> {
    if app.has_overlay() {
        Line::from(vec![
            Span::styled("Transcript", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(
                format!("overlay: {}", overlay_title(app.focus)),
                Style::default().fg(Color::Yellow),
            ),
        ])
    } else {
        Line::from("Transcript")
    }
}

fn help_line(app: &App) -> Line<'static> {
    let text = match app.focus {
        FocusPanel::Input => {
            "Ctrl-S sessions  Ctrl-E extensions  Ctrl-A pending  Ctrl-L plan  Ctrl-N new  Ctrl-R refresh  Ctrl-C quit"
        }
        FocusPanel::Sessions => {
            "Esc close  Up/Down move  Enter load  f fork  type to return to composer"
        }
        FocusPanel::Extensions => "Esc close  Up/Down move  type to return to composer",
        FocusPanel::Pending => {
            "Esc close  Up/Down move  a approve  r reject  c cancel  1-9 choose  i freeform"
        }
        FocusPanel::Plan => "Esc close  Up/Down move  type to return to composer",
    };
    Line::from(text)
}

fn render_overlay(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = centered_rect(82, 70, frame.area());
    frame.render_widget(Clear, area);

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let items = overlay_items(app)
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();
    let mut state = overlay_state(app);
    let overlay = List::new(items)
        .highlight_style(selection_style(true))
        .block(
            Block::default()
                .title(focus_title(overlay_title(app.focus), true))
                .borders(Borders::ALL),
        );
    frame.render_stateful_widget(overlay, sections[0], &mut state);

    let detail = Paragraph::new(detail_lines(app).join("\n"))
        .block(Block::default().title("Details").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, sections[1]);
}

fn focus_title<'a>(title: &'a str, focused: bool) -> Line<'a> {
    if focused {
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
        ])
    } else {
        Line::from(title)
    }
}

fn selection_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn overlay_title(focus: FocusPanel) -> &'static str {
    match focus {
        FocusPanel::Input => "Composer",
        FocusPanel::Sessions => "Recent Sessions",
        FocusPanel::Extensions => "Extensions",
        FocusPanel::Pending => "Pending",
        FocusPanel::Plan => "Plan",
    }
}

fn overlay_items(app: &App) -> Vec<String> {
    match app.focus {
        FocusPanel::Input => Vec::new(),
        FocusPanel::Sessions => app.sessions.clone(),
        FocusPanel::Extensions => app.extensions.clone(),
        FocusPanel::Pending => app.pending.clone(),
        FocusPanel::Plan => app.plan.clone(),
    }
}

fn overlay_state(app: &App) -> ListState {
    let mut state = ListState::default();
    match app.focus {
        FocusPanel::Input => {}
        FocusPanel::Sessions if !app.session_ids.is_empty() => {
            state.select(Some(app.selected_session));
        }
        FocusPanel::Extensions if !app.extensions.is_empty() => {
            state.select(Some(app.selected_extension));
        }
        FocusPanel::Pending if !app.pending_interactions.is_empty() => {
            state.select(Some(app.selected_pending));
        }
        FocusPanel::Plan if !app.plan.is_empty() => {
            state.select(Some(app.selected_plan));
        }
        _ => {}
    }
    state
}

fn detail_lines(app: &App) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "current session: {}",
        app.current_session
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "<new session draft>".to_string())
    ));

    match app.focus {
        FocusPanel::Input => {
            lines.push("Type to continue the current session.".to_string());
            lines.push("Ctrl-N starts a new draft session.".to_string());
            lines.push("Use /new <prompt> to branch into a fresh session.".to_string());
            if !app.pending_interactions.is_empty() {
                lines.push(format!(
                    "There are {} pending interactions. Press Ctrl-A to review them.",
                    app.pending_interactions.len()
                ));
            }
        }
        FocusPanel::Sessions => {
            lines.push(
                app.selected_session_label()
                    .map(|item| format!("selected: {item}"))
                    .unwrap_or_else(|| "selected: <none>".to_string()),
            );
            lines.push("Enter loads the selected session.".to_string());
            lines.push("f forks the selected session.".to_string());
            lines.push("Ctrl-R refreshes sessions from local-server.".to_string());
        }
        FocusPanel::Extensions => {
            lines.push(
                app.selected_extension_label()
                    .map(|item| format!("selected: {item}"))
                    .unwrap_or_else(|| "selected: <none>".to_string()),
            );
            lines.push("Extensions are loaded through compat + registry roots.".to_string());
        }
        FocusPanel::Pending => match app.pending_interactions.get(app.selected_pending) {
            Some(PendingInteraction::Approval(request)) => {
                lines.push(format!("approval {}", request.item_id));
                lines.push(format!(
                    "kind: {:?}  risk: {:?}",
                    request.kind, request.risk
                ));
                lines.push(format!("summary: {}", request.summary));
                if let Some(details) = request.details.as_deref() {
                    lines.push(format!("details: {details}"));
                }
                if let Some(tool) = request.context.tool_name.as_deref() {
                    lines.push(format!("tool: {tool}"));
                }
                if let Some(command) = request.context.command.as_deref() {
                    lines.push(format!("command: {command}"));
                }
                if let Some(path) = request.context.path.as_deref() {
                    lines.push(format!("path: {path}"));
                }
                lines.push(format!(
                    "workspace={} outside={} net={} destructive={}",
                    request.context.touches_workspace,
                    request.context.touches_outside_workspace,
                    request.context.has_network_access,
                    request.context.is_destructive
                ));
                lines.push("keys: a approve, r reject, c cancel".to_string());
            }
            Some(PendingInteraction::AskUser(prompt)) => {
                lines.push(format!("ask-user {}", prompt.item_id));
                lines.push(format!("title: {}", prompt.title));
                lines.push(format!("message: {}", prompt.message));
                if prompt.choices.is_empty() {
                    lines.push("choices: <none>".to_string());
                } else {
                    lines.push("choices:".to_string());
                    for (index, choice) in prompt.choices.iter().enumerate() {
                        lines.push(format!(
                            "{}. {}{}",
                            index + 1,
                            choice.label,
                            choice
                                .description
                                .as_deref()
                                .map(|value| format!(" - {value}"))
                                .unwrap_or_default()
                        ));
                    }
                }
                lines.push(format!("freeform: {}", prompt.allow_freeform));
                lines.push("keys: 1-9 answer, i prepare freeform".to_string());
            }
            None => {
                lines.push(
                    app.selected_pending_label()
                        .map(|item| format!("selected: {item}"))
                        .unwrap_or_else(|| "selected: <none>".to_string()),
                );
                lines.push("pending interactions appear here when tools block.".to_string());
            }
        },
        FocusPanel::Plan => {
            lines.push(
                app.selected_plan_line()
                    .map(|item| format!("selected: {item}"))
                    .unwrap_or_else(|| "selected: <none>".to_string()),
            );
            lines.push("Plan reflects the active persisted session plan.".to_string());
        }
    }

    lines
}

pub(crate) fn format_pending_interactions(
    approvals: &[ccodex_protocol::ApprovalRequest],
    ask_user: &[ccodex_protocol::AskUserPrompt],
) -> Vec<String> {
    let mut items = Vec::new();
    for request in approvals {
        let tool_name = request.context.tool_name.as_deref().unwrap_or("-");
        let command = request.context.command.as_deref().unwrap_or("-");
        let path = request.context.path.as_deref().unwrap_or("-");
        items.push(format!(
            "approval {} {:?}/{:?} tool={} cmd={} path={} net={} destructive={} {}",
            request.item_id,
            request.kind,
            request.risk,
            tool_name,
            command,
            path,
            request.context.has_network_access,
            request.context.is_destructive,
            request.summary
        ));
    }
    for prompt in ask_user {
        items.push(format!(
            "ask {} {} freeform={} [{}]",
            prompt.item_id,
            prompt.title,
            prompt.allow_freeform,
            prompt
                .choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if items.is_empty() {
        items.push("no pending interactions".to_string());
    }
    items
}

pub(crate) fn default_plan() -> Vec<String> {
    vec![
        "no active plan".to_string(),
        "bootstrap: unavailable".to_string(),
    ]
}

pub(crate) fn format_plan(session: &ccodex_protocol::Session) -> Vec<String> {
    let mut items = match &session.active_plan {
        Some(plan) => {
            let mut items = Vec::with_capacity(plan.items.len().saturating_add(6));
            items.push(
                plan.summary
                    .clone()
                    .unwrap_or_else(|| "active plan".to_string()),
            );
            items.extend(
                plan.items
                    .iter()
                    .map(|item| format!("[{:?}] {}", item.status, item.title)),
            );
            items
        }
        None => vec!["no active plan".to_string()],
    };

    let bootstrap_complete = session
        .metadata
        .get("bootstrap_complete")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    items.push(format!(
        "bootstrap: {}",
        if bootstrap_complete {
            "complete"
        } else {
            "pending"
        }
    ));

    let instruction_count = session
        .metadata
        .get("bootstrap_instruction_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let skill_count = session
        .metadata
        .get("bootstrap_skill_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let agent_count = session
        .metadata
        .get("bootstrap_agent_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let hook_count = session
        .metadata
        .get("bootstrap_hook_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let mcp_count = session
        .metadata
        .get("bootstrap_mcp_server_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    items.push(format!(
        "bootstrap counts: instructions={} skills={} agents={} hooks={} mcp={}",
        instruction_count, skill_count, agent_count, hook_count, mcp_count
    ));

    if let Some(skills) = session
        .metadata
        .get("bootstrap_skill_names")
        .and_then(serde_json::Value::as_array)
    {
        let rendered = skills
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !rendered.is_empty() {
            items.push(format!("skills: {rendered}"));
        }
    }

    if let Some(hooks) = session
        .metadata
        .get("bootstrap_hook_names")
        .and_then(serde_json::Value::as_array)
    {
        let rendered = hooks
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !rendered.is_empty() {
            items.push(format!("hooks: {rendered}"));
        }
    }

    if let Some(mcp_servers) = session
        .metadata
        .get("bootstrap_mcp_server_names")
        .and_then(serde_json::Value::as_array)
    {
        let rendered = mcp_servers
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !rendered.is_empty() {
            items.push(format!("mcp: {rendered}"));
        }
    }

    items
}
