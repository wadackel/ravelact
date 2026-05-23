use crate::cache;
use crate::ir::{
    ActionKind, DanglingLocalUsesKind, EventKind, Ir, ParseDiagnostic, WiringFinding, WiringKind,
};
use crate::markdown;
use crate::query::{self, impact::ImpactResult, trace_render::TreeStyle};
use crate::ui::{self, ColorMode, Status, Ui};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

// `render::browse::{proto, connect}` is exposed to integration tests in
// `tests/e2e_browse.rs`. Re-exporting the whole render module is the
// thinnest cut that makes the chain reachable; siblings remain
// crate-private per their `pub(super)` markers.
pub mod render;
mod stdin_input;

#[derive(Parser, Debug)]
#[command(
    name = "ravelact",
    version,
    about = "Static analysis CLI for GitHub Actions workflow estates",
    help_template = "\
{about}

Usage: {usage}

Inspect (exit 0; non-blocking reports):
  trace        Forward walk from a trigger event (push, pull_request, ...)
  triggers     Summarize trigger events declared by workflows
  callers      List call sites that reference a workflow / action
  impact       Reverse impact: which entry-points are affected by changed files
  orphans      Declared-but-unused report (workflows / actions / inputs / outputs)

Check (exit 0/1; non-zero on findings):
  permissions  Effective permissions scope across caller -> callee chains
  secrets      Secrets propagation across reusable-workflow chains
  wiring       Workflow dependency wiring consistency

Suggest (exit 0; refactor candidates, non-mutating):
  extract      Composite-action extraction candidates from duplicated step sequences
  dedup        Near-duplicate workflow clusters (reports clusters only)

Export (output artifacts):
  dump         Print IR as JSON
  graph        Render the call graph as Mermaid (use --event to filter)

Other:
  browse       Launch local server and render the workflow graph in a browser (PoC)
  build        Build IR and persist to ${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json
  completion   Generate shell completion setup snippet (bash / zsh / fish)
  help         Print this message or the help of the given subcommand(s)

Run `ravelact <COMMAND> --help` for details on a single command.

Options:
{options}
"
)]
pub struct Cli {
    /// Repository root. Defaults to the current working directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    /// Bypass the IR cache (under `${XDG_STATE_HOME}/ravelact/`) and force a full rebuild.
    #[arg(long, global = true, default_value_t = false)]
    pub no_cache: bool,

    /// Exclude local-action manifests whose workspace-relative path matches
    /// the given glob (repeatable). Useful for skipping `tests/fixtures/**`-
    /// style intentional test data when dogfooding ravelact on its own
    /// repository, or for narrowing analysis to a sub-tree of a monorepo.
    /// Patterns follow globset syntax (`*`, `**`, `[abc]`, `?`); `**` matches
    /// across directory separators. Workflow files under `.github/workflows/`
    /// are not affected.
    #[arg(long, global = true, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Color output: `auto` uses color only on terminals, `always` forces ANSI
    /// color unless `NO_COLOR` is set, and `never` disables color.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Discover workflows + local actions (composite / JavaScript / Docker), build IR, write the cache to `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (falls back to `$HOME/.local/state/...` when `XDG_STATE_HOME` is unset).
    Build,

    /// Forward walk from the given trigger event (e.g. `push`, `pull_request`,
    /// `workflow_dispatch`, `schedule`, `workflow_run`).
    ///
    /// `--type` filters to entry-points whose `types:` declaration matches
    /// at least one of the listed activity types (OR semantics across
    /// repeats). When omitted, no activity-type filtering is applied;
    /// `pull_request` / `pull_request_target` workflows that omit `types:`
    /// in their YAML still match the GitHub default subset
    /// (`opened` / `synchronize` / `reopened`). `repository_dispatch.types`
    /// (custom `event_type` values) is matched the same way.
    ///
    /// `--branch`, `--tag`, and `--path` further narrow the result by the
    /// trigger's `branches:` / `tags:` / `paths:` filter fields. Repeating a
    /// flag is OR within that filter (`--branch main --branch develop` =
    /// "either main or develop satisfies branches"); combining different
    /// flags is AND across filter types. Each `--path X` is interpreted as
    /// the single-file changeset of `X` (so `paths-ignore: [docs/**]` rejects
    /// `--path docs/x.md`). Pattern syntax follows globset's glob subset; see
    /// docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#filter-pattern-cheat-sheet
    /// for the upstream reference.
    Trace {
        /// The trigger event to walk forward from
        /// (e.g. `push`, `pull_request`, `workflow_dispatch`, `schedule`, `workflow_run`).
        #[arg(add = ArgValueCompleter::new(list_event_names))]
        event: Option<String>,
        #[arg(long = "type")]
        types: Vec<String>,
        /// Branch ref name (or glob) to test against the trigger's
        /// `branches:` / `branches-ignore:` filter. Repeatable.
        #[arg(long = "branch")]
        branches: Vec<String>,
        /// Tag ref name (or glob) to test against the trigger's
        /// `tags:` / `tags-ignore:` filter. Repeatable.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// File path to test against the trigger's `paths:` / `paths-ignore:`
        /// filter, interpreted as a single-file changeset. Repeatable.
        #[arg(long = "path")]
        paths: Vec<String>,
        /// Output format. `tree` (default) renders a Unicode box-drawing tree;
        /// `table` renders a 5-column audit table; `json` emits structured
        /// trace nodes; `markdown` emits a PR-comment-friendly table.
        #[arg(long, value_enum, default_value_t = TraceFormat::Tree)]
        format: TraceFormat,
        /// Use ASCII fallback border characters instead of Unicode. Affects
        /// the `tree` borders only (`├──` → `|--`); the `table` format is
        /// always plain regardless of this flag. Color output is independently
        /// controlled by `NO_COLOR` and TTY detection.
        #[arg(long, default_value_t = false)]
        ascii: bool,
    },

