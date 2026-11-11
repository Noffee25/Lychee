//! Domain model shared by every Lychee component.
//!
//! This crate deliberately has no knowledge of audio, storage or UI. It holds the plain data
//! types (tracks, albums, formats), the play queue and its rules (shuffle, repeat, "play next"),
//! and small helpers for formatting values the same way everywhere.

mod model;
mod queue;
mod time;

pub use model::*;
pub use queue::{Queue, QueueEntry};
pub use time::{format_duration, format_duration_ms, now_unix};
