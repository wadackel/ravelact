default:
    @just --list

format:
    cargo fmt --all

lint:
    cargo clippy --all-targets -- -D warnings

lint-actions:
    actionlint

test:
    cargo test

build:
    cargo build

build-release:
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
