//! Answer allowlisted Telegram messages with headless Claude Code runs.
//!
//! The allowlist ([`allowlist`]) decides who gets through before any model sees a
//! message; the trust tier of the admitting rule decides what a run may do
//! ([`runner`]); the runner, not the model, posts the reply.

pub mod allowlist;
pub mod config;
pub mod tg;

use std::time::{SystemTime, UNIX_EPOCH};

/// One timestamped line on stderr.
pub fn log(msg: &str) {
    eprintln!("{} {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
}

/// Unix seconds.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// At most `max` characters of `s`.
pub fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
