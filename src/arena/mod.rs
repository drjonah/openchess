//! Arena lab — bulk local Bot-vs-Bot battles (pillar **P11**).
//!
//! Runs many concurrent, isolated Bot-vs-Bot games for development, tuning,
//! and observation — **not** formal SPRT (P8-03) and **not** online play (P9).
//! See [`research/ARENA.md`](../../research/ARENA.md) for the full design.
//!
//! Entry point: `openchess arena run …` (headless batch) and
//! `openchess arena watch` (interactive ratatui inspector).

pub mod batch;
mod cli;
pub mod export;
pub mod profile;
pub mod runner;
pub mod slot;
pub mod snapshot;
mod watch;

pub use batch::{BatchOptions, BatchSummary, run as run_batch};
pub use profile::{ArenaProfile, ProfileSet};
pub use runner::{Arena, ArenaConfig};
pub use slot::{FinishReason, GameSlot, Outcome, SlotEvent, SlotStatus};
pub use snapshot::{GameSnapshot, MaterialBalance};

pub use cli::run;
