//! Compact engine stub info (footer of the right column).

use crate::config::Config;
use super::session::{EngineSession, SearchInfo};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(frame: &mut Frame, area: Rect, session: &EngineSession, config: &Config) {
    let info: &SearchInfo = session.info();
    let thinking = if info.thinking { "yes" } else { "no" };
    let score = format!("{:+}", info.score_cp);
    let best = info.bestmove.as_deref().unwrap_or("-");
    let pv = if info.pv.is_empty() {
        "-"
    } else {
        info.pv.as_str()
    };

    let text = vec![
        Line::from(Span::styled(
            session.mode().title(),
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(format!(
            "d{}  {score}cp  n{}  think:{thinking}",
            info.depth, info.nodes
        )),
        Line::from(format!(
            "limits d{} / {}ms · PV {pv}  best {best}",
            config.bot.depth, config.bot.movetime_ms
        )),
        Line::from(Span::styled(
            "g go · s stop · , settings",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" Engine ")
        .borders(Borders::ALL);
    frame.render_widget(Paragraph::new(text).block(block), area);
}
