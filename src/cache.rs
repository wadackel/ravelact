use crate::ir::{self, Ir, ParseDiagnostic};
use anyhow::{Context, Result};
use globset::GlobSet;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Default,
    NoCache,
}

#[derive(Debug, Clone)]
pub struct LoadOutcome {
    pub ir: Ir,
    pub cache_path: PathBuf,
    pub stats: ir::build::RebuildStats,
    /// Diagnostics surfaced during this load. Cached-only loads carry an empty
    /// `Vec` because the cache stores resolved IR (annotation Dangling state)
    /// but not the per-parse warning stream.
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheDocument {
    schema_version: u32,
    root: PathBuf,
    git_sha: Option<String>,
    sources: Vec<SourceFingerprint>,
    ir: Ir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceFingerprint {
    path: PathBuf,
    mtime_secs: u64,
    mtime_nanos: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheStatus {
    stale_sources: BTreeSet<PathBuf>,
    has_deleted_sources: bool,
}

/// Resolve the directory under which ravelact keeps its persistent state
/// (currently the IR cache). Honors `XDG_STATE_HOME` when set and non-empty,
/// otherwise falls back to `$HOME/.local/state`. Errors if neither is usable.
pub fn default_state_dir() -> Result<PathBuf> {
    let xdg = std::env::var_os("XDG_STATE_HOME");
    let home = std::env::var_os("HOME");
    state_dir_from(xdg.as_deref(), home.as_deref())
}

/// Pure decision function backing `default_state_dir`. Kept private so tests
/// can exercise XDG / HOME combinations without mutating process env.
fn state_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf> {
    if let Some(s) = xdg {
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    let home = home
        .filter(|s| !s.is_empty())
        .context("HOME environment variable is not set; ravelact needs HOME or XDG_STATE_HOME to locate its cache")?;
    Ok(PathBuf::from(home).join(".local").join("state"))
}

/// Stable 64-bit FNV-1a over the input bytes. Used only as a cache-subkey
/// disambiguator; collisions are caught by `cache_status`'s `document.root`
/// comparison, which forces a silent rebuild rather than a wrong-cache load.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Build the per-repo subdirectory name used under `<state_dir>/ravelact/`.
/// The suffix is the FNV-1a hash of the path bytes truncated to 8 hex chars.
/// The directory intentionally avoids `root.file_name()` so cache paths do not
/// echo local checkout names.
///
/// Callers should pass an already-canonical `root` (this is what
/// `cache::load_or_build` does — `discover_sources` canonicalizes it).
pub(crate) fn repo_subkey(root: &Path) -> String {
    let h = fnv1a_64(root.as_os_str().as_encoded_bytes());
    let hex8 = format!("{:08x}", (h >> 32) as u32);
    format!("repo-{hex8}")
}

/// Returns the cache file path for a repo at `root`, rooted under `state_dir`.
pub fn cache_path(root: &Path, state_dir: &Path) -> PathBuf {
    state_dir
        .join("ravelact")
        .join(repo_subkey(root))
        .join("cache.json")
}

pub fn load_or_build(
    root: &Path,
    mode: CacheMode,
    excludes: &GlobSet,
    state_dir: &Path,
) -> Result<LoadOutcome> {
    let inventory = ir::build::discover_sources(root, excludes)?;
    let path = cache_path(&inventory.root, state_dir);

    let outcome = if matches!(mode, CacheMode::NoCache) {
        let rebuilt = ir::build::rebuild_ir_from_inventory(&inventory, None, &BTreeSet::new())?;
        persist_ir(&rebuilt.ir, state_dir)?;
        LoadOutcome {
            ir: rebuilt.ir,
            cache_path: path,
            stats: rebuilt.stats,
            diagnostics: rebuilt.diagnostics,
        }
    } else if let Some(doc) = load_document(&inventory.root, state_dir)? {
        let current_git_sha = git_sha(&inventory.root);
        let status = cache_status(&inventory, &doc, current_git_sha.as_deref())?;
        if status.stale_sources.is_empty() && !status.has_deleted_sources {
            let reused_workflows = doc.ir.workflows.len();
            let reused_actions = doc.ir.actions.len();
            LoadOutcome {
                ir: doc.ir,
                cache_path: path,
                stats: ir::build::RebuildStats {
                    reused_workflows,
                    reused_actions,
                    ..Default::default()
                },
                diagnostics: Vec::new(),
            }
        } else {
            let rebuilt = ir::build::rebuild_ir_from_inventory(
                &inventory,
                Some(&doc.ir),
                &status.stale_sources,
            )?;
            persist_ir(&rebuilt.ir, state_dir)?;
            LoadOutcome {
                ir: rebuilt.ir,
                cache_path: path,
                stats: rebuilt.stats,
                diagnostics: rebuilt.diagnostics,
            }
        }
    } else {
        let rebuilt = ir::build::rebuild_ir_from_inventory(&inventory, None, &BTreeSet::new())?;
        persist_ir(&rebuilt.ir, state_dir)?;
        LoadOutcome {
            ir: rebuilt.ir,
            cache_path: path,
            stats: rebuilt.stats,
            diagnostics: rebuilt.diagnostics,
        }
    };

    Ok(outcome)
}

#[cfg(test)]
fn save(ir: &Ir, state_dir: &Path) -> Result<PathBuf> {
    persist_ir(ir, state_dir)
}

#[cfg(test)]
fn load(root: &Path, state_dir: &Path) -> Result<Option<Ir>> {
    Ok(load_document(root, state_dir)?.map(|doc| doc.ir))
}

fn persist_ir(ir: &Ir, state_dir: &Path) -> Result<PathBuf> {
    let document = CacheDocument {
        schema_version: ir::build::current_schema_version(),
        root: ir.root.clone(),
        git_sha: git_sha(&ir.root),
        sources: collect_fingerprints(ir)?,
        ir: ir.clone(),
    };
    write_document(&document, state_dir)
}

fn write_document(document: &CacheDocument, state_dir: &Path) -> Result<PathBuf> {
    let path = cache_path(&document.root, state_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(document).context("serialize cache document")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn load_document(root: &Path, state_dir: &Path) -> Result<Option<CacheDocument>> {
    let path = cache_path(root, state_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let Ok(document) = serde_json::from_str::<CacheDocument>(&raw) else {
        return Ok(None);
    };
    if document.schema_version != ir::build::current_schema_version()
        || document.ir.schema_version != ir::build::current_schema_version()
    {
        return Ok(None);
    }
    Ok(Some(document))
}

fn cache_status(
    inventory: &ir::build::SourceInventory,
    document: &CacheDocument,
    current_git_sha: Option<&str>,
) -> Result<CacheStatus> {
    if document.schema_version != ir::build::current_schema_version()
        || document.ir.schema_version != ir::build::current_schema_version()
        || document.root != inventory.root
    {
        return Ok(CacheStatus {
            stale_sources: inventory
                .workflow_files
                .iter()
                .chain(&inventory.action_files)
                .cloned()
                .collect(),
            has_deleted_sources: true,
        });
    }

    let current = fingerprint_map(inventory)?;
    let cached = document
        .sources
        .iter()
        .cloned()
        .map(|source| (source.path.clone(), source))
        .collect::<BTreeMap<_, _>>();

    let mut stale_sources = BTreeSet::new();
    for (path, current_fp) in &current {
        match cached.get(path) {
            Some(cached_fp)
                if cached_fp.mtime_secs == current_fp.mtime_secs
                    && cached_fp.mtime_nanos == current_fp.mtime_nanos => {}
            _ => {
                stale_sources.insert(path.clone());
            }
        }
    }

    let current_paths = current.keys().cloned().collect::<BTreeSet<_>>();
    let cached_paths = cached.keys().cloned().collect::<BTreeSet<_>>();
    let has_deleted_sources = !cached_paths.is_subset(&current_paths);

    match (document.git_sha.as_deref(), current_git_sha) {
        (Some(cached_sha), Some(current_sha)) if cached_sha == current_sha => {}
        (Some(cached_sha), Some(current_sha)) => {
            stale_sources.extend(git_changed_sources(
                &inventory.root,
                cached_sha,
                current_sha,
                &current_paths,
            )?);
        }
        (None, None) => {}
        _ => {
            stale_sources.extend(current_paths.iter().cloned());
        }
    }

    Ok(CacheStatus {
        stale_sources,
        has_deleted_sources,
    })
}

fn collect_fingerprints(ir: &Ir) -> Result<Vec<SourceFingerprint>> {
    let mut sources = ir
        .workflows
        .iter()
        .map(|wf| wf.source.file.clone())
        .chain(ir.actions.iter().map(|c| c.source.file.clone()))
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    let mut out = Vec::with_capacity(sources.len());
    for path in sources {
        out.push(fingerprint_for_path(&path)?);
    }
    Ok(out)
}

fn fingerprint_map(
    inventory: &ir::build::SourceInventory,
) -> Result<BTreeMap<PathBuf, SourceFingerprint>> {
    let mut out = BTreeMap::new();
    for path in inventory
        .workflow_files
        .iter()
        .chain(&inventory.action_files)
    {
        out.insert(path.clone(), fingerprint_for_path(path)?);
    }
    Ok(out)
}

fn fingerprint_for_path(path: &Path) -> Result<SourceFingerprint> {
    let modified = std::fs::metadata(path)
        .with_context(|| format!("metadata {}", path.display()))?
        .modified()
        .with_context(|| format!("modified {}", path.display()))?;
    let duration = modified
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("mtime before unix epoch {}", path.display()))?;
    Ok(SourceFingerprint {
        path: path.to_path_buf(),
        mtime_secs: duration.as_secs(),
        mtime_nanos: duration.subsec_nanos(),
    })
}

fn git_sha(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_string())
    }
}

fn git_changed_sources(
    root: &Path,
    old_sha: &str,
    new_sha: &str,
    current_paths: &BTreeSet<PathBuf>,
) -> Result<BTreeSet<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("diff")
        .arg("--name-only")
        .arg(old_sha)
        .arg(new_sha)
        .output()
        .with_context(|| format!("git diff {}..{}", old_sha, new_sha))?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }
    let stdout = String::from_utf8(output.stdout).context("git diff output is not utf8")?;
    let mut changed = BTreeSet::new();
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        let path = root.join(line);
        if current_paths.contains(&path) {
            changed.insert(path);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;
    use std::ffi::OsString;
    use tempfile::tempdir;

    fn empty_ir(root: &Path) -> Ir {
        Ir {
            schema_version: ir::build::current_schema_version(),
            root: root.to_path_buf(),
            workflows: Vec::new(),
            actions: Vec::new(),
            external_actions: Vec::new(),
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let state = tempdir().unwrap();
        let root = dir.path();
        let ir = empty_ir(root);
        let written = save(&ir, state.path()).unwrap();
        assert!(
            written.starts_with(state.path()),
            "cache must live under state_dir; got {written:?}"
        );
        assert!(
            written.ends_with("cache.json"),
            "cache file must be cache.json; got {written:?}"
        );
        let loaded = load(root, state.path())
            .unwrap()
            .expect("cache should exist");
        assert_eq!(loaded.schema_version, ir::build::current_schema_version());
        assert_eq!(loaded.workflows.len(), 0);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let state = tempdir().unwrap();
        let loaded = load(dir.path(), state.path()).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn load_returns_none_for_schema_mismatch() {
        let dir = tempdir().unwrap();
        let state = tempdir().unwrap();
        let root = dir.path();
        let path = cache_path(root, state.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = serde_json::json!({
            "schema_version": ir::build::current_schema_version() + 1,
            "root": root,
            "git_sha": null,
            "sources": [],
            "ir": {
                "schema_version": ir::build::current_schema_version() + 1,
                "root": root,
                "workflows": [],
                "actions": [],
                "external_actions": [],
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

        let loaded = load(root, state.path()).unwrap();
        assert!(loaded.is_none());
    }

    // ----- Test #11: explicit schema 2 → 3 invalidation ------------------
    //
    // Pre-uniform-record IR (schema 2) used `triggers: [{ "Push": {...} }]`-
    // shaped JSON. This test pins down the post-bump behavior: any cache
    // file whose schema_version is the previous value (2) must be treated
    // as a miss, not silently coerced into the new shape via deserialization
    // glue.
    #[test]
    fn load_returns_none_for_old_schema_2_cache() {
        let dir = tempdir().unwrap();
        let state = tempdir().unwrap();
        let root = dir.path();
        let path = cache_path(root, state.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = serde_json::json!({
            "schema_version": 2,
            "root": root,
            "git_sha": null,
            "sources": [],
            "ir": {
                "schema_version": 2,
                "root": root,
                "workflows": [],
                "actions": [],
                "external_actions": [],
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

        let loaded = load(root, state.path()).unwrap();
        assert!(loaded.is_none(), "schema 2 cache must invalidate");
    }

    /// Corrupted JSON on disk must not panic — `load_document` falls back to
    /// `Ok(None)` so callers treat the cache as a miss and rebuild.
    #[test]
    fn load_returns_none_for_corrupted_json() {
        let dir = tempdir().unwrap();
        let state = tempdir().unwrap();
        let root = dir.path();
        let path = cache_path(root, state.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ this is not valid json").unwrap();

        let loaded = load(root, state.path()).unwrap();
        assert!(
            loaded.is_none(),
            "corrupted cache JSON must invalidate cleanly without panicking",
        );
    }

    // ----- cache_status decision-helper tests -----------------------------
    //
    // These exercise the pure decision function that drives partial vs.
    // full invalidation, without writing real workflow files. We only need
    // a `SourceInventory` that points at on-disk paths so the mtime probe
    // can read metadata; the IR contents themselves are irrelevant here.

    fn make_inventory(root: &Path, files: &[&Path]) -> ir::build::SourceInventory {
        ir::build::SourceInventory {
            root: root.to_path_buf(),
            workflow_files: files.iter().map(|p| p.to_path_buf()).collect(),
            action_files: Vec::new(),
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "name: x\non: push\njobs: {}\n").unwrap();
    }

    fn make_doc(
        root: &Path,
        sources: Vec<SourceFingerprint>,
        git_sha: Option<&str>,
    ) -> CacheDocument {
        CacheDocument {
            schema_version: ir::build::current_schema_version(),
            root: root.to_path_buf(),
            git_sha: git_sha.map(|s| s.to_string()),
            sources,
            ir: empty_ir(root),
        }
    }

    #[test]
    fn cache_status_marks_only_changed_mtime_source_stale() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let kept = root.join(".github/workflows/kept.yml");
        let edited = root.join(".github/workflows/edited.yml");
        touch(&kept);
        touch(&edited);

        // Build the cached fingerprints from the actual files, then perturb
        // the cached entry for `edited` so it looks stale to cache_status.
        let mut cached_sources = vec![
            fingerprint_for_path(&kept).unwrap(),
            fingerprint_for_path(&edited).unwrap(),
        ];
        for fp in &mut cached_sources {
            if fp.path == edited {
                fp.mtime_secs = fp.mtime_secs.wrapping_add(1);
            }
        }
        let doc = make_doc(root, cached_sources, None);
        let inventory = make_inventory(root, &[&kept, &edited]);

        let status = cache_status(&inventory, &doc, None).unwrap();
        assert!(!status.has_deleted_sources);
        assert_eq!(
            status.stale_sources,
            std::iter::once(edited.clone()).collect::<BTreeSet<_>>(),
            "only the file with a changed fingerprint must be stale",
        );
    }

    #[test]
    fn cache_status_invalidates_all_when_git_sha_disappears() {
        // When the cached git_sha is `Some` but the current sha is `None`
        // (e.g. cache saved inside a git repo, then loaded with git unavailable
        // or repo state otherwise unreadable), every current source becomes
        // stale — we cannot trust mtime alone to detect ref-affecting changes.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let wf = root.join(".github/workflows/ci.yml");
        touch(&wf);

        let doc = make_doc(
            root,
            vec![fingerprint_for_path(&wf).unwrap()],
            Some("deadbeefcafebabe"),
        );
        let inventory = make_inventory(root, &[&wf]);

        let status = cache_status(&inventory, &doc, None).unwrap();
        assert_eq!(
            status.stale_sources,
            std::iter::once(wf.clone()).collect::<BTreeSet<_>>(),
            "sha drift (Some → None) must mark every current source stale",
        );
    }

    #[test]
    fn cache_status_marks_all_sources_stale_on_root_mismatch() {
        // When the cached `root` no longer matches the current inventory root
        // (e.g. the worktree moved), the entire cache must be discarded and
        // every current source flagged stale, with `has_deleted_sources` set
        // so the rebuild path runs end-to-end rather than reusing IR by path.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let wf = root.join(".github/workflows/ci.yml");
        touch(&wf);

        let mut doc = make_doc(root, vec![fingerprint_for_path(&wf).unwrap()], None);
        doc.root = PathBuf::from("/some/other/root");
        let inventory = make_inventory(root, &[&wf]);

        let status = cache_status(&inventory, &doc, None).unwrap();
        assert!(status.has_deleted_sources);
        assert_eq!(
            status.stale_sources,
            std::iter::once(wf.clone()).collect::<BTreeSet<_>>(),
            "root mismatch must force every current source to be reparsed",
        );
    }

    // ----- state_dir_from tests -------------------------------------------
    //
    // Pure decision function — exercise XDG / HOME combinations without
    // mutating process env. Confirms (a) XDG wins when set, (b) HOME-based
    // fallback when XDG missing, (c) empty XDG behaves as missing,
    // (d) both missing/empty errors out.

    #[test]
    fn state_dir_from_uses_xdg_when_set() {
        let xdg = OsString::from("/var/cache/state");
        let home = OsString::from("/home/alice");
        let got = state_dir_from(Some(&xdg), Some(&home)).unwrap();
        assert_eq!(got, PathBuf::from("/var/cache/state"));
    }

    #[test]
    fn state_dir_from_falls_back_to_home_when_xdg_missing() {
        let home = OsString::from("/home/alice");
        let got = state_dir_from(None, Some(&home)).unwrap();
        assert_eq!(got, PathBuf::from("/home/alice/.local/state"));
    }

    #[test]
    fn state_dir_from_treats_empty_xdg_as_missing() {
        let xdg = OsString::from("");
        let home = OsString::from("/home/alice");
        let got = state_dir_from(Some(&xdg), Some(&home)).unwrap();
        assert_eq!(got, PathBuf::from("/home/alice/.local/state"));
    }

    #[test]
    fn state_dir_from_errors_when_xdg_and_home_both_unset() {
        let err = state_dir_from(None, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("HOME"),
            "error must mention HOME prerequisite; got: {msg}",
        );
    }

    // ----- repo_subkey tests ----------------------------------------------

    #[test]
    fn repo_subkey_is_stable_for_same_root() {
        let a = repo_subkey(Path::new("/Users/alice/code/proj"));
        let b = repo_subkey(Path::new("/Users/alice/code/proj"));
        assert_eq!(a, b);
    }

    #[test]
    fn repo_subkey_differs_across_roots() {
        let a = repo_subkey(Path::new("/Users/alice/code/proj"));
        let b = repo_subkey(Path::new("/Users/alice/code/other"));
        assert_ne!(a, b);
    }

    #[test]
    fn repo_subkey_uses_hash_only_repo_prefix() {
        let multi = repo_subkey(Path::new("/work/日本語"));
        let (prefix, hash_part) = multi.rsplit_once('-').expect("subkey has '-' separator");
        assert_eq!(prefix, "repo");
        assert_eq!(hash_part.len(), 8);

        let root_only = repo_subkey(Path::new("/"));
        assert!(
            root_only.starts_with("repo-"),
            "`/` must use the repo prefix; got {root_only}",
        );

        let named = repo_subkey(Path::new("/work/checkout-name"));
        assert!(
            !named.contains("checkout-name"),
            "subkey must not contain the checkout basename; got {named}",
        );
    }

    // ----- git_sha + git_changed_sources tests ----------------------------
    //
    // Exercise the actual git plumbing with a real repo. Two commits give
    // us a known sha pair to feed `git_changed_sources`, and we can drive
    // the `(Some, Some) where != ` branch of `cache_status` end-to-end.

    fn run_git_in(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn seed_two_commit_repo() -> (tempfile::TempDir, String, String, PathBuf) {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        run_git_in(&root, &["init", "-q", "-b", "main"]);
        run_git_in(&root, &["config", "user.email", "t@example.com"]);
        run_git_in(&root, &["config", "user.name", "t"]);
        run_git_in(&root, &["config", "commit.gpgsign", "false"]);
        let wf = root.join(".github/workflows/ci.yml");
        touch(&wf);
        run_git_in(&root, &["add", "."]);
        run_git_in(&root, &["commit", "-q", "-m", "first"]);
        let old_sha = git_sha(&root).expect("first sha");
        // Edit the workflow and commit again to produce a known diff.
        std::fs::write(&wf, "name: ci\non: pull_request\njobs: {}\n").unwrap();
        run_git_in(&root, &["commit", "-q", "-am", "second"]);
        let new_sha = git_sha(&root).expect("second sha");
        assert_ne!(old_sha, new_sha);
        (dir, old_sha, new_sha, wf)
    }

    #[test]
    fn git_sha_returns_none_outside_git_repo() {
        let dir = tempdir().unwrap();
        assert_eq!(git_sha(dir.path()), None);
    }

    #[test]
    fn git_sha_returns_full_hex_inside_git_repo() {
        let (_dir, old_sha, _new_sha, _wf) = seed_two_commit_repo();
        assert_eq!(old_sha.len(), 40);
        assert!(old_sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn git_changed_sources_lists_modified_workflow_between_two_shas() {
        let (dir, old_sha, new_sha, wf) = seed_two_commit_repo();
        let mut current_paths = BTreeSet::new();
        current_paths.insert(wf.clone());
        let changed = git_changed_sources(dir.path(), &old_sha, &new_sha, &current_paths).unwrap();
        assert_eq!(changed, current_paths);
    }

    #[test]
    fn git_changed_sources_skips_paths_outside_current_inventory() {
        let (dir, old_sha, new_sha, _wf) = seed_two_commit_repo();
        // current_paths is empty: even though the diff lists the workflow,
        // it is not in `current_paths`, so nothing should be reported.
        let changed =
            git_changed_sources(dir.path(), &old_sha, &new_sha, &BTreeSet::new()).unwrap();
        assert!(changed.is_empty());
    }

    #[test]
    fn cache_status_uses_git_diff_when_both_shas_present_and_differ() {
        let (dir, old_sha, new_sha, wf) = seed_two_commit_repo();
        let inventory = make_inventory(dir.path(), &[&wf]);
        let doc = make_doc(
            dir.path(),
            vec![fingerprint_for_path(&wf).unwrap()],
            Some(&old_sha),
        );
        let status = cache_status(&inventory, &doc, Some(&new_sha)).unwrap();
        assert!(
            status.stale_sources.contains(&wf),
            "sha diff between old and new must mark the changed workflow as stale",
        );
    }
}
