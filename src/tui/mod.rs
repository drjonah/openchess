//! OpenChess terminal UI (ratatui).

mod board_view;
mod engine_panel;
mod piece_art;
mod eval_bar;
mod game;
#[cfg(feature = "chesscom")]
mod game_picker;
mod import;
mod input;
mod move_list;
mod settings;
pub mod session;

use crate::config::{Config, DefaultPlayMode};
use crate::types::Color as Side;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use import::ImportResult;
use input::{InputAction, MoveInput, PromptKind, HELP_PAGES};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use session::{EngineSession, PlayMode};
use settings::SettingsOverlay;
use std::io::{self, Stdout};
use std::time::Duration;
#[cfg(feature = "chesscom")]
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run() -> io::Result<()> {
    let mut terminal = setup()?;
    let result = run_app(&mut terminal);
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

#[cfg(feature = "chesscom")]
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let (mut config, load_msg) = Config::load();
    let mut session = EngineSession::new_with_config(&config);
    if let Some(msg) = load_msg {
        session.set_status(msg);
    }
    let mut input = MoveInput::default();
    let mut show_help = false;
    let mut help_page: usize = 0;
    let mut show_settings = false;
    let mut settings = SettingsOverlay::default();
    #[cfg(feature = "chesscom")]
    let mut game_picker: Option<game_picker::GamePicker> = None;
    maybe_start_engine(&mut session, &config);
    maybe_refresh_live_eval(&mut session, &config);

    loop {
        session.poll();
        maybe_start_engine(&mut session, &config);
        maybe_refresh_live_eval(&mut session, &config);
        terminal.draw(|frame| {
            draw(
                frame,
                &session,
                &input,
                &config,
                show_help.then_some(help_page),
                show_settings.then_some(&settings),
                #[cfg(feature = "chesscom")]
                game_picker.as_ref(),
            )
        })?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if show_help {
            match key.code {
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    show_help = false;
                    session.set_status(format!("{} — press ? for help", session.mode().title()));
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('[') => {
                    if help_page == 0 {
                        help_page = HELP_PAGES.len() - 1;
                    } else {
                        help_page -= 1;
                    }
                    session.set_status(help_status(help_page));
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(']') => {
                    help_page = (help_page + 1) % HELP_PAGES.len();
                    session.set_status(help_status(help_page));
                }
                _ => {}
            }
            continue;
        }

        if show_settings {
            match key.code {
                KeyCode::Char(',') | KeyCode::Esc => {
                    show_settings = false;
                    session.set_status(format!(
                        "{} — , settings · ? help",
                        session.mode().title()
                    ));
                }
                code => {
                    if settings.handle_key(code, &mut config) {
                        persist_and_apply(&mut config, &mut session);
                    }
                }
            }
            continue;
        }

        let importing = input.prompt() == PromptKind::Import;
        let Some(action) = input.handle_key(key) else {
            continue;
        };

        match action {
            InputAction::Quit => break,
            InputAction::Help => {
                show_help = true;
                help_page = 0;
                session.set_status(help_status(help_page));
            }
            InputAction::OpenSettings => {
                show_settings = true;
                settings = SettingsOverlay::default();
                session.set_status("Settings — Esc/, to close");
            }
            InputAction::NewGame => {
                session.new_game();
                maybe_start_engine(&mut session, &config);
            }
            InputAction::Undo => {
                let _ = session.undo();
            }
            InputAction::Flip => {
                session.toggle_flip();
                config.tui.flip_board = session.flipped();
                let _ = config.save();
            }
            InputAction::ToggleEvalBar => {
                session.toggle_eval_bar();
                if session.analyzed().is_none() {
                    config.tui.show_eval_bar = session.eval_bar_forced();
                    let _ = config.save();
                }
            }
            InputAction::EngineGo => {
                if !session.is_thinking() {
                    session.go(config.go_limits());
                }
            }
            InputAction::Stop => session.stop(),
            InputAction::ModePlayerVsPlayer => {
                session.set_mode(PlayMode::PlayerVsPlayer);
                sync_mode_to_config(&mut config, &session);
            }
            InputAction::ModePlayerVsBotWhite => {
                session.set_mode(PlayMode::PlayerVsBot {
                    human: Side::White,
                });
                sync_mode_to_config(&mut config, &session);
                maybe_start_engine(&mut session, &config);
            }
            InputAction::ModePlayerVsBotBlack => {
                session.set_mode(PlayMode::PlayerVsBot {
                    human: Side::Black,
                });
                sync_mode_to_config(&mut config, &session);
                maybe_start_engine(&mut session, &config);
            }
            InputAction::ModeBotVsBot => {
                session.set_mode(PlayMode::BotVsBot);
                sync_mode_to_config(&mut config, &session);
                maybe_start_engine(&mut session, &config);
            }
            InputAction::ModeAnalyze => {
                session.set_mode(PlayMode::Analyze);
                sync_mode_to_config(&mut config, &session);
            }
            InputAction::StartImport => {
                #[cfg(feature = "chesscom")]
                {
                    game_picker = None;
                }
                input.start_import();
                session.set_status(
                    "Import FEN / PGN / URL / username / file — Enter · Esc cancel",
                );
            }
            InputAction::CancelImport => {
                input.cancel_import();
                session.set_status("Import cancelled");
            }
            InputAction::StepBack => {
                let _ = session.step_back();
            }
            InputAction::StepForward => {
                let _ = session.step_forward();
            }
            InputAction::GotoStart => {
                let _ = session.goto_start();
            }
            InputAction::GotoEnd => {
                let _ = session.goto_end();
            }
            #[cfg(feature = "chesscom")]
            InputAction::ListUp => {
                if let Some(p) = game_picker.as_mut() {
                    p.select_up();
                }
            }
            #[cfg(feature = "chesscom")]
            InputAction::ListDown => {
                if let Some(p) = game_picker.as_mut() {
                    p.select_down();
                }
            }
            #[cfg(feature = "chesscom")]
            InputAction::SelectGame => {
                if let Some(picker) = game_picker.as_ref() {
                    let pgn = picker.selected_game().and_then(|g| g.pgn.clone());
                    match pgn {
                        Some(pgn) => match import::import_pgn(&mut session, &pgn) {
                            Ok(()) => {
                                game_picker = None;
                                input.cancel_import();
                                maybe_start_engine(&mut session, &config);
                            }
                            Err(e) => session.set_status(e),
                        },
                        None => session.set_status("selected game has no PGN"),
                    }
                }
            }
            #[cfg(feature = "chesscom")]
            InputAction::CancelGameList => {
                game_picker = None;
                input.back_to_import();
                session.set_status("Enter a chess.com username — Enter · Esc cancel");
            }
            #[cfg(feature = "chesscom")]
            InputAction::LoadMoreGames => {
                if let Some(p) = game_picker.as_mut() {
                    let added = p.load_more();
                    if added == 0 {
                        session.set_status(format!(
                            "Showing all {} games for {} — r refresh",
                            p.total(),
                            p.username
                        ));
                    } else {
                        session.set_status(format!(
                            "Showing {}/{} for {} — m more · r refresh",
                            p.shown(),
                            p.total(),
                            p.username
                        ));
                    }
                }
            }
            #[cfg(feature = "chesscom")]
            InputAction::RefreshGames => {
                if let Some(picker) = game_picker.as_ref() {
                    let user = picker.username.clone();
                    session.set_status(format!("Refreshing {user}…"));
                    terminal.draw(|frame| {
                        draw(
                            frame,
                            &session,
                            &input,
                            &config,
                            show_help.then_some(help_page),
                            show_settings.then_some(&settings),
                            game_picker.as_ref(),
                        )
                    })?;
                    match import::refresh_chesscom_user(&user) {
                        Ok(ImportResult::BrowseGames {
                            username,
                            games,
                            from_cache: _,
                            fetched_at: _,
                        }) => {
                            let total = games.len();
                            let shown = total.min(game_picker::PAGE_SIZE);
                            game_picker = Some(game_picker::GamePicker::new(username.clone(), games));
                            let more = if total > shown { " · m more" } else { "" };
                            session.set_status(format!(
                                "Showing {shown}/{total} for {username} (fresh){more} — r refresh"
                            ));
                        }
                        Ok(ImportResult::Loaded) => {
                            session.set_status("unexpected refresh result");
                        }
                        Err(e) => session.set_status(e),
                    }
                }
            }
            InputAction::Submit(text) => {
                if importing {
                    session.set_status("Importing…");
                    // Draw status before any blocking HTTP.
                    terminal.draw(|frame| {
                        draw(
                            frame,
                            &session,
                            &input,
                            &config,
                            show_help.then_some(help_page),
                            show_settings.then_some(&settings),
                            #[cfg(feature = "chesscom")]
                            game_picker.as_ref(),
                        )
                    })?;
                    match import::import_into(&mut session, &text) {
                        Ok(ImportResult::Loaded) => maybe_start_engine(&mut session, &config),
                        #[cfg(feature = "chesscom")]
                        Ok(ImportResult::BrowseGames {
                            username,
                            games,
                            from_cache,
                            fetched_at,
                        }) => {
                            let total = games.len();
                            let shown = total.min(game_picker::PAGE_SIZE);
                            game_picker =
                                Some(game_picker::GamePicker::new(username.clone(), games));
                            input.start_game_list();
                            let more = if total > shown { " · m more" } else { "" };
                            let source = if from_cache {
                                let age =
                                    crate::chesscom::format_cache_age(fetched_at, now_unix_secs());
                                format!("cached {age}")
                            } else {
                                "fresh".into()
                            };
                            session.set_status(format!(
                                "Showing {shown}/{total} for {username} ({source}){more} · r refresh"
                            ));
                        }
                        Err(e) => {
                            input.reopen_import();
                            session.set_status(e);
                        }
                    }
                } else if session.is_thinking() {
                    session.set_status("Wait for engine (or s to stop)");
                } else if matches!(session.mode(), PlayMode::BotVsBot) {
                    session.set_status("Bot vs Bot: bots move automatically");
                } else if !session.is_human_turn() {
                    session.set_status("Not your turn — wait for bot");
                } else {
                    match session.play_text(text.trim()) {
                        Ok(()) => maybe_start_engine(&mut session, &config),
                        Err(e) => session.set_status(e),
                    }
                }
            }
            InputAction::Redraw => {}
        }
    }
    Ok(())
}

fn sync_mode_to_config(config: &mut Config, session: &EngineSession) {
    config.tui.default_mode = DefaultPlayMode::from_play_mode(session.mode());
    config.tui.flip_board = session.flipped();
    let _ = config.save();
}

fn persist_and_apply(config: &mut Config, session: &mut EngineSession) {
    config.clamp();
    if let Err(e) = config.save() {
        session.set_status(format!("could not save config: {e}"));
        return;
    }
    session.apply_tui_config(config);
    session.set_status(format!(
        "Saved · depth {} · {}ms · {}",
        config.bot.depth,
        config.bot.movetime_ms,
        Config::path().display()
    ));
}

fn help_status(page: usize) -> String {
    let total = HELP_PAGES.len();
    let title = HELP_PAGES[page].title;
    format!("Help {title} ({}/{total}) — ←→ pages · Esc close", page + 1)
}

fn maybe_start_engine(session: &mut EngineSession, config: &Config) {
    if session.is_thinking() || !session.engine_should_auto_move() {
        return;
    }
    session.go(config.go_limits());
}

/// Refresh the live eval bar when the position is stale and the engine is idle.
///
/// Skips when the bot is about to auto-move — that search updates the bar instead.
fn maybe_refresh_live_eval(session: &mut EngineSession, config: &Config) {
    if session.is_thinking()
        || !session.show_eval_bar()
        || !session.live_eval_stale()
        || session.engine_should_auto_move()
        || session.analyzed().is_some()
    {
        return;
    }
    session.go_eval(config.go_limits());
}

fn draw(
    frame: &mut Frame,
    session: &EngineSession,
    input: &MoveInput,
    config: &Config,
    help_page: Option<usize>,
    settings_overlay: Option<&SettingsOverlay>,
    #[cfg(feature = "chesscom")] picker: Option<&game_picker::GamePicker>,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(12),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    let top = if session.show_eval_bar() {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(5),
                Constraint::Percentage(55),
                Constraint::Percentage(45),
            ])
            .split(chunks[0])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[0])
    };

    let (board_area, right_area) = if session.show_eval_bar() {
        eval_bar::render(frame, top[0], session.current_eval(), session.flipped());
        (top[1], top[2])
    } else {
        (top[0], top[1])
    };

    board_view::render(frame, board_area, session);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(6)])
        .split(right_area);
    move_list::render(frame, right[0], session);
    engine_panel::render(frame, right[1], session, config);

    let title = match input.prompt() {
        PromptKind::Move => " Your move (e4 or e2e4) ",
        PromptKind::Import => " Import ",
        #[cfg(feature = "chesscom")]
        PromptKind::GameList => " chess.com games ",
    };
    let input_line = match input.prompt() {
        #[cfg(feature = "chesscom")]
        PromptKind::GameList => "> (select a game)_".to_string(),
        _ => format!("> {}_", input.text()),
    };
    frame.render_widget(
        Paragraph::new(input_line).block(Block::default().title(title).borders(Borders::ALL)),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(session.status()).style(Style::default().fg(Color::Yellow)),
        chunks[2],
    );

    #[cfg(feature = "chesscom")]
    if let Some(picker) = picker {
        if input.prompt() == PromptKind::GameList {
            game_picker::render(frame, area, picker, now_unix_secs());
        }
    }

    if let Some(page) = help_page {
        let page = page.min(HELP_PAGES.len().saturating_sub(1));
        let help = &HELP_PAGES[page];
        let width = area.width.saturating_sub(2).min(56).max(28);
        let height = area.height.saturating_sub(1).min(22).max(12);
        let popup = Rect::new(
            area.x + (area.width.saturating_sub(width)) / 2,
            area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        );
        let title = format!(
            " Help · {} ({}/{}) ",
            help.title,
            page + 1,
            HELP_PAGES.len()
        );
        let body = format!(
            "{}\n  ←→ / h l   next page\n  Esc / ?    close",
            help.body
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(body)
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    if let Some(overlay) = settings_overlay {
        settings::render(frame, area, config, overlay);
    }
}
