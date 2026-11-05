#!/usr/bin/env bash
# Build release archives for every cli.toml target this host can build.
#
#   dist/<name>-<version>-<target>.tar.gz   (binary, completions, man pages, docs)
#   dist/<archive>.sha256 and dist/SHA256SUMS
#
# Usage: scripts/dist.sh [--require-all]
source "$(dirname "$0")/lib.sh"

require_all=false
[[ "${1:-}" == "--require-all" ]] && require_all=true

targets=()
while IFS= read -r line; do targets+=("$line"); done < <(project_list targets)
[[ ${#targets[@]} -gt 0 ]] || die "no targets in cli.toml [project].targets"

rm -rf dist && mkdir -p dist/stage

# Completions and man pages come from a native build: they don't depend on target.
info "generating completions and man pages"
cargo build --locked --release
host_bin="target/release/$NAME"
extras="dist/stage/extras"
mkdir -p "$extras/completions" "$extras/man"
"$host_bin" --generate complete-bash >"$extras/completions/$NAME.bash"
"$host_bin" --generate complete-zsh >"$extras/completions/_$NAME"
"$host_bin" --generate complete-fish >"$extras/completions/$NAME.fish"
# A single-command CLI has `--generate man`. Don't probe `man --help` first:
# a CLI that takes file operands reads `man` as a file and still exits 0.
if ! "$host_bin" --generate man >"$extras/man/$NAME.1" 2>/dev/null; then
  rm -f "$extras/man/$NAME.1"
  "$host_bin" man --out-dir "$extras/man" # one page per subcommand
fi

built=()
for target in "${targets[@]}"; do
  if ! can_build "$target"; then
    $require_all && die "this host cannot build $target (macOS targets need a Mac; Linux targets need zig + cargo-zigbuild). Remove it from [project].targets in cli.toml, or release from a host that can build it"
    info "skipping $target (not buildable on this host)"
    continue
  fi
  info "building $target"
  build_target "$target"
  built+=("$target")
done

package() {
  local target="$1" bin="$2"
  local dir="$NAME-$VERSION-$target"
  mkdir -p "dist/stage/$dir"
  cp "$bin" "dist/stage/$dir/$NAME"
  cp -R "$extras/completions" "$extras/man" "dist/stage/$dir/"
  for f in README.md CHANGELOG.md LICENSE* NOTICE; do [[ -e "$f" ]] && cp "$f" "dist/stage/$dir/"; done
  tar -C dist/stage -czf "dist/$dir.tar.gz" "$dir"
  info "packaged dist/$dir.tar.gz"
}

for target in ${built[@]+"${built[@]}"}; do
  package "$target" "target/$target/release/$NAME"
done

cp install.sh dist/ 2>/dev/null || true
(
  cd dist
  : >SHA256SUMS
  for f in *.tar.gz; do
    sum="$(sha256 "$f")"
    echo "$sum  $f" >"$f.sha256"
    echo "$sum  $f" >>SHA256SUMS
  done
)
rm -rf dist/stage
info "dist/ ready: ${#built[@]} target(s)"
