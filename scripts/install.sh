#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <version|vX.Y.Z> [install-dir]" >&2
  exit 2
fi

VERSION_INPUT="$1"
INSTALL_DIR="${2:-/usr/local/bin}"
REPO="${SMARTDUP_REPO:-oleksandrskoruk/smart-dup}"

if [[ "$VERSION_INPUT" == v* ]]; then
  TAG="$VERSION_INPUT"
else
  TAG="v$VERSION_INPUT"
fi

OS="$(uname -s)"
ARCH="$(uname -m)"
TARGET=""

case "$OS" in
  Darwin)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin" ;;
      arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
      *)
        echo "unsupported macOS architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64) TARGET="x86_64-unknown-linux-musl" ;;
      *)
        echo "unsupported Linux architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "unsupported OS: $OS (use scripts/install.ps1 on Windows)" >&2
    exit 1
    ;;
esac

ARCHIVE="smart-dup-${TAG}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "downloading ${URL}"
curl -fsSL "$URL" -o "$TMP_DIR/${ARCHIVE}"
tar -xzf "$TMP_DIR/${ARCHIVE}" -C "$TMP_DIR"

BIN_PATH="$(find "$TMP_DIR" -type f -name smart-dup | head -n 1)"
if [[ -z "${BIN_PATH}" ]]; then
  echo "smart-dup binary not found in archive" >&2
  exit 1
fi

if [[ -w "$INSTALL_DIR" ]]; then
  install -m 0755 "$BIN_PATH" "$INSTALL_DIR/smart-dup"
else
  sudo install -m 0755 "$BIN_PATH" "$INSTALL_DIR/smart-dup"
fi

echo "installed smart-dup to $INSTALL_DIR/smart-dup"
echo "run: smart-dup --help"
