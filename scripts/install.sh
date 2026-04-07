#!/usr/bin/env sh
set -eu

REPO="${SYFT_RELEASE_REPO:-chaqchase/syft}"
INSTALL_DIR="${SYFT_INSTALL_DIR:-$HOME/.local/bin}"
VERSION_INPUT="${1:-}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

latest_tag() {
  need_cmd curl
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | head -n 1 \
    | sed -E 's/.*"([^"]+)".*/\1/'
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
        *)
          echo "unsupported Linux architecture: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64|amd64) echo "x86_64-apple-darwin" ;;
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        *)
          echo "unsupported macOS architecture: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "use scripts/install.ps1 on Windows" >&2
      exit 1
      ;;
    *)
      echo "unsupported operating system: $os" >&2
      exit 1
      ;;
  esac
}

need_cmd tar

VERSION_TAG="${VERSION_INPUT:-$(latest_tag)}"
case "$VERSION_TAG" in
  v*) VERSION="${VERSION_TAG#v}" ;;
  *) VERSION="$VERSION_TAG"; VERSION_TAG="v$VERSION_TAG" ;;
esac

TARGET="$(detect_target)"
ASSET="syft-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION_TAG}/${ASSET}"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "downloading $URL"
curl -fsSL "$URL" -o "$TMP_DIR/$ASSET"
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
cp "$TMP_DIR/syft-${VERSION}-${TARGET}/syft" "$INSTALL_DIR/syft"
chmod +x "$INSTALL_DIR/syft"

echo "installed syft to $INSTALL_DIR/syft"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo "add $INSTALL_DIR to PATH if it is not there already"
    ;;
esac
