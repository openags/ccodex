use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Terminal;

use ccodex_protocol::SessionId;

use ccodex_tui::{list_extensions, list_sessions, ping, resume_prompt, run_prompt};

struct App {
    status: String,
    input: String,
    output: Vec<String>,
    sessions: Vec<String>,
    extensions: Vec<String>,
    current_session: Option<SessionId>,
}

impl App {
    fn bootstrap() -> Self {
        let status = match ping() {
            Ok(status) => format!("connected: {status}"),
            Err(err) => format!("server unavailable: {err}"),
        };

        let sessions = list_sessions(Some(10))
            .map(|items| {
                items.into_iter()
                    .map(|session| format!(
                        "{}  {}",
                        session.id,
                        session.title.unwrap_or_else(|| "Untitled".to_string())
                    ))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let extensions = list_extensions()
            .map(|items| {
                items.into_iter()
                    .map(|manifest| format!("{:?}  {}", manifest.kind, manifest.name))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            status,
            input: String::new(),
            output: vec![
                "CCODEX TUI".to_string(),
                "Enter submits a prompt through local-server.".to_string(),
                "Use /resume <session_id> <prompt> to continue a session.".to_string(),
                "Press q or Ctrl-C to quit.".to_string(),
            ],
            sessions,
            extensions,
            current_session: None,
        }
    }

    fn submit(&mut self) {
        let raw = self.input.trim().to_string();
        if raw.is_empty() {
            return;
        }

        self.output.push(format!("> {raw}"));

        let result = if let Some(rest) = raw.strip_prefix("/resume ") {
            let mut parts = rest.splitn(2, ' ');
            let session_id = parts.next().unwrap_or_default().trim().to_string();
            let prompt = parts.next().unwrap_or_default().trim().to_string();
            if session_id.is_empty() || prompt.is_empty() {
                Err(anyhow::anyhow!("usage: /resume <session_id> <prompt>"))
            } else {
                resume_prompt(SessionId(session_id), prompt)
            }
        } else {
            run_prompt(raw.clone())
        };

        match result {
            Ok((text, session_id)) => {
                self.current_session = Some(session_id.clone());
                self.output.push(text);
                self.status = format!("ok: session {}", session_id);
                self.refresh_sessions();
            }
            Err(err) => {
                self.output.push(format!("error: {err}"));
                self.status = format!("error: {err}");
            }
        }

        self.input.clear();
    }

    fn refresh_sessions(&mut self) {
        if let Ok(items) = list_sessions(Some(10)) {
            self.sessions = items
                .into_iter()
                .map(|session| format!(
                    "{}  {}",
                    session.id,
                    session.title.unwrap_or_else(|| "Untitled".to_string())
                ))
                .collect();
        }

        if let Ok(items) = list_extensions() {
            self.extensions = items
                .into_iter()
                .map(|manifest| format!("{:?}  {}", manifest.kind, manifest.name))
                .collect();
        }
    }
}

fn render(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &App) -> io::Result<()> {
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(3),
                Constraint::Length(12),
            ])
            .split(frame.area());

        let header = Paragraph::new(app.status.clone())
            .block(Block::default().title("Status").borders(Borders::ALL));
        frame.render_widget(header, chunks[0]);

        let body = Paragraph::new(app.output.join("\n"))
            .block(Block::default().title("Transcript").borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        frame.render_widget(body, chunks[1]);

        let input = Paragraph::new(app.input.clone())
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("Input", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("  "),
                        Span::raw("Enter=send"),
                    ]))
                    .borders(Borders::ALL),
            );
        frame.render_widget(input, chunks[2]);

        let lower = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[3]);

        let session_items = app.sessions.iter().cloned().map(ListItem::new).collect::<Vec<_>>();
        let sessions = List::new(session_items)
            .block(Block::default().title("Recent Sessions").borders(Borders::ALL));
        frame.render_widget(sessions, lower[0]);

        let extension_items = app
            .extensions
            .iter()
            .cloned()
            .map(ListItem::new)
            .collect::<Vec<_>>();
        let extensions = List::new(extension_items)
            .block(Block::default().title("Extensions").borders(Borders::ALL));
        frame.render_widget(extensions, lower[1]);
    })?;
    Ok(())
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::bootstrap();

    loop {
        render(&mut terminal, &app)?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.code == KeyCode::Char('q') => break,
                Event::Key(key)
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    break
                }
                Event::Key(key) if key.code == KeyCode::Enter => app.submit(),
                Event::Key(key) if key.code == KeyCode::Backspace => {
                    app.input.pop();
                }
                Event::Key(key) if let KeyCode::Char(ch) = key.code => {
                    app.input.push(ch);
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
