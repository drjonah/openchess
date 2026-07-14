//! NNUE training-data pipeline (Phase 2 **Q2-01**).
//!
//! Produces Bullet-ready text lines (`FEN | score | result`) from quiet
//! self-play positions. See [`research/nnue-training.md`].
//!
//! CLI: `openchess nnue-data …`

mod cli;
mod format;
mod generate;
mod quiet;

pub use cli::run;
pub use format::{validate_bullet_text, TrainingRecord};
pub use generate::{
    fixture_options, generate_dataset, load_openings, write_bullet_text, GenerateOptions,
};
pub use quiet::is_quiet_training_position;
