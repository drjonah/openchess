//! UCI protocol loop with options and debug helpers (P7-01 / P7-03).

use crate::board::Board;
use crate::book::{Book, BookConfig, BookRng, VarietyState};
use crate::eval::{self, Network};
use crate::search::{self, Limits};
use crate::time::DEFAULT_MOVE_OVERHEAD_MS;
use crate::tools::{self, BENCH_DEPTH};
use crate::transposition::TranspositionTable;
use crate::types::score::{VALUE_MATE, VALUE_MATED};
use crate::types::{Color, Value};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Session state for UCI options that persist across `go` commands.
struct UciState {
    move_overhead_ms: u64,
    /// Lazy SMP worker count.
    threads: u32,
    /// Loaded NNUE network (embedded by default). Search uses this leaf eval.
    network: Arc<Network>,
    eval_file: String,
    /// Opening-book settings (P7-06). `OwnBook`/`BookFile`/`BookDepth`.
    book_config: BookConfig,
    /// Ready-to-probe book built from [`Self::book_config`].
    opening_book: Book,
    /// PRNG for weighted book selection.
    book_rng: BookRng,
    /// Soft anti-repetition across games (P10-10).
    book_variety: VarietyState,
}

fn refresh_opening_book(state: &mut UciState) {
    state.opening_book = Book::from_config(&state.book_config);
}

impl Default for UciState {
    fn default() -> Self {
        let book_config = BookConfig::default();
        Self {
            move_overhead_ms: DEFAULT_MOVE_OVERHEAD_MS,
            threads: 1,
            network: Network::embedded_shared(),
            eval_file: String::new(),
            book_config: book_config.clone(),
            opening_book: Book::from_config(&book_config),
            book_rng: BookRng::from_entropy(),
            book_variety: VarietyState::default(),
        }
    }
}

