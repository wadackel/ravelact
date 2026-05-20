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
    let url = run_git(root, &["remote", "get-url", "origin"])?;
    let (host, owner, repo) = parse_github_url(&url)?;
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

/// Recognize `git@github.com:owner/repo[.git]` and `https://github.com/owner/repo[.git]`.
/// Returns `(host, owner, repo)`. `None` for non-github hosts / malformed URLs.
/// v1 limits acceptance to `github.com` exactly; GitHub Enterprise is a future task.
fn parse_github_url(url: &str) -> Option<(String, String, String)> {
    let url = url.trim();
    // SSH form: git@github.com:owner/repo[.git]
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let (owner, repo) = split_owner_repo(rest)?;
        return Some(("github.com".into(), owner, repo));
    }
    // HTTPS form: https://github.com/owner/repo[.git]
    // Also accept http:// for completeness.
    for prefix in ["https://github.com/", "http://github.com/"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            let (owner, repo) = split_owner_repo(rest)?;
            return Some(("github.com".into(), owner, repo));
        }
    }
    None
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
    // re-serialization. `None` means no GitHub remote / non-GitHub host /
    // detached HEAD with no SHA — the route returns 404 in that case so the
    // frontend can hide the "Open in GitHub" link gracefully.
    let repo_body: Option<Bytes> = repo_info
        .as_ref()
        .map(|r| Bytes::from(serde_json::to_vec(r).expect("RepoInfo serializes")));

    let app: Router = Router::new()
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
        .with_state(ir);

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
        // entry_triggers may be empty (workflow with only workflow_call) but
        // the field must always be present in the response shape.
        let _ = resp.0.entry_triggers;
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
    fn router_accepts_arc_ir_state() {
        // Regression: Task 0 introduced `with_state(Arc<Ir>)`. This test
        // confirms the Router compiles end-to-end with the new state type,
        // independent of whether any handler currently extracts it. Tasks
        // 1-4 add handlers that consume the state; this test guards the
        // wiring those handlers depend on.
        let ir = load_simple_ir();
        let _app: Router = Router::new().route("/", get(serve_index)).with_state(ir);
    }

    #[test]
    fn parse_github_url_accepts_ssh_and_https() {
        // SSH form
        assert_eq!(
            parse_github_url("git@github.com:wadackel/ravelact.git"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
        assert_eq!(
            parse_github_url("git@github.com:wadackel/ravelact"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
        // HTTPS form with and without .git suffix
        assert_eq!(
            parse_github_url("https://github.com/wadackel/ravelact.git"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
        assert_eq!(
            parse_github_url("https://github.com/wadackel/ravelact"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
        // Trailing slash tolerated
        assert_eq!(
            parse_github_url("https://github.com/wadackel/ravelact/"),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
        // Surrounding whitespace tolerated (git remote get-url emits a trailing
        // newline that run_git already trims, but defend in depth)
        assert_eq!(
            parse_github_url("  https://github.com/wadackel/ravelact  "),
            Some(("github.com".into(), "wadackel".into(), "ravelact".into())),
        );
    }

    #[test]
    fn parse_github_url_rejects_non_github() {
        // GitHub Enterprise is intentionally not accepted in v1
        assert_eq!(parse_github_url("https://github.example.com/o/r"), None);
        assert_eq!(parse_github_url("git@gitlab.com:o/r.git"), None);
        assert_eq!(parse_github_url("https://gitlab.com/o/r"), None);
        // Malformed
        assert_eq!(parse_github_url("not a url"), None);
        assert_eq!(parse_github_url("https://github.com/"), None);
        assert_eq!(parse_github_url("https://github.com/justowner"), None);
        // Sub-paths beyond owner/repo
        assert_eq!(parse_github_url("https://github.com/o/r/extra"), None);
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
}
