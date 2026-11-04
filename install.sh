#!/bin/sh
# Install rsort from GitHub Releases.
#
#   curl -fsSL https://github.com/mkjt2/rsort/releases/latest/download/install.sh | sh
#
# Environment:
#   RSORT_VERSION      version to install (default: latest)
#   RSORT_INSTALL_DIR  install directory (default: ~/.local/bin)
set -eu

REPO="mkjt2/rsort"
NAME="rsort"
VERSION="${RSORT_VERSION:-latest}"
INSTALL_DIR="${RSORT_INSTALL_DIR:-$HOME/.local/bin}"

die() { echo "install: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "requires $1"; }
need curl
need tar
need uname

case "$(uname -s)" in
  Linux) os="unknown-linux-musl" ;;
  Darwin) os="apple-darwin" ;;
  *) die "unsupported OS $(uname -s) (Linux and macOS only)" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) die "unsupported architecture $(uname -m)" ;;
esac
target="$arch-$os"

fetch() { curl -fsSL "$@"; }

if [ "$VERSION" = "latest" ]; then
  VERSION="$(fetch "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)"
  [ -n "$VERSION" ] || die "could not determine the latest version"
fi
VERSION="${VERSION#v}"

archive="$NAME-$VERSION-$target.tar.gz"
url="https://github.com/$REPO/releases/download/v$VERSION/$archive"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $NAME $VERSION for $target..." >&2
fetch -o "$tmp/$archive" "$url" || die "download failed: $url"
fetch -o "$tmp/$archive.sha256" "$url.sha256" || die "checksum download failed"

expected="$(cut -d' ' -f1 <"$tmp/$archive.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$archive" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $archive"

tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/$NAME-$VERSION-$target/$NAME" "$INSTALL_DIR/$NAME"
echo "Installed $NAME $VERSION to $INSTALL_DIR/$NAME" >&2

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Note: $INSTALL_DIR is not on your PATH." >&2 ;;
esac
