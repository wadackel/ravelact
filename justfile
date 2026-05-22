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
frontend: frontend-deps
    cd web && pnpm build

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
