//! Mode picker overlay — choose play mode before starting a game.

use super::session::PlayMode;
use crate::config::Config;
use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

pub struct ModePickerOverlay {
    selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    Confirm(PlayMode),
    Navigate,
    StartImport,
    OpenSettings,
    OpenHelp,
    Quit,
    NewGame,
}

impl ModePickerOverlay {
    pub fn new(config: &Config) -> Self {
        let preferred = config.tui.default_mode.to_play_mode();
        let selected = PlayMode::ALL
            .iter()
            .position(|&m| m == preferred)
            .unwrap_or(0);
        Self { selected }
    }

    pub fn selected_mode(&self) -> PlayMode {
        PlayMode::ALL[self.selected]
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % PlayMode::ALL.len();
    }

    pub fn select_prev(&mut self) {
        if self.selected == 0 {
            self.selected = PlayMode::ALL.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Option<PickerAction> {
        match key {
            KeyCode::Up => {
                self.select_prev();
                Some(PickerAction::Navigate)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                Some(PickerAction::Navigate)
            }
            KeyCode::Enter => Some(PickerAction::Confirm(self.selected_mode())),
            KeyCode::Char('p') => Some(PickerAction::Confirm(PlayMode::PlayerVsPlayer)),
            KeyCode::Char('w') => Some(PickerAction::Confirm(PlayMode::PlayerVsBot {
                human: crate::types::Color::White,
            })),
            KeyCode::Char('k') => Some(PickerAction::Confirm(PlayMode::PlayerVsBot {
                human: crate::types::Color::Black,
            })),
            KeyCode::Char('x') => Some(PickerAction::Confirm(PlayMode::BotVsBot)),
            KeyCode::Char('y') => Some(PickerAction::Confirm(PlayMode::Analyze)),
            KeyCode::Char('i') => Some(PickerAction::StartImport),
            KeyCode::Char(',') => Some(PickerAction::OpenSettings),
            KeyCode::Char('?') => Some(PickerAction::OpenHelp),
            KeyCode::Char('q') => Some(PickerAction::Quit),
            KeyCode::Char('n') => Some(PickerAction::NewGame),
            _ => None,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, overlay: &ModePickerOverlay) {
    let width = area.width.saturating_sub(2).min(64).max(40);
    let height = area.height.saturating_sub(1).min(16).max(12);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(popup);

    let items: Vec<ListItem> = PlayMode::ALL
        .iter()
        .enumerate()
        .map(|(i, mode)| {
            let marker = if i == overlay.selected { ">" } else { " " };
            let line = format!("{marker} {} — {}", mode.title(), mode.blurb());
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
                .title(" Choose game mode ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        chunks[0],
        &mut state,
    );

    let footer = "↑↓ select · Enter confirm · p/w/k/x/y shortcut\ni import · , settings · ? help · q quit";
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
