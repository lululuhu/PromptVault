//! Interactive TUI: browse commit history and inspect each commit's tree.
//!
//! Layout: left panel lists commits (newest first); right panel shows the
//! selected commit's message + tracked prompts. Press `q`/Esc to quit,
//! `j`/`k` or arrows to move, `Enter` to toggle the prompt list.

use std::io;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::core::objects::{self, Commit};
use crate::core::repository::Repo;

struct CommitInfo {
    hash: String,
    commit: Commit,
    entries: Vec<objects::TreeEntry>,
}

pub fn run() -> Result<()> {
    let repo = Repo::find()?;

    // Gather the full commit chain (newest first).
    let mut commits: Vec<CommitInfo> = Vec::new();
    let mut cur = repo.head_commit()?;
    while let Some(h) = cur {
        let c = objects::read_commit(&repo.pv_dir, &h)?;
        let entries = objects::read_tree(&repo.pv_dir, &c.tree).unwrap_or_default();
        let parent = c.parent.clone();
        commits.push(CommitInfo {
            hash: h.clone(),
            commit: c,
            entries,
        });
        cur = parent;
    }

    if commits.is_empty() {
        crate::ui::printer::warn("no commits yet — nothing to browse");
        return Ok(());
    }

    // Enter raw mode; if there's no TTY (e.g. piped output), give a clear hint.
    if let Err(e) = enable_raw_mode() {
        anyhow::bail!("cannot start TUI (is this a terminal?): {e}");
    }
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &commits);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    commits: &[CommitInfo],
) -> Result<()> {
    let mut list_state = ListState::default();
    list_state.select(Some(0));

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(f.area());

            // Left: commit list.
            let items: Vec<ListItem> = commits
                .iter()
                .map(|c| {
                    let short = &c.hash[..7];
                    let msg = c.commit.message.lines().next().unwrap_or("(no message)");
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{short} "),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(msg),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Commits"))
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, chunks[0], &mut list_state);

            // Right: details of selected commit.
            let detail = if let Some(i) = list_state.selected() {
                render_detail(&commits[i])
            } else {
                Paragraph::new("")
            };
            f.render_widget(detail, chunks[1]);
        })?;

        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = list_state.selected().unwrap_or(0);
                    if i + 1 < commits.len() {
                        list_state.select(Some(i + 1));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = list_state.selected().unwrap_or(0);
                    if i > 0 {
                        list_state.select(Some(i - 1));
                    }
                }
                KeyCode::Char('g') => list_state.select(Some(0)),
                KeyCode::Char('G') => list_state.select(Some(commits.len() - 1)),
                _ => {}
            }
        }
    }
}

fn render_detail<'a>(c: &CommitInfo) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("commit  ", Style::default().fg(Color::DarkGray)),
        Span::styled(c.hash.clone(), Style::default().fg(Color::Yellow)),
    ]));
    if let Some(p) = &c.commit.parent {
        lines.push(Line::from(vec![
            Span::styled("parent  ", Style::default().fg(Color::DarkGray)),
            Span::raw(p.clone()),
        ]));
    }
    if let Some(a) = &c.commit.author {
        lines.push(Line::from(vec![
            Span::styled("author  ", Style::default().fg(Color::DarkGray)),
            Span::raw(a.clone()),
        ]));
    }
    lines.push(Line::from(""));
    for l in c.commit.message.lines() {
        lines.push(Line::from(format!("    {l}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "tracked prompts:",
        Style::default().fg(Color::Cyan),
    )));
    for e in &c.entries {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}  ", &e.hash[..7]),
                Style::default().fg(Color::Green),
            ),
            Span::raw(e.path.clone()),
        ]));
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Detail"))
        .wrap(Wrap { trim: false })
}
