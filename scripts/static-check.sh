#!/usr/bin/env bash
# Verify Linux release binaries are fully statically linked.
source "$(dirname "$0")/lib.sh"

targets=()
while IFS= read -r line; do targets+=("$line"); done < <(project_list targets | grep -- '-linux-musl' || true)
[[ ${#targets[@]} -gt 0 ]] || { info "no Linux targets in cli.toml; skipping"; exit 0; }
if ! command -v cargo-zigbuild >/dev/null || ! command -v zig >/dev/null; then
  info "skipping static-link check: zig + cargo-zigbuild not installed (needed only for releases)"
  exit 0
fi

for target in "${targets[@]}"; do
  info "building $target"
  build_target "$target" >/dev/null 2>&1 || build_target "$target"
  bin="target/$target/release/$NAME"
  desc="$(file -b "$bin")"
  if [[ "$desc" == *"statically linked"* || "$desc" == *"static-pie linked"* ]]; then
    info "$target: static ✓"
  else
    die "$target is not statically linked: $desc"
  fi
done