    /// Summarize trigger events declared across workflows.
    Triggers {
        /// Output format. `text` (default) renders a fixed table; `json`
        /// emits structured summary rows; `markdown` emits a PR-comment-ready
        /// table.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// List call sites that reference the given workflow / action.
    Callers {
        /// One or more target paths relative to the repository root
        /// (e.g. `.github/workflows/build.yml` or `.github/actions/setup`).
        ///
        /// If no targets are given and stdin is piped (non-TTY), reads
        /// targets from stdin (one per line). `-` as a positional value is
        /// replaced by stdin lines (rg / grep convention).
        #[arg(add = ArgValueCompleter::new(list_workflow_targets))]
        targets: Vec<String>,
        /// Output format. `text` (default) renders one caller per line as
        /// `file:job:index` with a `# <target>` header per input; `json`
        /// emits an array of `{target, hits}` objects (one entry per input,
        /// preserving order; empty hits are not filtered).
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Reverse impact analysis: given a list of changed files, list the
    /// entry-point workflows and local-action consumers transitively affected.
    /// The input nodes themselves are excluded from the result; only
    /// downstream consumers are listed.
    Impact {
        /// One or more file paths (workflow YAML, action manifest, or any
        /// path under a local action directory). Paths are workspace-
        /// relative; `./` prefix and trailing `/` are tolerated.
        ///
        /// If no paths are given and stdin is piped (non-TTY), reads paths
        /// from stdin (one per line). `-` as a positional value is replaced
        /// by stdin lines (rg / grep convention).
        #[arg(add = ArgValueCompleter::new(list_action_paths))]
        files: Vec<String>,
        /// Output format. `text` (default) renders a line-based human-readable
        /// list; `json` emits a machine-readable object suitable for jq.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Report declared-but-unused items across the workflow estate. Emits four
    /// kinds: reusable workflows / local actions (composite / JavaScript /
    /// Docker) that nothing references, declared inputs that the callee body
    /// never references, and declared outputs that no caller consumes via
    /// `needs.<job>.outputs.<X>` / `steps.<id>.outputs.<X>`. Exit code is
    /// always 0 (informational).
    Orphans {
        /// Output format. `text` (default) renders one orphan per line per
        /// kind (`local-action-<kind>` rows for actions); `json` emits an
        /// object with four keys: `workflows`, `actions`,
        /// `unreferenced_inputs`, `unused_outputs`.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Verify that declared dependency edges resolve and that observable
    /// dispatches are declared. Reports unannotated `gh workflow run`
    /// invocations, dangling `# ravelact:` annotations, and unresolvable
    /// `on.workflow_run.workflows` entries. Exits non-zero when any
    /// finding is reported.
    Wiring {
        /// Output format. `text` (default) renders findings as
        /// `file:line: message`; `json` emits a JSON array of structured findings.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Compute the effective `permissions:` scope across caller→callee
    /// chains and surface (a) overly-broad coarse declarations on entry
    /// workflows, (b) callee declarations that exceed the caller, and
    /// (c) entry workflows with no permissions declared at any layer.
    /// Exit code: 0 when clean, 1 when any finding is reported.
    Permissions {
        /// Output format. `text` (default) renders human-readable findings;
        /// `json` emits a JSON array on stdout.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Trace `secrets:` propagation across entry-point → reusable workflow
    /// chains and surface (a) MissingSecretPropagation, (b)
    /// SecretsInheritChainBreak, and (c) EnvironmentInWorkflowCallCallee.
    /// External (cross-repo) callees are opaque and skipped. Exit code: 0
    /// when clean, 1 when any finding is reported.
    Secrets {
        /// Output format. `text` (default) renders human-readable findings;
        /// `json` emits a JSON array on stdout.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Detect duplicated step sequences across workflows / composite actions
    /// and emit ranked composite-action extraction candidates with a sketch
    /// `action.yml` per candidate. Exit code: always 0 (suggestions, not errors).
    Extract {
        /// Minimum step count for a candidate sequence (default 3).
        #[arg(long, default_value_t = 3)]
        min_length: usize,
        /// Minimum occurrence count to qualify as a candidate (default 2).
        #[arg(long, default_value_t = 2)]
        min_occurrences: usize,
        /// Output format. `text` (default) renders the candidate sketches as
        /// human-readable blocks; `json` emits a JSON array on stdout.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Cluster near-duplicate workflows by structural + run-script similarity.
    /// Outputs each cluster's representative, members, common/divergent
    /// `uses:` references, and whether the trigger sets differ. Non-mutating:
    /// reports clusters only, never rewrites files.
    Dedup {
        /// Pairs with weighted-Jaccard similarity ≥ this threshold are linked
        /// (single-linkage union-find). Default 0.8.
        #[arg(long, default_value_t = 0.8_f32)]
        threshold: f32,

        /// Output format. `text` (default) renders cluster blocks; `json`
        /// emits a JSON array on stdout.
        #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
    },

    /// Print the IR as JSON.
    Dump,

    /// Render the call graph as a Mermaid `graph LR`. Entry workflows are
    /// grouped into `subgraph` blocks per trigger event (multi-trigger
    /// workflows appear as one alias per trigger), reusable workflows /
    /// local actions / external actions are shared nodes, and edges follow
    /// `uses` / `workflow_call` / `workflow_run`. `--event <event>` filters
    /// to entry workflows for a single trigger and is recommended for large
    /// estates where the unfiltered graph is too dense to read.
    /// `--format text` (default) emits raw Mermaid suitable for `> graph.mmd`;
    /// `--format markdown` wraps the same Mermaid in a `### Graph` heading +
    /// fenced ```mermaid block for inline embedding in PR comments / GitHub
    /// Job Summaries. `--format json` is not supported (use `dump` for IR
    /// JSON).
    Graph {
        /// Filter to entry-point workflows for the given event.
        /// Examples: `push`, `pull_request`, `schedule`, `workflow_dispatch`,
        /// `workflow_run`.
        #[arg(long)]
        event: Option<String>,

        /// Output format. `text` (default) emits raw Mermaid; `markdown`
        /// wraps the Mermaid in a `### Graph` heading + fenced ```mermaid
        /// block for inline embedding in PR comments / GitHub Job Summaries.
        #[arg(long, value_enum, default_value_t = GraphFormat::Text)]
        format: GraphFormat,
    },

    /// Generate shell completion setup snippet for bash / zsh / fish.
    ///
    /// Prints instructions a user can `source` from their rc to enable
    /// dynamic completion via the `COMPLETE` environment variable.
    Completion {
        /// Shell type (`bash`, `zsh`, or `fish`).
        shell: String,
    },

    /// Launch a local HTTP server and render the workflow graph in a
    /// browser via Cytoscape.js. Minimal PoC — no filters, no detail
    /// panels, no live reload. Binds to `127.0.0.1` on an ephemeral
    /// port (override with `--port`), opens the default browser
    /// (skip with `--no-open`), and serves until `Ctrl+C`.
    Browse {
        /// TCP port to listen on. Defaults to an OS-assigned ephemeral
        /// port (`0`). Binds to `127.0.0.1` only.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Skip automatic browser launch (useful for headless or scripted use).
        #[arg(long, default_value_t = false)]
        no_open: bool,
        /// Include local-action manifests under `tests/fixtures/**` in the
        /// browse graph. By default `browse` excludes these to keep the
        /// dogfood view focused on production workflows.
        #[arg(long, default_value_t = false)]
        include_test_fixtures: bool,
    },
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum TraceFormat {
    #[default]
    Tree,
    Table,
    Json,
    Markdown,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ReportFormat {
    #[default]
    Text,
    Json,
    Markdown,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum GraphFormat {
    #[default]
    Text,
    Markdown,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

impl From<&ColorChoice> for ColorMode {
    fn from(value: &ColorChoice) -> Self {
        match value {
            ColorChoice::Auto => ColorMode::Auto,
            ColorChoice::Always => ColorMode::Always,
            ColorChoice::Never => ColorMode::Never,
        }
    }
}

impl Cli {
    // Dispatch boundary: commands with trivial rendering (small table, one-line
    // serialization, simple delegation to a `query::*` renderer) call inline
    // `cmd_*` helpers defined further down in this file. Commands whose Text
    // rendering is non-trivial — multi-section output, per-row helpers, or
    // enough scaffolding to clutter this file — live in `cli/render/<name>.rs`
    // and are invoked as `render::<name>::run(...)`. When a new command's
    // handler outgrows the inline shape, promote it to `cli/render/`.
    pub fn run(&self) -> Result<i32> {
        let root: &Path = self.root.as_deref().unwrap_or(Path::new("."));
        let ui = Ui::from_env((&self.color).into(), root);
        let cache_mode = if self.no_cache {
            cache::CacheMode::NoCache
        } else {
            cache::CacheMode::Default
        };
        let excludes = build_exclude_set(&self.exclude)?;
        match &self.command {
            Command::Build => cmd_build(root, cache_mode, &excludes, &ui).map(|_| 0),
            Command::Trace {
                event,
                types,
                branches,
                tags,
                paths,
                format,
                ascii,
            } => match event {
                Some(event) => cmd_trace(
                    root, cache_mode, &excludes, event, types, branches, tags, paths, format,
                    *ascii, &ui,
                )
                .map(|_| 0),
                None => Err(anyhow::anyhow!(
                    "`trace` requires a trigger event\n\nTry `ravelact triggers` to list trigger events found in this repository.\nThen run `ravelact trace <event>`, for example `ravelact trace push`."
                )),
            },
            Command::Triggers { format } => {
                cmd_triggers(root, cache_mode, &excludes, format, &ui).map(|_| 0)
            }
            Command::Callers { targets, format } => {
                render::callers::run(root, cache_mode, &excludes, targets, format, &ui).map(|_| 0)
            }
            Command::Impact { files, format } => {
                cmd_impact(root, cache_mode, &excludes, files, format, &ui).map(|_| 0)
            }
            Command::Orphans { format } => {
                render::orphans::run(root, cache_mode, &excludes, format, &ui).map(|_| 0)
            }
            Command::Wiring { format } => cmd_wiring(root, cache_mode, &excludes, format, &ui),
            Command::Permissions { format } => {
                render::permissions::run(root, cache_mode, &excludes, format, &ui)
            }
            Command::Secrets { format } => {
                render::secrets::run(root, cache_mode, &excludes, format, &ui)
            }
            Command::Extract {
                min_length,
                min_occurrences,
                format,
            } => render::extract::run(
                root,
                cache_mode,
                &excludes,
                *min_length,
                *min_occurrences,
                format,
                &ui,
            )
            .map(|_| 0),
            Command::Dedup { threshold, format } => {
                render::dedup::run(root, cache_mode, &excludes, *threshold, format, &ui).map(|_| 0)
            }
            Command::Dump => cmd_dump(root, cache_mode, &excludes).map(|_| 0),
            Command::Graph { event, format } => {
                cmd_graph(root, cache_mode, &excludes, event.as_deref(), format).map(|_| 0)
            }
            Command::Completion { shell } => cmd_completion(shell).map(|_| 0),
            Command::Browse {
                port,
                no_open,
                include_test_fixtures,
            } => {
                let browse_excludes = if *include_test_fixtures {
                    excludes.clone()
                } else {
                    let mut patterns = self.exclude.clone();
                    patterns.insert(0, "tests/fixtures/**".to_string());
                    build_exclude_set(&patterns)?
                };
                render::browse::run(root, cache_mode, &browse_excludes, *port, *no_open).map(|_| 0)
            }
        }
    }
}

fn cmd_completion(shell: &str) -> Result<()> {
    match shell {
        "bash" => print!(
            "# ravelact shell completion setup for Bash\n\
             # Add this to your ~/.bashrc:\n\
             source <(COMPLETE=bash ravelact)\n"
        ),
        "zsh" => print!(
            "# ravelact shell completion setup for Zsh\n\
             # Add this to your ~/.zshrc:\n\
             source <(COMPLETE=zsh ravelact)\n"
        ),
        "fish" => print!(
            "# ravelact shell completion setup for Fish\n\
             # Add this to your ~/.config/fish/config.fish:\n\
             COMPLETE=fish ravelact | source\n"
        ),
        other => anyhow::bail!("Invalid shell: {other}. Supported shells: bash, zsh, fish"),
    }
    Ok(())
}

/// Static enumeration of `EventKind` variant names for `trace <event>` completion.
/// `EventKind::Other { name }` is excluded — its `name` is data-dependent and cannot
/// be enumerated without loading the IR.
fn list_event_names(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let names: &[&str] = &[
        EventKind::Push.name(),
        EventKind::PullRequest.name(),
        EventKind::PullRequestTarget.name(),
        EventKind::PullRequestReview.name(),
        EventKind::PullRequestReviewComment.name(),
        EventKind::Issues.name(),
        EventKind::IssueComment.name(),
        EventKind::Release.name(),
        EventKind::Discussion.name(),
        EventKind::DiscussionComment.name(),
        EventKind::Schedule.name(),
        EventKind::WorkflowDispatch.name(),
        EventKind::WorkflowCall.name(),
        EventKind::WorkflowRun.name(),
        EventKind::RepositoryDispatch.name(),
        EventKind::CheckRun.name(),
        EventKind::CheckSuite.name(),
        EventKind::MergeGroup.name(),
        EventKind::Milestone.name(),
        EventKind::Label.name(),
        EventKind::RegistryPackage.name(),
        EventKind::BranchProtectionRule.name(),
        EventKind::Watch.name(),
    ];
    names
        .iter()
        .filter(|n| n.starts_with(&*prefix))
        .map(|n| CompletionCandidate::new(*n))
        .collect()
}

/// Resolve the `--root` value from the current process argv. The `ArgValueCompleter`
/// callback signature does not pass prior args, so we scan `std::env::args_os()`
/// directly. Supports `--root <PATH>` and `--root=<PATH>`. Falls back to cwd.
fn parse_root_from_argv() -> PathBuf {
    let mut iter = std::env::args_os().skip(1);
    while let Some(a) = iter.next() {
        let s = a.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix("--root=") {
            return PathBuf::from(rest);
        }
        if s == "--root" {
            if let Some(next) = iter.next() {
                return PathBuf::from(next);
            }
        }
    }
    PathBuf::from(".")
}

/// Walk `<root>/.github/workflows/` for workflow YAMLs and
/// `<root>/.github/actions/<name>/` for action **directories** (not `action.yml`
/// manifests) and collect repo-relative paths suitable as completion candidates
/// for `callers`. Action directories are emitted because `Target::from_user_input`
/// expects a directory path for action targets and matches the same value the IR
/// stores as `LocalAction.id`.
fn list_workflow_targets(current: &OsStr) -> Vec<CompletionCandidate> {
    let root = parse_root_from_argv();
    let prefix = current.to_string_lossy();
    let mut out = BTreeSet::new();

    let workflows_dir = root.join(".github/workflows");
    if workflows_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&workflows_dir)
            .max_depth(2)
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !(name.ends_with(".yaml") || name.ends_with(".yml")) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(&root) {
                let rel_str = rel.to_string_lossy().into_owned();
                if rel_str.starts_with(&*prefix) {
                    out.insert(rel_str);
                }
            }
        }
    }

    let actions_dir = root.join(".github/actions");
    if actions_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&actions_dir)
            .max_depth(1)
            .min_depth(1)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let dir = entry.path();
            if !dir.join("action.yml").is_file() && !dir.join("action.yaml").is_file() {
                continue;
            }
            if let Ok(rel) = dir.strip_prefix(&root) {
                let rel_str = rel.to_string_lossy().into_owned();
                if rel_str.starts_with(&*prefix) {
                    out.insert(rel_str);
                }
            }
        }
    }

    out.into_iter().map(CompletionCandidate::new).collect()
}

/// Walk `<root>/.github/workflows/` and `<root>/.github/actions/**` (any file under
/// local action dirs) and collect repo-relative paths for `impact`.
fn list_action_paths(current: &OsStr) -> Vec<CompletionCandidate> {
    let root = parse_root_from_argv();
    let prefix = current.to_string_lossy();
    let mut out = BTreeSet::new();

    let workflows_dir = root.join(".github/workflows");
    if workflows_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&workflows_dir)
            .max_depth(2)
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !(name.ends_with(".yaml") || name.ends_with(".yml")) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(&root) {
                let rel_str = rel.to_string_lossy().into_owned();
                if rel_str.starts_with(&*prefix) {
                    out.insert(rel_str);
                }
            }
        }
    }

    let actions_dir = root.join(".github/actions");
    if actions_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&actions_dir).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if let Ok(rel) = path.strip_prefix(&root) {
                let rel_str = rel.to_string_lossy().into_owned();
                if rel_str.starts_with(&*prefix) {
                    out.insert(rel_str);
                }
            }
        }
    }

