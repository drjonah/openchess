//! Settings overlay — edits common config fields and saves JSON immediately.

use crate::config::Config;
use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsField {
    BotDepth,
    BotMovetime,
    WhiteDepth,
    WhiteMovetime,
    BlackDepth,
    BlackMovetime,
    AnalysisDepth,
    AnalysisMovetime,
    ShowEvalBar,
    EvalDepth,
    EvalMovetime,
    DefaultMode,
    FlipBoard,
}

impl SettingsField {
    const ALL: [SettingsField; 13] = [
        SettingsField::BotDepth,
        SettingsField::BotMovetime,
        SettingsField::WhiteDepth,
        SettingsField::WhiteMovetime,
        SettingsField::BlackDepth,
        SettingsField::BlackMovetime,
        SettingsField::AnalysisDepth,
        SettingsField::AnalysisMovetime,
        SettingsField::ShowEvalBar,
        SettingsField::EvalDepth,
        SettingsField::EvalMovetime,
        SettingsField::DefaultMode,
        SettingsField::FlipBoard,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsField::BotDepth => "Depth",
            SettingsField::BotMovetime => "Movetime (ms)",
            SettingsField::WhiteDepth => "White depth",
            SettingsField::WhiteMovetime => "White movetime (ms)",
            SettingsField::BlackDepth => "Black depth",
            SettingsField::BlackMovetime => "Black movetime (ms)",
            SettingsField::AnalysisDepth => "Depth",
            SettingsField::AnalysisMovetime => "Movetime (ms)",
            SettingsField::ShowEvalBar => "Show by default",
            SettingsField::EvalDepth => "Search depth",
            SettingsField::EvalMovetime => "Search movetime (ms)",
            SettingsField::DefaultMode => "Default mode",
            SettingsField::FlipBoard => "Flip board",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            SettingsField::BotDepth | SettingsField::BotMovetime => {
                "Player vs Bot and Analyze / G search"
            }
            SettingsField::WhiteDepth
            | SettingsField::WhiteMovetime
            | SettingsField::BlackDepth
            | SettingsField::BlackMovetime => "Only used in Bot vs Bot (per-side strength)",
            SettingsField::AnalysisDepth | SettingsField::AnalysisMovetime => {
                "Used when analyzing imported games"
            }
            SettingsField::ShowEvalBar => "Toggle anytime with v; always on for imported games",
            SettingsField::EvalDepth | SettingsField::EvalMovetime => {
                "Live eval bar only — separate from bot moves"
            }
            SettingsField::DefaultMode => "Suggested mode when starting a new game",
            SettingsField::FlipBoard => "Black at bottom; auto-on when you play as Black",
        }
    }

    fn value(self, config: &Config) -> String {
        match self {
            SettingsField::BotDepth => config.bot.depth.to_string(),
            SettingsField::BotMovetime => config.bot.movetime_ms.to_string(),
            SettingsField::WhiteDepth => config.bot.white.depth.to_string(),
            SettingsField::WhiteMovetime => config.bot.white.movetime_ms.to_string(),
            SettingsField::BlackDepth => config.bot.black.depth.to_string(),
            SettingsField::BlackMovetime => config.bot.black.movetime_ms.to_string(),
            SettingsField::AnalysisDepth => config.analysis.depth.to_string(),
            SettingsField::AnalysisMovetime => config.analysis.movetime_ms.to_string(),
            SettingsField::ShowEvalBar => on_off(config.tui.show_eval_bar),
            SettingsField::EvalDepth => config.eval.depth.to_string(),
            SettingsField::EvalMovetime => config.eval.movetime_ms.to_string(),
            SettingsField::DefaultMode => config.tui.default_mode.title().to_string(),
            SettingsField::FlipBoard => on_off(config.tui.flip_board),
        }
    }
}

/// Visual rows: section headers plus selectable fields.
#[derive(Clone, Copy, Debug)]
enum SettingsRow {
    Header(&'static str),
    Field(SettingsField),
}

const ROWS: &[SettingsRow] = &[
    SettingsRow::Header("Player vs Bot / Analyze"),
    SettingsRow::Field(SettingsField::BotDepth),
    SettingsRow::Field(SettingsField::BotMovetime),
    SettingsRow::Header("Bot vs Bot"),
    SettingsRow::Field(SettingsField::WhiteDepth),
    SettingsRow::Field(SettingsField::WhiteMovetime),
    SettingsRow::Field(SettingsField::BlackDepth),
    SettingsRow::Field(SettingsField::BlackMovetime),
    SettingsRow::Header("Post-game analysis"),
    SettingsRow::Field(SettingsField::AnalysisDepth),
    SettingsRow::Field(SettingsField::AnalysisMovetime),
    SettingsRow::Header("Eval bar"),
    SettingsRow::Field(SettingsField::ShowEvalBar),
    SettingsRow::Field(SettingsField::EvalDepth),
    SettingsRow::Field(SettingsField::EvalMovetime),
    SettingsRow::Header("Display"),
    SettingsRow::Field(SettingsField::DefaultMode),
    SettingsRow::Field(SettingsField::FlipBoard),
];

fn on_off(v: bool) -> String {
    if v {
        "on".into()
    } else {
        "off".into()
    }
}

fn field_row_index(field: SettingsField) -> usize {
    ROWS.iter()
        .position(|row| matches!(row, SettingsRow::Field(f) if *f == field))
        .expect("every SettingsField appears in ROWS")
}

pub struct SettingsOverlay {
    selected: usize,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self { selected: 0 }
    }
}

