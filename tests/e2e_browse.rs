//! End-to-end tests for the `browse` subcommand HTTP API.
//!
//! Strategy: spawn `ravelact browse --port 0` as a child process, read its
//! stdout to discover the bind port, then hit the five HTTP endpoints via raw
//! TCP and assert HTTP/1.1 200 OK. Inline `write_synthetic_estate` is the
//! sole consumer of the generator (Rule of Three: no `tests/support/` module
//! until a second consumer appears).

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Write `workflows` `.yaml` files under `<dir>/.github/workflows/` to
/// simulate a complex estate. Up to 30 workflows are emitted as reusable
/// (`on: workflow_call`); the remainder are entry-point callers that
/// `uses:` one of the reusable workflows. This produces a realistic
/// distribution of `calls-workflow` and `uses-external-action` edges.
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
/// `<dir>/tests/fixtures/foo/.github/actions/foo/action.yaml`. This is the
/// canonical shape the browse default-exclude targets — ravelact's own
/// dogfood estate places test-fixture actions under `tests/fixtures/**`,
/// and the new `browse` default excludes that glob. The action is a minimal
/// composite that runs a single `echo`.
pub fn write_test_fixture_action(dir: &Path) -> std::io::Result<()> {
    let action_dir = dir.join("tests/fixtures/foo/.github/actions/foo");
    fs::create_dir_all(&action_dir)?;
    fs::write(
        action_dir.join("action.yaml"),
        "name: Foo Fixture\ndescription: Test fixture action\nruns:\n  using: composite\n  steps:\n    - run: echo foo\n      shell: bash\n",
    )
}

/// Spawn `ravelact browse --port 0 --no-open --root <dir>` and parse the
/// bind port from its stdout. The returned `Child` keeps the server alive;
/// the caller must `kill()` it when done.
fn spawn_browse_server(root: &Path) -> (Child, u16) {
    spawn_browse_server_with_args(root, &[])
}

/// Same as `spawn_browse_server` but appends `extra_args` after the
/// fixed `browse --no-open --port 0`. Used to pass flags like
/// `--include-test-fixtures` from the new orphan-policy tests.
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
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ravelact browse");

    // The bind announcement is the first stdout line:
    //   `ravelact browse listening on http://127.0.0.1:<port>/`
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
        // Empty line / EOF → loop with timeout guard above.
    };

    (child, port)
}

fn parse_bind_port(line: &str) -> Option<u16> {
    // Looks for `http://127.0.0.1:<port>/` anywhere on the line.
    let host_token = "http://127.0.0.1:";
    let idx = line.find(host_token)?;
    let rest = &line[idx + host_token.len()..];
    let port_end = rest.find('/')?;
    rest[..port_end].parse().ok()
}

/// Issue a minimal `GET <path>` over a fresh TCP connection, return the full
/// response (status line + headers + body) as bytes.
fn http_get(port: u16, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",);
    stream.write_all(request.as_bytes()).expect("write request");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read response");
    buf
}

fn assert_200(port: u16, path: &str) {
    let response = http_get(port, path);
    let head: &[u8] = response.get(..16).unwrap_or(&response);
    let head_str = String::from_utf8_lossy(head);
    assert!(
        head_str.starts_with("HTTP/1.1 200"),
        "{path} did not return 200 OK; got: {head_str:?}",
    );
}