    out.into_iter().map(CompletionCandidate::new).collect()
}

/// Resolve the default IR state directory and load (or rebuild) the cache,
/// emitting any parse diagnostics. The single entry point that pairs
/// `cache::default_state_dir` with `cache::load_or_build` so both
/// `build_or_load` and `cmd_build` go through the same site.
fn load_outcome_with_diagnostics(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
) -> Result<cache::LoadOutcome> {
    let state_dir = cache::default_state_dir()?;
    let outcome = cache::load_or_build(root, cache_mode, excludes, &state_dir)?;
    emit_diagnostics(&outcome.diagnostics);
    Ok(outcome)
}

pub(in crate::cli) fn build_or_load(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
) -> Result<Ir> {
    Ok(load_outcome_with_diagnostics(root, cache_mode, excludes)?.ir)
}

/// Build a `GlobSet` from CLI-supplied `--exclude` patterns. Empty input
/// yields `GlobSet::empty()`. Invalid glob syntax (e.g. `tests/[`) is reported
/// eagerly so the user gets feedback at startup rather than mid-run.
pub(in crate::cli) fn build_exclude_set(patterns: &[String]) -> Result<GlobSet> {
    if patterns.is_empty() {
        return Ok(GlobSet::empty());
    }
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let glob = Glob::new(pat).with_context(|| format!("invalid --exclude pattern `{pat}`"))?;
        builder.add(glob);
    }
    builder.build().context("compile --exclude glob set")
}

