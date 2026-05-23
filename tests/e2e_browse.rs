//! End-to-end tests for the `browse` subcommand ConnectRPC API.
//!
//! Strategy: spawn `ravelact browse --port 0` as a child process, read its
//! stdout to discover the bind port, then drive the 8 RPCs through a
//! generated Connect client. The single consumer of `write_synthetic_estate`
//! / `write_test_fixture_action` / `write_local_action` is this file
//! (Rule of Three: no `tests/support/` module until a second consumer
//! appears).

use axum::http::Uri;
use connectrpc::client::{ClientConfig, HttpClient};
use ravelact::cli::render::browse::connect::ravelact::browse::v1::BrowseServiceClient;
use ravelact::cli::render::browse::proto::ravelact::browse::v1::{
    self as pb, GetEventImpactRequest, GetGraphRequest, GetImpactRequest, GetNodeRequest,
    GetRepoRequest, ListTriggersRequest, SearchRequest, TraceRequest,
};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

/// Write `workflows` `.yaml` files under `<dir>/.github/workflows/` to
/// simulate a complex estate. Up to 30 workflows are emitted as reusable
/// (`on: workflow_call`); the remainder are entry-point callers that
/// `uses:` one of the reusable workflows.
pub fn write_synthetic_estate(dir: &Path, workflows: usize) -> std::io::Result<()> {
    let wf_dir = dir.join(".github/workflows");
    fs::create_dir_all(&wf_dir)?;
    let reusable_count = workflows.min(30);

    for i in 0..workflows {
        let path = wf_dir.join(format!("wf-{i:03}.yaml"));
        let content = if i < reusable_count {
            format!(
                "name: Reusable {i}\non:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-{i}\n"
            )
        } else {
            let callee = i % reusable_count;
            format!(
                "name: Caller {i}\non:\n  push:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-{i}\n  call:\n    uses: ./.github/workflows/wf-{callee:03}.yaml\n"
            )
        };
        fs::write(path, content)?;
    }
    Ok(())
}

/// Write a stand-alone local-action manifest under
/// `<dir>/tests/fixtures/foo/.github/actions/foo/action.yaml`. The canonical
/// shape the browse default-exclude targets — ravelact's dogfood estate
/// places test-fixture actions under `tests/fixtures/**`, and the new
/// `browse` default excludes that glob.
pub fn write_test_fixture_action(dir: &Path) -> std::io::Result<()> {
    let action_dir = dir.join("tests/fixtures/foo/.github/actions/foo");
    fs::create_dir_all(&action_dir)?;
    fs::write(
        action_dir.join("action.yaml"),
        "name: Foo Fixture\ndescription: Test fixture action\nruns:\n  using: composite\n  steps:\n    - run: echo foo\n      shell: bash\n",
    )
}

/// Write a stand-alone local-action manifest at the non-excluded path
/// `<dir>/.github/actions/foo/action.yaml`. Canonical local-action layout
/// `browse` should surface so that `GetNode(kind=local-action)` can be
/// exercised end-to-end.
pub fn write_local_action(dir: &Path) -> std::io::Result<()> {
    let action_dir = dir.join(".github/actions/foo");
    fs::create_dir_all(&action_dir)?;
    fs::write(
        action_dir.join("action.yaml"),
        "name: Foo Local\ndescription: Local action fixture\nruns:\n  using: composite\n  steps:\n    - run: echo foo\n      shell: bash\n",
    )
}

fn spawn_browse_server(root: &Path) -> (Child, u16) {
    spawn_browse_server_with_args(root, &[])
}

fn spawn_browse_server_with_args(root: &Path, extra_args: &[&str]) -> (Child, u16) {
    let bin = env!("CARGO_BIN_EXE_ravelact");
    let mut cmd = Command::new(bin);
    cmd.args([
        "--root",
        root.to_str().expect("utf8 root path"),
        "browse",
        "--no-open",
        "--port",
        "0",
    ]);
    cmd.args(extra_args);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn ravelact browse");

    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    let port = loop {
        line.clear();
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("timed out waiting for bind announcement");
        }
        reader.read_line(&mut line).expect("read bind announcement");
        if let Some(p) = parse_bind_port(&line) {
            break p;
        }
    };

    // Drain stdout in the background; see the JSON-era comment about
    // Broken Pipe on Linux CI for the rationale.
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    });

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    loop {
        if Instant::now() > ready_deadline {
            let _ = child.kill();
            panic!("server announced port {port} but never started accepting");
        }
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    (child, port)
}

fn parse_bind_port(line: &str) -> Option<u16> {
    let host_token = "http://127.0.0.1:";
    let idx = line.find(host_token)?;
    let rest = &line[idx + host_token.len()..];
    let port_end = rest.find('/')?;
    rest[..port_end].parse().ok()
}

