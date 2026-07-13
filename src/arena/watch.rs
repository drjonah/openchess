//! Interactive ratatui arena inspector (P11-04 / P11-05 / P11-06).
//!
//! Two-pane layout: game list + detail drill-down. The arena keeps ticking
//! while you inspect — never call `search::go` on the UI thread.

use std::io::{self, Stdout};
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::{
    Constraint, CrosstermBackend, Direction, Frame, Layout, Rect, Style, Terminal,
};
use ratatui::style::Color as TermColor;
use ratatui::style::Stylize;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::board::Board;
use crate::tui::{board_view, engine_panel, eval_bar, move_list};
use crate::types::{Color, Move, Square};

use super::runner::{Arena, ArenaConfig};
use super::slot::SlotStatus;
use super::snapshot::GameSnapshot;

const MIN_DEPTH: u32 = 1;
const MAX_DEPTH: u32 = 64;
const MIN_MOVETIME_MS: u64 = 50;
const MAX_MOVETIME_MS: u64 = 60_000;
const MOVETIME_STEP_MS: i64 = 50;

struct WatchState {
    selected: usize,
    flipped: bool,
    status: String,
}

/// Run the interactive inspector until quit.
pub fn run(config: &ArenaConfig) -> ExitCode {
    match run_inner(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena watch failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_inner(config: &ArenaConfig) -> io::Result<()> {
    let mut terminal = setup()?;
    let result = run_app(&mut terminal, config);
    restore(&mut terminal)?;
    result
}

fn setup() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    config: &ArenaConfig,
) -> io::Result<()> {
    let mut arena = Arena::from_config(config);
    let mut state = WatchState {
        selected: 0,
        flipped: false,
        status: "↑/↓ select · f flip · p/r pause/resume · n restart · s step · a abort · [/] depth · {/} movetime · m mirror · q quit".into(),
    };
    let mut list_state = ListState::default();
    list_state.select(Some(0));

    loop {
        let _ = arena.tick();
        let snapshots = arena.snapshots();
        if state.selected >= snapshots.len() && !snapshots.is_empty() {
            state.selected = snapshots.len() - 1;
        }
        list_state.select(Some(state.selected));

        terminal.draw(|frame| {
            draw(frame, &snapshots, &state, &mut list_state);
        })?;

        if event::poll(Duration::from_millis(50))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    arena.shutdown();
                    break;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.selected > 0 {
                        state.selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.selected + 1 < snapshots.len() {
                        state.selected += 1;
                    }
                }
                KeyCode::Char('f') => {
                    state.flipped = !state.flipped;
                }
                KeyCode::Char('p') => {
                    arena.pause_slot(state.selected);
                    state.status = format!("slot {} paused", state.selected);
                }
                KeyCode::Char('r') => {
                    arena.resume_slot(state.selected);
                    state.status = format!("slot {} resumed", state.selected);
                }
                KeyCode::Char('n') => {
                    arena.restart_slot(state.selected);
                    state.status = format!("slot {} restarted", state.selected);
                }
                KeyCode::Char('s') => {
                    arena.step_slot(state.selected);
                    state.status = format!("slot {} step requested", state.selected);
                }
                KeyCode::Char('a') => {
                    arena.abort_slot(state.selected);
                    state.status = format!("slot {} aborted", state.selected);
                }
                KeyCode::Char('[') => {
                    adjust_selected_depth(&mut arena, &snapshots, state.selected, -1, &mut state);
                }
                KeyCode::Char(']') => {
                    adjust_selected_depth(&mut arena, &snapshots, state.selected, 1, &mut state);
                }
                KeyCode::Char('{') => {
                    adjust_selected_movetime(
                        &mut arena,
                        &snapshots,
                        state.selected,
                        -MOVETIME_STEP_MS,
                        &mut state,
                    );
                }
                KeyCode::Char('}') => {
                    adjust_selected_movetime(
                        &mut arena,
                        &snapshots,
                        state.selected,
                        MOVETIME_STEP_MS,
                        &mut state,
                    );
                }
                KeyCode::Char('m') => {
                    mirror_strengths(&mut arena, &snapshots, state.selected, &mut state);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn edit_color(snap: &GameSnapshot) -> Color {
    snap.side_to_move
}

fn adjust_selected_depth(
    arena: &mut Arena,
    snapshots: &[GameSnapshot],
    id: usize,
    delta: i32,
    state: &mut WatchState,
) {
    let Some(snap) = snapshots.get(id) else {
        return;
    };
    let color = edit_color(snap);
    let mut strength = match color {
        Color::White => snap.white.clone(),
        Color::Black => snap.black.clone(),
    };
    let next = (strength.depth as i32 + delta).clamp(MIN_DEPTH as i32, MAX_DEPTH as i32);
    strength.depth = next as u32;
    arena.set_slot_strength(id, color, strength.clone());
    state.status = format!(
        "slot {id} {:?} depth → {} (next move)",
        color, strength.depth
    );
}

fn adjust_selected_movetime(
    arena: &mut Arena,
    snapshots: &[GameSnapshot],
    id: usize,
    delta_ms: i64,
    state: &mut WatchState,
) {
    let Some(snap) = snapshots.get(id) else {
        return;
    };
    let color = edit_color(snap);
    let mut strength = match color {
        Color::White => snap.white.clone(),
        Color::Black => snap.black.clone(),
    };
    strength.movetime_ms = adjust_movetime(strength.movetime_ms, delta_ms);
    arena.set_slot_strength(id, color, strength.clone());
    state.status = format!(
        "slot {id} {:?} movetime → {}ms (next move)",
        color, strength.movetime_ms
    );
}

fn adjust_movetime(current: u64, delta_ms: i64) -> u64 {
    if current == 0 && delta_ms > 0 {
        return MIN_MOVETIME_MS;
    }
    let next = current as i64 + delta_ms;
    if next <= 0 {
        return 0;
    }
    next.clamp(MIN_MOVETIME_MS as i64, MAX_MOVETIME_MS as i64) as u64
}

fn mirror_strengths(
    arena: &mut Arena,
    snapshots: &[GameSnapshot],
    id: usize,
    state: &mut WatchState,
) {
    let Some(snap) = snapshots.get(id) else {
        return;
    };
    let white = snap.white.clone();
    let black = snap.black.clone();
    arena.set_all_strength(Color::White, white);
    arena.set_all_strength(Color::Black, black);
    state.status = format!("mirrored slot {id} strengths to all slots");
}

fn draw(
    frame: &mut Frame,
    snapshots: &[GameSnapshot],
    state: &WatchState,
    list_state: &mut ListState,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(chunks[0]);

    draw_game_list(frame, panes[0], snapshots, list_state);
    draw_detail(frame, panes[1], snapshots.get(state.selected), state.flipped);

    frame.render_widget(
        Paragraph::new(state.status.as_str()).style(Style::default().fg(TermColor::DarkGray)),
        chunks[1],
    );
}

fn draw_game_list(
    frame: &mut Frame,
    area: Rect,
    snapshots: &[GameSnapshot],
    list_state: &mut ListState,
) {
    let items: Vec<ListItem> = snapshots
        .iter()
        .map(|snap| ListItem::new(list_line(snap)))
        .collect();

    let list = List::new(items)
        .block(Block::default().title(" Games ").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(TermColor::Black)
                .bg(TermColor::Rgb(180, 160, 70))
                .bold(),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, list_state);
}

fn list_line(snap: &GameSnapshot) -> String {
    let glyph = match snap.status {
        SlotStatus::Thinking => "▶",
        SlotStatus::Paused => "⏸",
        SlotStatus::Finished => match snap.outcome {
            super::slot::Outcome::Draw | super::slot::Outcome::Unfinished => "½",
            _ => "#",
        },
        SlotStatus::Idle => "·",
    };
    let last = snap.last_move.as_deref().unwrap_or("-");
    let eval = snap
        .eval_white_cp
        .map(eval_bar::format_eval_label)
        .unwrap_or_else(|| "-".into());
    let profile = snap
        .profile
        .as_deref()
        .map(|p| format!(" [{p}]"))
        .unwrap_or_default();
    format!(
        "{:>2} {} ply {:>3}  {:<6} {:>5}{}",
        snap.id, glyph, snap.ply, last, eval, profile
    )
}

fn draw_detail(frame: &mut Frame, area: Rect, snap: Option<&GameSnapshot>, flipped: bool) {
    let Some(snap) = snap else {
        frame.render_widget(
            Paragraph::new("No games").block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Percentage(25),
            Constraint::Percentage(20),
        ])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(20),
            Constraint::Percentage(40),
        ])
        .split(rows[0]);

    eval_bar::render(frame, top[0], snap.eval_white_cp, flipped);

    let board = Board::from_fen(&snap.fen).unwrap_or_else(|_| Board::startpos());
    // last_move is already applied on the board, so parse squares from the UCI token.
    let last_move = parse_uci_squares(snap.last_move.as_deref());

    board_view::render_position(
        frame,
        top[1],
        &board,
        flipped,
        last_move,
        snap.material.balance_cp,
    );

    let move_title = format!(
        "Slot {} · {}",
        snap.id,
        snap.profile.as_deref().unwrap_or("default")
    );
    move_list::render_transcript(frame, top[2], &snap.transcript, &move_title);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let strength_text = format!(
        "W d{}/{}ms\nB d{}/{}ms\nSTM {:?}\n{}",
        snap.white.depth,
        snap.white.movetime_ms,
        snap.black.depth,
        snap.black.movetime_ms,
        snap.side_to_move,
        snap.outcome.result_tag(),
    );
    frame.render_widget(
        Paragraph::new(strength_text).block(
            Block::default()
                .title(" Strength ")
                .borders(Borders::ALL),
        ),
        mid[0],
    );

    let material_text = format!(
        "balance {}\nW {}cp  B {}cp",
        crate::tui::material::format_material_score(snap.material.balance_cp),
        snap.material.white_cp,
        snap.material.black_cp,
    );
    frame.render_widget(
        Paragraph::new(material_text).block(
            Block::default()
                .title(" Material ")
                .borders(Borders::ALL),
        ),
        mid[1],
    );

    let limits_line = format!(
        "W d{}/{}ms · B d{}/{}ms",
        snap.white.depth, snap.white.movetime_ms, snap.black.depth, snap.black.movetime_ms
    );
    let title = format!("Arena slot {}", snap.id);
    engine_panel::render_info(
        frame,
        rows[2],
        &snap.info,
        &title,
        &limits_line,
        &snap.info.pv,
        "p/r pause · n restart · s step · a abort · [/] depth",
        true,
    );
}

/// Build a highlight-only move from a UCI string without needing the prior board.
fn parse_uci_squares(uci: Option<&str>) -> Option<Move> {
    let uci = uci?;
    if uci.len() < 4 {
        return None;
    }
    let from = Square::from_str(&uci[0..2]).ok()?;
    let to = Square::from_str(&uci[2..4]).ok()?;
    Some(Move::new(from, to))
}