fn emit_diagnostics(diags: &[ParseDiagnostic]) {
    for d in diags {
        eprintln!("{}:{}: {}", d.file.display(), d.line, d.message);
    }
}

fn cmd_build(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    ui: &Ui,
) -> Result<()> {
    let outcome = load_outcome_with_diagnostics(root, cache_mode, excludes)?;
    let ir = outcome.ir;
    let path = outcome.cache_path;
    let diag_count = outcome.diagnostics.len();
    let header = if diag_count == 0 {
        ui.status_header("build", Status::Clean, "workflow estate index built", &[])
    } else {
        ui.status_header(
            "build",
            Status::Warning,
            "workflow estate index built with diagnostics",
            &[format!("{diag_count} diagnostics")],
        )
    };
    println!("{}", header);
    println!();
    println!("{}", ui.section("Summary"));
    let rows = vec![
        vec!["workflows".into(), ir.workflows.len().to_string()],
        vec!["local actions".into(), ir.actions.len().to_string()],
        vec![
            "external actions".into(),
            ir.external_actions.len().to_string(),
        ],
        vec!["diagnostics".into(), diag_count.to_string()],
        vec!["cache".into(), ui::normalize_path(&path)],
    ];
    print!("{}", ui.table(&["metric", "value"], &rows));
    Ok(())
}

