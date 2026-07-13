//! Export helpers: PGN writer and JSONL event lines (P11-08).
//!
//! There is no PGN *writer* elsewhere in the tree (only readers under
//! `chesscom` / `tui/import`), so the arena adds one here.

use crate::board::Board;

use super::slot::{GameSlot, Outcome, SlotEvent};

/// Machine-readable outcome tag for JSONL.
fn outcome_key(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::WhiteWin => "white",
        Outcome::BlackWin => "black",
        Outcome::Draw => "draw",
        Outcome::Unfinished => "unfinished",
    }
}

/// Render one PGN game from a finished (or in-progress) slot.
pub fn slot_pgn(slot: &GameSlot) -> String {
    let white_label = slot
        .profile
        .clone()
        .map(|p| format!("{p} (W)"))
        .unwrap_or_else(|| format!("OpenChess d{}", slot.white.depth));
    let black_label = slot
        .profile
        .clone()
        .map(|p| format!("{p} (B)"))
        .unwrap_or_else(|| format!("OpenChess d{}", slot.black.depth));
    slot_pgn_with_labels(slot, &white_label, &black_label)
}

/// Render one PGN game with explicit player labels.
pub fn slot_pgn_with_labels(slot: &GameSlot, white_label: &str, black_label: &str) -> String {
    let result = slot.outcome().result_tag();
    let start_fen = slot.start_fen();
    let startpos = Board::startpos().to_fen();

    let mut pgn = String::new();
    pgn.push_str("[Event \"OpenChess Arena\"]\n");
    pgn.push_str("[Site \"local\"]\n");
    pgn.push_str(&format!("[White \"{white_label}\"]\n"));
    pgn.push_str(&format!("[Black \"{black_label}\"]\n"));
    pgn.push_str(&format!("[Result \"{result}\"]\n"));
    if start_fen != startpos {
        pgn.push_str("[SetUp \"1\"]\n");
        pgn.push_str(&format!("[FEN \"{start_fen}\"]\n"));
    }
    pgn.push('\n');
    pgn.push_str(&movetext(slot));
    pgn.push_str(result);
    pgn.push('\n');
    pgn
}

fn movetext(slot: &GameSlot) -> String {
    let start = Board::from_fen(slot.start_fen()).unwrap_or_else(|_| Board::startpos());
    let mut number = start.fullmove_number();
    let mut white_to_move = start.side_to_move() == crate::types::Color::White;

    let mut out = String::new();
    for (i, ply) in slot.transcript().iter().enumerate() {
        if white_to_move {
            out.push_str(&format!("{number}. "));
        } else if i == 0 {
            out.push_str(&format!("{number}... "));
        }
        out.push_str(&ply.san);
        out.push(' ');
        if !white_to_move {
            number += 1;
        }
        white_to_move = !white_to_move;
    }
    out
}

/// Render one arena event as a single JSON line.
pub fn event_jsonl(event: &SlotEvent) -> String {
    match event {
        SlotEvent::Move {
            slot,
            ply,
            uci,
            eval_cp,
        } => {
            let eval = match eval_cp {
                Some(v) => v.to_string(),
                None => "null".to_string(),
            };
            format!(
                "{{\"type\":\"move\",\"slot\":{slot},\"ply\":{ply},\"uci\":\"{uci}\",\"eval_cp\":{eval}}}"
            )
        }
        SlotEvent::Finish {
            slot,
            outcome,
            plies,
        } => format!(
            "{{\"type\":\"finish\",\"slot\":{slot},\"outcome\":\"{}\",\"plies\":{plies}}}",
            outcome_key(*outcome)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::runner::{Arena, ArenaConfig};
    use crate::config::SideStrength;

    fn strength(depth: u32) -> SideStrength {
        SideStrength {
            depth,
            movetime_ms: 0,
        }
    }

    #[test]
    fn pgn_has_headers_and_matches_transcript() {
        crate::lookup::initialize();
        let mut arena = Arena::from_config(&ArenaConfig {
            games: 1,
            white: strength(1),
            black: strength(1),
            ply_limit: 20,
            hash_mb: 1,
            ..ArenaConfig::default()
        });
        arena.run_to_completion(&mut |_| {});
        let slot = arena.slot(0).unwrap();

        let pgn = slot_pgn(slot);
        assert!(pgn.contains("[Event \"OpenChess Arena\"]"));
        assert!(pgn.contains("[Result \""));
        // Startpos game: first token is "1.".
        assert!(pgn.contains("1. "));
        // Every SAN token from the transcript appears in the movetext.
        for ply in slot.transcript() {
            assert!(pgn.contains(&ply.san), "missing SAN {} in PGN", ply.san);
        }
        assert!(pgn.trim_end().ends_with(slot.outcome().result_tag()));
    }

    #[test]
    fn jsonl_move_and_finish_format() {
        let mv = SlotEvent::Move {
            slot: 3,
            ply: 5,
            uci: "g1f3".into(),
            eval_cp: Some(34),
        };
        assert_eq!(
            event_jsonl(&mv),
            "{\"type\":\"move\",\"slot\":3,\"ply\":5,\"uci\":\"g1f3\",\"eval_cp\":34}"
        );

        let mv_none = SlotEvent::Move {
            slot: 0,
            ply: 1,
            uci: "e2e4".into(),
            eval_cp: None,
        };
        assert!(event_jsonl(&mv_none).contains("\"eval_cp\":null"));

        let fin = SlotEvent::Finish {
            slot: 1,
            outcome: Outcome::Draw,
            plies: 42,
        };
        assert_eq!(
            event_jsonl(&fin),
            "{\"type\":\"finish\",\"slot\":1,\"outcome\":\"draw\",\"plies\":42}"
        );
    }
}
