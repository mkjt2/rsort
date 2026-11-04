#!/usr/bin/env bash
# Write the Homebrew formula for the current version and push it to the tap
# named in cli.toml ([project].homebrew_tap = "OWNER/homebrew-tap").
# Checksums come from dist/ (local build) or the published GitHub release.
#
# Usage: scripts/homebrew.sh [--print]   (--print writes the formula to stdout only)
source "$(dirname "$0")/lib.sh"

print_only=false
[[ "${1:-}" == "--print" ]] && print_only=true

tap="$(project_value homebrew_tap)"
[[ -n "$tap" ]] || die "set [project].homebrew_tap in cli.toml"

sums="$(mktemp)"
if [[ -f dist/SHA256SUMS ]]; then
  cp dist/SHA256SUMS "$sums"
else
  gh release download "v$VERSION" --repo "$REPO_SLUG" --pattern SHA256SUMS --output "$sums" --clobber
fi
sum_for() { awk -v f="$NAME-$VERSION-$1.tar.gz" '$2 == f { print $1 }' "$sums"; }

base="https://github.com/$REPO_SLUG/releases/download/v$VERSION"
class="$(echo "$NAME" | awk -F- '{ for (i = 1; i <= NF; i++) printf "%s", toupper(substr($i,1,1)) substr($i,2) }')"
desc="$(sed -n 's/^description *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"

block() { # arch target
  local s; s="$(sum_for "$2")"
  [[ -n "$s" ]] || return 0
  printf '    on_%s do\n      url "%s/%s-%s-%s.tar.gz"\n      sha256 "%s"\n    end\n' "$1" "$base" "$NAME" "$VERSION" "$2" "$s"
}

formula="$(mktemp)"
{
  echo "class $class < Formula"
  echo "  desc \"$desc\""
  echo "  homepage \"$REPO_URL\""
  echo "  version \"$VERSION\""
  echo "  on_macos do"
  block arm aarch64-apple-darwin
  block intel x86_64-apple-darwin
  echo "  end"
  echo "  on_linux do"
  block arm aarch64-unknown-linux-musl
  block intel x86_64-unknown-linux-musl
  echo "  end"
  echo ""
  echo "  def install"
  echo "    bin.install \"$NAME\""
  echo "    bash_completion.install \"completions/$NAME.bash\" => \"$NAME\" if File.exist?(\"completions/$NAME.bash\")"
  echo "    zsh_completion.install \"completions/_$NAME\" if File.exist?(\"completions/_$NAME\")"
  echo "    fish_completion.install \"completions/$NAME.fish\" if File.exist?(\"completions/$NAME.fish\")"
  echo "    man1.install Dir[\"man/*.1\"]"
  echo "  end"
  echo ""
  echo "  test do"
  echo "    assert_match version.to_s, shell_output(\"#{bin}/$NAME --version\")"
  echo "  end"
  echo "end"
} >"$formula"

if $print_only; then cat "$formula"; exit 0; fi

work="$(mktemp -d)"
gh repo clone "$tap" "$work" -- --depth 1 >/dev/null 2>&1 \
  || die "cannot clone tap $tap (create it with: gh repo create $tap --public)"
mkdir -p "$work/Formula"
cp "$formula" "$work/Formula/$NAME.rb"
git -C "$work" add "Formula/$NAME.rb"
if git -C "$work" diff --cached --quiet; then
  info "homebrew formula already up to date"
else
  git -C "$work" commit -m "$NAME $VERSION" >/dev/null
  git -C "$work" push >/dev/null
  info "pushed $NAME $VERSION to $tap"
fi