fn cmd_dump(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    println!("{}", serde_json::to_string_pretty(&ir)?);
    Ok(())
}

fn cmd_graph(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    event: Option<&str>,
    format: &GraphFormat,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    match format {
        GraphFormat::Text => {
            print!("{}", query::mermaid::render(&ir, event));
        }
        GraphFormat::Markdown => {
            println!("### Graph");
            println!();
            println!("```mermaid");
            // `query::mermaid::render` always returns a string ending in a
            // single `\n`, so `print!` keeps the body and the closing fence
            // separated by exactly one newline.
            print!("{}", query::mermaid::render(&ir, event));
            println!("```");
        }
    }
    Ok(())
}
/// Map an `ActionKind` to the lowercase, single-token label used in
/// `local-action-<kind>` text rows and the `kind` field of JSON elements
/// emitted by `orphans` and `impact`. The `JavaScript` variant intentionally
/// drops `node_version` here — that detail is exposed via the IR `dump`
/// surface, not the orphans / impact summaries.
fn action_kind_label(kind: &ActionKind) -> &'static str {
    match kind {
        ActionKind::Composite => "composite",
        ActionKind::JavaScript { .. } => "javascript",
        ActionKind::Docker => "docker",
    }
}

fn cmd_triggers(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    format: &ReportFormat,
    ui: &Ui,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let rows = query::triggers::triggers(&ir);
    match format {
        ReportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        ReportFormat::Markdown => {
            println!("### Triggers");
            println!();
            if rows.is_empty() {
                println!("No trigger declarations found.");
            } else {
                println!(
                    "{} found.",
                    ui::plural(rows.len(), "trigger event", "trigger events")
                );
                println!();
                println!(
                    "| Event | Entry workflows | Declarations | Typed | Filtered | Examples |"
                );
                println!("|---|---:|---:|---:|---:|---|");
                for row in rows {
                    println!(
                        "| {} | {} | {} | {} | {} | {} |",
                        markdown::code_cell(&row.event),
                        row.entry_workflows,
                        row.declarations,
                        row.typed,
                        row.filtered,
                        row.examples
                            .iter()
                            .map(|example| markdown::code_cell(example))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }
        ReportFormat::Text => {
            if rows.is_empty() {
                println!(
                    "{}",
                    ui.status_header("triggers", Status::Clean, "no trigger declarations", &[])
                );
                return Ok(());
            }

            let total_declarations: usize = rows.iter().map(|row| row.declarations).sum();
            let summary = vec![ui::plural(
                total_declarations,
                "trigger declaration",
                "trigger declarations",
            )];
            println!(
                "{}",
                ui.status_header(
                    "triggers",
                    Status::Found,
                    ui::plural(rows.len(), "trigger event", "trigger events"),
                    &summary,
                )
            );
            println!();
            let table_rows: Vec<Vec<String>> = rows
                .into_iter()
                .map(|row| {
                    vec![
                        row.event,
                        row.entry_workflows.to_string(),
                        row.declarations.to_string(),
                        row.typed.to_string(),
                        row.filtered.to_string(),
                        row.examples.join(", "),
                    ]
                })
                .collect();
            print!(
                "{}",
                ui.table(
                    &[
                        "event",
                        "entry workflows",
                        "declarations",
                        "typed",
                        "filtered",
                        "examples",
                    ],
                    &table_rows,
                )
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_trace(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    event: &str,
    types: &[String],
    branches: &[String],
    tags: &[String],
    paths: &[String],
    format: &TraceFormat,
    ascii: bool,
    ui: &Ui,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let entries: Vec<query::trace::TraceEntry> =
        query::trace::trace(&ir, event, types, branches, tags, paths);
    let metadata = trace_filter_metadata(types, branches, tags, paths);
    let command = format!("trace {event}");
    let unicode = !ascii;
    match format {
        TraceFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&query::trace::trace_json_entries(&entries))?
            );
        }
        TraceFormat::Markdown => {
            println!("### Trace");
            println!();
            if entries.is_empty() {
                println!("No entry-point matches found for `{event}`.");
            } else {
                println!(
                    "{} found for `{event}`.",
                    ui::plural(entries.len(), "entry workflow", "entry workflows")
                );
                println!();
                print!("{}", query::trace_render::render_markdown_table(&entries));
            }
        }
        TraceFormat::Tree | TraceFormat::Table if entries.is_empty() => {
            println!(
                "{}",
                ui.status_header(&command, Status::Clean, "no entry-point matches", &metadata)
            );
        }
        TraceFormat::Tree => {
            // Tree mode hoists the event + filter metadata into the tree's
            // synthetic root, so the status header carries only the count.
            let entry_count = ui::plural(entries.len(), "entry workflow", "entry workflows");
            println!(
                "{}",
                ui.status_header(&command, Status::Found, entry_count, &[])
            );
            println!();
            let style = TreeStyle { unicode };
            let event_meta = query::trace_render::EventMeta {
                event,
                summary: &metadata,
            };
            print!(
                "{}",
                query::trace_render::render_tree(&entries, Some(event_meta), &style, ui)
            );
        }
        TraceFormat::Table => {
            // Table mode keeps filter metadata in the status header — the
            // 5-column table has no event-row concept.
            let entry_count = ui::plural(entries.len(), "entry workflow", "entry workflows");
            println!(
                "{}",
                ui.status_header(&command, Status::Found, entry_count, &metadata)
            );
            println!();
            print!("{}", query::trace_render::render_table(&entries, unicode));
        }
    }
    Ok(())
}

fn trace_filter_metadata(
    types: &[String],
    branches: &[String],
    tags: &[String],
    paths: &[String],
) -> Vec<String> {
    let mut summary: Vec<String> = Vec::new();
    if !types.is_empty() {
        summary.push(format!("types=[{}]", types.join(",")));
    }
    if !branches.is_empty() {
        summary.push(format!("branches=[{}]", branches.join(",")));
    }
    if !tags.is_empty() {
        summary.push(format!("tags=[{}]", tags.join(",")));
    }
    if !paths.is_empty() {
        summary.push(format!("paths=[{}]", paths.join(",")));
    }
    if summary.is_empty() {
        summary.push("filters=none".to_string());
    }
    summary
}

fn cmd_impact(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    files: &[String],
    format: &ReportFormat,
    ui: &Ui,
) -> Result<()> {
    let inputs = stdin_input::collect(files)?;
    let ir = build_or_load(root, cache_mode, excludes)?;
    let (ImpactResult { workflows, actions }, unknowns) = query::impact::impact(&ir, &inputs);

    for u in &unknowns {
        eprintln!("warn: {u}: not mapped to any IR node, skipping");
    }

    match format {
        ReportFormat::Markdown => {
            println!("### Impact");
            println!();
            if workflows.is_empty() && actions.is_empty() {
                println!("No impacted targets found.");
            } else {
                let total = workflows.len() + actions.len();
                println!(
                    "{} found: {}, {}.",
                    ui::plural(total, "impacted target", "impacted targets"),
                    ui::plural(workflows.len(), "workflow", "workflows"),
                    ui::plural(actions.len(), "local action", "local actions"),
                );
                println!();
                println!("| Kind | Target |");
                println!("|---|---|");
                for wf in &workflows {
                    println!("| workflow | `{}` |", wf.0);
                }
                for (id, kind) in &actions {
                    println!("| local-action-{} | `{}` |", action_kind_label(kind), id.0);
                }
            }
        }
        ReportFormat::Json => {
            let actions_json: Vec<serde_json::Value> = actions
                .iter()
                .map(|(id, kind)| {
                    serde_json::json!({
                        "id": &id.0,
                        "kind": action_kind_label(kind),
                    })
                })
                .collect();
            let payload = serde_json::json!({
                "workflows": workflows.iter().map(|w| &w.0).collect::<Vec<_>>(),
                "actions": actions_json,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ReportFormat::Text => {
            if workflows.is_empty() && actions.is_empty() {
                println!(
                    "{}",
                    ui.status_header("impact", Status::Clean, "no impacted targets", &[])
                );
                return Ok(());
            }
            let total = workflows.len() + actions.len();
            let mut summary: Vec<String> = Vec::new();
            if !workflows.is_empty() {
                summary.push(format!("{} workflows", workflows.len()));
            }
            if !actions.is_empty() {
                summary.push(format!("{} actions", actions.len()));
            }
            if !unknowns.is_empty() {
                summary.push(format!("{} unknown", unknowns.len()));
            }
            println!(
                "{}",
                ui.status_header(
                    "impact",
                    Status::Found,
                    ui::plural(total, "impacted target", "impacted targets"),
                    &summary,
                )
            );
            println!();
            if !workflows.is_empty() {
                println!("{}", ui.section("Workflows"));
                for wf in &workflows {
                    println!("{}", ui.item(&wf.0));
                }
            }
            if !actions.is_empty() {
                if !workflows.is_empty() {
                    println!();
                }
                println!("{}", ui.section("Actions"));
                let rows: Vec<Vec<String>> = actions
                    .iter()
                    .map(|(id, kind)| vec![action_kind_label(kind).into(), id.0.clone()])
                    .collect();
                print!("{}", ui.table(&["kind", "target"], &rows));
            }
        }
    }
    Ok(())
}

fn cmd_wiring(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    format: &ReportFormat,
    ui: &Ui,
) -> Result<i32> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let findings = query::wiring(&ir);
    match format {
        ReportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&findings)?);
        }
        ReportFormat::Markdown => {
            println!("### Wiring");
            println!();
            if findings.is_empty() {
                println!("No findings.");
            } else {
                println!(
                    "{} found.",
                    ui::plural(findings.len(), "finding", "findings")
                );
                println!();
                println!("| Kind | Location | Message |");
                println!("|---|---|---|");
                for f in &findings {
                    let location = format!("{}:{}", ui.path(root, &f.file), f.line);
                    println!(
                        "| `{}` | {} | {} |",
                        wiring_kind_label(&f.kind),
                        markdown::code_cell(&location),
                        markdown::table_cell(&wiring_message(f))
                    );
                }
            }
        }
        ReportFormat::Text => {
            if findings.is_empty() {
                println!(
                    "{}",
                    ui.status_header("wiring", Status::Clean, "no findings", &[])
                );
            } else {
                let metadata = wiring_kind_breakdown(&findings);
                println!(
                    "{}",
                    ui.status_header(
                        "wiring",
                        Status::Error,
                        ui::plural(findings.len(), "finding", "findings"),
                        &metadata,
                    )
                );
                println!();
                for f in &findings {
                    let location = format!("{}:{}", ui.path(root, &f.file), f.line);
                    print!(
                        "{}",
                        ui.detail_block(
                            None,
                            wiring_kind_label(&f.kind),
                            &location,
                            &wiring_message(f),
                        )
                    );
                }
            }
        }
    }
    Ok(if findings.is_empty() { 0 } else { 1 })
}

/// Count findings per `WiringKind` variant for the status-header metadata.
/// All four variants are enumerated; zero counts are omitted so the resulting
/// values sum to the total finding count.
fn wiring_kind_breakdown(findings: &[WiringFinding]) -> Vec<String> {
    let mut unannotated_dispatch: usize = 0;
    let mut dangling_annotation: usize = 0;
    let mut dangling_workflow_run: usize = 0;
    let mut dangling_local_uses: usize = 0;
    for f in findings {
        match f.kind {
            WiringKind::UnannotatedDispatch { .. } => unannotated_dispatch += 1,
            WiringKind::DanglingAnnotation { .. } => dangling_annotation += 1,
            WiringKind::DanglingWorkflowRun { .. } => dangling_workflow_run += 1,
            WiringKind::DanglingLocalUses { .. } => dangling_local_uses += 1,
        }
    }
    let mut summary: Vec<String> = Vec::new();
    if unannotated_dispatch > 0 {
        summary.push(format!("{unannotated_dispatch} unannotated-dispatch"));
    }
    if dangling_annotation > 0 {
        summary.push(format!("{dangling_annotation} dangling-annotation"));
    }
    if dangling_workflow_run > 0 {
        summary.push(format!("{dangling_workflow_run} dangling-workflow-run"));
    }
    if dangling_local_uses > 0 {
        summary.push(format!("{dangling_local_uses} dangling-local-uses"));
    }
    summary
}

fn wiring_kind_label(kind: &WiringKind) -> &'static str {
    match kind {
        WiringKind::UnannotatedDispatch { .. } => "unannotated-dispatch",
        WiringKind::DanglingAnnotation { .. } => "dangling-annotation",
        WiringKind::DanglingWorkflowRun { .. } => "dangling-workflow-run",
        WiringKind::DanglingLocalUses { .. } => "dangling-local-uses",
    }
}

fn wiring_message(f: &WiringFinding) -> String {
    let msg = match &f.kind {
        WiringKind::UnannotatedDispatch { raw_target } => {
            format!("missing ravelact:dispatches annotation for `gh workflow run {raw_target}`")
        }
        WiringKind::DanglingAnnotation { raw_target, reason } => {
            format!("dangling ravelact annotation `{raw_target}`: {reason}")
        }
        WiringKind::DanglingWorkflowRun { raw_name } => {
            format!("workflow_run.workflows entry `{raw_name}` does not match any local workflow by name or path")
        }
        WiringKind::DanglingLocalUses {
            local_kind,
            raw_target,
        } => {
            let what = match local_kind {
                DanglingLocalUsesKind::Action => "local action",
                DanglingLocalUsesKind::Workflow => "local workflow",
            };
            format!("`uses: ./{raw_target}` references a {what} that is not present in the IR")
        }
    };
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn candidate_values(candidates: Vec<CompletionCandidate>) -> Vec<String> {
        candidates
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn cli_parses_global_flags_and_orphans_json_format() {
        let cli = Cli::try_parse_from([
            "ravelact",
            "--root",
            "fixtures/repo",
            "--no-cache",
            "--exclude",
            "tests/fixtures/**",
            "--color",
            "never",
            "orphans",
            "--format",
            "json",
        ])
        .expect("valid orphans invocation");

        assert_eq!(cli.root.as_deref(), Some(Path::new("fixtures/repo")));
        assert!(cli.no_cache);
        assert_eq!(cli.exclude, vec!["tests/fixtures/**"]);
        assert!(matches!(cli.color, ColorChoice::Never));
        match cli.command {
            Command::Orphans { format } => assert!(matches!(format, ReportFormat::Json)),
            other => panic!("expected orphans command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_trace_filters_format_and_ascii_flag() {
        let cli = Cli::try_parse_from([
            "ravelact",
            "trace",
            "pull_request",
            "--type",
            "opened",
            "--branch",
            "main",
            "--tag",
            "v*",
            "--path",
            "src/lib.rs",
            "--format",
            "table",
            "--ascii",
        ])
        .expect("valid trace invocation");

        match cli.command {
            Command::Trace {
                event,
                types,
                branches,
                tags,
                paths,
                format,
                ascii,
            } => {
                assert_eq!(event.as_deref(), Some("pull_request"));
                assert_eq!(types, vec!["opened"]);
                assert_eq!(branches, vec!["main"]);
                assert_eq!(tags, vec!["v*"]);
                assert_eq!(paths, vec!["src/lib.rs"]);
                assert!(matches!(format, TraceFormat::Table));
                assert!(ascii);
            }
            other => panic!("expected trace command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_browse_flags() {
        let cli = Cli::try_parse_from(["ravelact", "browse", "--no-open", "--port", "8765"])
            .expect("valid browse invocation");
        match cli.command {
            Command::Browse {
                port,
                no_open,
                include_test_fixtures,
            } => {
                assert_eq!(port, Some(8765));
                assert!(no_open);
                assert!(
                    !include_test_fixtures,
                    "--include-test-fixtures defaults to false",
                );
            }
            other => panic!("expected browse command, got {other:?}"),
        }

        let cli = Cli::try_parse_from(["ravelact", "browse", "--include-test-fixtures"])
            .expect("valid browse invocation with --include-test-fixtures");
        match cli.command {
            Command::Browse {
                port,
                no_open,
                include_test_fixtures,
            } => {
                assert_eq!(port, None);
                assert!(!no_open);
                assert!(include_test_fixtures, "--include-test-fixtures sets true");
            }
            other => panic!("expected browse command, got {other:?}"),
        }
    }

    #[test]
    fn cli_rejects_removed_trace_view_flag() {
        let err = Cli::try_parse_from(["ravelact", "trace", "push", "--view", "table"])
            .expect_err("trace --view must be removed");
        let msg = err.to_string();
        assert!(
            msg.contains("unexpected argument '--view'"),
            "expected unknown --view error, got: {msg}"
        );
    }

    #[test]
    fn color_choice_maps_to_ui_color_mode() {
        let auto: ColorMode = (&ColorChoice::Auto).into();
        let always: ColorMode = (&ColorChoice::Always).into();
        let never: ColorMode = (&ColorChoice::Never).into();

        assert!(matches!(auto, ColorMode::Auto));
        assert!(matches!(always, ColorMode::Always));
        assert!(matches!(never, ColorMode::Never));
    }

    #[test]
    fn list_event_names_filters_by_prefix_and_excludes_dynamic_other() {
        let values = candidate_values(list_event_names(OsStr::new("pull_request")));
        assert_eq!(
            values,
            vec![
                "pull_request",
                "pull_request_target",
                "pull_request_review",
                "pull_request_review_comment",
            ]
        );

        let all = candidate_values(list_event_names(OsStr::new("")));
        assert!(all.contains(&"workflow_dispatch".to_string()));
        assert!(
            !all.contains(&"other".to_string()),
            "EventKind::Other is data-dependent and must not be statically completed: {all:?}"
        );
    }

    #[test]
    fn trace_filter_metadata_reports_none_or_joined_filters() {
        assert_eq!(
            trace_filter_metadata(&[], &[], &[], &[]),
            vec!["filters=none".to_string()]
        );
        assert_eq!(
            trace_filter_metadata(
                &["opened".to_string(), "reopened".to_string()],
                &["main".to_string()],
                &["v*".to_string()],
                &["src/lib.rs".to_string(), "Cargo.toml".to_string()],
            ),
            vec![
                "types=[opened,reopened]".to_string(),
                "branches=[main]".to_string(),
                "tags=[v*]".to_string(),
                "paths=[src/lib.rs,Cargo.toml]".to_string(),
            ]
        );
    }

    #[test]
    fn build_exclude_set_accepts_empty_and_valid_patterns() {
        let empty = build_exclude_set(&[]).expect("empty exclude set");
        assert!(!empty.is_match("tests/fixtures/action.yml"));

        let set =
            build_exclude_set(&["tests/fixtures/**".to_string()]).expect("valid exclude pattern");
        assert!(set.is_match("tests/fixtures/simple/action.yml"));
        assert!(!set.is_match("src/lib.rs"));
    }

    #[test]
    fn build_exclude_set_rejects_invalid_pattern() {
        let err =
            build_exclude_set(&["tests/[".to_string()]).expect_err("invalid glob must be rejected");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid --exclude pattern `tests/[`"),
            "expected error to cite the offending pattern, got: {msg}"
        );
    }

    #[test]
    fn action_kind_label_covers_all_public_action_kinds() {
        assert_eq!(action_kind_label(&ActionKind::Composite), "composite");
        assert_eq!(
            action_kind_label(&ActionKind::JavaScript {
                node_version: "20".to_string(),
            }),
            "javascript"
        );
        assert_eq!(action_kind_label(&ActionKind::Docker), "docker");
    }

    #[test]
    fn completion_errors_name_supported_commands() {
        let completion = cmd_completion("powershell").expect_err("invalid shell must fail");
        assert!(
            completion
                .to_string()
                .contains("Supported shells: bash, zsh, fish"),
            "completion error should list supported shells: {completion:#}"
        );
    }

    #[test]
    fn wiring_kind_label_covers_all_variants() {
        use crate::ir::{DanglingLocalUsesKind, WiringKind};
        assert_eq!(
            wiring_kind_label(&WiringKind::UnannotatedDispatch {
                raw_target: "x".into(),
            }),
            "unannotated-dispatch"
        );
        assert_eq!(
            wiring_kind_label(&WiringKind::DanglingAnnotation {
                raw_target: "x".into(),
                reason: "r".into(),
            }),
            "dangling-annotation"
        );
        assert_eq!(
            wiring_kind_label(&WiringKind::DanglingWorkflowRun {
                raw_name: "x".into(),
            }),
            "dangling-workflow-run"
        );
        assert_eq!(
            wiring_kind_label(&WiringKind::DanglingLocalUses {
                local_kind: DanglingLocalUsesKind::Action,
                raw_target: "x".into(),
            }),
            "dangling-local-uses"
        );
    }

    #[test]
    fn wiring_kind_breakdown_sums_each_variant_and_skips_zero_counts() {
        use crate::ir::{DanglingLocalUsesKind, WiringFinding, WiringKind};
        use std::path::PathBuf;
        let findings = vec![
            WiringFinding {
                file: PathBuf::from("a.yml"),
                line: 1,
                kind: WiringKind::UnannotatedDispatch {
                    raw_target: "x".into(),
                },
            },
            WiringFinding {
                file: PathBuf::from("a.yml"),
                line: 2,
                kind: WiringKind::DanglingAnnotation {
                    raw_target: "x".into(),
                    reason: "r".into(),
                },
            },
            WiringFinding {
                file: PathBuf::from("a.yml"),
                line: 3,
                kind: WiringKind::DanglingWorkflowRun {
                    raw_name: "x".into(),
                },
            },
            WiringFinding {
                file: PathBuf::from("a.yml"),
                line: 4,
                kind: WiringKind::DanglingLocalUses {
                    local_kind: DanglingLocalUsesKind::Workflow,
                    raw_target: "x".into(),
                },
            },
        ];
        let summary = wiring_kind_breakdown(&findings);
        assert_eq!(summary.len(), 4);
        assert!(summary.iter().any(|s| s == "1 unannotated-dispatch"));
        assert!(summary.iter().any(|s| s == "1 dangling-annotation"));
        assert!(summary.iter().any(|s| s == "1 dangling-workflow-run"));
        assert!(summary.iter().any(|s| s == "1 dangling-local-uses"));
        // Empty findings -> empty summary (no zero-count rows leak through).
        assert!(wiring_kind_breakdown(&[]).is_empty());
    }

    #[test]
    fn wiring_message_formats_each_variant_distinctly() {
        use crate::ir::{DanglingLocalUsesKind, WiringFinding, WiringKind};
        use std::path::PathBuf;
        let pos = |kind| WiringFinding {
            file: PathBuf::from("a.yml"),
            line: 1,
            kind,
        };
        let m = wiring_message(&pos(WiringKind::DanglingAnnotation {
            raw_target: "../bad".into(),
            reason: "x".into(),
        }));
        assert!(m.contains("../bad") && m.contains("dangling"));
        let m = wiring_message(&pos(WiringKind::DanglingWorkflowRun {
            raw_name: "ghost".into(),
        }));
        assert!(m.contains("ghost") && m.contains("workflow_run"));
        let m_action = wiring_message(&pos(WiringKind::DanglingLocalUses {
            local_kind: DanglingLocalUsesKind::Action,
            raw_target: ".github/actions/missing".into(),
        }));
        assert!(m_action.contains("local action"));
        let m_wf = wiring_message(&pos(WiringKind::DanglingLocalUses {
            local_kind: DanglingLocalUsesKind::Workflow,
            raw_target: ".github/workflows/missing.yml".into(),
        }));
        assert!(m_wf.contains("local workflow"));
    }
}
