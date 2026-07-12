//! Compact engine think panel (footer of the right column).

use crate::config::Config;
use super::session::{EngineSession, PlayMode, SearchInfo};
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

    let limits_line = if matches!(session.mode(), Some(PlayMode::BotVsBot)) {
        let (w_depth, w_ms) = match session.bvb_shared_side() {
            Some(crate::types::Color::White) => (config.bot.depth, config.bot.movetime_ms),
            _ => (config.bot.white.depth, config.bot.white.movetime_ms),
        };
        let (b_depth, b_ms) = match session.bvb_shared_side() {
            Some(crate::types::Color::Black) => (config.bot.depth, config.bot.movetime_ms),
            _ => (config.bot.black.depth, config.bot.black.movetime_ms),
        };
        format!(
            "W d{w_depth}/{w_ms}ms · B d{b_depth}/{b_ms}ms · eval d{}/{}ms · best {best}",
            config.eval.depth,
            config.eval.movetime_ms
        )
    } else {
        format!(
            "bot d{}/{}ms · eval d{}/{}ms · best {best}",
            config.bot.depth,
            config.bot.movetime_ms,
            config.eval.depth,
            config.eval.movetime_ms
        )
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
        Line::from(limits_line),
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
