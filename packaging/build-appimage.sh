#!/usr/bin/env bash
# Builds a Linux AppImage of Mem Reduct.
#
# Needs on the build machine:
#   * cargo (Rust)
#   * linuxdeploy x86_64 AppImage, either on PATH or via LINUXDEPLOY env var:
#       https://github.com/linuxdeploy/linuxdeploy/releases
set -euo pipefail

cd "$(dirname "$0")/.."

LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy-x86_64.AppImage}"
if ! command -v "$LINUXDEPLOY" >/dev/null 2>&1; then
  echo "linuxdeploy not found. Download it, e.g.:"
  echo "  curl -LO https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
  echo "  chmod +x linuxdeploy-x86_64.AppImage"
  exit 1
fi

cargo build --release

STAGE="$(pwd)/AppDir"
rm -rf "$STAGE"
install -Dm755 target/release/memreduct "$STAGE/usr/bin/memreduct"
install -Dm644 packaging/memreduct.desktop "$STAGE/usr/share/applications/memreduct.desktop"
install -Dm644 packaging/memreduct.svg    "$STAGE/usr/share/icons/hicolor/scalable/apps/memreduct.svg"

export APPIMAGE_EXTRACT_AND_RUN=1
"$LINUXDEPLOY" \
  --appdir "$STAGE" \
  -d "$STAGE/usr/share/applications/memreduct.desktop" \
  -i "$STAGE/usr/share/icons/hicolor/scalable/apps/memreduct.svg" \
  --output appimage

echo "AppImage written to: $(pwd)/Mem_Reduct-${PKG_VERSION:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}-x86_64.AppImage"
