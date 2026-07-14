//! Engine tooling: perft, bench, and NNUE training-data pipeline.

pub mod bench;
pub mod nnue_data;
pub mod perft;

pub use bench::{format_bench_report, run_bench, BenchReport, BENCH_DEPTH};
pub use perft::{perft, perft_divide};
