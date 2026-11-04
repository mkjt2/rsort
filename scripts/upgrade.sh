#!/usr/bin/env bash
# Upgrade the Rust toolchain pin and every dependency to the latest stable
# release, then run the full CI suite. Review the diff before committing.
# rust-version in Cargo.toml follows the toolchain pin.
source "$(dirname "$0")/lib.sh"

latest_rust="$(curl -fsSL https://static.rust-lang.org/dist/channel-rust-stable.toml \
  | awk '/^\[pkg.rustc\]/ { getline; sub(/^version = "/, ""); sub(/ .*/, ""); print; exit }')"
[[ -n "$latest_rust" ]] || die "could not determine the latest stable Rust"
info "rust toolchain -> $latest_rust"
perl -pi -e "s/^channel = \"[^\"]*\"/channel = \"$latest_rust\"/" rust-toolchain.toml
perl -pi -e "s/^rust-version = \"[^\"]*\"/rust-version = \"${latest_rust%.*}\"/" Cargo.toml

latest_crate() {
  curl -fsSL -A "$NAME-upgrade-script" "https://crates.io/api/v1/crates/$1" \
    | sed -n 's/.*"max_stable_version":"\([^"]*\)".*/\1/p' | head -1
}

# Rewrite `name = "x.y.z"` and `name = { version = "x.y.z", ... }` lines.
deps="$(awk '/^\[(dev-|build-)?dependencies\]/ { on = 1; next } /^\[/ { on = 0 } on && /^[a-zA-Z0-9_-]+ *=/ { print $1 }' Cargo.toml)"
for dep in $deps; do
  v="$(latest_crate "$dep")"
  [[ -n "$v" ]] || { info "skip $dep (lookup failed)"; continue; }
  perl -pi -e "s/^(\Q$dep\E *= *\")[^\"]+(\")/\${1}$v\${2}/; s/^(\Q$dep\E *= *\{.*?version *= *\")[^\"]+(\")/\${1}$v\${2}/" Cargo.toml
  info "$dep -> $v"
  sleep 1 # crates.io asks API clients for at most one request per second
done

cargo update
info "running just ci"
just ci
