//! Engine tooling: perft and bench.

pub mod bench;
pub mod perft;

pub use bench::{format_bench_report, run_bench, BenchReport, BENCH_DEPTH};
pub use perft::{perft, perft_divide};
