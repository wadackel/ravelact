default:
    @just --list

format:
    cargo fmt --all
    cd web && pnpm format

lint:
    cargo clippy --all-targets -- -D warnings
    cd web && pnpm lint && pnpm format:check

lint-actions:
    actionlint
    zizmor --offline .github/workflows .github/actions action.yaml

test:
    cargo test

build:
    cargo build

# Install web/ dependencies only. Use this when only `node_modules` is
# needed (e.g. `just format`, which invokes `vp fmt`) but `web/dist/` is
# not required.
frontend-deps:
    cd web && pnpm install --frozen-lockfile

# Build the web frontend (deps + vite build) and emit web/dist/.
# rust-embed in src/cli/render/browse.rs reads web/dist/, so this must run
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

coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    cargo llvm-cov report

# Enforce the per-file >= 90% line coverage floor against lcov.info.
# Mirrors the CI gate's intent so contributors can reproduce the verdict
# locally before pushing. Strips the SF: prefix and the repo root so the
# error output uses repo-relative paths.
coverage-gate: coverage
    @awk -v root="$PWD/" 'BEGIN { th=90 } \
      /^SF:/ { p=substr($0,4); if (index(p,root)==1) p=substr(p,length(root)+1); sf=p } \
      /^LF:/ { lf=substr($0,4)+0 } \
      /^LH:/ { lh=substr($0,4)+0 } \
      /^end_of_record/ { pct = lf>0 ? lh*100/lf : 100; if (pct + 0 < th) { printf "%s: %.2f%% below %d%%\n", sf, pct, th; failed=1 }; sf=""; lf=0; lh=0 } \
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
