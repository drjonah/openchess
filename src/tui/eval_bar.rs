//! Vertical eval bar (White vs Black). Empty until analysis fills scores.

use crate::types::score::{VALUE_MATE, VALUE_NONE};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Soft clamp: ±800 cp ≈ full bar.
const EVAL_CLAMP_CP: f32 = 800.0;

/// Render a narrow vertical eval bar.
///
/// `eval_cp` is White-relative. `None` draws a neutral empty state.
/// When `flipped` is true (Black at bottom), White's fill is at the top.
pub fn render(frame: &mut Frame, area: Rect, eval_cp: Option<i32>, flipped: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block = Block::default().borders(Borders::ALL).title(" E ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let white_frac = match eval_cp {
        None => None,
        Some(v) if v == VALUE_NONE => None,
        Some(cp) => Some(eval_to_white_fraction(cp)),
    };

    let mut lines: Vec<Line> = Vec::with_capacity(inner.height as usize);

    match white_frac {
        None => {
            // Neutral empty state — muted mid split, no analysis claim.
            let mid = inner.height / 2;
            for row in 0..inner.height {
                let style = if row == mid {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().bg(Color::Rgb(45, 45, 48)).fg(Color::DarkGray)
                };
                let ch = if row == mid { "—" } else { " " };
                lines.push(Line::from(Span::styled(
                    ch.repeat(inner.width as usize),
                    style,
                )));
            }
        }
        Some(frac) => {
            let white_rows = ((frac * inner.height as f32).round() as u16).min(inner.height);
            let black_rows = inner.height.saturating_sub(white_rows);
            // Screen top → bottom. If not flipped, Black is top (White at bottom).
            let (top_is_white, top_rows, bottom_rows) = if flipped {
                (true, white_rows, black_rows)
            } else {
                (false, black_rows, white_rows)
            };
            let top_style = if top_is_white {
                Style::default().bg(Color::Rgb(235, 235, 230)).fg(Color::Black)
            } else {
                Style::default().bg(Color::Rgb(40, 40, 42)).fg(Color::White)
            };
            let bottom_style = if top_is_white {
                Style::default().bg(Color::Rgb(40, 40, 42)).fg(Color::White)
            } else {
                Style::default().bg(Color::Rgb(235, 235, 230)).fg(Color::Black)
            };
            for _ in 0..top_rows {
                lines.push(Line::from(Span::styled(
                    " ".repeat(inner.width as usize),
                    top_style,
                )));
            }
            for _ in 0..bottom_rows {
                lines.push(Line::from(Span::styled(
                    " ".repeat(inner.width as usize),
                    bottom_style,
                )));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn eval_to_white_fraction(cp: i32) -> f32 {
    if cp >= VALUE_MATE - 1000 {
        return 1.0;
    }
    if cp <= -VALUE_MATE + 1000 {
        return 0.0;
    }
    let t = (cp as f32 / EVAL_CLAMP_CP).clamp(-1.0, 1.0);
    // Map [-1, 1] → [0, 1] with a soft curve near equality.
    0.5 + 0.5 * t
}
