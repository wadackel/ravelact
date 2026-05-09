//! Custom shell completion adapters that hide flag candidates unless the current word starts with `-`.
//!
//! Wraps `clap_complete`'s built-in `EnvCompleter` implementations (Bash/Zsh/Fish) and post-filters
//! the candidate list returned from `clap_complete::engine::complete`, removing entries whose value
//! starts with `-` when the user has not yet typed a dash.
//!
//! Ported 1:1 from the `ofsht` project (https://github.com/wadackel/ofsht/blob/main/src/shell_completion.rs)
//! with the inline test module adapted to ravelact's `Cli` (no top-level global `ValueEnum`,
//! so the `option-value` regression test exercises `trace --format` instead of `--color`).

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;

use clap::Command;
use clap_complete::engine::{complete, CompletionCandidate};
use clap_complete::env::{Bash, EnvCompleter, Fish, Zsh};

/// Drop flag candidates (values starting with `-`) unless the current word also starts with `-`.
fn filter_flag_candidates(
    completions: Vec<CompletionCandidate>,
    current_word: &OsStr,
) -> Vec<CompletionCandidate> {
    if current_word.to_string_lossy().starts_with('-') {
        return completions;
    }
    completions
        .into_iter()
        .filter(|c| !c.get_value().to_string_lossy().starts_with('-'))
        .collect()
}

/// Run `engine::complete` and apply the flag filter against the current word at `args[index]`.
fn filtered_candidates(
    cmd: &mut Command,
    args: Vec<OsString>,
    index: usize,
    current_dir: Option<&Path>,
) -> io::Result<Vec<CompletionCandidate>> {
    let current_word = args.get(index).cloned().unwrap_or_default();
    let completions = complete(cmd, args, index, current_dir)?;
    Ok(filter_flag_candidates(completions, &current_word))
}

/// Bash adapter: identical registration to the built-in, filtered completion output.
pub struct FilteredBash;

impl EnvCompleter for FilteredBash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn is(&self, name: &str) -> bool {
        name == "bash"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Bash.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        let index: usize = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or_default();
        let ifs: Option<String> = std::env::var("_CLAP_IFS").ok();
        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for (i, candidate) in filtered.iter().enumerate() {
            if i != 0 {
                write!(buf, "{}", ifs.as_deref().unwrap_or("\n"))?;
            }
            write!(buf, "{}", candidate.get_value().to_string_lossy())?;
        }
        Ok(())
    }
}

/// Zsh adapter: identical registration, filtered output, preserves `value:help` display format.
pub struct FilteredZsh;

impl EnvCompleter for FilteredZsh {
    fn name(&self) -> &'static str {
        "zsh"
    }

    fn is(&self, name: &str) -> bool {
        name == "zsh"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Zsh.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        let index: usize = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or_default();
        let ifs: Option<String> = std::env::var("_CLAP_IFS").ok();

        // Match built-in Zsh: if current word is one beyond the last arg, pad with "".
        // Source: clap_complete-4.5.60/src/env/shells.rs:410-414
        let mut args = args;
        if args.len() == index {
            args.push(OsString::new());
        }

        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for (i, candidate) in filtered.iter().enumerate() {
            if i != 0 {
                write!(buf, "{}", ifs.as_deref().unwrap_or("\n"))?;
            }
            write!(
                buf,
                "{}",
                escape_zsh_value(&candidate.get_value().to_string_lossy())
            )?;
            if let Some(help) = candidate.get_help() {
                write!(
                    buf,
                    ":{}",
                    escape_zsh_help(help.to_string().lines().next().unwrap_or_default())
                )?;
            }
        }
        Ok(())
    }
}

/// Zsh escape: backslash and colon are special within `value:help` records.
/// Source: clap_complete-4.5.60/src/env/shells.rs:440-442
fn escape_zsh_value(s: &str) -> String {
    s.replace('\\', "\\\\").replace(':', "\\:")
}

