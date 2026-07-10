//! Overlay to pick a chess.com game after entering a username.

use crate::chesscom::{GameOutcome, GameSummary};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

/// How many games to reveal per page in the picker.
pub const PAGE_SIZE: usize = 10;

#[derive(Clone, Debug)]
pub struct GamePicker {
    pub username: String,
    /// Full newest-first list (also written to the session cache file).
    games: Vec<GameSummary>,
    /// How many leading games are currently visible.
    shown: usize,
    pub selected: usize,
}

impl GamePicker {
    pub fn new(username: String, games: Vec<GameSummary>) -> Self {
        let shown = games.len().min(PAGE_SIZE);
        Self {
            username,
            games,
            shown,
            selected: 0,
        }
    }

    pub fn total(&self) -> usize {
        self.games.len()
    }

    pub fn shown(&self) -> usize {
        self.shown
    }

    pub fn has_more(&self) -> bool {
        self.shown < self.games.len()
    }

    /// Reveal the next page from the cached list. Returns how many were added.
    pub fn load_more(&mut self) -> usize {
        let before = self.shown;
        self.shown = (self.shown + PAGE_SIZE).min(self.games.len());
        self.shown - before
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_down(&mut self) {
        let max = self.shown.saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn selected_game(&self) -> Option<&GameSummary> {
        self.games.get(self.selected)
    }

    fn visible(&self) -> &[GameSummary] {
        &self.games[..self.shown]
    }

    fn row_label(&self, game: &GameSummary, now_secs: u64) -> String {
        let color = game.color_for(&self.username).unwrap_or("?");
        let result = match game.outcome_for(&self.username) {
            Some(GameOutcome::Win) => "win",
            Some(GameOutcome::Loss) => "loss",
            Some(GameOutcome::Draw) => "draw",
            None => "?",
        };
        let opponent = game.opponent_for(&self.username);
        let when = game.relative_time(now_secs);
        format!("{color:<5}  {result:<4}  vs {opponent}  ·  {when}")
    }
}

pub fn render(frame: &mut Frame, area: Rect, picker: &GamePicker, now_secs: u64) {
    let width = area.width.saturating_sub(2).min(72).max(36);
    let height = area.height.saturating_sub(1).min(24).max(10);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup);

    let title = format!(
        " Games for {} ({}/{}) ",
        picker.username,
        picker.shown(),
        picker.total()
    );
    let hints = if picker.has_more() {
        " ↑↓ · m more · r refresh · Enter · Esc "
    } else {
        " ↑↓ · r refresh · Enter · Esc "
    };
    let block = Block::default()
        .title(title)
        .title_bottom(hints)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let visible = picker.visible();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("No games").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let marker = if i == picker.selected { "▶ " } else { "  " };
            let line = format!("{marker}{}", picker.row_label(g, now_secs));
            let style = if i == picker.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(picker.selected));
    frame.render_stateful_widget(List::new(items), inner, &mut state);
}
