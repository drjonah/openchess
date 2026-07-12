//! Chessboard widget — wooden squares and Unicode / block-drawing pieces.

use super::material::format_material_score;
use super::piece_art::{self, PieceSize};
use super::session::EngineSession;
use crate::types::{Color as Side, Piece, Square};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

// Classic palette adapted from chess-tui (MIT).
const BOARD_LIGHT: Color = Color::Rgb(240, 217, 181);
const BOARD_DARK: Color = Color::Rgb(181, 136, 99);
const PIECE_WHITE: Color = Color::Rgb(255, 255, 255);
const PIECE_BLACK: Color = Color::Rgb(20, 20, 20);
const LAST_MOVE: Color = Color::Rgb(100, 200, 100);
const BEST_MOVE: Color = Color::Rgb(100, 160, 220);

fn piece_fg(piece: Piece) -> Color {
    match piece.color() {
        Some(Side::White) => PIECE_WHITE,
        Some(Side::Black) => PIECE_BLACK,
        None => Color::White,
    }
}

fn square_bg(session: &EngineSession, sq: Square, file: u8, rank: u8) -> Color {
    let light = (file + rank) % 2 == 1;
    let mut bg = if light { BOARD_LIGHT } else { BOARD_DARK };
    if let Some(mv) = session.last_move() {
        if sq == mv.from() || sq == mv.to() {
            bg = LAST_MOVE;
        }
    }
    // Best-move hint is Analyze-only (Shift+G); never paint it during live play.
    if session.show_engine_hints() {
        if let Some(best) = session.info().bestmove.as_deref() {
            if let Ok(bm) = super::session::resolve_player_move(session.board(), best) {
                if sq == bm.from() || sq == bm.to() {
                    bg = BEST_MOVE;
                }
            }
        }
    }
    bg
}

fn cell_content(
    piece: Piece,
    row_in_cell: u16,
    cell_h: u16,
    cell_w: u16,
    mid_col: u16,
) -> String {
    if piece.is_empty() {
        if row_in_cell == cell_h / 2 && cell_h == 1 {
            let mut s = String::with_capacity(cell_w as usize);
            for col in 0..cell_w {
                s.push(if col == mid_col { '·' } else { ' ' });
            }
            return s;
        }
        return " ".repeat(cell_w as usize);
    }

    let Some(pt) = piece.piece_type() else {
        return " ".repeat(cell_w as usize);
    };
    let Some(side) = piece.color() else {
        return " ".repeat(cell_w as usize);
    };

    let size = PieceSize::from_cell_height(cell_h);
    let art = piece_art::piece_art(pt, side, size);
    let lines = piece_art::art_lines(art);
    piece_art::line_for_row(&lines, row_in_cell, cell_h, cell_w)
}

fn material_score_style(balance_cp: i32) -> Style {
    if balance_cp > 0 {
        Style::default().fg(Color::Rgb(120, 200, 120))
    } else if balance_cp < 0 {
        Style::default().fg(Color::Rgb(220, 120, 120))
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn render(frame: &mut Frame, area: Rect, session: &EngineSession) {
    let block = Block::default().title(" Board ").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 10 || inner.height < 6 {
        return;
    }

    let flipped = session.flipped();
    let stm = match session.side_to_move() {
        Side::White => "White",
        Side::Black => "Black",
    };

    let header_rows: u16 = 2;
    let footer_rows: u16 = 1;
    let rank_label_cols: u16 = 2;

    let board_h = inner.height.saturating_sub(header_rows + footer_rows);
    let board_w = inner.width.saturating_sub(rank_label_cols);

    let cell_h = (board_h / 8).max(1);
    let cell_w = (board_w / 8).max(3);

    let ranks: Vec<u8> = if flipped {
        (0..8).collect()
    } else {
        (0..8).rev().collect()
    };
    let files: Vec<u8> = if flipped {
        (0..8).rev().collect()
    } else {
        (0..8).collect()
    };

    let mut lines: Vec<Line> = Vec::new();
    let balance_cp = session.board().material_balance();
    let score_label = format_material_score(balance_cp);
    lines.push(Line::from(vec![
        Span::raw(format!(
            "Move {} · {} to play · Material ",
            session.fullmove_number(),
            stm
        )),
        Span::styled(score_label, material_score_style(balance_cp)),
    ]));
    lines.push(Line::from(""));

    let mid_col = (cell_w.saturating_sub(1)) / 2;

    for &rank in &ranks {
        for row_in_cell in 0..cell_h {
            let mut spans: Vec<Span> = Vec::new();
            if row_in_cell == cell_h / 2 {
                spans.push(Span::raw(format!("{} ", rank + 1)));
            } else {
                spans.push(Span::raw("  "));
            }

            for &file in &files {
                let sq = Square::from_file_rank(file, rank).unwrap();
                let piece = session.piece_on(sq);
                let bg = square_bg(session, sq, file, rank);
                let cell = cell_content(piece, row_in_cell, cell_h, cell_w, mid_col);
                let style = Style::default().bg(bg).fg(piece_fg(piece));
                spans.push(Span::styled(cell, style));
            }
            lines.push(Line::from(spans));
        }
    }

    let mut file_spans: Vec<Span> = vec![Span::raw("  ")];
    for &file in &files {
        let mut label = String::with_capacity(cell_w as usize);
        let ch = (b'a' + file) as char;
        for col in 0..cell_w {
            if col == mid_col {
                label.push(ch);
            } else {
                label.push(' ');
            }
        }
        file_spans.push(Span::raw(label));
    }
    lines.push(Line::from(file_spans));

    frame.render_widget(Paragraph::new(lines), inner);
}
