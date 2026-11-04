#!/usr/bin/env bash
# Cut a release from this machine.
#
# Usage: scripts/release.sh [--dry-run] VERSION
#
# Before running, describe the changes under `## [Unreleased]` in CHANGELOG.md.
#
# Steps:
#   1. Preconditions: valid new VERSION, clean tree, gh authenticated, release
#      tools installed, a non-empty [Unreleased] changelog section, `just ci` passes.
#   2. Set VERSION in Cargo.toml + Cargo.lock, date the changelog section,
#      commit, tag vVERSION.
#   3. Build archives for every target (scripts/dist.sh --require-all), push,
#      create the GitHub release with the archives and install.sh.
#   4. If cli.toml sets [project].homebrew_tap, update the formula in the tap.
#
# --dry-run checks the preconditions and prints the steps without changing anything.
source "$(dirname "$0")/lib.sh"

dry_run=false
if [[ "${1:-}" == "--dry-run" ]]; then dry_run=true; shift; fi
run() { if $dry_run; then echo "+ $*" >&2; else "$@"; fi; }

next="${1:-}"
next="${next#v}"
[[ -n "$next" ]] || die "usage: just release VERSION   (current version: $VERSION)"
[[ "$next" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "invalid version '$next' (expected e.g. 1.2.0)"
[[ "$next" != "$VERSION" ]] || die "version is already $VERSION; pick the next one"
if git rev-parse -q --verify "refs/tags/v$next" >/dev/null; then die "tag v$next already exists"; fi

for tool in gh cargo-zigbuild zig; do
  command -v "$tool" >/dev/null || die "$tool is required for releases (see README: Development)"
done
$dry_run || [[ -z "$(git status --porcelain)" ]] || die "working tree is not clean"
$dry_run || gh auth status >/dev/null 2>&1 || die "run 'gh auth login' first"

notes="$(mktemp)"
unreleased_notes >"$notes"
grep -q '[^[:space:]]' "$notes" \
  || die "CHANGELOG.md has nothing under '## [Unreleased]'; describe the changes first"

tap="$(project_value homebrew_tap)"
info "releasing $NAME $VERSION -> $next${tap:+ (homebrew: $tap)}"

info "running just ci"
run just ci

info "bumping version and dating the changelog"
run perl -0pi -e "s/^version = \"[^\"]*\"/version = \"$next\"/m" Cargo.toml
run cargo update --workspace --offline
run perl -0pi -e "s/^## \\[Unreleased\\]\\n/## [Unreleased]\\n\\n## [$next] - $(date +%Y-%m-%d)\\n/m" CHANGELOG.md
run git add Cargo.toml Cargo.lock CHANGELOG.md
run git commit -m "release: v$next"
run git tag -a "v$next" -m "v$next"
VERSION="$next"

info "building release archives"
run "$(dirname "$0")/dist.sh" --require-all
run git push --follow-tags
run gh release create "v$next" dist/*.tar.gz dist/*.sha256 dist/SHA256SUMS dist/install.sh \
  --title "v$next" --notes-file "$notes"

if [[ -n "$tap" ]]; then
  run "$(dirname "$0")/homebrew.sh"
fi

info "released v$next"