impl SettingsOverlay {
    fn selected_field(&self) -> SettingsField {
        SettingsField::ALL[self.selected]
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % SettingsField::ALL.len();
    }

    pub fn select_prev(&mut self) {
        if self.selected == 0 {
            self.selected = SettingsField::ALL.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Adjust the selected field. Returns true if config changed.
    pub fn handle_key(&mut self, key: KeyCode, config: &mut Config) -> bool {
        let field = self.selected_field();
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                false
            }
            KeyCode::Left | KeyCode::Char('-') => self.adjust(field, config, -1),
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                self.adjust(field, config, 1)
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(field, config),
            _ => false,
        }
    }

    fn adjust(&self, field: SettingsField, config: &mut Config, dir: i32) -> bool {
        match field {
            SettingsField::BotDepth => {
                config.adjust_depth(dir);
                true
            }
            SettingsField::BotMovetime => {
                config.adjust_movetime(i64::from(dir) * 50);
                true
            }
            SettingsField::WhiteDepth => {
                config.adjust_white_depth(dir);
                true
            }
            SettingsField::WhiteMovetime => {
                config.adjust_white_movetime(i64::from(dir) * 50);
                true
            }
            SettingsField::BlackDepth => {
                config.adjust_black_depth(dir);
                true
            }
            SettingsField::BlackMovetime => {
                config.adjust_black_movetime(i64::from(dir) * 50);
                true
            }
            SettingsField::AnalysisDepth => {
                config.adjust_analysis_depth(dir);
                true
            }
            SettingsField::AnalysisMovetime => {
                config.adjust_analysis_movetime(i64::from(dir) * 50);
                true
            }
            SettingsField::EvalDepth => {
                config.adjust_eval_depth(dir);
                true
            }
            SettingsField::EvalMovetime => {
                config.adjust_eval_movetime(i64::from(dir) * 50);
                true
            }
            SettingsField::DefaultMode => {
                config.tui.default_mode = if dir < 0 {
                    config.tui.default_mode.prev()
                } else {
                    config.tui.default_mode.next()
                };
                // Match session.set_mode: auto-flip when playing as Black.
                config.tui.flip_board =
                    config.tui.default_mode == crate::config::DefaultPlayMode::PlayerVsBotBlack;
                true
            }
            SettingsField::FlipBoard => {
                config.tui.flip_board = !config.tui.flip_board;
                true
            }
            SettingsField::ShowEvalBar => {
                config.tui.show_eval_bar = !config.tui.show_eval_bar;
                true
            }
        }
    }

    fn activate(&self, field: SettingsField, config: &mut Config) -> bool {
        match field {
            SettingsField::DefaultMode => {
                config.tui.default_mode = config.tui.default_mode.next();
                config.tui.flip_board =
                    config.tui.default_mode == crate::config::DefaultPlayMode::PlayerVsBotBlack;
                true
            }
            SettingsField::FlipBoard => {
                config.tui.flip_board = !config.tui.flip_board;
                true
            }
            SettingsField::ShowEvalBar => {
                config.tui.show_eval_bar = !config.tui.show_eval_bar;
                true
            }
            SettingsField::BotDepth
            | SettingsField::BotMovetime
            | SettingsField::WhiteDepth
            | SettingsField::WhiteMovetime
            | SettingsField::BlackDepth
            | SettingsField::BlackMovetime
            | SettingsField::AnalysisDepth
            | SettingsField::AnalysisMovetime
            | SettingsField::EvalDepth
            | SettingsField::EvalMovetime => false,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, config: &Config, overlay: &SettingsOverlay) {
    let width = area.width.saturating_sub(2).min(72).max(48);
    let height = area.height.saturating_sub(1).min(30).max(20);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(12), Constraint::Length(5)])
        .split(popup);

    let selected_field = overlay.selected_field();
    let items: Vec<ListItem> = ROWS
        .iter()
        .map(|row| match row {
            SettingsRow::Header(title) => {
                let line = format!("── {title} ──");
                ListItem::new(Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Yellow).bold(),
                )))
            }
            SettingsRow::Field(field) => {
                let selected = *field == selected_field;
                let marker = if selected { ">" } else { " " };
                let line = format!("{marker} {:<24} {}", field.label(), field.value(config));
                let style = if selected {
                    Style::default().fg(Color::Cyan).bold()
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(line, style)))
            }
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(field_row_index(selected_field)));

    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(" Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        chunks[0],
        &mut state,
    );

    let path = Config::path();
    let footer = format!(
        "{}\n↑↓ select · ←→ adjust · Enter toggle · Esc/, close\nAdvanced: {}",
        selected_field.hint(),
        path.display()
    );
    frame.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
        chunks[1],
    );
}
