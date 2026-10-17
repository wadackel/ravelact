//! `ravelact skill` prints the embedded agent guide that the
//! `skills/ravelact/SKILL.md` wrapper tells agents to load.

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn skill_prints_guide_outside_a_repository() {
    let cwd = TempDir::new().unwrap();
    let out = Command::cargo_bin("ravelact")
        .unwrap()
        .current_dir(cwd.path())
        .env("XDG_STATE_HOME", cwd.path())
        .arg("skill")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("# ravelact\n"),
        "guide must start at its title, not frontmatter"
    );
    assert!(
        stdout.contains("#### `trace <event>`"),
        "guide should document trace"
    );
    assert!(
        std::fs::read_dir(cwd.path()).unwrap().next().is_none(),
        "skill must not build or write an IR cache"
    );
}
