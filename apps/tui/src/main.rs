use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

mod app;
mod render;

use app::App;
use render::render;

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::bootstrap();

    loop {
        app.drain_server_events();
        render(&mut terminal, &app)?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key)
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    break;
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('r')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.refresh_sessions();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('n')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.start_new_session();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('s')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.toggle_panel(app::FocusPanel::Sessions);
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('e')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.toggle_panel(app::FocusPanel::Extensions);
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('a')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.toggle_panel(app::FocusPanel::Pending);
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('l')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.toggle_panel(app::FocusPanel::Plan);
                }
                Event::Key(key) if key.code == KeyCode::Esc => {
                    app.close_overlay();
                }
                Event::Key(key) if key.code == KeyCode::Tab => app.focus_next(),
                Event::Key(key) if key.code == KeyCode::BackTab => app.focus_prev(),
                Event::Key(key) if key.code == KeyCode::Up => app.move_selection_prev(),
                Event::Key(key) if key.code == KeyCode::Down => app.move_selection_next(),
                Event::Key(key) if key.code == KeyCode::Enter => app.activate(),
                Event::Key(key)
                    if key.code == KeyCode::Char('f') && app.focus == app::FocusPanel::Sessions =>
                {
                    app.fork_selected_session();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('a') && app.focus == app::FocusPanel::Pending =>
                {
                    app.approve_selected_pending();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('r') && app.focus == app::FocusPanel::Pending =>
                {
                    app.reject_selected_pending();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('c') && app.focus == app::FocusPanel::Pending =>
                {
                    app.cancel_selected_pending();
                }
                Event::Key(key)
                    if key.code == KeyCode::Char('i') && app.focus == app::FocusPanel::Pending =>
                {
                    app.prepare_selected_pending_freeform();
                }
                Event::Key(key) if app.focus == app::FocusPanel::Pending => match key.code {
                    KeyCode::Char('1') => app.answer_selected_pending_choice(0),
                    KeyCode::Char('2') => app.answer_selected_pending_choice(1),
                    KeyCode::Char('3') => app.answer_selected_pending_choice(2),
                    KeyCode::Char('4') => app.answer_selected_pending_choice(3),
                    KeyCode::Char('5') => app.answer_selected_pending_choice(4),
                    KeyCode::Char('6') => app.answer_selected_pending_choice(5),
                    KeyCode::Char('7') => app.answer_selected_pending_choice(6),
                    KeyCode::Char('8') => app.answer_selected_pending_choice(7),
                    KeyCode::Char('9') => app.answer_selected_pending_choice(8),
                    _ => {}
                },
                Event::Key(key) if key.code == KeyCode::Backspace => {
                    app.input.pop();
                }
                Event::Key(key)
                    if matches!(key.code, KeyCode::Char(_))
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    let KeyCode::Char(ch) = key.code else {
                        unreachable!()
                    };
                    app.begin_typing(ch);
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::OffsetDateTime;

    use ccodex_protocol::{
        ApprovalDecision, ApprovalResponse, AskUserChoice, AskUserPrompt, AskUserResponse, Item,
        ItemId, ItemPayload, LocalServerStoredTurn, PlanId, PlanItem, PlanItemStatus, PlanMode,
        PlanState, Session, SessionId, SessionStatus, ToolCall, ToolCallId, ToolResult, Turn,
        TurnId, TurnStatus,
    };

    use crate::render::{format_pending_interactions, format_plan, render_stored_turns};

    #[test]
    fn format_plan_includes_bootstrap_context() {
        let mut session = Session {
            id: SessionId("session-plan".to_string()),
            title: Some("Plan Session".to_string()),
            workspace_root: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            status: SessionStatus::Active,
            active_plan: Some(PlanState {
                id: PlanId("plan-ctx".to_string()),
                session_id: SessionId("session-plan".to_string()),
                mode: PlanMode::Active,
                summary: Some("active bootstrap plan".to_string()),
                items: vec![PlanItem {
                    id: "step-1".to_string(),
                    title: "Inspect repo".to_string(),
                    notes: None,
                    status: PlanItemStatus::InProgress,
                }],
                updated_at: OffsetDateTime::now_utc(),
            }),
            metadata: std::collections::BTreeMap::new(),
        };
        session
            .metadata
            .insert("bootstrap_complete".to_string(), json!(true));
        session
            .metadata
            .insert("bootstrap_instruction_count".to_string(), json!(1));
        session
            .metadata
            .insert("bootstrap_skill_count".to_string(), json!(2));
        session
            .metadata
            .insert("bootstrap_agent_count".to_string(), json!(1));
        session
            .metadata
            .insert("bootstrap_hook_count".to_string(), json!(1));
        session
            .metadata
            .insert("bootstrap_mcp_server_count".to_string(), json!(1));
        session.metadata.insert(
            "bootstrap_skill_names".to_string(),
            json!(["review", "ship"]),
        );
        session
            .metadata
            .insert("bootstrap_hook_names".to_string(), json!(["prepare"]));
        session
            .metadata
            .insert("bootstrap_mcp_server_names".to_string(), json!(["echo"]));

        let lines = format_plan(&session).join("\n");
        assert!(lines.contains("active bootstrap plan"));
        assert!(lines.contains("[InProgress] Inspect repo"));
        assert!(lines.contains("bootstrap: complete"));
        assert!(lines.contains("bootstrap counts: instructions=1 skills=2 agents=1 hooks=1 mcp=1"));
        assert!(lines.contains("skills: review, ship"));
        assert!(lines.contains("hooks: prepare"));
        assert!(lines.contains("mcp: echo"));
    }

    #[test]
    fn format_pending_interactions_renders_structured_context() {
        let approvals = vec![
            ccodex_protocol::ApprovalRequest::new(
                ItemId("approval-1".to_string()),
                Some(ToolCallId("tool-1".to_string())),
                ccodex_protocol::ApprovalKind::CommandExecution,
                "Approve command".to_string(),
                Some("curl https://example.com".to_string()),
            )
            .with_risk(ccodex_protocol::ApprovalRisk::High)
            .with_context(ccodex_protocol::ApprovalContext {
                tool_name: Some("bash".to_string()),
                command: Some("curl https://example.com".to_string()),
                path: None,
                touches_workspace: true,
                touches_outside_workspace: false,
                has_network_access: true,
                is_destructive: false,
            }),
        ];
        let ask_user = vec![AskUserPrompt {
            item_id: ItemId("ask-1".to_string()),
            title: "Choose deployment".to_string(),
            message: "Pick one".to_string(),
            choices: vec![
                AskUserChoice {
                    id: "choice-1".to_string(),
                    label: "Staging".to_string(),
                    description: None,
                },
                AskUserChoice {
                    id: "choice-2".to_string(),
                    label: "Production".to_string(),
                    description: None,
                },
            ],
            allow_freeform: true,
        }];
        let freeform_only = vec![AskUserPrompt {
            item_id: ItemId("ask-2".to_string()),
            title: "Other environment".to_string(),
            message: "Type a custom target".to_string(),
            choices: vec![],
            allow_freeform: true,
        }];

        let freeform_lines = format_pending_interactions(&approvals, &freeform_only).join("\n");
        let lines = format_pending_interactions(&approvals, &ask_user).join("\n");
        assert!(lines.contains("approval approval-1 CommandExecution/High tool=bash"));
        assert!(lines.contains("cmd=curl https://example.com"));
        assert!(lines.contains("net=true"));
        assert!(lines.contains("ask ask-1 Choose deployment freeform=true [Staging, Production]"));
        assert!(freeform_lines.contains("ask ask-2 Other environment freeform=true []"));
    }

    #[test]
    fn render_stored_turns_formats_key_item_types() {
        let session_id = SessionId("session-1".to_string());
        let turn = Turn {
            id: TurnId("turn-1".to_string()),
            session_id: session_id.clone(),
            item_ids: vec![],
            started_at: OffsetDateTime::now_utc(),
            completed_at: Some(OffsetDateTime::now_utc()),
            status: TurnStatus::Completed,
        };
        let turns = vec![LocalServerStoredTurn {
            turn,
            items: vec![
                Item {
                    id: ItemId("item-1".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::UserMessage {
                        content: "read Cargo.toml".to_string(),
                    },
                },
                Item {
                    id: ItemId("item-2".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallStarted {
                        call: ToolCall {
                            id: ToolCallId("tool-1".to_string()),
                            tool_name: "read_file".to_string(),
                            input: json!({"path":"Cargo.toml"}),
                        },
                    },
                },
                Item {
                    id: ItemId("item-3".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallDelta {
                        tool_call_id: ToolCallId("tool-1".to_string()),
                        delta: json!({
                            "phase": "completed",
                            "tool_name": "read_file",
                            "status": "ok",
                            "is_error": false
                        }),
                    },
                },
                Item {
                    id: ItemId("item-3c".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallDelta {
                        tool_call_id: ToolCallId("tool-ask".to_string()),
                        delta: json!({
                            "phase": "completed",
                            "tool_name": "ask_user",
                            "status": "resolved",
                            "is_error": false,
                            "resolution_mode": "freeform"
                        }),
                    },
                },
                Item {
                    id: ItemId("item-3d".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallDelta {
                        tool_call_id: ToolCallId("tool-plan".to_string()),
                        delta: json!({
                            "phase": "completed",
                            "tool_name": "update_plan",
                            "status": "entered",
                            "is_error": false,
                            "plan_item_count": 1
                        }),
                    },
                },
                Item {
                    id: ItemId("item-3e".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::SystemEvent {
                        name: "subagent_finished".to_string(),
                        payload: json!({
                            "agent_name": "reviewer",
                            "parent_session_id": "session-1",
                            "parent_turn_id": "turn-1",
                            "child_turn_id": "turn-child-1",
                            "subagent_depth": 1,
                            "assistant_text": "child done"
                        }),
                    },
                },
                Item {
                    id: ItemId("item-3b".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallFinished {
                        result: ToolResult {
                            tool_call_id: ToolCallId("tool-1".to_string()),
                            output: json!({"content":"[package]"}),
                            is_error: false,
                        },
                    },
                },
                Item {
                    id: ItemId("item-4".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::AskUserRequested {
                        prompt: AskUserPrompt {
                            item_id: ItemId("item-4".to_string()),
                            title: "Choose".to_string(),
                            message: "Pick one".to_string(),
                            choices: vec![AskUserChoice {
                                id: "choice-1".to_string(),
                                label: "First".to_string(),
                                description: None,
                            }],
                            allow_freeform: false,
                        },
                    },
                },
                Item {
                    id: ItemId("item-5".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::AskUserResolved {
                        response: AskUserResponse {
                            request_item_id: ItemId("item-4".to_string()),
                            selected_choice_id: Some("choice-1".to_string()),
                            freeform_text: None,
                        },
                    },
                },
                Item {
                    id: ItemId("item-6".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ToolCallDelta {
                        tool_call_id: ToolCallId("tool-1".to_string()),
                        delta: json!({
                            "phase": "approval_assessed",
                            "kind": "CommandExecution",
                            "risk": "High",
                            "tool_name": "bash",
                            "path": "/tmp/demo"
                        }),
                    },
                },
                Item {
                    id: ItemId("item-7".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ApprovalRequested {
                        request: ccodex_protocol::ApprovalRequest::new(
                            ItemId("item-7".to_string()),
                            Some(ToolCallId("tool-1".to_string())),
                            ccodex_protocol::ApprovalKind::CommandExecution,
                            "Approve command".to_string(),
                            Some("curl https://example.com".to_string()),
                        )
                        .with_risk(ccodex_protocol::ApprovalRisk::High),
                    },
                },
                Item {
                    id: ItemId("item-8".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::ApprovalResolved {
                        response: ApprovalResponse::new(
                            ItemId("item-2".to_string()),
                            ApprovalDecision::Approved,
                            Some("allowed".to_string()),
                            Some(ccodex_protocol::ApprovalReasonCode::InteractiveApproved),
                        ),
                    },
                },
                Item {
                    id: ItemId("item-9".to_string()),
                    turn_id: TurnId("turn-1".to_string()),
                    created_at: OffsetDateTime::now_utc(),
                    payload: ItemPayload::PlanUpdated {
                        plan: PlanState {
                            id: PlanId("plan-1".to_string()),
                            session_id,
                            mode: PlanMode::Active,
                            summary: None,
                            items: vec![PlanItem {
                                id: "step-1".to_string(),
                                title: "Read config".to_string(),
                                notes: None,
                                status: PlanItemStatus::InProgress,
                            }],
                            updated_at: OffsetDateTime::now_utc(),
                        },
                    },
                },
            ],
        }];

        let lines = render_stored_turns(&turns).join("\n");
        assert!(lines.contains("user> read Cargo.toml"));
        assert!(lines.contains("tool:start read_file"));
        assert!(lines.contains("tool:completed tool-1 tool=read_file status=ok error=false"));
        assert!(lines.contains(
            "tool:completed tool-ask tool=ask_user status=resolved error=false resolution_mode=freeform"
        ));
        assert!(lines.contains(
            "tool:completed tool-plan tool=update_plan status=entered error=false plan_items=1"
        ));
        assert!(lines.contains("tool:done tool-1"));
        assert!(lines.contains("ask-user> item-4 Choose [First]"));
        assert!(lines.contains("ask-user:resolved item-4 choice=choice-1"));
        assert!(lines.contains("approval:resolved item-2 Approved allowed"));
        assert!(lines.contains(
            "system:subagent_finished agent=reviewer parent_turn=turn-1 child_turn=turn-child-1 child done"
        ));
        assert!(lines.contains("plan> [InProgress] Read config"));
    }
}
