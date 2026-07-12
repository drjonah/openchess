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
    EvalDepth,
    EvalMovetime,
    DefaultMode,
    FlipBoard,
    ShowEvalBar,
}

impl SettingsField {
    const ALL: [SettingsField; 11] = [
        SettingsField::BotDepth,
        SettingsField::BotMovetime,
        SettingsField::WhiteDepth,
        SettingsField::WhiteMovetime,
        SettingsField::BlackDepth,
        SettingsField::BlackMovetime,
        SettingsField::EvalDepth,
        SettingsField::EvalMovetime,
        SettingsField::DefaultMode,
        SettingsField::FlipBoard,
        SettingsField::ShowEvalBar,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsField::BotDepth => "Bot depth",
            SettingsField::BotMovetime => "Bot movetime (ms)",
            SettingsField::WhiteDepth => "White depth (BvB)",
            SettingsField::WhiteMovetime => "White movetime (ms)",
            SettingsField::BlackDepth => "Black depth (BvB)",
            SettingsField::BlackMovetime => "Black movetime (ms)",
            SettingsField::EvalDepth => "Eval depth",
            SettingsField::EvalMovetime => "Eval movetime (ms)",
            SettingsField::DefaultMode => "Default mode",
            SettingsField::FlipBoard => "Flip board",
            SettingsField::ShowEvalBar => "Show eval bar",
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
            SettingsField::EvalDepth => config.eval.depth.to_string(),
            SettingsField::EvalMovetime => config.eval.movetime_ms.to_string(),
            SettingsField::DefaultMode => config.tui.default_mode.title().to_string(),
            SettingsField::FlipBoard => on_off(config.tui.flip_board),
            SettingsField::ShowEvalBar => on_off(config.tui.show_eval_bar),
        }
    }
}

fn on_off(v: bool) -> String {
    if v {
        "on".into()
    } else {
        "off".into()
    }
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
            | SettingsField::EvalDepth
            | SettingsField::EvalMovetime => false,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, config: &Config, overlay: &SettingsOverlay) {
    let width = area.width.saturating_sub(2).min(68).max(44);
    let height = area.height.saturating_sub(1).min(22).max(16);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(4)])
        .split(popup);

    let items: Vec<ListItem> = SettingsField::ALL
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let marker = if i == overlay.selected { ">" } else { " " };
            let line = format!("{marker} {:<22} {}", field.label(), field.value(config));
            let style = if i == overlay.selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(line, style)))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(overlay.selected));

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
        "↑↓ select · ←→ adjust · Enter toggle\nAdvanced settings: edit {}",
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
