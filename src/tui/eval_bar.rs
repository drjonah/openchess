//! Vertical eval bar (White vs Black). Empty until analysis fills scores.

use crate::types::score::{
    VALUE_MATE, VALUE_NONE, is_loss_score, is_win_score,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Soft clamp: ±800 cp ≈ full bar.
const EVAL_CLAMP_CP: f32 = 800.0;

/// Render a narrow vertical eval bar with an optional numeric overlay.
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
                    Style::default()
                        .bg(Color::Rgb(45, 45, 48))
                        .fg(Color::DarkGray)
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
                Style::default()
                    .bg(Color::Rgb(235, 235, 230))
                    .fg(Color::Black)
            } else {
                Style::default()
                    .bg(Color::Rgb(40, 40, 42))
                    .fg(Color::White)
            };
            let bottom_style = if top_is_white {
                Style::default()
                    .bg(Color::Rgb(40, 40, 42))
                    .fg(Color::White)
            } else {
                Style::default()
                    .bg(Color::Rgb(235, 235, 230))
                    .fg(Color::Black)
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

            // Numeric overlay on the majority side, near the fill boundary.
            if let Some(cp) = eval_cp.filter(|&v| v != VALUE_NONE) {
                let label = format_eval_label(cp);
                let (label_row, style) = label_row_and_style(
                    inner.height,
                    top_rows,
                    top_is_white,
                    frac,
                    top_style,
                    bottom_style,
                );
                if let Some(line) = lines.get_mut(label_row as usize) {
                    let w = inner.width as usize;
                    let text = fit_label(&label, w);
                    *line = Line::from(Span::styled(text, style));
                }
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Compact White-relative eval for the bar overlay (`+0.35`, `-1.2`, `M3`, `-M2`).
pub fn format_eval_label(cp: i32) -> String {
    if is_win_score(cp) {
        let ply = VALUE_MATE - cp;
        format!("M{ply}")
    } else if is_loss_score(cp) {
        let ply = VALUE_MATE + cp;
        format!("-M{ply}")
    } else {
        let pawns = cp as f32 / 100.0;
        if pawns.abs() >= 10.0 {
            format!("{pawns:+.0}")
        } else if (pawns * 10.0).fract().abs() < 0.05 {
            format!("{pawns:+.1}")
        } else {
            format!("{pawns:+.2}")
        }
    }
}

fn fit_label(label: &str, width: usize) -> String {
    if label.len() >= width {
        label.chars().take(width).collect()
    } else {
        // Center within the bar column.
        let pad = width - label.len();
        let left = pad / 2;
        format!(
            "{}{}{}",
            " ".repeat(left),
            label,
            " ".repeat(pad - left)
        )
    }
}

fn label_row_and_style(
    height: u16,
    top_rows: u16,
    top_is_white: bool,
    frac: f32,
    top_style: Style,
    bottom_style: Style,
) -> (u16, Style) {
    let white_winning = frac >= 0.5;
    // Prefer a row inside the majority color, adjacent to the boundary.
    if white_winning == top_is_white {
        // Majority is the top section.
        let row = if top_rows == 0 {
            0
        } else {
            top_rows.saturating_sub(1)
        };
        (row.min(height.saturating_sub(1)), top_style)
    } else {
        // Majority is the bottom section.
        let row = if top_rows >= height {
            height.saturating_sub(1)
        } else {
            top_rows
        };
        (row, bottom_style)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::score::{mate_in, mated_in};

    #[test]
    fn format_eval_label_pawns() {
        assert_eq!(format_eval_label(0), "+0.0");
        assert_eq!(format_eval_label(35), "+0.35");
        assert_eq!(format_eval_label(-120), "-1.2");
        assert_eq!(format_eval_label(1500), "+15");
    }

    #[test]
    fn format_eval_label_mate() {
        assert_eq!(format_eval_label(mate_in(3)), "M3");
        assert_eq!(format_eval_label(mated_in(2)), "-M2");
    }
}