/// Build a `(runtime, client)` pair pointing at the spawned server.
/// Tests stay sync (matching the existing harness) and `block_on` each
/// RPC on demand.
fn connect_client(port: u16) -> (Runtime, BrowseServiceClient<HttpClient>) {
    let runtime = Runtime::new().expect("tokio runtime");
    let transport = HttpClient::plaintext();
    let uri: Uri = format!("http://127.0.0.1:{port}")
        .parse()
        .expect("parse base URI");
    let config = ClientConfig::new(uri).with_codec_format(connectrpc::CodecFormat::Json);
    let client = BrowseServiceClient::new(transport, config);
    (runtime, client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;
    use globset::GlobSet;
    use ravelact::ir::build::build_ir;
    use tempfile::tempdir;

    #[test]
    fn synthetic_estate_generates_300_workflows() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 300).expect("write_synthetic_estate");
        let count = fs::read_dir(dir.path().join(".github/workflows"))
            .expect("read workflows dir")
            .count();
        assert_eq!(count, 300, "expected 300 yaml files");

        let ir = build_ir(dir.path(), &GlobSet::empty()).expect("build_ir");
        assert_eq!(
            ir.workflows.len(),
            300,
            "IR should contain 300 workflow entries"
        );
    }

    #[test]
    fn eight_rpcs_return_ok() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        // wf-000 is reusable (workflow_call) so Trace would NotFound on it.
        // wf-030 is a caller (on: push), valid for trace.
        let trace_id = ".github/workflows/wf-030.yaml".to_string();
        let impact_id = trace_id.clone();
        let node_id = trace_id.clone();

        rt.block_on(async {
            client
                .get_graph(GetGraphRequest::default())
                .await
                .expect("GetGraph 200");
            client
                .list_triggers(ListTriggersRequest::default())
                .await
                .expect("ListTriggers 200");

            let node_resp = client
                .get_node(GetNodeRequest {
                    kind: "workflow".into(),
                    id: node_id.clone(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("GetNode 200")
                .into_owned();
            // GetNode.file is browse-root-relative forward-slash — Issue #21
            // pinned this and the e2e suite is its strongest guard.
            assert_eq!(
                node_resp.file, ".github/workflows/wf-030.yaml",
                "GetNode.file must be browse-root-relative",
            );

            client
                .get_impact(GetImpactRequest {
                    id: impact_id.clone(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("GetImpact 200");
            client
                .trace(TraceRequest {
                    id: trace_id.clone(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("Trace 200");
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// Pair with `eight_rpcs_return_ok` to cover the local-action branch
    /// of `GetNode.file` relative-path logic.
    #[test]
    fn get_node_returns_relative_path_for_local_action() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");
        write_local_action(dir.path()).expect("write_local_action");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_node(GetNodeRequest {
                    kind: "local-action".into(),
                    id: ".github/actions/foo".into(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("GetNode 200")
                .into_owned();
            assert_eq!(
                resp.file, ".github/actions/foo/action.yaml",
                "local-action file must be browse-root-relative",
            );
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// Workflow with both job-level and step-level `if:` must surface them
    /// in source order via `GetNode.if_conditions`. Strong oracle: explicit
    /// match on each oneof variant rather than substring matching.
    #[test]
    fn get_node_surfaces_if_conditions() {
        use pb::if_condition::Scope;
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic/step-if-guard");
        let (mut child, port) = spawn_browse_server(&fixture);
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_node(GetNodeRequest {
                    kind: "workflow".into(),
                    id: ".github/workflows/ci.yml".into(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("GetNode 200")
                .into_owned();
            assert!(
                !resp.if_conditions.is_empty(),
                "step-if-guard fixture must produce at least one if-condition row"
            );
            let has_job_combined = resp.if_conditions.iter().any(|c| match &c.scope {
                Some(Scope::Job(j)) => j.job_id == "combined",
                _ => false,
            });
            let has_step_index_1 = resp.if_conditions.iter().any(|c| match &c.scope {
                Some(Scope::Step(s)) => s.step_index == 1,
                _ => false,
            });
            assert!(
                has_job_combined,
                "must include job-scope row for `combined`"
            );
            assert!(
                has_step_index_1,
                "must include step-scope row with 1-based step_index=1"
            );
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `tests/fixtures/foo/.github/actions/foo/action.yaml` is the canonical
    /// shape `browse` excludes by default.
    #[test]
    fn browse_default_excludes_test_fixtures() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");
        write_test_fixture_action(dir.path()).expect("write_test_fixture_action");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_graph(GetGraphRequest::default())
                .await
                .expect("GetGraph 200");
            let owned = resp.into_owned();
            let has_fixture_node = owned.nodes.iter().any(|n| {
                if let Some(d) = n.data.as_option() {
                    d.id.contains("tests/fixtures/foo")
                } else {
                    false
                }
            });
            assert!(
                !has_fixture_node,
                "default browse must hide tests/fixtures/** local-actions",
            );
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// Search "caller" must hit at least one workflow whose label contains
    /// the token. Empty / absent tokens short-circuit to empty results.
    #[test]
    fn search_returns_expected_matches() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .search(SearchRequest {
                    q: "caller".into(),
                    kind: None,
                    limit: None,
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("Search 200");
            let owned = resp.into_owned();
            assert!(
                owned.matches.iter().any(|m| m.label.starts_with("Caller ")),
                "search `caller` must hit Caller workflows",
            );

            let miss = client
                .search(SearchRequest {
                    q: "zzqx-no-such-token-anywhere".into(),
                    kind: None,
                    limit: None,
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("Search 200 even when empty")
                .into_owned();
            assert!(miss.matches.is_empty());
            assert_eq!(miss.total, 0);

            // Empty query short-circuits to empty result with 200 OK.
            let empty = client
                .search(SearchRequest {
                    q: "".into(),
                    kind: None,
                    limit: None,
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("Search 200 on empty q")
                .into_owned();
            assert!(empty.matches.is_empty());
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `GetEventImpact(event=push)` must return both entry workflows and
    /// their downstream nodes.
    #[test]
    fn event_impact_returns_entry_workflows_and_downstream() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_event_impact(GetEventImpactRequest {
                    event: "push".into(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("GetEventImpact 200")
                .into_owned();
            assert_eq!(resp.event, "push");
            assert!(
                resp.entry_workflows
                    .iter()
                    .any(|w| w == "wf:.github/workflows/wf-030.yaml"),
                "downstream must list caller workflow id",
            );
            assert!(
                resp.node_ids.iter().any(|n| n == "ea:actions/checkout@v4"),
                "downstream must list actions/checkout external action",
            );

            let absent = client
                .get_event_impact(GetEventImpactRequest {
                    event: "zznever".into(),
                    __buffa_unknown_fields: Default::default(),
                })
                .await
                .expect("absent event 200")
                .into_owned();
            assert!(absent.entry_workflows.is_empty());
            assert!(absent.node_ids.is_empty());
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `GetRepo` returns Err(NotFound) when the `--root` is not a git
    /// repository.
    #[test]
    fn repo_returns_not_found_for_non_git_root() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let err = client
                .get_repo(GetRepoRequest::default())
                .await
                .expect_err("non-git root must surface NotFound");
            assert_eq!(
                err.code,
                ErrorCode::NotFound,
                "expected NotFound, got {:?}",
                err.code
            );
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `GetRepo` returns the expected RepoInfo when the `--root` is a git
    /// repository with a github.com `origin`.
    #[test]
    fn repo_returns_github_provenance_for_git_root() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");

        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("run git");
            assert!(status.success(), "git {:?} failed", args);
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/wadackel/ravelact.git",
        ]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_repo(GetRepoRequest::default())
                .await
                .expect("GetRepo 200")
                .into_owned();
            let view = &resp;
            assert_eq!(view.host, "github.com");
            assert_eq!(view.owner, "wadackel");
            assert_eq!(view.repo, "ravelact");
            assert_eq!(view.r#ref, "main");
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `GetRepo` resolves a GitHub Enterprise origin too.
    #[test]
    fn repo_returns_ghe_provenance_for_git_root() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");

        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("run git");
            assert!(status.success(), "git {:?} failed", args);
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&[
            "remote",
            "add",
            "origin",
            "https://ghe.example.com/acme/widget.git",
        ]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);

        let (mut child, port) = spawn_browse_server(dir.path());
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_repo(GetRepoRequest::default())
                .await
                .expect("GetRepo 200")
                .into_owned();
            let view = &resp;
            assert_eq!(view.host, "ghe.example.com");
            assert_eq!(view.owner, "acme");
            assert_eq!(view.repo, "widget");
            assert_eq!(view.r#ref, "main");
        });

        let _ = child.kill();
        let _ = child.wait();
    }

    /// With `--include-test-fixtures` the fixture local-action must appear
    /// in the graph response.
    #[test]
    fn browse_include_test_fixtures_flag() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");
        write_test_fixture_action(dir.path()).expect("write_test_fixture_action");

        let (mut child, port) =
            spawn_browse_server_with_args(dir.path(), &["--include-test-fixtures"]);
        let (rt, client) = connect_client(port);

        rt.block_on(async {
            let resp = client
                .get_graph(GetGraphRequest::default())
                .await
                .expect("GetGraph 200");
            let owned = resp.into_owned();
            let has_fixture_node = owned.nodes.iter().any(|n| {
                if let Some(d) = n.data.as_option() {
                    d.id.contains("tests/fixtures/foo/.github/actions/foo")
                } else {
                    false
                }
            });
            assert!(
                has_fixture_node,
                "--include-test-fixtures must surface fixture local-actions",
            );
        });

        let _ = child.kill();
        let _ = child.wait();
    }
}
