use anyhow::{bail, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    io,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use crate::commit::{build_commit_message, parse_full_commit};
use crate::git::FileStat;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

enum State {
    Loading {
        tick: usize,
        rx: Receiver<Result<String>>,
    },
    Menu {
        selected: usize,
    },
    EditingImprove {
        input: String,
    },
    EditingScope {
        input: String,
    },
    EditingBody {
        input: String,
    },
}

struct App {
    /// Description only — type/scope/! assembled by build_commit_message at render/proceed time.
    msg: String,
    scope: String,
    breaking: bool,
    body: String,
    body_rx: Option<Receiver<Result<String>>>,
    body_tick: usize,
    state: State,
    commit_type: Option<String>,
    use_ai: bool,
    diff: String,
    stats: Vec<FileStat>,
    log: String,
}

enum KeyAction {
    None,
    Proceed,
    NavUp,
    NavDown,
    ToggleBreaking,
    StartEditingImprove,
    StartEditingScope,
    StartEditingBody,
    GenerateBody,
    InsertChar(char),
    DeleteChar,
    SubmitEditing,
    BackToMenu { restore_selected: usize },
    Cancel,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    initial_msg: Option<String>,
    commit_type: Option<&str>,
    use_ai: bool,
    diff: String,
    stats: Vec<FileStat>,
    log: String,
    initial_scope: String,
    initial_breaking: bool,
    initial_body: String,
) -> Result<String> {
    let commit_type = commit_type.map(str::to_string);

    let (msg, state) = match initial_msg {
        Some(m) => (m, State::Menu { selected: 0 }),
        None => {
            let rx = spawn_generation(&diff, commit_type.as_deref(), None, &initial_scope);
            (String::new(), State::Loading { tick: 0, rx })
        }
    };

    let mut app = App {
        msg,
        scope: initial_scope,
        breaking: initial_breaking,
        body: initial_body,
        body_rx: None,
        body_tick: 0,
        state,
        commit_type,
        use_ai,
        diff,
        stats,
        log,
    };

    enable_raw_mode()?;
    execute!(io::stderr(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<String> {
    loop {
        if let State::Loading { tick, .. } = &mut app.state {
            *tick = (*tick + 1) % SPINNER.len();
        }
        if app.body_rx.is_some() {
            app.body_tick = (app.body_tick + 1) % SPINNER.len();
        }

        terminal.draw(|f| render(f, app))?;

        let gen_result: Option<Result<String>> = match &app.state {
            State::Loading { rx, .. } => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(anyhow::anyhow!("AI generation thread died")))
                }
            },
            _ => None,
        };
        if let Some(res) = gen_result {
            let raw = res?;
            if app.commit_type.is_none() {
                // AI returned a full "type: desc" string — parse so scope/! can be applied.
                // parse_full_commit extracts only the header description (no body paragraphs).
                let (parsed_type, _, _, parsed_desc, _) = parse_full_commit(&raw);
                if let Some(t) = parsed_type {
                    app.commit_type = Some(t);
                    app.msg = parsed_desc;
                } else {
                    app.msg = raw.lines().next().unwrap_or("").to_string();
                }
            } else {
                // Type was pre-set; AI returned description only. Take first line in case
                // the model returned extra paragraphs.
                app.msg = raw.lines().next().unwrap_or("").to_string();
            }
            app.state = State::Menu { selected: 0 };
            continue;
        }

        let body_result: Option<Result<String>> = if let Some(rx) = &app.body_rx {
            match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(anyhow::anyhow!("body generation thread died")))
                }
            }
        } else {
            None
        };
        if let Some(res) = body_result {
            app.body = res?;
            app.body_rx = None;
        }

        let timeout = if matches!(app.state, State::Loading { .. }) || app.body_rx.is_some() {
            Duration::from_millis(80)
        } else {
            Duration::from_secs(10)
        };

        if !event::poll(timeout)? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        let action = compute_action(&app.state, key.code);

        match action {
            KeyAction::None => {}
            KeyAction::Proceed => {
                return Ok(build_commit_message(
                    app.commit_type.as_deref(),
                    &app.scope,
                    app.breaking,
                    &app.msg,
                    &app.body,
                ));
            }
            KeyAction::Cancel => bail!("aborted"),
            KeyAction::NavUp => {
                if let State::Menu { selected } = &mut app.state {
                    *selected = selected.saturating_sub(1);
                }
            }
            KeyAction::NavDown => {
                if let State::Menu { selected } = &mut app.state {
                    if *selected < 1 {
                        *selected += 1;
                    }
                }
            }
            KeyAction::ToggleBreaking => {
                app.breaking = !app.breaking;
            }
            KeyAction::StartEditingImprove => {
                app.state = State::EditingImprove {
                    input: String::new(),
                };
            }
            KeyAction::StartEditingScope => {
                let prefill = app.scope.clone();
                app.state = State::EditingScope { input: prefill };
            }
            KeyAction::StartEditingBody => {
                let prefill = app.body.clone();
                app.state = State::EditingBody { input: prefill };
            }
            KeyAction::GenerateBody => {
                let subject = build_commit_message(
                    app.commit_type.as_deref(),
                    &app.scope,
                    app.breaking,
                    &app.msg,
                    "",
                );
                app.body_rx = Some(spawn_body_generation(&app.diff, &subject));
            }
            KeyAction::BackToMenu { restore_selected } => {
                app.state = State::Menu {
                    selected: restore_selected,
                };
            }
            KeyAction::InsertChar(c) => match &mut app.state {
                State::EditingImprove { input }
                | State::EditingScope { input }
                | State::EditingBody { input } => input.push(c),
                _ => {}
            },
            KeyAction::DeleteChar => match &mut app.state {
                State::EditingImprove { input }
                | State::EditingScope { input }
                | State::EditingBody { input } => {
                    input.pop();
                }
                _ => {}
            },
            KeyAction::SubmitEditing => match &app.state {
                State::EditingImprove { input } => {
                    let input = input.clone();
                    if app.use_ai {
                        let rx = spawn_generation(
                            &app.diff,
                            app.commit_type.as_deref(),
                            Some(&input),
                            &app.scope,
                        );
                        app.state = State::Loading { tick: 0, rx };
                    } else {
                        app.msg = input;
                        app.state = State::Menu { selected: 0 };
                    }
                }
                State::EditingScope { input } => {
                    app.scope = input.clone();
                    app.state = State::Menu { selected: 0 };
                }
                State::EditingBody { input } => {
                    app.body = input.clone();
                    app.state = State::Menu { selected: 0 };
                }
                _ => {}
            },
        }
    }
}