/// Half-moves played to reach `board` (for book `max_plies`).
fn game_ply(board: &Board) -> u32 {
    let full = u32::from(board.fullmove_number()).max(1);
    let black_to_move = board.side_to_move() == Color::Black;
    (full - 1) * 2 + u32::from(black_to_move)
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
                let _ = writeln!(stdout, "option name Threads type spin default 1 min 1 max 512");
                let _ = writeln!(
                    stdout,
                    "option name Move Overhead type spin default {DEFAULT_MOVE_OVERHEAD_MS} min 0 max 5000"
                );
                let _ = writeln!(
                    stdout,
                    "option name EvalFile type string default <embedded>"
                );
                let _ = writeln!(stdout, "option name OwnBook type check default true");
                let _ = writeln!(stdout, "option name BookFile type string default <none>");
                let _ = writeln!(
                    stdout,
                    "option name BookDepth type spin default 16 min 0 max 60"
                );
                let _ = writeln!(
                    stdout,
                    "option name BookRepertoire type check default false"
                );
                let _ = writeln!(
                    stdout,
                    "option name BookStyle type combo default mixed var mixed var solid var aggressive"
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

                // Opening book: play a book move immediately when OwnBook is on
                // and we are still within BookDepth (P7-06).
                if state.opening_book.is_enabled() {
                    if let Some(mv) = state.opening_book.probe_varied(
                        &board,
                        game_ply(&board),
                        &mut state.book_rng,
                        Some(&mut state.book_variety),
                    ) {
                        let _ = writeln!(stdout, "info string book move {mv}");
                        let _ = writeln!(stdout, "bestmove {mv}");
                        let _ = stdout.flush();
                        continue;
                    }
                }

                let mut limits = parse_go(line, state.move_overhead_ms);
                limits.threads = state.threads;
                limits.network = Some(Arc::clone(&state.network));
                let mut b = board.clone();

                let mut on_info =
                    |depth: i32, score: Value, nodes: u64, time: Duration, pv: &str, hashfull: u32| {
                        let score_str = format_score(score);
                        let ms = time.as_millis();
                        println!(
                            "info depth {depth} score {score_str} nodes {nodes} time {ms} hashfull {hashfull} pv {pv}"
                        );
                    };

                let result = search::go(&mut b, limits, &tt, &stop, Some(&mut on_info));

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
                let hce = eval::evaluate_hce(&board);
                let nnue = eval::evaluate_nnue(&board, &state.network);
                let raw = eval::nnue::evaluate_raw_board(&board, &state.network);
                let _ = writeln!(stdout, "eval hce cp {hce}");
                let _ = writeln!(stdout, "eval nnue cp {nnue} (raw {raw})");
                // Primary line: NNUE is the search leaf.
                let _ = writeln!(stdout, "eval cp {nnue}");
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
                state.threads = n.clamp(1, 512);
            }
        }
        "Move Overhead" => {
            if let Ok(ms) = value.parse::<u64>() {
                state.move_overhead_ms = ms.min(5000);
            }
        }
        "EvalFile" => {
            let path = tokens[val_i + 1..].join(" ");
            if path.is_empty() || path == "<embedded>" || path == "None" {
                state.network = Network::embedded_shared();
                state.eval_file.clear();
            } else {
                match Network::load_file(Path::new(&path)) {
                    Ok(net) => {
                        state.network = Arc::new(net);
                        state.eval_file = path;
                    }
                    Err(e) => {
                        eprintln!("info string failed to load EvalFile '{path}': {e}");
                    }
                }
            }
        }
        "OwnBook" => {
            state.book_config.enabled =
                matches!(value.to_ascii_lowercase().as_str(), "true" | "on" | "1");
            refresh_opening_book(state);
        }
        "BookFile" => {
            let path = tokens[val_i + 1..].join(" ");
            if path.is_empty() || path == "<none>" || path == "None" || path == "<empty>" {
                state.book_config.file = None;
            } else {
                state.book_config.file = Some(PathBuf::from(path));
            }
            refresh_opening_book(state);
        }
        "BookDepth" => {
            if let Ok(n) = value.parse::<u32>() {
                state.book_config.max_plies = n.min(60);
            }
            refresh_opening_book(state);
        }
        "BookRepertoire" => {
            state.book_config.repertoire =
                matches!(value.to_ascii_lowercase().as_str(), "true" | "on" | "1");
            refresh_opening_book(state);
        }
        "BookStyle" => {
            state.book_config.style = crate::book::repertoire::BookStyle::parse(value)
                .as_str()
                .into();
            refresh_opening_book(state);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_ply_counts_half_moves() {
        crate::lookup::initialize();
        assert_eq!(game_ply(&Board::startpos()), 0);

        let mut b = Board::startpos();
        b.make(b.parse_uci_move("e2e4").unwrap());
        assert_eq!(game_ply(&b), 1);
        b.make(b.parse_uci_move("e7e5").unwrap());
        assert_eq!(game_ply(&b), 2);
        b.make(b.parse_uci_move("g1f3").unwrap());
        assert_eq!(game_ply(&b), 3);
    }

    #[test]
    fn own_book_off_leaves_book_disabled() {
        let mut tt = TranspositionTable::new(1);
        let mut state = UciState::default();
        apply_setoption("setoption name OwnBook value false", &mut tt, &mut state);
        assert!(!state.book_config.enabled);
        assert!(!state.opening_book.is_enabled());
        apply_setoption("setoption name OwnBook value true", &mut tt, &mut state);
        assert!(state.book_config.enabled);
        assert!(state.opening_book.is_enabled());
    }

    #[test]
    fn book_depth_and_file_options_parse() {
        let mut tt = TranspositionTable::new(1);
        let mut state = UciState::default();
        apply_setoption("setoption name BookDepth value 8", &mut tt, &mut state);
        assert_eq!(state.book_config.max_plies, 8);
        assert_eq!(state.opening_book.max_plies(), 8);
        apply_setoption("setoption name BookFile value /tmp/book.bin", &mut tt, &mut state);
        assert_eq!(state.book_config.file, Some(PathBuf::from("/tmp/book.bin")));
        apply_setoption("setoption name BookFile value <none>", &mut tt, &mut state);
        assert_eq!(state.book_config.file, None);
    }
}
