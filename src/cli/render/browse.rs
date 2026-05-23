//! `browse` subcommand: local GUI for the workflow graph.
//!
//! Loads the IR through the same `build_or_load` path as every other
//! subcommand, converts it into a Cytoscape.js `{nodes, edges}` document,
//! and serves a React SPA bundled by Vite (`web/dist/`) from an embedded
//! asset bundle. tokio's `current_thread` runtime is built locally inside
//! `run` so the rest of the binary stays synchronous — no `#[tokio::main]`
//! shim on the main entry point.
//!
//! Static routing is intentionally minimal: `/` serves `index.html` and
//! `/assets/{*path}` serves the hashed `web/dist/assets/*` outputs. Vite's
//! deterministic hash pattern (see `web/vite.config.ts`) keeps `rust-embed`
//! reproducible. `/api/*` endpoints are query-string only — workflow ids
//! contain `/`, which would break path-param routing.

use crate::cache::CacheMode;
use crate::ir::{
    ActionId, AnnotationResolution, ExternalActionRef, Ir, UsesRef, WorkflowId, WorkflowRef,
};
use crate::query::{
    impact as impact_query, trace as trace_query, triggers as triggers_query, walk,
};
use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use globset::GlobSet;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use url::Url;

#[derive(Embed)]
#[folder = "web/dist/"]
struct WebAssets;

pub(in crate::cli) fn run(
    root: &Path,
    cache_mode: CacheMode,
    excludes: &GlobSet,
    port: Option<u16>,
    no_open: bool,
) -> Result<()> {
    let ir = Arc::new(super::super::build_or_load(root, cache_mode, excludes)?);
    let graph = build_graph_json(&ir);
    let repo_info = compute_repo_info(root);

    // current_thread runtime: this is a single-user local server with at
    // most a handful of in-flight requests, so multi-threading would add
    // weight without benefit.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(ir, graph, repo_info, port, no_open))
}

fn build_graph_json(ir: &Ir) -> Value {
    let mut b = GraphBuilder::new();

    // Initial nodes from IR collections. Anything reached via edge
    // traversal that is not in these collections is synthesised on the
    // fly (see GraphBuilder::ensure_*).
    for wf in &ir.workflows {
        let id = workflow_node_id(&wf.id);
        let label = wf.name.clone().unwrap_or_else(|| wf.id.0.clone());
        b.mark_workflow(&id);
        b.add_node(id, label, "workflow");
    }
    for la in &ir.actions {
        let id = action_node_id(&la.id);
        let label = la.name.clone().unwrap_or_else(|| la.id.0.clone());
        b.mark_local_action(&id);
        b.add_node(id, label, "local-action");
    }
    for ea in &ir.external_actions {
        let id = external_action_node_id(ea);
        b.mark_external_action(&id);
        b.add_node(id, external_action_label(ea), "external-action");
    }

    for wf in &ir.workflows {
        let source = workflow_node_id(&wf.id);
        b.visit(source, walk::Node::Workflow(wf));
    }
    for la in &ir.actions {
        let source = action_node_id(&la.id);
        b.visit(source, walk::Node::Action(la));
    }

    json!({ "nodes": b.nodes, "edges": b.edges })
}

struct GraphBuilder {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    seen_workflows: HashSet<String>,
    seen_local_actions: HashSet<String>,
    seen_external_actions: HashSet<String>,
    seen_external_workflows: HashSet<String>,
    seen_dockers: HashSet<String>,
    edge_counter: usize,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            seen_workflows: HashSet::new(),
            seen_local_actions: HashSet::new(),
            seen_external_actions: HashSet::new(),
            seen_external_workflows: HashSet::new(),
            seen_dockers: HashSet::new(),
            edge_counter: 0,
        }
    }

    fn add_node(&mut self, id: String, label: String, kind: &str) {
        self.nodes
            .push(json!({ "data": { "id": id, "label": label, "kind": kind } }));
    }

    fn mark_workflow(&mut self, id: &str) {
        self.seen_workflows.insert(id.to_string());
    }

    fn mark_local_action(&mut self, id: &str) {
        self.seen_local_actions.insert(id.to_string());
    }

    fn mark_external_action(&mut self, id: &str) {
        self.seen_external_actions.insert(id.to_string());
    }

    /// Cytoscape rejects edges whose endpoints do not match an existing node id,
    /// so we silently drop edges to targets that never made it into the node
    /// set. The same situation can arise for resolved annotations whose target
    /// workflow is excluded from the build, or `uses: ./...` references that
    /// `wiring` separately surfaces as `DanglingLocalUses`. The browse view is
    /// not the place to re-report those — it just stays well-formed.
    fn add_edge(&mut self, source: &str, target: String, kind: &str) {
        if !self.is_known_target(&target) {
            return;
        }
        let id = format!("e{}", self.edge_counter);
        self.edge_counter += 1;
        self.edges.push(
            json!({ "data": { "id": id, "source": source, "target": target, "kind": kind } }),
        );
    }

    fn is_known_target(&self, id: &str) -> bool {
        self.seen_workflows.contains(id)
            || self.seen_local_actions.contains(id)
            || self.seen_external_actions.contains(id)
            || self.seen_external_workflows.contains(id)
            || self.seen_dockers.contains(id)
    }

    fn ensure_external_workflow(
        &mut self,
        owner: &str,
        repo: &str,
        path: &str,
        gitref: &str,
    ) -> String {
        let id = external_workflow_node_id(owner, repo, path, gitref);
        if self.seen_external_workflows.insert(id.clone()) {
            let label = format!("{owner}/{repo}/{path}@{gitref}");
            self.add_node(id.clone(), label, "external-workflow");
        }
        id
    }

    fn ensure_external_action(
        &mut self,
        owner: &str,
        repo: &str,
        subpath: Option<&str>,
        gitref: &str,
    ) -> String {
        let sub = subpath.map(|s| format!("/{s}")).unwrap_or_default();
        let id = format!("ea:{owner}/{repo}{sub}@{gitref}");
        if self.seen_external_actions.insert(id.clone()) {
            let label = format!("{owner}/{repo}{sub}@{gitref}");
            self.add_node(id.clone(), label, "external-action");
        }
        id
    }

    fn ensure_docker(&mut self, d: &crate::ir::DockerRef) -> String {
        let display = d.display_str();
        let id = docker_node_id(&display);
        if self.seen_dockers.insert(id.clone()) {
            self.add_node(id.clone(), display, "docker");
        }
        id
    }

    fn visit(&mut self, source: String, node: walk::Node<'_>) {
        walk::for_each_outgoing_edge(node, |ctx| match ctx.edge {
            walk::Edge::Annotation(ann) => {
                if let AnnotationResolution::Resolved { target } = &ann.resolution {
                    let tgt = workflow_node_id(target);
                    self.add_edge(&source, tgt, "annotation");
                }
            }
            walk::Edge::CallsWorkflow(call) => {
                let tgt = match &call.workflow_ref {
                    WorkflowRef::Local(wid) => workflow_node_id(wid),
                    WorkflowRef::External {
                        owner,
                        repo,
                        path,
                        gitref,
                    } => self.ensure_external_workflow(owner, repo, path, gitref),
                };
                self.add_edge(&source, tgt, "calls-workflow");
            }
            walk::Edge::Uses(uses) => {
                let (tgt, kind) = match uses {
                    UsesRef::LocalWorkflow(wid) => (workflow_node_id(wid), "uses-local-workflow"),
                    UsesRef::LocalAction(aid) => (action_node_id(aid), "uses-local-action"),
                    UsesRef::External {
                        owner,
                        repo,
                        subpath,
                        gitref,
                    } => (
                        self.ensure_external_action(owner, repo, subpath.as_deref(), gitref),
                        "uses-external-action",
                    ),
                    UsesRef::Docker(d) => (self.ensure_docker(d), "uses-docker"),
                };
                self.add_edge(&source, tgt, kind);
            }
        });
    }
}