/// Minimal RFC 3986 path-segment encoder for `/`. Not exposed as a general
/// utility — only `.` and `/` appear in workflow ids that need encoding.
fn urlencode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn five_endpoints_return_200() {
        let dir = tempdir().expect("tempdir");
        // 50 workflows so we have both reusables (i < 30) and callers
        // (i >= 30). Perf gating uses 300 separately (Task 17).
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());

        // wf-000 is reusable (workflow_call) so /api/trace would 404 on it
        // (no entry trigger). wf-030 is a caller (on: push), valid for trace.
        let trace_id = ".github/workflows/wf-030.yaml";
        let impact_id = trace_id;

        assert_200(port, "/api/graph");
        assert_200(port, "/api/triggers");
        assert_200(
            port,
            "/api/node?kind=workflow&id=.github%2Fworkflows%2Fwf-030.yaml",
        );
        assert_200(
            port,
            &format!("/api/impact?id={}", urlencode_path(impact_id),),
        );
        assert_200(
            port,
            &format!("/api/trace?id={}", urlencode_path(trace_id),),
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// Read the full HTTP response body (after the first blank line). The
    /// /api/graph handler returns Content-Length-tagged bytes that the
    /// existing `http_get` already reads to EOF; we just need to skip
    /// the headers.
    fn body_after_headers(response: &[u8]) -> &[u8] {
        let mut i = 0;
        while i + 3 < response.len() {
            if &response[i..i + 4] == b"\r\n\r\n" {
                return &response[i + 4..];
            }
            i += 1;
        }
        response
    }

    /// `tests/fixtures/foo/.github/actions/foo/action.yaml` is the canonical
    /// shape `browse` excludes by default. With no opt-out, `/api/graph`
    /// must not surface this local-action node.
    #[test]
    fn browse_default_excludes_test_fixtures() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");
        write_test_fixture_action(dir.path()).expect("write_test_fixture_action");

        let (mut child, port) = spawn_browse_server(dir.path());
        let response = http_get(port, "/api/graph");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);

        assert!(
            !body_str.contains("tests/fixtures/foo"),
            "default browse must hide tests/fixtures/** local-actions; body: {body_str}",
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `/api/search` should rank a known workflow file's substring high
    /// (workflow name "Caller 31" contains the substring "caller") and
    /// return zero matches for a guaranteed-absent token.
    #[test]
    fn api_search_returns_expected_matches() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());

        // hit: every caller workflow has name "Caller N" in the IR
        let response = http_get(port, "/api/search?q=caller");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);
        assert!(
            body_str.contains("\"matches\""),
            "/api/search response must contain matches: {body_str}",
        );
        assert!(
            body_str.contains("Caller "),
            "/api/search?q=caller should return Caller workflow labels: {body_str}",
        );

        // miss: a token that cannot occur anywhere in the corpus
        let response = http_get(port, "/api/search?q=zzqx-no-such-token-anywhere");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);
        assert!(
            body_str.contains("\"matches\":[]"),
            "absent token must return empty matches array: {body_str}",
        );
        assert!(
            body_str.contains("\"total\":0"),
            "absent token must return total=0: {body_str}",
        );

        // empty query short-circuits to empty result, no error
        let response = http_get(port, "/api/search?q=");
        let head: &[u8] = response.get(..16).unwrap_or(&response);
        assert!(
            String::from_utf8_lossy(head).starts_with("HTTP/1.1 200"),
            "empty q must still be 200 OK",
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `/api/event-impact?event=push` should return both the entry
    /// workflows and their downstream node ids (transitive). For the
    /// synthetic estate every caller workflow is `on: push` and uses
    /// `actions/checkout@v4`, so the response must include caller
    /// workflow ids AND `ea:actions/checkout@v4`.
    #[test]
    fn api_event_impact_returns_entry_workflows_and_downstream() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 50).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());

        let response = http_get(port, "/api/event-impact?event=push");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);
        assert!(
            body_str.contains("\"event\":\"push\""),
            "event echoed in response: {body_str}",
        );
        assert!(
            body_str.contains("wf:.github/workflows/wf-030.yaml"),
            "downstream must list caller workflow id: {body_str}",
        );
        assert!(
            body_str.contains("ea:actions/checkout@v4"),
            "downstream must list actions/checkout external action: {body_str}",
        );

        // missing event → empty arrays
        let response = http_get(port, "/api/event-impact?event=zznever");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);
        assert!(
            body_str.contains("\"entry_workflows\":[]"),
            "absent event must return empty entry_workflows: {body_str}",
        );
        assert!(
            body_str.contains("\"node_ids\":[]"),
            "absent event must return empty node_ids: {body_str}",
        );

        // empty event short-circuits
        let response = http_get(port, "/api/event-impact?event=");
        let head: &[u8] = response.get(..16).unwrap_or(&response);
        assert!(
            String::from_utf8_lossy(head).starts_with("HTTP/1.1 200"),
            "empty event must still be 200 OK",
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `/api/repo` returns 404 when the `--root` is not a git repository.
    /// A tempdir starts out without `.git`, so `git remote get-url origin`
    /// fails and `compute_repo_info` returns `None`. The frontend treats
    /// this as "hide the Open-in-GitHub link for local nodes".
    #[test]
    fn api_repo_returns_404_for_non_git_root() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");

        let (mut child, port) = spawn_browse_server(dir.path());
        let response = http_get(port, "/api/repo");
        let head: &[u8] = response.get(..16).unwrap_or(&response);
        let head_str = String::from_utf8_lossy(head);
        assert!(
            head_str.starts_with("HTTP/1.1 404"),
            "non-git root must yield 404 for /api/repo; got: {head_str:?}",
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `/api/repo` returns 200 + the expected JSON shape when the `--root`
    /// is a git repository with a github.com `origin`. Built from scratch
    /// inside a tempdir using plain `git` commands so the test is
    /// hermetic and does not depend on the surrounding host repo.
    #[test]
    fn api_repo_returns_github_provenance_for_git_root() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");

        // Initialize a minimal git repo with a github.com origin and one
        // commit on `main`. We do not actually push — only `git remote
        // get-url origin` and `git symbolic-ref --short HEAD` matter.
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
        let response = http_get(port, "/api/repo");
        let head: &[u8] = response.get(..16).unwrap_or(&response);
        let head_str = String::from_utf8_lossy(head);
        assert!(
            head_str.starts_with("HTTP/1.1 200"),
            "git root must yield 200 for /api/repo; got: {head_str:?}",
        );
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);
        assert!(
            body_str.contains("\"host\":\"github.com\""),
            "host should be github.com: {body_str}",
        );
        assert!(
            body_str.contains("\"owner\":\"wadackel\""),
            "owner should be wadackel: {body_str}",
        );
        assert!(
            body_str.contains("\"repo\":\"ravelact\""),
            "repo should be ravelact: {body_str}",
        );
        assert!(
            body_str.contains("\"ref\":\"main\""),
            "ref should be the branch name (main): {body_str}",
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// With `--include-test-fixtures` the same fixture must appear in the
    /// graph response — proves the opt-out actually reaches the backend.
    #[test]
    fn browse_include_test_fixtures_flag() {
        let dir = tempdir().expect("tempdir");
        write_synthetic_estate(dir.path(), 5).expect("write_synthetic_estate");
        write_test_fixture_action(dir.path()).expect("write_test_fixture_action");

        let (mut child, port) =
            spawn_browse_server_with_args(dir.path(), &["--include-test-fixtures"]);
        let response = http_get(port, "/api/graph");
        let body = body_after_headers(&response);
        let body_str = String::from_utf8_lossy(body);

        assert!(
            body_str.contains("tests/fixtures/foo/.github/actions/foo"),
            "--include-test-fixtures must surface fixture local-actions; body: {body_str}",
        );

        let _ = child.kill();
        let _ = child.wait();
    }
}
