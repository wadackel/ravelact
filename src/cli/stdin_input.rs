//! Resolve positional input lists for commands that accept a path / target list,
//! honoring the rg / fd / grep / cat convention: read stdin when stdin is not a
//! TTY and no positional inputs are given, OR when `-` is supplied as a
//! positional input. See issue #75 for the activation table.

use anyhow::{anyhow, Result};
use std::io::{self, IsTerminal, Read};

pub(super) const NO_INPUT_MSG: &str = "no input: provide files as args or pipe via stdin";

/// Parse stdin bytes into a trimmed, NUL-rejecting line list.
///
/// UTF-8 decoded, split on `\n`, each line trimmed (handles CRLF — `\r` is a
/// Unicode whitespace), empty lines dropped. Any line containing `\0` is
/// rejected so silent mishandling of `git diff -z` style output cannot occur.
fn parse_lines(input: &[u8]) -> Result<Vec<String>> {
    let text = std::str::from_utf8(input).map_err(|e| anyhow!("stdin is not valid UTF-8: {e}"))?;
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.as_bytes().contains(&0) {
            return Err(anyhow!(
                "stdin line contains null byte; NUL-separated input is not supported"
            ));
        }
        out.push(line.to_string());
    }
    Ok(out)
}

/// Pure routing per the issue #75 activation table.
fn route(args: &[String], stdin_is_tty: bool, lines: &[String]) -> Result<Vec<String>> {
    let dash_count = args.iter().filter(|a| a.as_str() == "-").count();
    if dash_count > 0 {
        if stdin_is_tty {
            return Err(anyhow!("'-' requires piped stdin"));
        }
        let mut out = Vec::with_capacity(args.len() - dash_count + lines.len() * dash_count);
        for a in args {
            if a == "-" {
                out.extend(lines.iter().cloned());
            } else {
                out.push(a.clone());
            }
        }
        return Ok(out);
    }
    if !args.is_empty() {
        return Ok(args.to_vec());
    }
    if stdin_is_tty {
        return Err(anyhow!(NO_INPUT_MSG));
    }
    if lines.is_empty() {
        return Err(anyhow!(NO_INPUT_MSG));
    }
    Ok(lines.to_vec())
}

fn read_stdin() -> Result<Vec<String>> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    parse_lines(&buf)
}

/// Resolve the final positional input list for a CLI command. Reads stdin
/// only when the routing rule actually consumes it (args empty or `-`
/// present). Args with no `-` are returned verbatim regardless of pipe state.
pub(super) fn collect(args: &[String]) -> Result<Vec<String>> {
    let stdin_is_tty = io::stdin().is_terminal();
    let needs_stdin = args.is_empty() || args.iter().any(|a| a.as_str() == "-");
    let lines: Vec<String> = if needs_stdin && !stdin_is_tty {
        read_stdin()?
    } else {
        Vec::new()
    };
    route(args, stdin_is_tty, &lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_lines -----------------------------------------------------

    #[test]
    fn parse_lines_basic() {
        let out = parse_lines(b"a\nb\nc\n").unwrap();
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_lines_drops_empty() {
        let out = parse_lines(b"a\n\nb\n").unwrap();
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn parse_lines_trims_ascii() {
        let out = parse_lines(b"  a  \n\tb\t\n").unwrap();
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn parse_lines_handles_crlf() {
        let out = parse_lines(b"a\r\nb\r\n").unwrap();
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn parse_lines_rejects_nul_byte() {
        let err = parse_lines(b"a\nb\0c\n").unwrap_err().to_string();
        assert!(
            err.contains("null byte"),
            "expected null-byte error, got: {err}"
        );
    }

    // --- route -----------------------------------------------------------

    #[test]
    fn route_empty_args_tty_errors() {
        let err = route(&[], true, &[]).unwrap_err().to_string();
        assert_eq!(err, NO_INPUT_MSG);
    }

    #[test]
    fn route_empty_args_nontty_uses_lines() {
        let lines = vec!["x".to_string(), "y".to_string()];
        let out = route(&[], false, &lines).unwrap();
        assert_eq!(out, lines);
    }

    #[test]
    fn route_empty_args_nontty_empty_lines_errors() {
        let err = route(&[], false, &[]).unwrap_err().to_string();
        assert_eq!(
            err, NO_INPUT_MSG,
            "must match TTY-empty-args error verbatim"
        );
    }

    #[test]
    fn route_args_no_dash_uses_args() {
        let args = vec!["a".to_string(), "b".to_string()];
        let ignored = vec!["should-not-appear".to_string()];
        let out_pipe = route(&args, false, &ignored).unwrap();
        let out_tty = route(&args, true, &ignored).unwrap();
        assert_eq!(out_pipe, args);
        assert_eq!(out_tty, args);
    }

    #[test]
    fn route_dash_with_lines() {
        let args = vec!["-".to_string(), "extra".to_string()];
        let lines = vec!["x".to_string(), "y".to_string()];
        let out = route(&args, false, &lines).unwrap();
        assert_eq!(out, vec!["x", "y", "extra"]);
    }

    #[test]
    fn route_dash_at_tty_errors() {
        let args = vec!["-".to_string()];
        let err = route(&args, true, &[]).unwrap_err().to_string();
        assert!(
            err.contains("requires piped stdin"),
            "expected 'requires piped stdin' error, got: {err}"
        );
    }
}