fn compute_action(state: &State, code: KeyCode) -> KeyAction {
    match state {
        State::Loading { .. } => KeyAction::None,
        State::Menu { selected } => match code {
            KeyCode::Left => KeyAction::NavUp,
            KeyCode::Right => KeyAction::NavDown,
            KeyCode::Char('s') => KeyAction::StartEditingScope,
            KeyCode::Char('!') => KeyAction::ToggleBreaking,
            KeyCode::Char('b') => KeyAction::StartEditingBody,
            KeyCode::Char('B') => KeyAction::GenerateBody,
            KeyCode::Enter => match *selected {
                0 => KeyAction::Proceed,
                _ => KeyAction::StartEditingImprove,
            },
            KeyCode::Esc | KeyCode::Char('q') => KeyAction::Cancel,
            _ => KeyAction::None,
        },
        State::EditingImprove { input } => editing_key_action(input, code, 1),
        State::EditingScope { input } => editing_key_action(input, code, 0),
        State::EditingBody { input } => editing_key_action(input, code, 0),
    }
}

fn editing_key_action(input: &str, code: KeyCode, back_selected: usize) -> KeyAction {
    match code {
        KeyCode::Enter if !input.is_empty() => KeyAction::SubmitEditing,
        KeyCode::Esc => KeyAction::BackToMenu {
            restore_selected: back_selected,
        },
        KeyCode::Backspace => KeyAction::DeleteChar,
        KeyCode::Char(c) => KeyAction::InsertChar(c),
        _ => KeyAction::None,
    }
}