fn workflow_node_id(id: &WorkflowId) -> String {
    format!("wf:{}", id.0)
}

fn action_node_id(id: &ActionId) -> String {
    format!("la:{}", id.0)
}

fn external_action_node_id(r: &ExternalActionRef) -> String {
    let sub = r
        .subpath
        .as_deref()
        .map(|s| format!("/{s}"))
        .unwrap_or_default();
    format!("ea:{}/{}{}@{}", r.owner, r.repo, sub, r.gitref)
}

fn external_action_label(r: &ExternalActionRef) -> String {
    let sub = r
        .subpath
        .as_deref()
        .map(|s| format!("/{s}"))
        .unwrap_or_default();
    format!("{}/{}{}@{}", r.owner, r.repo, sub, r.gitref)
}

// Centralize the `ew:` / `dk:` id format so the graph builder and
// downstream handlers (e.g. `/api/event-impact`) cannot drift.
fn external_workflow_node_id(owner: &str, repo: &str, path: &str, gitref: &str) -> String {
    format!("ew:{owner}/{repo}/{path}@{gitref}")
}

fn docker_node_id(display: &str) -> String {
    format!("dk:{display}")
}

// ---------------------------------------------------------------------------
// /api/repo: GitHub provenance of the local repository
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct RepoInfo {
    host: String,
    owner: String,
    repo: String,
    #[serde(rename = "ref")]
    git_ref: String,
}

fn compute_repo_info(root: &Path) -> Option<RepoInfo> {
    // Do not log the raw `url` string anywhere — `git remote get-url origin`
    // can return credentials embedded in the URL (e.g. CI clones with
    // `https://x-access-token:TOKEN@github.com/...`). `parse_remote_url`
    // drops userinfo from URL forms via `url::Url::host_str` and from SCP
    // forms via `rsplit_once('@')`, so the returned `RepoInfo` is
    // credential-free; the raw input must stay scoped to this function.
    let url = run_git(root, &["remote", "get-url", "origin"])?;
    let (host, owner, repo) = parse_remote_url(&url)?;
    let git_ref = run_git(root, &["symbolic-ref", "--short", "HEAD"])
        .or_else(|| run_git(root, &["rev-parse", "HEAD"]))?;
    Some(RepoInfo {
        host,
        owner,
        repo,
        git_ref,
    })
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parse a `git remote get-url` value into `(host, owner, repo)`.
///
/// Accepted forms (host is host-agnostic; github.com and GitHub Enterprise
/// alike pass through):
///   - SCP form: `[user@]host:owner/repo[.git]` (e.g. `git@github.com:o/r`)
///   - URL form: `ssh://[user[:pw]@]host[:port]/owner/repo[.git]`,
///     `https://[user[:pw]@]host[:port]/owner/repo[.git]`, or `http://...`
///
/// SCP form is detected **before** `Url::parse` because the colon in
/// `git@host:path` is not a port separator. `Url::parse` currently rejects
/// SCP with `RelativeUrlWithoutBase`, but routing through the URL branch
/// would be fragile if upstream behavior shifted, so SCP wins by ordering.
///
/// `git://` and `file://` are rejected: the former is unauthenticated and
/// nearly absent in practice, the latter can't link to a public GitHub-like
/// host. Schemes outside {ssh, https, http} are rejected for the same reason.
///
/// Credential safety: userinfo (`user@` or `user:pw@`) is stripped — via
/// `Url::host_str` for URL forms and via `rsplit_once('@')` for SCP form —
/// so neither the returned host nor the caller's `RepoInfo` ever sees the
/// userinfo. The raw input string must still be treated as secret by the
/// caller (see `compute_repo_info`).
///
/// Host is normalized to ASCII lowercase. `url`'s special schemes
/// (http/https) already lowercase the host, but `ssh` is non-special and
/// preserves case (`ssh://git@GitHub.com/...` → `host_str() == "GitHub.com"`),
/// so the lowercasing is explicit.
fn parse_remote_url(url: &str) -> Option<(String, String, String)> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // SCP form: `[user@]host:path`, with `host` and `path` separated by the
    // first `:`. Distinguishable from URL form by the absence of `://`. A
    // bare `host:port` without `@` is also possible but extremely rare for
    // git remotes; we still accept it as long as the part after `:` parses
    // as `owner/repo`.
    if !url.contains("://") {
        if let Some(colon) = url.find(':') {
            let host_part = &url[..colon];
            let path = &url[colon + 1..];
            let host = host_part.rsplit_once('@').map_or(host_part, |(_, h)| h);
            if host.is_empty() {
                return None;
            }
            let (owner, repo) = split_owner_repo(path)?;
            return Some((host.to_ascii_lowercase(), owner, repo));
        }
        return None;
    }

    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "ssh" | "https" | "http") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().trim_start_matches('/');
    let (owner, repo) = split_owner_repo(path)?;
    Some((host, owner, repo))
}

fn split_owner_repo(rest: &str) -> Option<(String, String)> {
    // Strip a single trailing slash, then optional `.git` suffix.
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (owner, repo_with_suffix) = rest.split_once('/')?;
    let repo = repo_with_suffix
        .strip_suffix(".git")
        .unwrap_or(repo_with_suffix);
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Build the axum router for `ravelact browse` from pre-serialized bodies.
///
/// `api_body` and `repo_body` are pre-serialized so each request is a cheap
/// `Bytes` clone (Arc-backed) instead of repeated JSON serialization. The
/// router shape — route ordering, `with_state(ir)` placement — is kept in
/// lockstep with the serve loop so the integration tests in
/// `tests/e2e_browse.rs` continue to observe identical behavior.
pub(crate) fn build_router(ir: Arc<Ir>, api_body: Bytes, repo_body: Option<Bytes>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        // axum's catch-all `{*path}` is evaluated AFTER more-specific routes,
        // so `/api/*` handlers below still take precedence.
        .route("/assets/{*path}", get(serve_asset))
        .route(
            "/api/graph",
            get({
                let body = api_body.clone();
                move || {
                    let body = body.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], body) }
                }
            }),
        )
        .route(
            "/api/repo",
            get({
                let body = repo_body.clone();
                move || {
                    let body = body.clone();
                    async move {
                        match body {
                            Some(b) => {
                                ([(header::CONTENT_TYPE, "application/json")], b).into_response()
                            }
                            None => StatusCode::NOT_FOUND.into_response(),
                        }
                    }
                }
            }),
        )
        .route("/api/triggers", get(api_triggers))
        .route("/api/search", get(api_search))
        .route("/api/event-impact", get(api_event_impact))
        .route("/api/node", get(api_node))
        .route("/api/impact", get(api_impact))
        .route("/api/trace", get(api_trace))
        .with_state(ir)
}

