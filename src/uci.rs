//! UCI protocol loop with options and debug helpers (P7-01 / P7-03).

use crate::board::Board;
use crate::eval;
use crate::search::{self, Limits};
use crate::time::DEFAULT_MOVE_OVERHEAD_MS;
use crate::tools::{self, BENCH_DEPTH};
use crate::transposition::TranspositionTable;
use crate::types::score::{VALUE_MATE, VALUE_MATED};
use crate::types::{Color, Value};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Session state for UCI options that persist across `go` commands.
struct UciState {
    move_overhead_ms: u64,
    /// Stub until Lazy SMP (P8-01); accepted but ignored.
    threads: u32,
}

impl Default for UciState {
    fn default() -> Self {
        Self {
            move_overhead_ms: DEFAULT_MOVE_OVERHEAD_MS,
            threads: 1,
        }
    }
}

/// Run the UCI message loop on stdin/stdout until `quit`.
pub fn message_loop() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut board = Board::startpos();
    let mut tt = TranspositionTable::new(16);
    let stop = AtomicBool::new(false);
    let mut state = UciState::default();

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
                let _ = writeln!(stdout, "option name Threads type spin default 1 min 1 max 1");
                let _ = writeln!(
                    stdout,
                    "option name Move Overhead type spin default {DEFAULT_MOVE_OVERHEAD_MS} min 0 max 5000"
                );
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
                let limits = parse_go(line, state.move_overhead_ms);
                let mut b = board.clone();

                let mut on_info =
                    |depth: i32, score: Value, nodes: u64, time: Duration, pv: &str, hashfull: u32| {
                        let score_str = format_score(score);
                        let ms = time.as_millis();
                        println!(
                            "info depth {depth} score {score_str} nodes {nodes} time {ms} hashfull {hashfull} pv {pv}"
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
                apply_setoption(line, &mut tt, &mut state);
            }
            "bench" => {
                let depth = parts.next().and_then(|s| s.parse().ok()).unwrap_or(BENCH_DEPTH);
                let report = tools::run_bench(depth, 16);
                let _ = write!(stdout, "{}", tools::format_bench_report(&report));
            }
            "perft" => {
                let depth = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                let divide = parts.next() == Some("divide");
                let mut b = board.clone();
                if divide {
                    for (mv, nodes) in tools::perft_divide(&mut b, depth) {
                        let _ = writeln!(stdout, "{mv}: {nodes}");
                    }
                }
                let nodes = tools::perft(&mut b, depth);
                let _ = writeln!(stdout, "nodes {nodes}");
            }
            "eval" => {
                let score = eval::evaluate(&board);
                let _ = writeln!(stdout, "eval cp {score}");
            }
            "d" => {
                dump_position(&mut stdout, &board);
            }
            _ => {}
        }
        let _ = stdout.flush();
    }
}

fn apply_setoption(line: &str, tt: &mut TranspositionTable, state: &mut UciState) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let Some(name_i) = tokens.iter().position(|&t| t == "name") else {
        return;
    };
    let Some(val_i) = tokens.iter().position(|&t| t == "value") else {
        return;
    };
    if val_i <= name_i + 1 {
        return;
    }
    let name = tokens[name_i + 1..val_i].join(" ");
    let Some(value) = tokens.get(val_i + 1) else {
        return;
    };

    match name.as_str() {
        "Hash" => {
            if let Ok(mb) = value.parse::<usize>() {
                *tt = TranspositionTable::new(mb);
            }
        }
        "Threads" => {
            if let Ok(n) = value.parse::<u32>() {
                // Stub for P8-01 Lazy SMP: accept and store, still single-threaded.
                state.threads = n.max(1);
                let _ = state.threads;
            }
        }
        "Move Overhead" => {
            if let Ok(ms) = value.parse::<u64>() {
                state.move_overhead_ms = ms.min(5000);
            }
        }
        _ => {}
    }
}

fn dump_position(stdout: &mut impl Write, board: &Board) {
    let _ = writeln!(stdout, "{}", board);
    let _ = writeln!(stdout, "Fen: {}", board.to_fen());
    let side = match board.side_to_move() {
        Color::White => "White",
        Color::Black => "Black",
    };
    let _ = writeln!(stdout, "Side: {side}");
    let _ = writeln!(stdout, "Key: {:016x}", board.key());
    let _ = writeln!(
        stdout,
        "Checkers: {}",
        if board.in_check() { "yes" } else { "no" }
    );
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

fn parse_go(line: &str, move_overhead_ms: u64) -> Limits {
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
    limits.move_overhead = Duration::from_millis(move_overhead_ms);

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
