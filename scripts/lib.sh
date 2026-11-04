#!/usr/bin/env bash
# Shared helpers for release scripts. Source, don't execute.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*" >&2; }

# Package name and version from Cargo.toml.
NAME="$(sed -n 's/^name *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
REPO_URL="$(sed -n 's/^repository *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
REPO_SLUG="${REPO_URL#https://github.com/}"

# Read a TOML string array (single- or multi-line) from cli.toml's [project] table.
project_list() {
  awk -v key="$1" '
    /^\[/ { in_project = ($0 == "[project]") }
    in_project && $0 ~ "^"key" *= *\\[" { collecting = 1 }
    collecting { buf = buf $0; if ($0 ~ /\]/) { collecting = 0 } }
    END {
      sub(/^[^\[]*\[/, "", buf); sub(/\].*$/, "", buf)
      n = split(buf, parts, ",")
      for (i = 1; i <= n; i++) { gsub(/[ "\t]/, "", parts[i]); if (parts[i] != "") print parts[i] }
    }' cli.toml
}

# Read a scalar from cli.toml's [project] table.
project_value() {
  awk -v key="$1" '
    /^\[/ { in_project = ($0 == "[project]") }
    in_project && $0 ~ "^"key" *= *" { sub(/^[^=]*= */, ""); sub(/ *#.*$/, ""); gsub(/"/, ""); print; exit }' cli.toml
}

# Print the body of CHANGELOG.md's `## [Unreleased]` section.
unreleased_notes() {
  awk '/^## \[Unreleased\]/ { on = 1; next } /^## / { on = 0 } on' CHANGELOG.md
}

host_os() { uname -s | tr '[:upper:]' '[:lower:]'; }

# Can this host build TARGET?
can_build() {
  case "$1" in
    *-linux-musl) command -v cargo-zigbuild >/dev/null ;;
    *-apple-darwin) [[ "$(host_os)" == "darwin" ]] ;;
    *) return 1 ;;
  esac
}

build_target() {
  local target="$1"
  case "$target" in
    *-linux-musl) cargo zigbuild --locked --release --target "$target" ;;
    *-apple-darwin) cargo build --locked --release --target "$target" ;;
    *) die "unsupported target $target" ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