async fn serve(
    ir: Arc<Ir>,
    graph: Value,
    repo_info: Option<RepoInfo>,
    port: Option<u16>,
    no_open: bool,
) -> Result<()> {
    // Serialize once and hand each request a cheap Bytes clone (Arc-backed).
    let api_body = Bytes::from(serde_json::to_vec(&graph)?);
    // Pre-serialize repo info too so each request is a Bytes clone, not a
    // re-serialization. `None` means no `origin` remote / unsupported scheme
    // (`git://`, `file://`, …) / malformed URL / detached HEAD with no SHA —
    // the route returns 404 in that case so the frontend can hide the
    // "Open in GitHub" link gracefully.
    let repo_body: Option<Bytes> = repo_info
        .as_ref()
        .map(|r| Bytes::from(serde_json::to_vec(r).expect("RepoInfo serializes")));

    let app = build_router(ir, api_body, repo_body);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/");
    println!("ravelact browse listening on {url}");
    println!("press Ctrl+C to stop");

    if !no_open {
        // webbrowser::open can block on text-mode browsers (lynx/w3m), so
        // keep the runtime free by handing it to a blocking worker.
        let url_for_open = url.clone();
        tokio::task::spawn_blocking(move || {
            let _ = webbrowser::open(&url_for_open);
        });
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn serve_index() -> impl IntoResponse {
    // `web/dist/index.html` is produced by `pnpm build` (see `just frontend`).
    // rust-embed errors at compile time if `web/dist/` does not exist
    // (`#[derive(RustEmbed)] folder '...' does not exist`). If the folder
    // exists but is empty / missing `index.html`, this handler panics at
    // runtime instead.
    let file = WebAssets::get("index.html")
        .expect("web/dist/index.html embedded at build time (run `just frontend` first)");
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        file.data.into_owned(),
    )
}

async fn serve_asset(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    // Vite emits hashed filenames under `web/dist/assets/`. The catch-all
    // matches `/assets/{*path}` and we look up `assets/{path}` in the
    // embedded bundle. If absent, return 404 with no body — index.html
    // routes (SPA) are handled by `serve_index` at `/` directly.
    let key = format!("assets/{path}");
    let Some(file) = WebAssets::get(&key) else {
        return (axum::http::StatusCode::NOT_FOUND, "").into_response();
    };
    let mime = mime_for_extension(&path);
    ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
}

fn mime_for_extension(path: &str) -> &'static str {
    // Minimal table — Vite only emits .js / .css / .map / .svg / a few
    // image formats. Anything else falls back to octet-stream.
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "map" | "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Serialize)]
struct TriggersResponse {
    rows: Vec<triggers_query::TriggerSummary>,
}

async fn api_triggers(State(ir): State<Arc<Ir>>) -> Json<TriggersResponse> {
    Json(TriggersResponse {
        rows: triggers_query::triggers(&ir),
    })
}

// ---------------------------------------------------------------------------
// /api/search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchParams {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct SearchMatch {
    id: String,
    kind: String,
    label: String,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    matches: Vec<SearchMatch>,
    truncated: bool,
    total: usize,
}

/// `/api/search` — multi-token AND case-insensitive substring search over
/// the same node set served by `/api/graph`. The corpus per node is
/// `id + label + file-path + entry-trigger event names` joined with `\n`
/// and lowercased once. Scoring biases short / early-position hits and
/// awards a +1000 bonus for an exact lowercased label match. Empty `q`
/// short-circuits to an empty result; results are truncated to `limit`
/// (default 200) with `truncated` flag.
async fn api_search(
    State(ir): State<Arc<Ir>>,
    Query(params): Query<SearchParams>,
) -> Json<SearchResponse> {
    let q = params.q.unwrap_or_default();
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return Json(SearchResponse {
            matches: Vec::new(),
            truncated: false,
            total: 0,
        });
    }
    let limit = params.limit.unwrap_or(200).max(1);
    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    if tokens.is_empty() {
        return Json(SearchResponse {
            matches: Vec::new(),
            truncated: false,
            total: 0,
        });
    }

    let kind_filter = params.kind.as_deref();
    let corpus = build_search_corpus(&ir);

    let mut hits: Vec<(i64, SearchMatch)> = Vec::new();
    for entry in &corpus {
        if let Some(k) = kind_filter {
            if entry.kind != k {
                continue;
            }
        }
        let mut score: i64 = 0;
        let mut all_present = true;
        for token in &tokens {
            match entry.searchable_lower.find(token) {
                Some(idx) => score += 256i64.saturating_sub(idx as i64).max(1),
                None => {
                    all_present = false;
                    break;
                }
            }
        }
        if !all_present {
            continue;
        }
        if entry.label_lower == *trimmed.to_lowercase() {
            score += 1000;
        }
        hits.push((
            score,
            SearchMatch {
                id: entry.id.clone(),
                kind: entry.kind.to_string(),
                label: entry.label.clone(),
            },
        ));
    }
    let total = hits.len();
    // Sort by score desc, then id asc for stability.
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let truncated = hits.len() > limit;
    hits.truncate(limit);

    Json(SearchResponse {
        matches: hits.into_iter().map(|(_, m)| m).collect(),
        truncated,
        total,
    })
}

struct CorpusEntry {
    id: String,
    kind: &'static str,
    label: String,
    label_lower: String,
    searchable_lower: String,
}

fn build_search_corpus(ir: &Ir) -> Vec<CorpusEntry> {
    let mut out: Vec<CorpusEntry> = Vec::new();

    for wf in &ir.workflows {
        let id = workflow_node_id(&wf.id);
        let label = wf.name.clone().unwrap_or_else(|| wf.id.0.clone());
        let file = wf.source.file.display().to_string();
        let mut searchable = format!("{id}\n{label}\n{file}");
        for t in &wf.triggers {
            searchable.push('\n');
            searchable.push_str(t.event.name());
        }
        let label_lower = label.to_lowercase();
        let searchable_lower = searchable.to_lowercase();
        out.push(CorpusEntry {
            id,
            kind: "workflow",
            label,
            label_lower,
            searchable_lower,
        });
    }

    for la in &ir.actions {
        let id = action_node_id(&la.id);
        let label = la.name.clone().unwrap_or_else(|| la.id.0.clone());
        let file = la.source.file.display().to_string();
        let searchable = format!("{id}\n{label}\n{file}");
        let label_lower = label.to_lowercase();
        let searchable_lower = searchable.to_lowercase();
        out.push(CorpusEntry {
            id,
            kind: "local-action",
            label,
            label_lower,
            searchable_lower,
        });
    }

    for ea in &ir.external_actions {
        let id = external_action_node_id(ea);
        let label = external_action_label(ea);
        let searchable = format!("{id}\n{label}");
        let label_lower = label.to_lowercase();
        let searchable_lower = searchable.to_lowercase();
        out.push(CorpusEntry {
            id,
            kind: "external-action",
            label,
            label_lower,
            searchable_lower,
        });
    }

    out
}

// ---------------------------------------------------------------------------
// /api/event-impact
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EventImpactParams {
    event: Option<String>,
}

#[derive(Debug, Serialize)]
struct EventImpactResponse {
    event: String,
    entry_workflows: Vec<String>,
    node_ids: Vec<String>,
}

/// `/api/event-impact` — return every node reachable from a workflow
/// that lists `event` as an entry trigger. Equivalent of running the
/// CLI `ravelact trace --event <event>` and flattening every entry's
/// tree into a flat id set. Used by the SPA's overview pane to
/// highlight what an event drives.
async fn api_event_impact(
    State(ir): State<Arc<Ir>>,
    Query(params): Query<EventImpactParams>,
) -> Json<EventImpactResponse> {
    let event = params.event.unwrap_or_default();
    if event.trim().is_empty() {
        return Json(EventImpactResponse {
            event,
            entry_workflows: Vec::new(),
            node_ids: Vec::new(),
        });
    }
    let entries = trace_query::trace(&ir, &event, &[], &[], &[], &[]);
    let mut entry_workflows: Vec<String> = Vec::new();
    let mut node_ids: HashSet<String> = HashSet::new();
    for entry in &entries {
        collect_trace_node_ids(&entry.root, &mut node_ids);
        if let trace_query::TraceNode::Workflow { id, .. } = &entry.root {
            entry_workflows.push(workflow_node_id(id));
        }
    }
    entry_workflows.sort();
    let mut node_ids: Vec<String> = node_ids.into_iter().collect();
    node_ids.sort();
    Json(EventImpactResponse {
        event,
        entry_workflows,
        node_ids,
    })
}

