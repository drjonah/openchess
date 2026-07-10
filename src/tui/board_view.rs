//! Unicode chessboard widget — squares and piece sprites scale to the panel.

use super::session::EngineSession;
use crate::types::{Color as Side, Piece, PieceType, Square};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// 7×7 silhouettes — shapes chosen to stay distinct when scaled.
const SPRITE_W: usize = 7;
const SPRITE_H: usize = 7;

fn sprite(pt: PieceType) -> [[u8; SPRITE_W]; SPRITE_H] {
    match pt {
        // Cross crown + wide base
        PieceType::King => [
            [0, 0, 0, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 0, 1, 0, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
        ],
        // Spiky 5-point crown (unique vs rook/king)
        PieceType::Queen => [
            [1, 0, 1, 0, 1, 0, 1],
            [1, 1, 1, 1, 1, 1, 1],
            [0, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1],
        ],
        // Battlement top — rectangular tower
        PieceType::Rook => [
            [1, 0, 1, 0, 1, 0, 1],
            [1, 1, 1, 1, 1, 1, 1],
            [0, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1],
        ],
        // Mitre with diagonal cut (slash) — tall & pointed
        PieceType::Bishop => [
            [0, 0, 0, 1, 0, 0, 0],
            [0, 0, 1, 0, 1, 0, 0],
            [0, 0, 1, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1],
        ],
        // Horse head facing right — L / snout profile
        PieceType::Knight => [
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 0, 1, 0],
            [0, 1, 1, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
        ],
        // Smallest: ball + thin stem + pedestal
        PieceType::Pawn => [
            [0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 0, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 0, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 0],
        ],
    }
}

fn sample_sprite(pt: PieceType, row: u16, col: u16, cell_h: u16, cell_w: u16) -> bool {
    let bitmap = sprite(pt);
    // Map cell coords into sprite with a 1-cell margin when the square is large.
    let margin_y = if cell_h >= 5 { 1 } else { 0 };
    let margin_x = if cell_w >= 5 { 1 } else { 0 };
    let inner_h = cell_h.saturating_sub(margin_y * 2).max(1);
    let inner_w = cell_w.saturating_sub(margin_x * 2).max(1);

    if row < margin_y || col < margin_x {
        return false;
    }
    let ry = row - margin_y;
    let cx = col - margin_x;
    if ry >= inner_h || cx >= inner_w {
        return false;
    }

    let sy = (ry as usize * SPRITE_H) / inner_h as usize;
    let sx = (cx as usize * SPRITE_W) / inner_w as usize;
    bitmap[sy.min(SPRITE_H - 1)][sx.min(SPRITE_W - 1)] != 0
}

fn piece_fg(piece: Piece) -> Color {
    match piece.color() {
        // Warm ivory vs cool charcoal so sides stay readable on both squares.
        Some(Side::White) => Color::Rgb(250, 245, 230),
        Some(Side::Black) => Color::Rgb(25, 28, 35),
        None => Color::White,
    }
}

fn square_bg(session: &EngineSession, sq: Square, file: u8, rank: u8) -> Color {
    let light = (file + rank) % 2 == 1;
    let mut bg = if light {
        Color::Rgb(110, 110, 110)
    } else {
        Color::Rgb(55, 55, 55)
    };
    if let Some(mv) = session.last_move() {
        if sq == mv.from() || sq == mv.to() {
            bg = Color::Rgb(150, 125, 50);
        }
    }
    if let Some(best) = session.info().bestmove.as_deref() {
        if let Ok(bm) = super::session::resolve_player_move(session.board(), best) {
            if sq == bm.from() || sq == bm.to() {
                bg = Color::Rgb(50, 110, 140);
            }
        }
    }
    bg
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
    lines.push(Line::from(format!(
        "Move {} · {} to play",
        session.fullmove_number(),
        stm
    )));
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

                let mut cell = String::with_capacity(cell_w as usize);
                for col in 0..cell_w {
                    let ch = if piece.is_empty() {
                        if row_in_cell == cell_h / 2 && col == mid_col && cell_h == 1 {
                            '·'
                        } else {
                            ' '
                        }
                    } else if let Some(pt) = piece.piece_type() {
                        if sample_sprite(pt, row_in_cell, col, cell_h, cell_w) {
                            '█'
                        } else {
                            ' '
                        }
                    } else {
                        ' '
                    };
                    cell.push(ch);
                }

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
