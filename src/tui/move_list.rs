//! Annotated move list with blank rating slots.

use super::game::{MoveClass, PlyRecord};
use super::session::EngineSession;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(frame: &mut Frame, area: Rect, session: &EngineSession) {
    let block = Block::default()
        .title(" Moves ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let Some(game) = session.analyzed() else {
        frame.render_widget(
            Paragraph::new("No game loaded\ni import · ←/→ step")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Header: players / result
    let h = &game.headers;
    if h.white.is_some() || h.black.is_some() {
        let white = h.white.as_deref().unwrap_or("?");
        let black = h.black.as_deref().unwrap_or("?");
        lines.push(Line::from(Span::styled(
            format!("{white} vs {black}"),
            Style::default().fg(Color::Cyan).bold(),
        )));
    }
    if let Some(ref result) = h.result {
        lines.push(Line::from(Span::styled(
            result.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }

    let header_rows = lines.len();
    let body_height = inner.height.saturating_sub(header_rows as u16) as usize;

    // Build move pairs: "1. e4 ·  e5 ·"
    let plies = &game.plies;
    let cursor = game.cursor;
    let mut pair_lines: Vec<(usize, Line<'static>)> = Vec::new(); // (first_ply_index, line)

    let mut i = 0usize;
    while i < plies.len() {
        let move_no = i / 2 + 1;
        let white = &plies[i];
        let white_hl = cursor == i + 1;
        let mut spans = vec![Span::styled(
            format!("{move_no}. "),
            Style::default().fg(Color::DarkGray),
        )];
        spans.extend(ply_spans(white, white_hl));

        if let Some(black) = plies.get(i + 1) {
            spans.push(Span::raw("  "));
            let black_hl = cursor == i + 2;
            spans.extend(ply_spans(black, black_hl));
        }
        pair_lines.push((i, Line::from(spans)));
        i += 2;
    }

    // Scroll so the cursor ply's pair stays visible.
    let focus_pair = if cursor == 0 {
        0
    } else {
        (cursor - 1) / 2
    };
    let total = pair_lines.len();
    let scroll = if total <= body_height {
        0
    } else {
        let max_scroll = total - body_height;
        focus_pair
            .saturating_sub(body_height / 2)
            .min(max_scroll)
    };

    for (idx, (_ply, line)) in pair_lines.into_iter().enumerate() {
        if idx < scroll {
            continue;
        }
        if lines.len() >= header_rows + body_height {
            break;
        }
        lines.push(line);
    }

    if plies.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no moves)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("ply {cursor}/{}", plies.len()),
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn ply_spans(ply: &PlyRecord, highlight: bool) -> Vec<Span<'static>> {
    let base = if highlight {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(180, 160, 70))
            .bold()
    } else {
        Style::default().fg(Color::White)
    };
    let mut spans = vec![Span::styled(ply.san.clone(), base)];
    spans.push(Span::raw(" "));
    spans.push(rating_span(ply.analysis.as_ref().map(|a| a.classification)));
    spans
}

fn rating_span(class: Option<MoveClass>) -> Span<'static> {
    match class {
        None => Span::styled("·", Style::default().fg(Color::DarkGray)),
        Some(c) => {
            let color = match c {
                MoveClass::Brilliant => Color::Cyan,
                MoveClass::Best | MoveClass::Excellent => Color::Green,
                MoveClass::Good | MoveClass::Book => Color::Gray,
                MoveClass::Inaccuracy => Color::Yellow,
                MoveClass::Mistake => Color::Rgb(255, 140, 0),
                MoveClass::Blunder | MoveClass::Miss => Color::Red,
            };
            Span::styled(c.glyph().to_string(), Style::default().fg(color))
        }
    }
}
