//! Per-command rendering modules for handlers whose output is non-trivial.
//!
//! A command lives here when its Text rendering grows beyond a small table or
//! single-line print — typically multi-section output, per-row helpers, or
//! enough scaffolding that keeping it inline in `cli/mod.rs` would clutter the
//! dispatch surface. Each module exposes a single `pub(in crate::cli) fn run`
//! entry point invoked from `Cli::run`. Commands with trivial rendering stay
//! inline as `cmd_*` helpers in `cli/mod.rs`; promote one here once its
//! handler outgrows that shape.

pub(super) mod callers;
pub(super) mod dedup;
pub(super) mod extract;
pub(super) mod orphans;
pub(super) mod permissions;
pub(super) mod secrets;