fn collect_trace_node_ids(node: &trace_query::TraceNode, out: &mut HashSet<String>) {
    use trace_query::TraceNode;
    match node {
        TraceNode::Workflow { id, children } => {
            out.insert(workflow_node_id(id));
            for c in children {
                collect_trace_node_ids(c, out);
            }
        }
        TraceNode::Action { id, children } => {
            out.insert(action_node_id(id));
            for c in children {
                collect_trace_node_ids(c, out);
            }
        }
        TraceNode::External(ea) => {
            out.insert(external_action_node_id(ea));
        }
        TraceNode::ExternalWorkflow {
            owner,
            repo,
            path,
            gitref,
        } => {
            out.insert(external_workflow_node_id(owner, repo, path, gitref));
        }
        TraceNode::Docker(d) => {
            out.insert(docker_node_id(&d.display_str()));
        }
        TraceNode::Annotated { children, .. } => {
            for c in children {
                collect_trace_node_ids(c, out);
            }
        }
        TraceNode::Guarded { inner, .. } => collect_trace_node_ids(inner, out),
        // Cycle targets are already inserted at the original visit; skipping
        // here avoids re-counting and keeps the set's semantics — "every
        // unique node touched by the trace" — clean.
        TraceNode::Cycle(_) => {}
    }
}

#[derive(Deserialize)]
struct NodeParams {
    kind: String,
    id: String,
}

#[derive(Debug, Serialize)]
struct NodeResponse {
    id: String,
    kind: String,
    label: String,
    file: String,
    summary: String,
    entry_triggers: Vec<String>,
    refs_in: Vec<String>,
    refs_out: Vec<String>,
}

