//! Live material-balance formatting for the TUI.

/// Format White-relative material balance as pawn-equivalents: `+1`, `-5.0`, `0.0`.
pub fn format_material_score(balance_cp: i32) -> String {
    if balance_cp == 0 {
        return "0.0".to_string();
    }

    let pawns = balance_cp as f32 / 100.0;
    let rounded_one = (pawns * 10.0).round() / 10.0;
    if (rounded_one.fract()).abs() < 0.05 {
        if rounded_one.fract().abs() < 0.001 {
            format!("{:+.0}", rounded_one as i32)
        } else {
            format!("{rounded_one:+.1}")
        }
    } else {
        format!("{pawns:+.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::score::{BISHOP_VALUE, KNIGHT_VALUE, PAWN_VALUE, QUEEN_VALUE, ROOK_VALUE};

    #[test]
    fn format_zero() {
        assert_eq!(format_material_score(0), "0.0");
    }

    #[test]
    fn format_pawn() {
        assert_eq!(format_material_score(PAWN_VALUE), "+1");
        assert_eq!(format_material_score(-PAWN_VALUE), "-1");
    }

    #[test]
    fn format_rook() {
        assert_eq!(format_material_score(-ROOK_VALUE), "-5");
    }

    #[test]
    fn format_queen() {
        assert_eq!(format_material_score(QUEEN_VALUE), "+9");
    }

    #[test]
    fn format_knight_and_bishop() {
        assert_eq!(format_material_score(KNIGHT_VALUE), "+3.2");
        assert_eq!(format_material_score(BISHOP_VALUE), "+3.3");
    }
}
