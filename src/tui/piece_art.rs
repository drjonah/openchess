//! Multi-line Unicode piece art for the board widget.
//!
//! DEFAULT glyphs and block-drawing designs adapted from
//! [chess-tui](https://github.com/thomas-mauran/chess-tui) (MIT).

use crate::types::{Color as Side, PieceType};

/// Piece rendering size chosen from cell height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceSize {
    /// Single Unicode chess symbol.
    Small,
    /// 2–3 line block art.
    Compact,
    /// 3–4 line block art.
    Extended,
    /// ~5 line block art.
    Large,
}

impl PieceSize {
    /// Map cell height (rows) to a piece size tier.
    #[must_use]
    pub fn from_cell_height(height: u16) -> Self {
        if height < 3 {
            PieceSize::Small
        } else if height < 4 {
            PieceSize::Compact
        } else if height < 5 {
            PieceSize::Extended
        } else {
            PieceSize::Large
        }
    }
}

/// Return the DEFAULT art string for a piece at the given size.
///
/// Small size uses filled vs outline Unicode symbols by color; larger sizes
/// use the same block art for both colors (caller applies foreground color).
#[must_use]
pub fn piece_art(pt: PieceType, side: Side, size: PieceSize) -> &'static str {
    match pt {
        PieceType::King => king_art(side, size),
        PieceType::Queen => queen_art(side, size),
        PieceType::Rook => rook_art(side, size),
        PieceType::Bishop => bishop_art(side, size),
        PieceType::Knight => knight_art(side, size),
        PieceType::Pawn => pawn_art(side, size),
    }
}

fn king_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♔",
            Side::Black => "♚",
        },
        PieceSize::Compact => "▗▂╋▂▖\n ▀█▀ \n ▀▀▀ ",
        PieceSize::Extended => " ▂╋▂ \n▜███▛\n ▜█▛ \n▝▀▀▀▘",
        PieceSize::Large => "  ▂▃╋▃▂  \n ▐█████▋ \n  ▜███▛  \n   ▟█▙   \n  ▀▀▀▀▀  ",
    }
}

fn queen_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♕",
            Side::Black => "♛",
        },
        PieceSize::Compact => " ▆▄▆ \n ▗█▖ \n ▀▀▀ ",
        PieceSize::Extended => "▂ ▄ ▂\n▜▙█▟▛\n ▜█▛ \n▝▀▀▀▘",
        PieceSize::Large => "▗  ▂  ▖\n▐▙▟█▙▟▌\n ▜███▛ \n ▗███▖ \n▝▀▀▀▀▀▘",
    }
}

fn rook_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♖",
            Side::Black => "♜",
        },
        PieceSize::Compact => " ▅ ▅ \n ███ \n▝▀▀▀▘",
        PieceSize::Extended => "▄ ▄ ▄\n█████\n ███ \n▀▀▀▀▀",
        PieceSize::Large => "▗▄ ▃ ▄▖\n▐█▄█▄█▌\n▝▜███▛▘\n ▟███▙ \n▝▀▀▀▀▀▘",
    }
}

fn bishop_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♗",
            Side::Black => "♝",
        },
        PieceSize::Compact => " ▆▖▆ \n ▐▙▌ \n ▀▀▀ ",
        PieceSize::Extended => " ▄▁▗ \n ██▟ \n ▟█▙ \n▝▀▀▀▘",
        PieceSize::Large => "▗▅  ▖\n██0 █\n███0█\n▝███▘\n▀▀▀▀▀",
    }
}

fn knight_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♘",
            Side::Black => "♞",
        },
        PieceSize::Compact => " ▄▟▟▖\n ▂█▛▘\n▝▀▀▀▘",
        PieceSize::Extended => "  ▖▗ \n▗▇▟█▌\n ▟█▛ \n▝▀▀▀▘",
        PieceSize::Large => "  ▅ ▅\n ▟▛███▖\n▝▀▜███▊\n ▗███▛ \n ▀▀▀▀▀ ",
    }
}

fn pawn_art(side: Side, size: PieceSize) -> &'static str {
    match size {
        PieceSize::Small => match side {
            Side::White => "♙",
            Side::Black => "♟",
        },
        PieceSize::Compact => "  ▂  \n ▆█▆ \n ▔▔▔ ",
        PieceSize::Extended => "     \n ▝█▘ \n ▟█▙ \n ▔▔▔ ",
        PieceSize::Large => "\n ▄▇▄\n ▜█▛\n▄███▄\n▔▔▔▔▔",
    }
}

/// Horizontally center `line` in `width` columns (Unicode-aware display width).
#[must_use]
pub fn center_line(line: &str, width: u16) -> String {
    let w = width as usize;
    let line_w = unicode_width(line);
    if line_w >= w {
        // Truncate by chars if somehow wider than the cell.
        return line.chars().take(w).collect();
    }
    let pad = w - line_w;
    let left = pad / 2;
    let right = pad - left;
    let mut out = String::with_capacity(w);
    out.extend(std::iter::repeat_n(' ', left));
    out.push_str(line);
    out.extend(std::iter::repeat_n(' ', right));
    out
}

/// Approximate display width: most chess/block glyphs are width 1 in terminals
/// that render them as single cells; treat each char as one column.
fn unicode_width(s: &str) -> usize {
    s.chars().count()
}

/// Lines of art for a piece, trimmed of a trailing empty line from raw strings.
#[must_use]
pub fn art_lines(art: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = art.split('\n').collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    // Keep a leading empty line for Large pawn (vertical padding in the art).
    lines
}

/// Vertically center `lines` in `cell_h` rows; return the line for `row_in_cell`
/// (spaces if outside the art block).
#[must_use]
pub fn line_for_row(lines: &[&str], row_in_cell: u16, cell_h: u16, cell_w: u16) -> String {
    let n = lines.len() as u16;
    let top = cell_h.saturating_sub(n) / 2;
    if row_in_cell < top || row_in_cell >= top + n {
        return " ".repeat(cell_w as usize);
    }
    let idx = (row_in_cell - top) as usize;
    center_line(lines.get(idx).copied().unwrap_or(""), cell_w)
}
