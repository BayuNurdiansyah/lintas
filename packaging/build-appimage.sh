#!/usr/bin/env bash
# Builds a portable AppImage from the release binary.
#
# Requires `linuxdeploy` (https://github.com/linuxdeploy/linuxdeploy) on
# PATH, or it's downloaded here on first run. In a container/CI without
# FUSE, set APPIMAGE_EXTRACT_AND_RUN=1 first.
set -e
cd "$(dirname "$0")/.."
# Skip the build if a binary is already there (e.g. downloaded from a
# previous CI job) — lets this run without a Rust toolchain installed.
if [ ! -x target/release/lintas ]; then
    cargo build --release
fi

APPDIR=target/AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"

LINUXDEPLOY=linuxdeploy-x86_64.AppImage
if ! command -v linuxdeploy >/dev/null && [ ! -f "$LINUXDEPLOY" ]; then
    echo "Downloading linuxdeploy..."
    curl -fsSLo "$LINUXDEPLOY" \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
    chmod +x "$LINUXDEPLOY"
fi
LINUXDEPLOY_BIN=$(command -v linuxdeploy || echo "./$LINUXDEPLOY")

"$LINUXDEPLOY_BIN" --appdir "$APPDIR" \
    --executable target/release/lintas \
    --desktop-file packaging/lintas.desktop \
    --icon-file assets/icons/lintas-256.png \
    --icon-filename lintas \
    --output appimage

echo "Done: lintas-*.AppImage"
echo "Note: this bundles the settings GUI's shared libraries too, but device"
echo "access (/dev/input, /dev/uinput) still needs the one-time setup from"
echo "packaging/install.sh (or its manual equivalent) on the target machine."
