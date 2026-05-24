default:
    @just --list

format:
    cargo fmt --all
    cd web && pnpm format

lint:
    cargo clippy --all-targets -- -D warnings
    cd web && pnpm lint && pnpm format:check
    buf lint

lint-actions:
    actionlint
    zizmor --offline .github/workflows .github/actions action.yaml

test:
    cargo test

build:
    cargo build

# Codegen plugin existence check. Used by `proto-gen` and
# `proto-check-drift` so a missing plugin produces a clear install
# hint instead of a cryptic buf failure. The Rust plugins are
# `cargo install`-only (not in nixpkgs); `protoc-gen-es` ships via
# the SPA's npm devDependency.
[private]
proto-plugins-check:
    #!/usr/bin/env bash
    set -euo pipefail
    missing=()
    for bin in protoc-gen-buffa protoc-gen-buffa-packaging protoc-gen-connect-rust; do
      command -v "$bin" >/dev/null 2>&1 || missing+=("$bin")
    done
    if [ ${#missing[@]} -gt 0 ]; then
      echo "missing codegen plugin(s): ${missing[*]}" >&2
      echo "install with: cargo install --locked protoc-gen-buffa protoc-gen-buffa-packaging connectrpc-codegen" >&2
      exit 1
    fi
    # protoc-gen-es lives in web/node_modules/.bin/ after `pnpm install`.
    if [ ! -x web/node_modules/.bin/protoc-gen-es ]; then
      echo "missing web/node_modules/.bin/protoc-gen-es" >&2
      echo "install with: cd web && pnpm install" >&2
      exit 1
    fi

# Regenerate vendored Rust + TS code from proto/. Run after editing any
# .proto file. Plugin discovery merges $PATH with the SPA's
# node_modules/.bin so `protoc-gen-es` is found without a global install.
#
# `cargo fmt` runs immediately afterwards because rustfmt is stable
# and reformats `#[allow(...)]` lists from buffa-codegen's single-line
# output into multi-line form. Without this post-fmt step, `just format`
# and `just proto-check-drift` would disagree about the canonical
# shape of the generated Rust files. Stable rustfmt has no per-file
# ignore directive, so we make the post-fmt form the canonical form.
proto-gen: proto-plugins-check
    PATH="$PWD/web/node_modules/.bin:$PATH" buf generate
    cargo fmt --all

# Lint .proto files against buf's STANDARD ruleset.
proto-lint:
    buf lint

# Fail when vendored generated code is out of sync with proto/. Mirrors
# the CI drift job so contributors can reproduce the verdict locally.
proto-check-drift: proto-gen
    git diff --exit-code -- src/cli/render/browse/proto src/cli/render/browse/connect web/src/proto

# Install web/ dependencies only. Use this when only `node_modules` is
# needed (e.g. `just format`, which invokes `vp fmt`) but `web/dist/` is
# not required.
frontend-deps:
    cd web && pnpm install --frozen-lockfile

# Build the web frontend (deps + vite build) and emit web/dist/.
# rust-embed in src/cli/render/browse/mod.rs reads web/dist/, so this must run
# before any `cargo build` that needs the browse subcommand to serve assets
# at runtime. Dev workflow uses `pnpm dev` instead (see README).
#
# `touch web/dist/.gitkeep` re-creates the rust-embed folder-existence
# placeholder that vite's default `emptyOutDir: true` removes during the
# build. The placeholder is git-tracked so release-plz's cargo package
# verify (which copies tracked files only into a temp worktree) sees a
# non-missing `web/dist/`.
frontend: frontend-deps
    cd web && pnpm build
    touch web/dist/.gitkeep

# Canonical release path: build the frontend first so rust-embed has
# web/dist/ to bundle, then build the Rust binary. CI invokes this.
build-release: frontend
    cargo build --release --locked

install:
    cargo install --path . --locked

bench:
    cargo bench

# `--ignore-filename-regex` strips the vendored ConnectRPC + buffa
# codegen output from both the lcov export and the printed report.
# Those files are `// @generated` and exempt from the per-file
# coverage floor (CLAUDE.md "Intentional Conventions").
coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info --ignore-filename-regex 'src/cli/render/browse/(proto|connect)/'
    cargo llvm-cov report --ignore-filename-regex 'src/cli/render/browse/(proto|connect)/'

# Enforce the per-file >= 90% line coverage floor against lcov.info.
# Mirrors the CI gate's intent so contributors can reproduce the verdict
# locally before pushing. Strips the SF: prefix and the repo root so the
# error output uses repo-relative paths.
coverage-gate: coverage
    @awk -v root="$PWD/" 'BEGIN { th=90 } \
      /^SF:/ { p=substr($0,4); if (index(p,root)==1) p=substr(p,length(root)+1); sf=p } \
      /^LF:/ { lf=substr($0,4)+0 } \
      /^LH:/ { lh=substr($0,4)+0 } \
      /^end_of_record/ { \
        pct = lf>0 ? lh*100/lf : 100; \
        if (pct + 0 < th) { printf "%s: %.2f%% below %d%%\n", sf, pct, th; failed=1 }; \
        sf=""; lf=0; lh=0 \
      } \
      END { if (failed) exit 1 }' lcov.info

clean:
    cargo clean

# Launch `ravelact browse` against a freshly generated synthetic estate
# of N workflows (default 300) so you can manually feel the high-scale
# UX. Requires `just build-release` first to produce the binary. The
# tempdir is cleaned up on Ctrl+C. Examples:
#   just dev-synthetic                  # 300 workflows, auto-open browser
#   just dev-synthetic 1000             # 1000 workflows
#   just dev-synthetic 300 --no-open    # suppress browser auto-open
#   just dev-synthetic 300 --port 7878  # pin port
dev-synthetic *args:
    deno run --allow-all script/spawn-synthetic-browse.ts {{args}}

# Regenerate the browse screenshots committed under docs/images/. Runs
# web/scripts/snapshot-readme.ts (Node + tsx + @playwright/test) against
# `target/release/ravelact` and rewrites docs/images/browse-*.png.
#
# Requires:
#   - the release binary (`just build-release`)
#   - Playwright Chromium in the global cache (once:
#     `cd web && nix develop -c pnpm exec playwright install chromium`)
#
# The script runs `oxipng -o 4 --strip safe` on each emitted PNG and exits
# non-zero if the regenerated PNGs differ from the committed files. Pass
# `--update` to commit new bytes:
#   just snapshot-readme            # verify only (fails on drift)
#   just snapshot-readme --update   # rewrite committed PNGs
#
# Must not run concurrently with `pnpm e2e` — both may bind localhost ports.
snapshot-readme *args:
    cd web && pnpm exec tsx scripts/snapshot-readme.ts {{args}}