fn spawn_generation(
    diff: &str,
    commit_type: Option<&str>,
    feedback: Option<&str>,
    scope: &str,
) -> Receiver<Result<String>> {
    let diff = diff.to_string();
    let commit_type = commit_type.map(str::to_string);
    let feedback = feedback.map(str::to_string);
    let scope = scope.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let scope_arg = if scope.is_empty() {
            None
        } else {
            Some(scope.as_str())
        };
        let result = crate::ai::generate_with_hint(
            &diff,
            commit_type.as_deref(),
            feedback.as_deref(),
            scope_arg,
        );
        tx.send(result).ok();
    });
    rx
}

fn spawn_body_generation(diff: &str, subject: &str) -> Receiver<Result<String>> {
    let diff = diff.to_string();
    let subject = subject.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        tx.send(crate::ai::generate_body(&diff, &subject)).ok();
    });
    rx
}

// ── Layout ───────────────────────────────────────────────────────────────────
//
//  ┌─────────────────── commit (full width) ──────────────────────────────────┐
//  │ feat(auth)!: add login                                                   │
//  └──────────────────────────────────────────────────────────────────────────┘
//  ┌── changes (40%) ──────────┐  ┌── log (60%) ─────────────────────────────┐
//  │ src/ui.rs  +180  -45      │  │ * abc feat: …                            │
//  └───────────────────────────┘  └──────────────────────────────────────────┘
//  ┌─────────────────── actions ──────────────────────────────────────────────┐
//  │ ▶ Proceed    Improve    [s] (auth)    [!] ✓    [b] body✓                │
//  └──────────────────────────────────────────────────────────────────────────┘
//  j/k navigate  Enter select  s scope  ! breaking  b body  q cancel

