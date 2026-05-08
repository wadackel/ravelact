#!/usr/bin/env bash
#
# check-skill-drift.sh — guard against drift between the ravelact CLI surface
# and skills/ravelact/SKILL.md. Invoked by the `skills` job in
# .github/workflows/ci.yaml after `cargo build --locked` so that
# ./target/debug/ravelact is available.
#
# Checks (Codex-recommended priority order):
#   1. Subcommand inventory — every name printed by `ravelact --help` (under
#      the Inspect/Check/Suggest/Export/Other groups, excluding `help`) must
#      have its own `#### `<cmd>`` (or `#### `<cmd> ...``) section heading in
#      skills/ravelact/SKILL.md. We require an actual heading rather than a
#      substring match because common subcommand names ("build", "dump",
#      "extract") routinely appear in incidental prose, which would let a
#      missing section slip through a substring check.
#   2. Group labels — Inspect / Check / Suggest / Export must appear so the
#      exit-code policy narrative stays in sync with src/cli/mod.rs.
#   3. Frontmatter sanity — name == directory name; line count < 500
#      (agentskills.io progressive-disclosure recommendation).
#
# `gh skill publish --dry-run` covers the rest of the agentskills.io spec
# validation; this script intentionally does not duplicate it.
#
# Known limitation: the reverse direction (SKILL.md mentions a subcommand-shaped
# token that the CLI does not export — e.g. a stale `lint` reference left over
# after the lint→wiring rename) is NOT enforced. A token-based check produced
# too many false positives from prose backtick-quoted identifiers (`gh`, `act`,
# `subgraph`, etc.). When a rename lands, reviewers must grep SKILL.md for the
# old name during PR review.
#
# Exit 0 on success, 1 on any drift (with the cause printed to stderr).

set -euo pipefail

SKILL_DIR="skills/ravelact"
SKILL_FILE="$SKILL_DIR/SKILL.md"
BIN="./target/debug/ravelact"

if [[ ! -f "$SKILL_FILE" ]]; then
  echo "drift: $SKILL_FILE not found" >&2
  exit 1
fi

if [[ ! -x "$BIN" ]]; then
  echo "drift: $BIN not built — run \`cargo build --locked\` first" >&2
  exit 1
fi

# ---- 1. Subcommand inventory -------------------------------------------------
#
# The `ravelact --help` output uses a custom help_template (see
# src/cli/mod.rs) that lists subcommands under group headings, two-space
# indented:
#
#   Inspect (exit 0; non-blocking reports):
#     trace        Forward walk from a trigger event ...
#
# We accept lines that begin with at least two spaces followed by an identifier
# starting with a lowercase letter, then whitespace and a description. This
# matches the help_template format precisely without depending on a stable
# column alignment.
mapfile -t cli_cmds < <(
  "$BIN" --help \
    | awk '/^  [a-z][a-z0-9-]*[[:space:]]+[A-Z]/ { print $1 }' \
    | sort -u
)

if [[ ${#cli_cmds[@]} -eq 0 ]]; then
  echo "drift: failed to parse any subcommand from \`$BIN --help\`" >&2
  echo "       (help_template format may have changed; update this script)" >&2
  exit 1
fi

missing=()
for cmd in "${cli_cmds[@]}"; do
  # `help` is a clap-injected entry, never documented in user-facing skill body.
  [[ "$cmd" == "help" ]] && continue
  # Require a dedicated `#### `cmd`` heading (cmd may be followed by either a
  # closing backtick — bare verb form — or a space — verb-with-args form).
  if ! grep -qE "^#### \`${cmd}(\`| )" "$SKILL_FILE"; then
    missing+=("$cmd")
  fi
done

if [[ ${#missing[@]} -gt 0 ]]; then
  echo "drift: SKILL.md missing dedicated section heading for: ${missing[*]}" >&2
  echo "       (each subcommand needs a \`#### \\\`<cmd>\\\`\` or \`#### \\\`<cmd> ...\\\`\` heading)" >&2
  exit 1
fi

# ---- 2. Group labels --------------------------------------------------------
for label in Inspect Check Suggest Export; do
  if ! grep -qF "$label" "$SKILL_FILE"; then
    echo "drift: SKILL.md missing group label: $label" >&2
    exit 1
  fi
done

# ---- 3. Frontmatter sanity --------------------------------------------------
if ! grep -qE '^name: ravelact$' "$SKILL_FILE"; then
  echo "drift: frontmatter \`name:\` does not equal directory name (ravelact)" >&2
  exit 1
fi

lines=$(wc -l <"$SKILL_FILE" | tr -d '[:space:]')
if (( lines >= 500 )); then
  echo "drift: SKILL.md has $lines lines (>=500). Move detail into references/ to honour agentskills.io progressive disclosure." >&2
  exit 1
fi

echo "skill drift check OK ($lines lines, ${#cli_cmds[@]} subcommands)"
