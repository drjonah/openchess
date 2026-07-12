//! Minimal UCI protocol loop (P7-01).

use crate::board::Board;
use crate::search::{self, Limits};
use crate::transposition::TranspositionTable;
use crate::types::score::{VALUE_MATE, VALUE_MATED};
use crate::types::Value;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Run the UCI message loop on stdin/stdout until `quit`.
pub fn message_loop() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut board = Board::startpos();
    let mut tt = TranspositionTable::new(16);
    let stop = AtomicBool::new(false);

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else { continue };

        match cmd {
            "uci" => {
                let _ = writeln!(stdout, "id name OpenChess");
                let _ = writeln!(stdout, "id author OpenChess contributors");
                let _ = writeln!(stdout, "option name Hash type spin default 16 min 1 max 1024");
                let _ = writeln!(stdout, "uciok");
            }
            "isready" => {
                let _ = writeln!(stdout, "readyok");
            }
            "ucinewgame" => {
                board = Board::startpos();
                tt.clear();
            }
            "position" => {
                board = parse_position(line);
            }
            "go" => {
                stop.store(false, Ordering::Relaxed);
                let limits = parse_go(line);
                let mut b = board.clone();

                let mut on_info =
                    |depth: i32, score: Value, nodes: u64, time: Duration, pv: &str| {
                        let score_str = format_score(score);
                        let ms = time.as_millis();
                        println!(
                            "info depth {depth} score {score_str} nodes {nodes} time {ms} pv {pv}"
                        );
                    };

                let result = search::go(&mut b, limits, &mut tt, &stop, Some(&mut on_info));

                let mv = if result.best_move.is_none() {
                    "0000".to_string()
                } else {
                    result.best_move.to_string()
                };
                let _ = writeln!(stdout, "bestmove {mv}");
            }
            "stop" => {
                stop.store(true, Ordering::Relaxed);
            }
            "quit" => break,
            "setoption" => {
                let tokens: Vec<&str> = line.split_whitespace().collect();
                if let Some(name_i) = tokens.iter().position(|&t| t == "name") {
                    if tokens.get(name_i + 1) == Some(&"Hash") {
                        if let Some(val_i) = tokens.iter().position(|&t| t == "value") {
                            if let Some(v) = tokens.get(val_i + 1).and_then(|s| s.parse().ok()) {
                                tt = TranspositionTable::new(v);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let _ = stdout.flush();
    }
}

fn format_score(score: Value) -> String {
    if score >= VALUE_MATE - 256 {
        let mate_plies = VALUE_MATE - score;
        format!("mate {}", (mate_plies + 1) / 2)
    } else if score <= VALUE_MATED + 256 {
        let mate_plies = score - VALUE_MATED;
        format!("mate -{}", (mate_plies + 1) / 2)
    } else {
        format!("cp {score}")
    }
}

fn parse_position(line: &str) -> Board {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut board = if tokens.get(1) == Some(&"startpos") {
        Board::startpos()
    } else if tokens.get(1) == Some(&"fen") {
        let fen_parts: Vec<&str> = tokens.iter().skip(2).take(6).copied().collect();
        let fen = fen_parts.join(" ");
        Board::from_fen(&fen).unwrap_or_else(|_| Board::startpos())
    } else {
        Board::startpos()
    };

    if let Some(moves_i) = tokens.iter().position(|&t| t == "moves") {
        for uci in tokens.iter().skip(moves_i + 1) {
            if let Ok(mv) = board.parse_uci_move(uci) {
                board.make(mv);
            }
        }
    }
    board
}

fn parse_go(line: &str) -> Limits {
    let mut limits = Limits::default();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                if let Some(d) = tokens.get(i + 1).and_then(|s| s.parse().ok()) {
                    limits.depth = Some(d);
                }
                i += 2;
            }
            "movetime" => {
                if let Some(ms) = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    limits.movetime = Some(Duration::from_millis(ms));
                }
                i += 2;
            }
            "nodes" => {
                if let Some(n) = tokens.get(i + 1).and_then(|s| s.parse().ok()) {
                    limits.nodes = Some(n);
                }
                i += 2;
            }
            "wtime" => {
                if let Some(ms) = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    limits.wtime = Some(Duration::from_millis(ms));
                }
                i += 2;
            }
            "btime" => {
                if let Some(ms) = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    limits.btime = Some(Duration::from_millis(ms));
                }
                i += 2;
            }
            "winc" => {
                if let Some(ms) = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    limits.winc = Some(Duration::from_millis(ms));
                }
                i += 2;
            }
            "binc" => {
                if let Some(ms) = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    limits.binc = Some(Duration::from_millis(ms));
                }
                i += 2;
            }
            "movestogo" => {
                if let Some(n) = tokens.get(i + 1).and_then(|s| s.parse().ok()) {
                    limits.movestogo = Some(n);
                }
                i += 2;
            }
            "infinite" => {
                limits.infinite = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    // Default Move Overhead matches config / time::DEFAULT_MOVE_OVERHEAD (50ms).
    limits.move_overhead = crate::time::DEFAULT_MOVE_OVERHEAD;

    // Do not default depth when clocks, movetime, nodes, or infinite are set.
    if limits.depth.is_none()
        && limits.movetime.is_none()
        && limits.nodes.is_none()
        && !limits.has_clock()
        && !limits.infinite
    {
        limits.depth = Some(6);
    }
    limits
}
