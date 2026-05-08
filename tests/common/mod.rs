//! Shared helpers for the `ravelact` integration test binaries.
//!
//! Each test binary under `tests/*.rs` is its own crate; including this
//! module via `mod common;` gives that binary a process-wide
//! `XDG_STATE_HOME` tempdir so cache writes never touch the developer's
//! `~/.local/state/ravelact/`.
//!
//! The `LazyLock<TempDir>` static is per-binary (not shared across binaries),
//! so each `cargo test` test binary gets its own isolated state dir created
//! on first access and removed at process exit.

#![allow(dead_code)] // Each test binary uses a subset of these helpers.

use ravelact::cache;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tempfile::TempDir;

/// Process-wide XDG state directory for one test binary. Initialized lazily
/// on first access and dropped at process exit. Per-test isolation is
/// provided by per-test `root` tempdirs producing distinct
/// `repo-<sha8>` subkeys, so parallel tests never collide.
pub static TEST_STATE: LazyLock<TempDir> =
    LazyLock::new(|| tempfile::tempdir().expect("test XDG state dir"));

/// Path that should be wired into spawned `ravelact` invocations as both
/// `XDG_STATE_HOME` and `HOME`, and into in-process `cache::load_or_build`
/// calls as the `state_dir` argument.
pub fn test_state_dir() -> &'static Path {
    TEST_STATE.path()
}

/// Compute the cache path that a `ravelact` invocation rooted at `root`
/// would write to, given the binary's shared `XDG_STATE_HOME` tempdir. The
/// binary canonicalizes its root inside `discover_sources`, so we
/// canonicalize here too — `unwrap` is sound because `root` is always a
/// freshly created tempdir in tests.
pub fn test_cache_path(root: &Path) -> PathBuf {
    cache::cache_path(
        &root.canonicalize().expect("canonicalize tempdir root"),
        test_state_dir(),
    )
}
