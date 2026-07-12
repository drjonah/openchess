//! Compact engine think panel (footer of the right column).

use crate::config::Config;
use super::session::{EngineSession, SearchInfo};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(frame: &mut Frame, area: Rect, session: &EngineSession, config: &Config) {
    let info: &SearchInfo = session.info();
    let score = format!("{:+}", info.score_cp);
    let show_hints = session.show_engine_hints();
    let best = if show_hints {
        info.bestmove.as_deref().unwrap_or("-")
    } else {
        "-"
    };
    let ms = info.time.as_millis();
    let nps = if ms > 0 {
        info.nodes.saturating_mul(1000) / ms as u64
    } else {
        0
    };
    let pv = if !show_hints || info.pv.is_empty() {
        "-"
    } else {
        info.pv.as_str()
    };
    let status = if info.thinking {
        "thinking…"
    } else {
        "idle"
    };

    let text = vec![
        Line::from(Span::styled(
            session.mode_title(),
            if session.mode().is_some() {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        Line::from(Span::styled(
            status,
            if info.thinking {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        Line::from(format!(
            "d{}  {score}cp  n{}  {ms}ms  {nps} nps",
            info.depth, info.nodes
        )),
        Line::from(format!("PV {pv}")),
        Line::from(format!(
            "limits d{} / {}ms · best {best}",
            config.bot.depth, config.bot.movetime_ms
        )),
        Line::from(Span::styled(
            "G go · s stop · , settings",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" Engine ")
        .borders(Borders::ALL);
    frame.render_widget(Paragraph::new(text).block(block), area);
}
