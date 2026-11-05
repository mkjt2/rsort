# Local CI and release tasks. `just ci` is the canonical CI: it runs fully
# locally and does not depend on GitHub Actions.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List recipes
default:
    @just --list

# Everything CI checks: format, lints, tests + coverage gate, static linking
ci: fmt-check lint cov static-check

# Format the code
fmt:
    cargo fmt

# Fail if the code is not formatted
fmt-check:
    cargo fmt --check

# Clippy with warnings as errors
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Run the test suite (snapshots must already match)
test *args:
    INSTA_UPDATE=no cargo test --locked {{ args }}

# Tests with coverage; fails below 90% line coverage
cov:
    INSTA_UPDATE=no cargo llvm-cov --locked --fail-under-lines 90 --summary-only

# HTML coverage report in target/llvm-cov/html
cov-html:
    INSTA_UPDATE=no cargo llvm-cov --locked --html

# Accept changed --help snapshots (review the diff with `git diff tests/snapshots`)
snapshots:
    INSTA_UPDATE=always cargo test --locked --test help

# Benchmark rsort against the system sort; ASCII report on stdout (`just bench --help`)
bench *args:
    cargo build --locked --release
    scripts/bench.py {{ args }}

# Verify Linux musl release binaries are fully static (skipped without zig)
static-check:
    scripts/static-check.sh

# Build release archives for every target this host can build
dist *args:
    scripts/dist.sh {{ args }}

# Release VERSION: tag, build all targets, publish to GitHub (see scripts/release.sh)
release *args:
    scripts/release.sh {{ args }}

# Update the Homebrew tap for the current version
homebrew *args:
    scripts/homebrew.sh {{ args }}

# Upgrade toolchain and dependencies to latest stable, then run CI
upgrade:
    scripts/upgrade.sh

# Install the binary from source into ~/.cargo/bin
install:
    cargo install --locked --path .
