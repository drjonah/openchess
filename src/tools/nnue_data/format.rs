//! Bullet-compatible text training records.
//!
//! Each line is:
//! ```text
//! <FEN> | <score> | <result>
//! ```
//! where `score` is white-relative centipawns and `result` is white-relative
//! WDL in `{1.0, 0.5, 0.0}` (see [Bullet docs](https://github.com/jw1912/bullet)).

use crate::board::GameResult;
use crate::types::{score, Color, Value};

/// One labeled quiet position ready for Bullet text conversion.
#[derive(Clone, Debug, PartialEq)]
pub struct TrainingRecord {
    pub fen: String,
    /// White-relative evaluation in centipawns.
    pub score_cp: i32,
    /// White-relative game result: `1.0` win, `0.5` draw, `0.0` loss.
    pub result: f32,
}

impl TrainingRecord {
    /// Format as a single Bullet text line (no trailing newline).
    pub fn to_bullet_line(&self) -> String {
        format!("{} | {} | {:.1}", self.fen, self.score_cp, self.result)
    }

    /// Parse a Bullet text line. Returns `None` on blank/comment lines.
    pub fn parse_bullet_line(line: &str) -> Result<Option<Self>, String> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Ok(None);
        }
        let parts: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if parts.len() != 3 {
            return Err(format!(
                "expected 'FEN | score | result', got {} fields",
                parts.len()
            ));
        }
        let fen = parts[0].to_string();
        if fen.split_whitespace().count() < 4 {
            return Err(format!("FEN looks incomplete: {fen}"));
        }
        let score_cp: i32 = parts[1]
            .parse()
            .map_err(|_| format!("invalid score '{}'", parts[1]))?;
        let result: f32 = parts[2]
            .parse()
            .map_err(|_| format!("invalid result '{}'", parts[2]))?;
        if !(result == 0.0 || result == 0.5 || result == 1.0) {
            return Err(format!(
                "result must be 0.0, 0.5, or 1.0 (got {result})"
            ));
        }
        Ok(Some(Self {
            fen,
            score_cp,
            result,
        }))
    }
}

/// Convert a side-to-move-relative search score to white-relative centipawns.
pub fn white_relative_score(stm: Color, stm_score: Value) -> i32 {
    if stm == Color::White {
        stm_score
    } else {
        -stm_score
    }
}

/// Map a finished game to Bullet's white-relative WDL target.
pub fn white_relative_result(result: GameResult) -> Option<f32> {
    match result {
        GameResult::Checkmate { winner: Color::White } => Some(1.0),
        GameResult::Checkmate { winner: Color::Black } => Some(0.0),
        GameResult::Stalemate
        | GameResult::DrawRepetition
        | GameResult::DrawFiftyMove
        | GameResult::DrawInsufficientMaterial => Some(0.5),
        GameResult::Ongoing => None,
    }
}

/// True when the score is in the mate/TB band and should not be used as a CP label.
pub fn is_mate_score(v: Value) -> bool {
    score::is_win_score(v) || score::is_loss_score(v)
}

/// Validate an entire Bullet text file; returns record count.
pub fn validate_bullet_text(text: &str) -> Result<usize, String> {
    let mut count = 0usize;
    for (i, line) in text.lines().enumerate() {
        match TrainingRecord::parse_bullet_line(line) {
            Ok(Some(_)) => count += 1,
            Ok(None) => {}
            Err(e) => return Err(format!("line {}: {e}", i + 1)),
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bullet_line() {
        let rec = TrainingRecord {
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
            score_cp: 12,
            result: 0.5,
        };
        let line = rec.to_bullet_line();
        let parsed = TrainingRecord::parse_bullet_line(&line).unwrap().unwrap();
        assert_eq!(parsed, rec);
    }

    #[test]
    fn white_relative_flips_for_black() {
        assert_eq!(white_relative_score(Color::White, 40), 40);
        assert_eq!(white_relative_score(Color::Black, 40), -40);
    }

    #[test]
    fn validate_rejects_bad_result() {
        let bad = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 | 0 | 0.25\n";
        assert!(validate_bullet_text(bad).is_err());
    }
}