fn render(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();

    // Live body: use in-progress input when editing body, committed value otherwise.
    let live_body = match &app.state {
        State::EditingBody { input } => input.as_str(),
        _ => app.body.as_str(),
    };

    // Dynamic commit panel height: header(1) + blank(1) + body rows + 2 borders.
    // Inner width = area - margin(2) - borders(2).
    let commit_height = if live_body.is_empty() {
        3
    } else {
        let inner_w = area.width.saturating_sub(6).max(1) as usize;
        let body_rows: u16 = live_body
            .lines()
            .map(|l| l.chars().count().div_ceil(inner_w).max(1) as u16)
            .sum();
        (2 + 1 + 1 + body_rows).min(12) // cap at 12 rows so log/changes stay visible
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(commit_height), // commit header [+ body]
            Constraint::Min(0),                // changes | log
            Constraint::Length(3),             // actions
            Constraint::Length(1),             // hint
        ])
        .split(area);

    render_commit(f, app, rows[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(rows[1]);

    render_changes(f, app, cols[0]);
    render_log(f, app, cols[1]);

    render_actions(f, app, rows[2]);
    render_hint(f, &app.state, app.use_ai, rows[3]);
}

fn render_commit(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let desc = if app.msg.is_empty() { "…" } else { &app.msg };

    // Show live input values while editing so the header updates as the user types.
    let (scope, body) = match &app.state {
        State::EditingScope { input } => (input.as_str(), app.body.as_str()),
        State::EditingBody { input } => (app.scope.as_str(), input.as_str()),
        _ => (app.scope.as_str(), app.body.as_str()),
    };

    let preview = build_commit_message(app.commit_type.as_deref(), scope, app.breaking, desc, body);
    f.render_widget(
        Paragraph::new(preview)
            .block(Block::default().title(" commit ").borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_actions(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let block = Block::default().title(" actions ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let y = inner.y + inner.height.saturating_sub(1) / 2;
    let row = Rect {
        y,
        height: 1,
        ..inner
    };

    match &app.state {
        State::Loading { tick, .. } => {
            let spinner = SPINNER[*tick % SPINNER.len()];
            f.render_widget(
                Paragraph::new(format!("  {spinner}  generating…"))
                    .style(Style::default().fg(Color::Yellow)),
                row,
            );
        }
        State::EditingImprove { input } => {
            render_inline_input(
                f,
                row,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "▶ Improve",
                input,
            );
        }
        State::EditingScope { input } => {
            render_inline_input(
                f,
                row,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "  Scope",
                input,
            );
        }
        State::EditingBody { input } => {
            render_inline_input(
                f,
                row,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "  Body",
                input,
            );
        }
        State::Menu { selected } => {
            let sel = *selected;

            let proceed_style = if sel == 0 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let improve_style = if sel == 1 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let proceed_prefix = if sel == 0 { "▶ " } else { "  " };
            let improve_prefix = if sel == 1 { "▶ " } else { "  " };

            let scope_label = if app.scope.is_empty() {
                "[s] scope".to_string()
            } else {
                format!("[s] ({})", app.scope)
            };
            let breaking_label = if app.breaking {
                "[!] ✓".to_string()
            } else {
                "[!]".to_string()
            };
            let body_label = if app.body_rx.is_some() {
                format!(
                    "[B] {}  generating…",
                    SPINNER[app.body_tick % SPINNER.len()]
                )
            } else if app.body.is_empty() {
                "[b] body  [B] ai".to_string()
            } else {
                "[b] body✓  [B] ai".to_string()
            };

            let line = Line::from(vec![
                Span::styled(format!("{proceed_prefix}Proceed"), proceed_style),
                Span::raw("    "),
                Span::styled(format!("{improve_prefix}Improve"), improve_style),
                Span::raw("    "),
                Span::styled(scope_label, Style::default().fg(Color::DarkGray)),
                Span::raw("    "),
                Span::styled(
                    breaking_label,
                    if app.breaking {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw("    "),
                Span::styled(
                    body_label,
                    if app.body.is_empty() {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                ),
            ]);

            f.render_widget(Paragraph::new(line), row);
        }
    }
}

fn render_inline_input(
    f: &mut ratatui::Frame,
    row: Rect,
    label_style: Style,
    label: &str,
    input: &str,
) {
    let label_text = format!("{label}  ");
    let label_w = label_text.chars().count() as u16;

    if row.width <= label_w + 2 {
        f.render_widget(Paragraph::new(label_text).style(label_style), row);
        return;
    }

    let label_area = Rect {
        width: label_w,
        ..row
    };
    let input_area = Rect {
        x: row.x + label_w,
        width: row.width - label_w,
        ..row
    };

    f.render_widget(Paragraph::new(label_text).style(label_style), label_area);
    f.render_widget(
        Paragraph::new(input).style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        input_area,
    );

    let cursor_x =
        (input_area.x + input.chars().count() as u16).min(input_area.x + input_area.width - 1);
    f.set_cursor_position((cursor_x, row.y));
}

fn render_hint(f: &mut ratatui::Frame, state: &State, use_ai: bool, area: Rect) {
    let hint = match state {
        State::Loading { .. } => "",
        State::Menu { .. } => {
            if use_ai {
                "←/→ navigate   Enter select   s scope   ! breaking   b body   B ai-body   q cancel"
            } else {
                "←/→ navigate   Enter select   s scope   ! breaking   b body   q cancel"
            }
        }
        State::EditingImprove { .. } | State::EditingScope { .. } | State::EditingBody { .. } => {
            "Enter confirm   Esc back"
        }
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_changes(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let max_file = app.stats.iter().map(|s| s.file.len()).max().unwrap_or(0);

    let lines: Vec<String> = app
        .stats
        .iter()
        .map(|s| {
            let padding = max_file - s.file.len();
            format!(
                "{}{}  +{}  -{}",
                s.file,
                " ".repeat(padding),
                s.added,
                s.deleted
            )
        })
        .collect();

    f.render_widget(
        Paragraph::new(lines.join("\n"))
            .block(Block::default().title(" changes ").borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_log(f: &mut ratatui::Frame, app: &App, area: Rect) {
    f.render_widget(
        Paragraph::new(app.log.as_str())
            .block(Block::default().title(" log ").borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false }),
        area,
    );
}