/// Zsh help escape: only backslash needs doubling (colon already split by caller).
/// Source: clap_complete-4.5.60/src/env/shells.rs:445-447
fn escape_zsh_help(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// Fish adapter: identical registration, filtered output, `value\thelp\n` per record.
pub struct FilteredFish;

impl EnvCompleter for FilteredFish {
    fn name(&self) -> &'static str {
        "fish"
    }

    fn is(&self, name: &str) -> bool {
        name == "fish"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Fish.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        // Match built-in Fish: current word is the last arg.
        // Source: clap_complete-4.5.60/src/env/shells.rs:237
        let index = args.len().saturating_sub(1);
        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for candidate in &filtered {
            write!(buf, "{}", candidate.get_value().to_string_lossy())?;
            if let Some(help) = candidate.get_help() {
                write!(
                    buf,
                    "\t{}",
                    help.to_string().lines().next().unwrap_or_default()
                )?;
            }
            writeln!(buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use ravelact::cli::Cli;
    use serial_test::serial;
    use std::ffi::OsString;

    struct EnvGuard {
        key: &'static str,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn cand(value: &str) -> CompletionCandidate {
        CompletionCandidate::new(value)
    }

    #[test]
    fn filter_drops_flags_when_current_word_empty() {
        let completions = vec![
            cand("trace"),
            cand("callers"),
            cand("--root"),
            cand("--help"),
            cand("-v"),
        ];
        let filtered = filter_flag_candidates(completions, OsStr::new(""));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["trace", "callers"]);
    }

    #[test]
    fn filter_keeps_all_when_current_word_is_dash() {
        let completions = vec![cand("trace"), cand("--root"), cand("-v")];
        let filtered = filter_flag_candidates(completions, OsStr::new("-"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["trace", "--root", "-v"]);
    }

    #[test]
    fn filter_keeps_all_when_current_word_is_long_prefix() {
        let completions = vec![cand("trace"), cand("--root"), cand("--no-cache")];
        let filtered = filter_flag_candidates(completions, OsStr::new("--r"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["trace", "--root", "--no-cache"]);
    }

    #[test]
    fn filter_drops_dashes_when_current_word_is_non_dash_text() {
        let completions = vec![cand("trace"), cand("callers"), cand("--root"), cand("-v")];
        let filtered = filter_flag_candidates(completions, OsStr::new("foo"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["trace", "callers"]);
    }

    fn values_of(candidates: &[CompletionCandidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    #[serial]
    fn bash_write_complete_uses_custom_separator_and_filters_empty_word_flags() {
        let _index = EnvGuard::set("_CLAP_COMPLETE_INDEX", "1");
        let _ifs = EnvGuard::set("_CLAP_IFS", "|");
        let mut cmd = Cli::command();
        let mut out = Vec::new();

        FilteredBash
            .write_complete(&mut cmd, args(&["ravelact", ""]), None, &mut out)
            .expect("bash completion must succeed");

        let stdout = String::from_utf8(out).expect("utf8 completion output");
        assert!(
            stdout.contains("trace|") || stdout.contains("|trace"),
            "custom IFS separator must be used between bash records: {stdout:?}"
        );
        assert!(
            !stdout.contains("--root") && !stdout.contains("--help"),
            "empty current word must hide flag candidates: {stdout:?}"
        );
    }

    #[test]
    #[serial]
    fn bash_write_complete_falls_back_to_newline_when_ifs_is_missing() {
        let _index = EnvGuard::set("_CLAP_COMPLETE_INDEX", "1");
        let _ifs = EnvGuard::remove("_CLAP_IFS");
        let mut cmd = Cli::command();
        let mut out = Vec::new();

        FilteredBash
            .write_complete(&mut cmd, args(&["ravelact", ""]), None, &mut out)
            .expect("bash completion must succeed");

        let stdout = String::from_utf8(out).expect("utf8 completion output");
        assert!(
            stdout.lines().any(|line| line == "trace"),
            "bash fallback separator must produce one candidate per line: {stdout:?}"
        );
    }

    #[test]
    #[serial]
    fn bash_write_complete_invalid_index_env_falls_back_to_zero() {
        let _ifs = EnvGuard::set("_CLAP_IFS", "\n");

        let _invalid_index = EnvGuard::set("_CLAP_COMPLETE_INDEX", "not-a-number");
        let mut invalid_cmd = Cli::command();
        let mut invalid_out = Vec::new();
        let invalid_result = FilteredBash.write_complete(
            &mut invalid_cmd,
            args(&["ravelact", "trace"]),
            None,
            &mut invalid_out,
        );
        drop(_invalid_index);

        let _zero_index = EnvGuard::set("_CLAP_COMPLETE_INDEX", "0");
        let mut zero_cmd = Cli::command();
        let mut zero_out = Vec::new();
        let zero_result = FilteredBash.write_complete(
            &mut zero_cmd,
            args(&["ravelact", "trace"]),
            None,
            &mut zero_out,
        );

        assert_eq!(
            invalid_result.err().map(|e| e.to_string()),
            zero_result.err().map(|e| e.to_string()),
            "invalid _CLAP_COMPLETE_INDEX must use the same fallback as index 0"
        );
        assert_eq!(
            invalid_out, zero_out,
            "invalid _CLAP_COMPLETE_INDEX output must match index 0 output"
        );
    }

    #[test]
    #[serial]
    fn zsh_write_complete_pads_missing_current_word_and_preserves_help_format() {
        let _index = EnvGuard::set("_CLAP_COMPLETE_INDEX", "1");
        let _ifs = EnvGuard::set("_CLAP_IFS", "\n");
        let mut cmd = Cli::command();
        let mut out = Vec::new();

        FilteredZsh
            .write_complete(&mut cmd, args(&["ravelact"]), None, &mut out)
            .expect("zsh completion must succeed");

        let stdout = String::from_utf8(out).expect("utf8 completion output");
        assert!(
            stdout
                .lines()
                .any(|line| line.starts_with("trace:Forward walk")),
            "zsh output must include value:help records after padding args: {stdout:?}"
        );
        assert!(
            !stdout.contains("--root") && !stdout.contains("--help"),
            "padded empty current word must hide flag candidates: {stdout:?}"
        );
    }

    #[test]
    #[serial]
    fn fish_write_complete_emits_tab_help_and_filters_empty_word_flags() {
        let mut cmd = Cli::command();
        let mut out = Vec::new();

        FilteredFish
            .write_complete(&mut cmd, args(&["ravelact", ""]), None, &mut out)
            .expect("fish completion must succeed");

        let stdout = String::from_utf8(out).expect("utf8 completion output");
        assert!(
            stdout
                .lines()
                .any(|line| line.starts_with("trace\tForward walk")),
            "fish output must include value-tab-help records: {stdout:?}"
        );
        assert!(
            !stdout.contains("--root") && !stdout.contains("--help"),
            "empty current word must hide flag candidates: {stdout:?}"
        );
    }

    #[test]
    #[serial]
    fn fish_write_complete_omits_help_column_for_value_candidates() {
        let mut cmd = Cli::command();
        let mut out = Vec::new();

        FilteredFish
            .write_complete(
                &mut cmd,
                args(&["ravelact", "trace", "push", "--format", ""]),
                None,
                &mut out,
            )
            .expect("fish completion must succeed");

        let stdout = String::from_utf8(out).expect("utf8 completion output");
        assert!(
            stdout.lines().any(|line| line == "tree"),
            "value candidate without help should be emitted without a tab suffix: {stdout:?}"
        );
    }

    #[test]
    #[serial]
    fn filtered_shell_names_and_matches() {
        assert_eq!(FilteredBash.name(), "bash");
        assert!(FilteredBash.is("bash"));
        assert!(!FilteredBash.is("zsh"));

        assert_eq!(FilteredZsh.name(), "zsh");
        assert!(FilteredZsh.is("zsh"));
        assert!(!FilteredZsh.is("bash"));

        assert_eq!(FilteredFish.name(), "fish");
        assert!(FilteredFish.is("fish"));
        assert!(!FilteredFish.is("bash"));
    }

    #[test]
    fn zsh_escape_value_doubles_backslash_and_escapes_colon() {
        assert_eq!(escape_zsh_value("plain"), "plain");
        assert_eq!(escape_zsh_value("a:b"), "a\\:b");
        assert_eq!(escape_zsh_value("a\\b"), "a\\\\b");
        assert_eq!(escape_zsh_value("a\\b:c"), "a\\\\b\\:c");
    }

    #[test]
    fn zsh_escape_help_doubles_backslash_only() {
        assert_eq!(escape_zsh_help("plain"), "plain");
        assert_eq!(escape_zsh_help("a:b"), "a:b");
        assert_eq!(escape_zsh_help("a\\b"), "a\\\\b");
    }

    /// Option value completion path must not be affected by the filter — trace
    /// `--format` takes a `ValueEnum` (`tree`/`table`/`json`/`markdown`);
    /// none of them start with `-`.
    /// ravelact analog of ofsht's `--color` regression test.
    #[test]
    #[serial]
    fn filtered_candidates_option_value_regression() {
        let mut cmd = Cli::command();
        let result = filtered_candidates(
            &mut cmd,
            args(&["ravelact", "trace", "push", "--format", ""]),
            4,
            None,
        )
        .expect("filtered_candidates must succeed");
        let values = values_of(&result);
        assert!(
            values.iter().any(|v| v == "tree"),
            "tree must be present in {values:?}"
        );
        assert!(
            values.iter().any(|v| v == "table"),
            "table must be present in {values:?}"
        );
        assert!(
            !values.iter().any(|v| v.starts_with("--")),
            "no `--` prefixed value must appear in option-value path: {values:?}"
        );
    }
}
