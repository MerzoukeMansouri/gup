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
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    io,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

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
    Editing {
        input: String,
    },
}

struct App {
    msg: String,
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
    StartEditing,
    InsertChar(char),
    DeleteChar,
    SubmitEditing,
    BackToMenu,
    Cancel,
}

pub fn run(
    initial_msg: Option<String>,
    commit_type: Option<&str>,
    use_ai: bool,
    diff: String,
    stats: Vec<FileStat>,
    log: String,
) -> Result<String> {
    let commit_type = commit_type.map(str::to_string);

    let (msg, state) = match initial_msg {
        Some(m) => (m, State::Menu { selected: 0 }),
        None => {
            let rx = spawn_generation(&diff, commit_type.as_deref(), None);
            (String::new(), State::Loading { tick: 0, rx })
        }
    };

    let mut app = App {
        msg,
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
            app.msg = res?;
            app.state = State::Menu { selected: 0 };
            continue;
        }

        let timeout = if matches!(app.state, State::Loading { .. }) {
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
            KeyAction::Proceed => return Ok(app.msg.clone()),
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
            KeyAction::StartEditing => {
                app.state = State::Editing {
                    input: String::new(),
                };
            }
            KeyAction::BackToMenu => {
                app.state = State::Menu { selected: 1 };
            }
            KeyAction::InsertChar(c) => {
                if let State::Editing { input } = &mut app.state {
                    input.push(c);
                }
            }
            KeyAction::DeleteChar => {
                if let State::Editing { input } = &mut app.state {
                    input.pop();
                }
            }
            KeyAction::SubmitEditing => {
                let input = match &app.state {
                    State::Editing { input } => input.clone(),
                    _ => unreachable!(),
                };
                if app.use_ai {
                    let rx = spawn_generation(&app.diff, app.commit_type.as_deref(), Some(&input));
                    app.state = State::Loading { tick: 0, rx };
                } else {
                    app.msg = match app.commit_type.as_deref() {
                        Some(t) => format!("{t}: {input}"),
                        None => input,
                    };
                    app.state = State::Menu { selected: 0 };
                }
            }
        }
    }
}

fn compute_action(state: &State, code: KeyCode) -> KeyAction {
    match state {
        State::Loading { .. } => KeyAction::None,
        State::Menu { selected } => match code {
            KeyCode::Up | KeyCode::Char('k') => KeyAction::NavUp,
            KeyCode::Down | KeyCode::Char('j') => KeyAction::NavDown,
            KeyCode::Enter => match *selected {
                0 => KeyAction::Proceed,
                _ => KeyAction::StartEditing,
            },
            KeyCode::Esc | KeyCode::Char('q') => KeyAction::Cancel,
            _ => KeyAction::None,
        },
        State::Editing { input } => match code {
            KeyCode::Enter if !input.is_empty() => KeyAction::SubmitEditing,
            KeyCode::Esc => KeyAction::BackToMenu,
            KeyCode::Backspace => KeyAction::DeleteChar,
            KeyCode::Char(c) => KeyAction::InsertChar(c),
            _ => KeyAction::None,
        },
    }
}

fn spawn_generation(
    diff: &str,
    commit_type: Option<&str>,
    feedback: Option<&str>,
) -> Receiver<Result<String>> {
    let diff = diff.to_string();
    let commit_type = commit_type.map(str::to_string);
    let feedback = feedback.map(str::to_string);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result =
            crate::ai::generate_with_hint(&diff, commit_type.as_deref(), feedback.as_deref());
        let formatted = result.map(|body| match commit_type.as_deref() {
            Some(t) => format!("{t}: {body}"),
            None => body,
        });
        tx.send(formatted).ok();
    });
    rx
}

// ── Layout ───────────────────────────────────────────────────────────────────
//
//  ┌────────────── left (40%) ──────────────┐  ┌─── right (60%) ───────────┐
//  │ ┌ commit ──────────────────────────┐   │  │ ┌ changes ──────────────┐ │
//  │ │ feat: …                          │   │  │ │ src/ui.rs  +180  -45  │ │
//  │ └──────────────────────────────────┘   │  │ └───────────────────────┘ │
//  │ ┌ actions ─────────────────────────┐   │  │ ┌ log ───────────────────┐│
//  │ │ ▶ Proceed                        │   │  │ │ * abc feat: …          ││
//  │ │   Improve  [feedback___________] │   │  │ │ * def fix: …           ││
//  │ └──────────────────────────────────┘   │  │ └───────────────────────┘ │
//  │ hint                                   │  └────────────────────────────┘
//  └────────────────────────────────────────┘

fn render(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // commit header
            Constraint::Min(0),    // changes | log
            Constraint::Length(3), // actions
            Constraint::Length(1), // hint
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
    render_hint(f, &app.state, rows[3]);
}

fn render_commit(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let text = if app.msg.is_empty() {
        "…"
    } else {
        app.msg.as_str()
    };
    f.render_widget(
        Paragraph::new(text)
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

    match &app.state {
        State::Loading { tick, .. } => {
            let spinner = SPINNER[*tick % SPINNER.len()];
            let y = inner.y + inner.height.saturating_sub(1) / 2;
            f.render_widget(
                Paragraph::new(format!("  {spinner}  generating…"))
                    .style(Style::default().fg(Color::Yellow)),
                Rect {
                    y,
                    height: 1,
                    ..inner
                },
            );
        }
        _ => {
            let selected = match &app.state {
                State::Menu { selected } => *selected,
                State::Editing { .. } => 1,
                _ => 0,
            };

            // Horizontal layout: [Proceed] [Improve inline-input]
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(inner);

            for (i, (label, area)) in ["Proceed", "Improve"]
                .iter()
                .zip([cols[0], cols[1]])
                .enumerate()
            {
                let is_sel = i == selected;
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_sel { "▶ " } else { "  " };

                if i == 1 {
                    if let State::Editing { input } = &app.state {
                        render_inline_input(f, area, style, label, prefix, input);
                        continue;
                    }
                }

                f.render_widget(
                    Paragraph::new(format!("{prefix}{label}")).style(style),
                    area,
                );
            }
        }
    }
}

fn render_inline_input(
    f: &mut ratatui::Frame,
    row: Rect,
    style: Style,
    label: &str,
    prefix: &str,
    input: &str,
) {
    let label_text = format!("{prefix}{label}  ");
    let label_w = label_text.chars().count() as u16;

    if row.width <= label_w + 2 {
        f.render_widget(Paragraph::new(format!("{prefix}{label}")).style(style), row);
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

    f.render_widget(Paragraph::new(label_text).style(style), label_area);
    f.render_widget(
        Paragraph::new(input).style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        input_area,
    );

    let cursor_x =
        (input_area.x + input.chars().count() as u16).min(input_area.x + input_area.width - 1);
    f.set_cursor_position((cursor_x, row.y));
}

fn render_hint(f: &mut ratatui::Frame, state: &State, area: Rect) {
    let hint = match state {
        State::Loading { .. } => "",
        State::Menu { .. } => "↑/↓  navigate   Enter  select   Esc/q  cancel",
        State::Editing { .. } => "Enter  confirm   Esc  back",
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