async fn api_node(
    State(ir): State<Arc<Ir>>,
    Query(params): Query<NodeParams>,
) -> Result<Json<NodeResponse>, StatusCode> {
    match params.kind.as_str() {
        "workflow" => {
            let wf = ir
                .workflows
                .iter()
                .find(|w| w.id.0 == params.id)
                .ok_or(StatusCode::NOT_FOUND)?;
            let entry_triggers: Vec<String> = wf
                .triggers
                .iter()
                .filter(|t| t.is_entry_point())
                .map(|t| t.event.name().to_string())
                .collect();
            Ok(Json(NodeResponse {
                id: workflow_node_id(&wf.id),
                kind: "workflow".into(),
                label: wf.name.clone().unwrap_or_else(|| wf.id.0.clone()),
                file: wf.source.file.display().to_string(),
                summary: format!("{} job(s), {} trigger(s)", wf.jobs.len(), wf.triggers.len()),
                entry_triggers,
                refs_in: Vec::new(),
                refs_out: Vec::new(),
            }))
        }
        "local-action" => {
            let la = ir
                .actions
                .iter()
                .find(|a| a.id.0 == params.id)
                .ok_or(StatusCode::NOT_FOUND)?;
            let kind_label = match la.kind {
                crate::ir::ActionKind::Composite => "composite",
                crate::ir::ActionKind::JavaScript { .. } => "javascript",
                crate::ir::ActionKind::Docker => "docker",
            };
            Ok(Json(NodeResponse {
                id: action_node_id(&la.id),
                kind: "local-action".into(),
                label: la.name.clone().unwrap_or_else(|| la.id.0.clone()),
                file: la.source.file.display().to_string(),
                summary: format!("{kind_label}; {} step(s)", la.steps.len()),
                entry_triggers: Vec::new(),
                refs_in: Vec::new(),
                refs_out: Vec::new(),
            }))
        }
        "external-action" => {
            let ea = ir
                .external_actions
                .iter()
                .find(|e| {
                    external_action_node_id(e).strip_prefix("ea:") == Some(params.id.as_str())
                })
                .ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(NodeResponse {
                id: external_action_node_id(ea),
                kind: "external-action".into(),
                label: external_action_label(ea),
                file: String::new(),
                summary: format!("{}/{}@{}", ea.owner, ea.repo, ea.gitref),
                entry_triggers: Vec::new(),
                refs_in: Vec::new(),
                refs_out: Vec::new(),
            }))
        }
        _ => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
struct IdParams {
    id: String,
}

#[derive(Debug, Serialize)]
struct ImpactAction {
    id: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct ImpactResponse {
    workflows: Vec<String>,
    actions: Vec<ImpactAction>,
    unknowns: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TraceResponse {
    tree: trace_query::TraceJsonNode,
    event_used: String,
}

async fn api_trace(
    State(ir): State<Arc<Ir>>,
    Query(params): Query<IdParams>,
) -> Result<Json<TraceResponse>, StatusCode> {
    let wf = ir
        .workflows
        .iter()
        .find(|w| w.id.0 == params.id)
        .ok_or(StatusCode::NOT_FOUND)?;
    // First entry trigger (skips reusable-only `workflow_call`).
    let first_event = wf
        .triggers
        .iter()
        .find(|t| t.is_entry_point())
        .map(|t| t.event.name().to_string())
        .ok_or(StatusCode::NOT_FOUND)?;
    let entries = trace_query::trace(&ir, &first_event, &[], &[], &[], &[]);
    // Post-filter: trace() returns every workflow matching `first_event`; keep
    // the entry whose root workflow id equals the requested id.
    let entry = entries
        .into_iter()
        .find(
            |e| matches!(&e.root, trace_query::TraceNode::Workflow { id, .. } if id.0 == params.id),
        )
        .ok_or(StatusCode::NOT_FOUND)?;
    let json_entries = trace_query::trace_json_entries(std::slice::from_ref(&entry));
    let tree = json_entries
        .into_iter()
        .next()
        .map(|e| e.root)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(TraceResponse {
        tree,
        event_used: first_event,
    }))
}

async fn api_impact(
    State(ir): State<Arc<Ir>>,
    Query(params): Query<IdParams>,
) -> Json<ImpactResponse> {
    let (result, unknowns) = impact_query::impact(&ir, std::slice::from_ref(&params.id));
    let workflows = result.workflows.into_iter().map(|w| w.0).collect();
    let actions = result
        .actions
        .into_iter()
        .map(|(aid, kind)| ImpactAction {
            id: aid.0,
            kind: match kind {
                crate::ir::ActionKind::Composite => "composite".into(),
                crate::ir::ActionKind::JavaScript { .. } => "javascript".into(),
                crate::ir::ActionKind::Docker => "docker".into(),
            },
        })
        .collect();
    Json(ImpactResponse {
        workflows,
        actions,
        unknowns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::build::build_ir;
    use globset::GlobSet;
    use std::path::Path;
    use std::sync::Arc;

    fn load_simple_ir() -> Arc<Ir> {
        let ir = build_ir(Path::new("tests/fixtures/simple"), &GlobSet::empty())
            .expect("simple fixture should load");
        Arc::new(ir)
    }

    const ALLOWED_NODE_KINDS: &[&str] = &[
        "workflow",
        "local-action",
        "external-action",
        "external-workflow",
        "docker",
    ];

    const ALLOWED_EDGE_KINDS: &[&str] = &[
        "annotation",
        "calls-workflow",
        "uses-local-workflow",
        "uses-local-action",
        "uses-external-action",
        "uses-docker",
    ];

    #[tokio::test]
    async fn api_node_returns_workflow_with_entry_triggers() {
        let ir = load_simple_ir();
        // Find any workflow id from the fixture.
        let wf_id = ir
            .workflows
            .first()
            .expect("simple fixture has at least one workflow")
            .id
            .0
            .clone();
        let params = NodeParams {
            kind: "workflow".into(),
            id: wf_id.clone(),
        };
        let resp = api_node(State(ir), Query(params))
            .await
            .expect("workflow lookup should succeed");
        assert_eq!(resp.0.kind, "workflow");
        assert_eq!(resp.0.id, format!("wf:{wf_id}"));
        assert!(!resp.0.label.is_empty(), "label must be non-empty");
    }

    #[tokio::test]
    async fn api_node_returns_404_for_unknown_id() {
        let ir = load_simple_ir();
        let params = NodeParams {
            kind: "workflow".into(),
            id: "nonexistent/workflow.yml".into(),
        };
        let result = api_node(State(ir), Query(params)).await;
        assert!(matches!(result, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn api_node_returns_404_for_unknown_kind() {
        let ir = load_simple_ir();
        let params = NodeParams {
            kind: "external-workflow".into(),
            id: "anything".into(),
        };
        let result = api_node(State(ir), Query(params)).await;
        assert!(matches!(result, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn api_trace_returns_tree_for_first_event() {
        let ir = load_simple_ir();
        // Find a workflow that has at least one entry trigger.
        let wf = ir
            .workflows
            .iter()
            .find(|w| w.triggers.iter().any(|t| t.is_entry_point()))
            .expect("simple fixture should have at least one entry workflow");
        let seed_id = wf.id.0.clone();
        let params = IdParams {
            id: seed_id.clone(),
        };
        let resp = api_trace(State(ir), Query(params))
            .await
            .expect("trace lookup should succeed");
        let TraceResponse { tree, event_used } = resp.0;
        assert!(!event_used.is_empty(), "event_used must be populated");
        match tree {
            trace_query::TraceJsonNode::Workflow { id, .. } => {
                assert_eq!(id, seed_id, "trace tree root must match requested workflow");
            }
            other => panic!("trace root must be Workflow variant, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn api_trace_returns_404_for_workflow_without_entry() {
        let ir = load_simple_ir();
        // Find a workflow that has NO entry trigger (e.g., workflow_call-only).
        // If none exists in the simple fixture, fall back to "unknown id".
        let candidate = ir
            .workflows
            .iter()
            .find(|w| !w.triggers.iter().any(|t| t.is_entry_point()));
        let target_id = candidate
            .map(|w| w.id.0.clone())
            .unwrap_or_else(|| "completely/unknown.yml".to_string());
        let params = IdParams { id: target_id };
        let result = api_trace(State(ir), Query(params)).await;
        assert!(matches!(result, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn api_impact_returns_shape_for_known_workflow() {
        let ir = load_simple_ir();
        let seed = ir
            .workflows
            .first()
            .expect("simple fixture has at least one workflow")
            .id
            .0
            .clone();
        let params = IdParams { id: seed };
        let Json(resp) = api_impact(State(ir), Query(params)).await;
        // ImpactResponse must always include all three fields, even when empty.
        // workflows + actions populated depends on fixture; just verify shape.
        let _ = resp.workflows;
        let _ = resp.actions;
        assert!(
            resp.unknowns.is_empty(),
            "known seed should not appear in unknowns: {:?}",
            resp.unknowns
        );
    }

    #[tokio::test]
    async fn api_impact_reports_unknown_path() {
        let ir = load_simple_ir();
        let params = IdParams {
            id: "completely/nonexistent.yml".into(),
        };
        let Json(resp) = api_impact(State(ir), Query(params)).await;
        assert_eq!(
            resp.unknowns,
            vec!["completely/nonexistent.yml".to_string()],
            "unknown seed should be echoed in unknowns",
        );
    }

    #[tokio::test]
    async fn api_triggers_returns_global_summary() {
        let ir = load_simple_ir();
        let Json(resp) = api_triggers(State(ir.clone())).await;
        assert!(
            !resp.rows.is_empty(),
            "simple fixture should declare at least one trigger event",
        );
        for row in &resp.rows {
            assert!(!row.event.is_empty(), "event name should be non-empty");
            // Sanity: declarations is the total count, entry_workflows is a
            // unique subset, so entry_workflows <= declarations.
            assert!(
                row.entry_workflows <= row.declarations,
                "entry_workflows must not exceed declarations: {row:?}",
            );
        }
    }

    #[test]
    fn parse_remote_url_accepts_matrix() {
        let gh = |o: &str, r: &str| Some(("github.com".into(), o.into(), r.into()));
        let ghe = |o: &str, r: &str| Some(("ghe.example.com".into(), o.into(), r.into()));

        // SCP form (with / without `.git`)
        assert_eq!(
            parse_remote_url("git@github.com:wadackel/ravelact.git"),
            gh("wadackel", "ravelact"),
        );
        assert_eq!(
            parse_remote_url("git@github.com:wadackel/ravelact"),
            gh("wadackel", "ravelact"),
        );
        // SSH URL form, with and without port
        assert_eq!(
            parse_remote_url("ssh://git@github.com/wadackel/ravelact.git"),
            gh("wadackel", "ravelact"),
        );
        assert_eq!(
            parse_remote_url("ssh://git@github.com:22/wadackel/ravelact.git"),
            gh("wadackel", "ravelact"),
        );
        // HTTPS, with / without `.git` / trailing slash
        assert_eq!(
            parse_remote_url("https://github.com/wadackel/ravelact.git"),
            gh("wadackel", "ravelact"),
        );
        assert_eq!(
            parse_remote_url("https://github.com/wadackel/ravelact"),
            gh("wadackel", "ravelact"),
        );
        assert_eq!(
            parse_remote_url("https://github.com/wadackel/ravelact/"),
            gh("wadackel", "ravelact"),
        );
        // HTTPS with userinfo (CI clones often embed PATs here)
        assert_eq!(
            parse_remote_url("https://octocat@github.com/wadackel/ravelact"),
            gh("wadackel", "ravelact"),
        );
        // HTTPS with explicit port
        assert_eq!(
            parse_remote_url("https://github.com:443/wadackel/ravelact"),
            gh("wadackel", "ravelact"),
        );
        // Uppercase host — special scheme (https): `url` already lowercases
        assert_eq!(
            parse_remote_url("https://GitHub.com/wadackel/ravelact"),
            gh("wadackel", "ravelact"),
        );
        // Uppercase host — non-special scheme (ssh): we lowercase explicitly
        assert_eq!(
            parse_remote_url("ssh://git@GitHub.com/wadackel/ravelact.git"),
            gh("wadackel", "ravelact"),
        );
        // Surrounding whitespace tolerated (defence in depth above run_git's trim)
        assert_eq!(
            parse_remote_url("  https://github.com/wadackel/ravelact  "),
            gh("wadackel", "ravelact"),
        );
        // GitHub Enterprise (SCP and HTTPS)
        assert_eq!(
            parse_remote_url("git@ghe.example.com:acme/widget"),
            ghe("acme", "widget"),
        );
        assert_eq!(
            parse_remote_url("https://ghe.example.com/acme/widget.git"),
            ghe("acme", "widget"),
        );
    }

    #[test]
    fn parse_remote_url_strips_credentials() {
        // `git remote get-url origin` may return PAT-embedded URLs in CI;
        // confirm host/owner/repo are clean (no userinfo leakage).
        let parsed = parse_remote_url("https://x-access-token:secret@github.com/acme/widget.git")
            .expect("PAT-embedded URL should parse");
        assert_eq!(
            parsed,
            ("github.com".into(), "acme".into(), "widget".into())
        );
        let (host, owner, repo) = parsed;
        for field in [&host, &owner, &repo] {
            assert!(
                !field.contains("x-access-token") && !field.contains("secret"),
                "credential leaked into RepoInfo field: {field}",
            );
        }
    }

    #[test]
    fn parse_remote_url_rejects_invalid() {
        // Unauthenticated / unsupported schemes
        assert_eq!(parse_remote_url("git://github.com/o/r"), None);
        assert_eq!(parse_remote_url("file:///tmp/repo"), None);
        // Empty / garbage
        assert_eq!(parse_remote_url(""), None);
        assert_eq!(parse_remote_url("   "), None);
        assert_eq!(parse_remote_url("not a url"), None);
        // Missing repo segment
        assert_eq!(parse_remote_url("https://github.com/"), None);
        assert_eq!(parse_remote_url("https://github.com/justowner"), None);
        // Extra path segments beyond `owner/repo`
        assert_eq!(parse_remote_url("https://github.com/o/r/extra"), None);
        // SSH URL with single segment
        assert_eq!(parse_remote_url("ssh://git@github.com/o"), None);
        // SCP form with single segment
        assert_eq!(parse_remote_url("git@github.com:wadackel"), None);
    }

    #[test]
    fn build_graph_json_emits_cytoscape_shape() {
        let ir = build_ir(Path::new("tests/fixtures/simple"), &GlobSet::empty())
            .expect("simple fixture should load");

        let v = build_graph_json(&ir);

        let nodes = v
            .get("nodes")
            .and_then(Value::as_array)
            .expect("nodes is an array");
        let edges = v
            .get("edges")
            .and_then(Value::as_array)
            .expect("edges is an array");

        assert!(
            !nodes.is_empty(),
            "simple fixture should produce at least one node"
        );

        let mut node_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for n in nodes {
            let data = n.get("data").expect("node has data");
            let id = data.get("id").and_then(Value::as_str).expect("node id");
            assert!(!id.is_empty(), "node id must be non-empty");
            node_ids.insert(id);
            let kind = data.get("kind").and_then(Value::as_str).expect("node kind");
            assert!(
                ALLOWED_NODE_KINDS.contains(&kind),
                "unexpected node kind: {kind}",
            );
            assert!(
                data.get("label").and_then(Value::as_str).is_some(),
                "node missing label: {data}",
            );
        }

        for e in edges {
            let data = e.get("data").expect("edge has data");
            assert!(data.get("id").and_then(Value::as_str).is_some());
            let source = data.get("source").and_then(Value::as_str).expect("source");
            let target = data.get("target").and_then(Value::as_str).expect("target");
            // Cytoscape requires every edge endpoint to match a declared node.
            assert!(
                node_ids.contains(source),
                "edge source `{source}` is not a declared node",
            );
            assert!(
                node_ids.contains(target),
                "edge target `{target}` is not a declared node",
            );
            let kind = data.get("kind").and_then(Value::as_str).expect("edge kind");
            assert!(
                ALLOWED_EDGE_KINDS.contains(&kind),
                "unexpected edge kind: {kind}",
            );
        }
    }

    #[test]
    fn build_graph_json_covers_synthetic_external_refs() {
        // Synthetic fixtures include external action `uses: actions/checkout@v4`,
        // which exercises `ensure_external_action` / `external_action_*`
        // branches that the `simple` fixture does not reach.
        let ir = build_ir(
            Path::new("tests/fixtures/synthetic/nonstandard-composite-path"),
            &GlobSet::empty(),
        )
        .expect("synthetic fixture should load");
        let v = build_graph_json(&ir);
        let nodes = v
            .get("nodes")
            .and_then(Value::as_array)
            .expect("nodes is an array");
        let kinds: std::collections::HashSet<&str> = nodes
            .iter()
            .filter_map(|n| n.get("data")?.get("kind")?.as_str())
            .collect();
        assert!(
            kinds.contains("external-action"),
            "synthetic fixture should include external-action nodes, got: {kinds:?}",
        );
    }

    #[test]
    fn build_graph_json_covers_local_workflow_calls() {
        // multi-caller fixture has `uses: ./.github/workflows/callee.yml`
        // edges that exercise the local-workflow ensure path.
        let ir = build_ir(Path::new("tests/fixtures/multi-caller"), &GlobSet::empty())
            .expect("multi-caller fixture should load");
        let v = build_graph_json(&ir);
        let edges = v
            .get("edges")
            .and_then(Value::as_array)
            .expect("edges is an array");
        let kinds: std::collections::HashSet<&str> = edges
            .iter()
            .filter_map(|e| e.get("data")?.get("kind")?.as_str())
            .collect();
        assert!(
            kinds.contains("calls-workflow") || kinds.contains("uses-local-workflow"),
            "multi-caller fixture should produce workflow-call edges, got: {kinds:?}",
        );
    }

    // -----------------------------------------------------------------
    // /api/search
    // -----------------------------------------------------------------

    fn run_api_search(ir: Arc<Ir>, params: SearchParams) -> SearchResponse {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async { api_search(State(ir), Query(params)).await.0 })
    }

    #[test]
    fn api_search_empty_query_returns_empty() {
        let ir = load_simple_ir();
        let resp = run_api_search(
            ir,
            SearchParams {
                q: None,
                kind: None,
                limit: None,
            },
        );
        assert!(resp.matches.is_empty());
        assert_eq!(resp.total, 0);
        assert!(!resp.truncated);
    }

    #[test]
    fn api_search_whitespace_only_query_returns_empty() {
        let ir = load_simple_ir();
        let resp = run_api_search(
            ir,
            SearchParams {
                q: Some("   ".into()),
                kind: None,
                limit: None,
            },
        );
        assert!(resp.matches.is_empty());
    }

    #[test]
    fn api_search_returns_workflow_matches_for_known_trigger() {
        // The `simple` fixture's ci.yml lists `push` as a trigger; that
        // event name participates in the search corpus, so a query for
        // "push" must yield at least one workflow row.
        let ir = load_simple_ir();
        let resp = run_api_search(
            ir,
            SearchParams {
                q: Some("push".into()),
                kind: None,
                limit: None,
            },
        );
        assert!(
            resp.matches.iter().any(|m| m.kind == "workflow"),
            "search for `push` should hit at least one workflow",
        );
    }

    #[test]
    fn api_search_kind_filter_restricts_results() {
        let ir = load_simple_ir();
        // First, baseline — search without filter.
        let unfiltered = run_api_search(
            ir.clone(),
            SearchParams {
                q: Some("ci".into()),
                kind: None,
                limit: None,
            },
        );
        // Filter to local-action only; no local-action carries "ci" as a
        // substring in the simple fixture, so the result must be empty
        // even though the unfiltered query has hits.
        let filtered = run_api_search(
            ir,
            SearchParams {
                q: Some("ci".into()),
                kind: Some("local-action".into()),
                limit: None,
            },
        );
        assert!(!unfiltered.matches.is_empty());
        // The simple fixture has no local-action whose corpus contains "ci",
        // so the kind-restricted result must be empty. Asserting only
        // `.all(|m| m.kind == "local-action")` would pass vacuously on an
        // empty slice and miss a regression where the filter silently
        // returned everything (or nothing) regardless of the requested kind.
        assert!(filtered.matches.is_empty());
    }

    #[test]
    fn api_search_truncates_to_limit() {
        let ir = load_simple_ir();
        let resp = run_api_search(
            ir,
            SearchParams {
                q: Some("workflows".into()), // file-path token that every workflow shares
                kind: None,
                limit: Some(1),
            },
        );
        assert!(resp.matches.len() <= 1);
        if resp.total > 1 {
            assert!(resp.truncated);
        }
    }

    #[test]
    fn api_search_multi_token_requires_all_to_match() {
        let ir = load_simple_ir();
        let resp = run_api_search(
            ir,
            SearchParams {
                // Token combination unlikely to all appear in a single node.
                q: Some("push zzzzzzzz".into()),
                kind: None,
                limit: None,
            },
        );
        assert!(
            resp.matches.is_empty(),
            "multi-token AND should reject when one token is absent",
        );
    }

    // -----------------------------------------------------------------
    // /api/event-impact
    // -----------------------------------------------------------------

    fn run_event_impact(ir: Arc<Ir>, event: Option<String>) -> EventImpactResponse {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            api_event_impact(State(ir), Query(EventImpactParams { event }))
                .await
                .0
        })
    }

    #[test]
    fn api_event_impact_empty_or_whitespace_returns_empty() {
        // None.unwrap_or_default() = "" and "   " both trim to empty,
        // hitting the same trim().is_empty() short-circuit branch.
        let ir = load_simple_ir();
        for event in [None, Some("   ".into())] {
            let resp = run_event_impact(ir.clone(), event);
            assert!(resp.entry_workflows.is_empty());
            assert!(resp.node_ids.is_empty());
        }
    }

    #[test]
    fn api_event_impact_known_event_returns_workflows() {
        let ir = load_simple_ir();
        let resp = run_event_impact(ir, Some("push".into()));
        assert_eq!(resp.event, "push");
        assert!(
            !resp.entry_workflows.is_empty(),
            "push event should resolve to at least one entry workflow",
        );
        // Returned node ids must be sorted, deduplicated, and non-empty.
        assert!(resp.node_ids.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn api_event_impact_unknown_event_returns_empty_collections() {
        let ir = load_simple_ir();
        let resp = run_event_impact(ir, Some("nonexistent-event".into()));
        assert_eq!(resp.event, "nonexistent-event");
        assert!(resp.entry_workflows.is_empty());
        assert!(resp.node_ids.is_empty());
    }

    // -----------------------------------------------------------------
    // build_router + /api/* / static asset routing
    // -----------------------------------------------------------------

    fn router_for(ir: Arc<Ir>, repo_body: Option<Bytes>) -> Router {
        let api_body = Bytes::from(serde_json::to_vec(&build_graph_json(&ir)).expect("graph"));
        build_router(ir, api_body, repo_body)
    }

    async fn oneshot_get(router: Router, uri: &str) -> axum::http::Response<axum::body::Body> {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("request");
        router.oneshot(req).await.expect("oneshot")
    }

    async fn read_body(resp: axum::http::Response<axum::body::Body>) -> Vec<u8> {
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body bytes")
            .to_vec()
    }

    #[tokio::test]
    async fn build_router_serves_index_html() {
        let router = router_for(load_simple_ir(), None);
        let resp = oneshot_get(router, "/").await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("text/html"));
        let body = read_body(resp).await;
        assert!(!body.is_empty(), "index.html body must be non-empty");
    }

    #[tokio::test]
    async fn build_router_returns_404_for_unknown_asset() {
        let router = router_for(load_simple_ir(), None);
        let resp = oneshot_get(router, "/assets/does-not-exist-xyz.js").await;
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn build_router_api_graph_serves_pre_serialized_body() {
        let ir = load_simple_ir();
        let graph = build_graph_json(&ir);
        let expected = serde_json::to_vec(&graph).expect("graph bytes");
        let router = build_router(ir, Bytes::from(expected.clone()), None);
        let resp = oneshot_get(router, "/api/graph").await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("application/json"));
        let body = read_body(resp).await;
        assert_eq!(body, expected);
    }

    #[tokio::test]
    async fn build_router_api_repo_returns_404_when_none() {
        let router = router_for(load_simple_ir(), None);
        let resp = oneshot_get(router, "/api/repo").await;
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn build_router_api_repo_returns_200_when_present() {
        let info = RepoInfo {
            host: "github.com".into(),
            owner: "wadackel".into(),
            repo: "ravelact".into(),
            git_ref: "main".into(),
        };
        let body = Bytes::from(serde_json::to_vec(&info).expect("info bytes"));
        let router = router_for(load_simple_ir(), Some(body.clone()));
        let resp = oneshot_get(router, "/api/repo").await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("application/json"));
        let got = read_body(resp).await;
        assert_eq!(got, body.as_ref());
    }

    #[tokio::test]
    async fn build_router_api_triggers_returns_json_rows() {
        let router = router_for(load_simple_ir(), None);
        let resp = oneshot_get(router, "/api/triggers").await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = read_body(resp).await;
        let v: Value = serde_json::from_slice(&body).expect("json");
        assert!(v.get("rows").and_then(Value::as_array).is_some());
    }

    // -----------------------------------------------------------------
    // mime_for_extension
    // -----------------------------------------------------------------

    #[test]
    fn mime_for_extension_covers_known_types_and_fallback() {
        assert_eq!(
            mime_for_extension("app.js"),
            "application/javascript; charset=utf-8",
        );
        assert_eq!(
            mime_for_extension("worker.mjs"),
            "application/javascript; charset=utf-8",
        );
        assert_eq!(mime_for_extension("style.css"), "text/css; charset=utf-8",);
        assert_eq!(
            mime_for_extension("data.json"),
            "application/json; charset=utf-8",
        );
        assert_eq!(
            mime_for_extension("source.js.map"),
            "application/json; charset=utf-8",
        );
        assert_eq!(mime_for_extension("icon.svg"), "image/svg+xml");
        assert_eq!(mime_for_extension("logo.png"), "image/png");
        assert_eq!(mime_for_extension("photo.jpg"), "image/jpeg");
        assert_eq!(mime_for_extension("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_for_extension("anim.gif"), "image/gif");
        assert_eq!(mime_for_extension("pic.webp"), "image/webp");
        assert_eq!(mime_for_extension("favicon.ico"), "image/x-icon");
        assert_eq!(mime_for_extension("font.woff"), "font/woff");
        assert_eq!(mime_for_extension("font.woff2"), "font/woff2");
        // Unknown extension / no extension at all.
        assert_eq!(mime_for_extension("blob.bin"), "application/octet-stream");
        assert_eq!(
            mime_for_extension("no-extension"),
            "application/octet-stream",
        );
    }

    // -----------------------------------------------------------------
    // parse_remote_url — additional matrix
    // -----------------------------------------------------------------

    #[test]
    fn parse_remote_url_accepts_scp_form_without_user() {
        // `host:path` SCP form without the `user@` prefix — supported
        // because some private mirrors omit the login.
        assert_eq!(
            parse_remote_url("github.com:wadackel/ravelact.git"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
    }

    #[test]
    fn parse_remote_url_lowercases_host() {
        // `ssh://` is non-special in `url` crate, so the host is preserved
        // verbatim and must be lowercased explicitly.
        assert_eq!(
            parse_remote_url("ssh://git@GitHub.com/wadackel/ravelact"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
    }

    #[test]
    fn parse_remote_url_rejects_empty_scp_host() {
        // Leading colon means empty host part — must reject.
        assert_eq!(parse_remote_url(":owner/repo"), None);
        // SCP with explicit `user@` but no host.
        assert_eq!(parse_remote_url("git@:owner/repo"), None);
    }

    #[test]
    fn parse_remote_url_strips_single_trailing_slash() {
        assert_eq!(
            parse_remote_url("https://github.com/wadackel/ravelact/"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
    }

    #[test]
    fn parse_remote_url_rejects_owner_or_repo_empty_post_strip() {
        // `.git` strip leaves empty repo.
        assert_eq!(parse_remote_url("https://github.com/owner/.git"), None);
    }

    // -----------------------------------------------------------------
    // run_git + compute_repo_info — tempdir + real git
    // -----------------------------------------------------------------

    /// Seed a tempdir with a minimal git repo. Returns the dir handle.
    fn seed_git_repo(remote: Option<&str>, default_branch: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q", "-b", default_branch]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        run(&["config", "commit.gpgsign", "false"]);
        if let Some(url) = remote {
            run(&["remote", "add", "origin", url]);
        }
        std::fs::write(dir.path().join("README"), "seed").expect("write");
        run(&["add", "README"]);
        run(&["commit", "-q", "-m", "seed"]);
        dir
    }

    #[test]
    fn run_git_returns_none_when_command_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `dir` is not a git repo, so `rev-parse HEAD` exits non-zero.
        assert_eq!(run_git(dir.path(), &["rev-parse", "HEAD"]), None);
    }

    #[test]
    fn run_git_returns_trimmed_stdout_on_success() {
        let dir = seed_git_repo(None, "main");
        let head = run_git(dir.path(), &["rev-parse", "HEAD"]).expect("HEAD");
        assert_eq!(head.len(), 40, "sha should be 40 hex chars: {head}");
        assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compute_repo_info_returns_none_without_origin() {
        let dir = seed_git_repo(None, "main");
        assert_eq!(compute_repo_info(dir.path()), None);
    }

    #[test]
    fn compute_repo_info_returns_none_for_non_github_scheme() {
        let dir = seed_git_repo(Some("git://example.com/o/r"), "main");
        assert_eq!(compute_repo_info(dir.path()), None);
    }

    #[test]
    fn compute_repo_info_resolves_https_origin_and_branch() {
        let dir = seed_git_repo(Some("https://github.com/wadackel/ravelact.git"), "main");
        let info = compute_repo_info(dir.path()).expect("repo info");
        assert_eq!(info.host, "github.com");
        assert_eq!(info.owner, "wadackel");
        assert_eq!(info.repo, "ravelact");
        assert_eq!(info.git_ref, "main");
    }

    #[test]
    fn compute_repo_info_resolves_scp_origin() {
        let dir = seed_git_repo(Some("git@github.com:wadackel/ravelact.git"), "trunk");
        let info = compute_repo_info(dir.path()).expect("repo info");
        assert_eq!(info.host, "github.com");
        assert_eq!(info.owner, "wadackel");
        assert_eq!(info.repo, "ravelact");
        assert_eq!(info.git_ref, "trunk");
    }

    #[test]
    fn compute_repo_info_falls_back_to_sha_for_detached_head() {
        let dir = seed_git_repo(Some("https://github.com/o/r.git"), "main");
        // Detach HEAD by checking out the commit directly.
        let head = run_git(dir.path(), &["rev-parse", "HEAD"]).expect("HEAD");
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["checkout", "--detach", &head])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git");
        assert!(status.success());
        let info = compute_repo_info(dir.path()).expect("repo info");
        assert_eq!(info.git_ref, head);
    }

    #[test]
    fn compute_repo_info_resolves_ghe_host() {
        let dir = seed_git_repo(Some("https://ghe.example.com/team/proj.git"), "main");
        let info = compute_repo_info(dir.path()).expect("repo info");
        assert_eq!(info.host, "ghe.example.com");
        assert_eq!(info.owner, "team");
        assert_eq!(info.repo, "proj");
    }

    // -----------------------------------------------------------------
    // build_graph_json — external workflow + docker branches
    // -----------------------------------------------------------------

    #[test]
    fn build_graph_json_emits_external_workflow_node_for_cross_repo_call() {
        // cross-repo-call fixture has `uses: example-org/.../@SHA` at the
        // job level — exercises the ensure_external_workflow path that the
        // simple / multi-caller fixtures do not reach.
        let ir = build_ir(
            Path::new("tests/fixtures/synthetic/cross-repo-call"),
            &GlobSet::empty(),
        )
        .expect("cross-repo-call fixture should load");
        let v = build_graph_json(&ir);
        let nodes = v
            .get("nodes")
            .and_then(Value::as_array)
            .expect("nodes is an array");
        let kinds: std::collections::HashSet<&str> = nodes
            .iter()
            .filter_map(|n| n.get("data")?.get("kind")?.as_str())
            .collect();
        assert!(
            kinds.contains("external-workflow"),
            "cross-repo-call should emit external-workflow node, got: {kinds:?}",
        );
    }

    // -----------------------------------------------------------------
    // api_node — local-action / external-action / unknown kind
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn api_node_returns_local_action_summary() {
        let ir = load_simple_ir();
        let action_id = ir
            .actions
            .first()
            .expect("simple fixture has at least one local action")
            .id
            .0
            .clone();
        let params = NodeParams {
            kind: "local-action".into(),
            id: action_id.clone(),
        };
        let resp = api_node(State(ir), Query(params))
            .await
            .expect("local-action lookup");
        assert_eq!(resp.0.kind, "local-action");
        assert_eq!(resp.0.id, format!("la:{action_id}"));
        assert!(!resp.0.summary.is_empty());
    }

    #[tokio::test]
    async fn api_node_returns_external_action_summary() {
        let ir = Arc::new(
            build_ir(
                Path::new("tests/fixtures/synthetic/nonstandard-composite-path"),
                &GlobSet::empty(),
            )
            .expect("synthetic fixture"),
        );
        let ea = ir
            .external_actions
            .first()
            .expect("fixture must include an external action")
            .clone();
        // api_node expects the id WITHOUT the `ea:` prefix.
        let full = external_action_node_id(&ea);
        let bare = full.strip_prefix("ea:").expect("ea: prefix").to_string();
        let params = NodeParams {
            kind: "external-action".into(),
            id: bare,
        };
        let resp = api_node(State(ir), Query(params))
            .await
            .expect("external-action lookup");
        assert_eq!(resp.0.kind, "external-action");
        assert_eq!(resp.0.id, full);
        assert!(resp.0.summary.contains('@'));
    }

    #[tokio::test]
    async fn api_node_returns_404_for_unknown_local_action() {
        let ir = load_simple_ir();
        let resp = api_node(
            State(ir),
            Query(NodeParams {
                kind: "local-action".into(),
                id: "does-not-exist".into(),
            }),
        )
        .await;
        assert_eq!(resp.err(), Some(StatusCode::NOT_FOUND));
    }

    #[tokio::test]
    async fn api_node_returns_404_for_unknown_external_action() {
        let ir = load_simple_ir();
        let resp = api_node(
            State(ir),
            Query(NodeParams {
                kind: "external-action".into(),
                id: "nope/nope@deadbeef".into(),
            }),
        )
        .await;
        assert_eq!(resp.err(), Some(StatusCode::NOT_FOUND));
    }

    // -----------------------------------------------------------------
    // api_event_impact for external-workflow trace (collect_trace_node_ids
    // ExternalWorkflow / Docker branches)
    // -----------------------------------------------------------------

    #[test]
    fn api_event_impact_includes_external_workflow_nodes() {
        let ir = Arc::new(
            build_ir(
                Path::new("tests/fixtures/synthetic/cross-repo-call"),
                &GlobSet::empty(),
            )
            .expect("cross-repo-call fixture"),
        );
        let resp = run_event_impact(ir, Some("workflow_dispatch".into()));
        assert!(!resp.entry_workflows.is_empty());
        let has_external = resp.node_ids.iter().any(|n| n.starts_with("ew:"));
        assert!(
            has_external,
            "cross-repo-call should produce an ew: node id: {:?}",
            resp.node_ids,
        );
    }
}
