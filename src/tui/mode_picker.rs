//! Mode picker overlay — choose play mode before starting a game.

use super::session::PlayMode;
use crate::config::Config;
use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerScreen {
    Modes,
    Analyze,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnalyzeChoice {
    Startpos,
    Import,
}

impl AnalyzeChoice {
    const ALL: [AnalyzeChoice; 2] = [AnalyzeChoice::Startpos, AnalyzeChoice::Import];

    fn title(self) -> &'static str {
        match self {
            AnalyzeChoice::Startpos => "From starting position",
            AnalyzeChoice::Import => "Import FEN / PGN / game",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            AnalyzeChoice::Startpos => "Empty board — play moves, G for hints",
            AnalyzeChoice::Import => "Paste FEN, PGN, URL, username, or file path",
        }
    }
}

pub struct ModePickerOverlay {
    screen: PickerScreen,
    selected: usize,
    analyze_selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    Confirm(PlayMode),
    Navigate,
    OpenAnalyzeMenu,
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
        Self {
            screen: PickerScreen::Modes,
            selected,
            analyze_selected: 0,
        }
    }

    pub fn selected_mode(&self) -> PlayMode {
        PlayMode::ALL[self.selected]
    }

    fn select_next_mode(&mut self) {
        self.selected = (self.selected + 1) % PlayMode::ALL.len();
    }

    fn select_prev_mode(&mut self) {
        if self.selected == 0 {
            self.selected = PlayMode::ALL.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn select_next_analyze(&mut self) {
        self.analyze_selected = (self.analyze_selected + 1) % AnalyzeChoice::ALL.len();
    }

    fn select_prev_analyze(&mut self) {
        if self.analyze_selected == 0 {
            self.analyze_selected = AnalyzeChoice::ALL.len() - 1;
        } else {
            self.analyze_selected -= 1;
        }
    }

    fn open_analyze_menu(&mut self) -> PickerAction {
        self.screen = PickerScreen::Analyze;
        self.analyze_selected = 0;
        PickerAction::OpenAnalyzeMenu
    }

    fn back_to_modes(&mut self) -> PickerAction {
        self.screen = PickerScreen::Modes;
        // Keep Analyze highlighted on the main list.
        if let Some(i) = PlayMode::ALL.iter().position(|&m| m == PlayMode::Analyze) {
            self.selected = i;
        }
        PickerAction::Navigate
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Option<PickerAction> {
        match self.screen {
            PickerScreen::Modes => self.handle_modes_key(key),
            PickerScreen::Analyze => self.handle_analyze_key(key),
        }
    }

    fn handle_modes_key(&mut self, key: KeyCode) -> Option<PickerAction> {
        match key {
            KeyCode::Up => {
                self.select_prev_mode();
                Some(PickerAction::Navigate)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next_mode();
                Some(PickerAction::Navigate)
            }
            KeyCode::Enter => {
                let mode = self.selected_mode();
                if mode == PlayMode::Analyze {
                    Some(self.open_analyze_menu())
                } else {
                    Some(PickerAction::Confirm(mode))
                }
            }
            KeyCode::Char('p') => Some(PickerAction::Confirm(PlayMode::PlayerVsPlayer)),
            KeyCode::Char('w') => Some(PickerAction::Confirm(PlayMode::PlayerVsBot {
                human: crate::types::Color::White,
            })),
            KeyCode::Char('k') => Some(PickerAction::Confirm(PlayMode::PlayerVsBot {
                human: crate::types::Color::Black,
            })),
            KeyCode::Char('x') => Some(PickerAction::Confirm(PlayMode::BotVsBot)),
            KeyCode::Char('y') => Some(self.open_analyze_menu()),
            KeyCode::Char('i') => Some(PickerAction::StartImport),
            KeyCode::Char(',') => Some(PickerAction::OpenSettings),
            KeyCode::Char('?') => Some(PickerAction::OpenHelp),
            KeyCode::Char('q') => Some(PickerAction::Quit),
            KeyCode::Char('n') => Some(PickerAction::NewGame),
            _ => None,
        }
    }

    fn handle_analyze_key(&mut self, key: KeyCode) -> Option<PickerAction> {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev_analyze();
                Some(PickerAction::Navigate)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next_analyze();
                Some(PickerAction::Navigate)
            }
            KeyCode::Esc | KeyCode::Backspace => Some(self.back_to_modes()),
            KeyCode::Enter => match AnalyzeChoice::ALL[self.analyze_selected] {
                AnalyzeChoice::Startpos => Some(PickerAction::Confirm(PlayMode::Analyze)),
                AnalyzeChoice::Import => Some(PickerAction::StartImport),
            },
            KeyCode::Char('i') => Some(PickerAction::StartImport),
            KeyCode::Char('s') => Some(PickerAction::Confirm(PlayMode::Analyze)),
            KeyCode::Char(',') => Some(PickerAction::OpenSettings),
            KeyCode::Char('?') => Some(PickerAction::OpenHelp),
            KeyCode::Char('q') => Some(PickerAction::Quit),
            _ => None,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, overlay: &ModePickerOverlay) {
    match overlay.screen {
        PickerScreen::Modes => render_modes(frame, area, overlay),
        PickerScreen::Analyze => render_analyze(frame, area, overlay),
    }
}

fn render_modes(frame: &mut Frame, area: Rect, overlay: &ModePickerOverlay) {
    let width = area.width.saturating_sub(2).min(64).max(40);
    let height = area.height.saturating_sub(1).min(16).max(12);
    let popup = centered(area, width, height);
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

    let footer =
        "↑↓ select · Enter confirm · p/w/k/x/y shortcut\ni import · , settings · ? help · q quit";
    render_footer(frame, chunks[1], footer);
}

fn render_analyze(frame: &mut Frame, area: Rect, overlay: &ModePickerOverlay) {
    let width = area.width.saturating_sub(2).min(64).max(40);
    let height = area.height.saturating_sub(1).min(12).max(10);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(4)])
        .split(popup);

    let items: Vec<ListItem> = AnalyzeChoice::ALL
        .iter()
        .enumerate()
        .map(|(i, choice)| {
            let marker = if i == overlay.analyze_selected {
                ">"
            } else {
                " "
            };
            let line = format!("{marker} {} — {}", choice.title(), choice.blurb());
            let style = if i == overlay.analyze_selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(line, style)))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(overlay.analyze_selected));

    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(" Analyze ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        chunks[0],
        &mut state,
    );

    let footer = "↑↓ select · Enter confirm · s startpos · i import\nEsc back · , settings · ? help · q quit";
    render_footer(frame, chunks[1], footer);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

fn render_footer(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
        area,
    );
}
