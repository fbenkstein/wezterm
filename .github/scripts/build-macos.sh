#!/bin/bash
# Build wezterm for macOS and package it as a universal-binary .app zip.
#
# Assumes: cargo with x86_64-apple-darwin and aarch64-apple-darwin targets,
#          lipo, tic (ncurses), zip, codesign.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target"
TAG_NAME="${TAG_NAME:-$(git -C "$REPO_ROOT" -c core.abbrev=8 show -s --format=%cd-%h --date=format:%Y%m%d-%H%M%S)}"

BINS=(wezterm wezterm-gui wezterm-mux-server strip-ansi-escapes)
TARGETS=(x86_64-apple-darwin aarch64-apple-darwin)

cd "$REPO_ROOT"

# One cargo invocation per target with all packages — avoids repeated
# dependency-graph resolution and lets cargo schedule across the package set.
PKG_ARGS=()
for bin in "${BINS[@]}"; do
    PKG_ARGS+=(-p "$bin")
done
for target in "${TARGETS[@]}"; do
    cargo build --release --locked --target "$target" "${PKG_ARGS[@]}"
done

ZIPDIR="$REPO_ROOT/WezTerm-macos-$TAG_NAME"
ZIPFILE="$ZIPDIR.zip"
APP="$ZIPDIR/WezTerm.app"

rm -rf "$ZIPDIR" "$ZIPFILE"
mkdir "$ZIPDIR"
cp -r assets/macos/WezTerm.app "$ZIPDIR/"
# MetalANGLE omitted: CGL is preferred; on Apple Silicon, CGL is Metal-backed anyway
rm "$APP/"*.dylib
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp -r assets/shell-integration/* "$APP/Contents/Resources/"
cp -r assets/shell-completion "$APP/Contents/Resources/"
tic -xe wezterm -o "$APP/Contents/Resources/terminfo" termwiz/data/wezterm.terminfo

for bin in "${BINS[@]}"; do
    lipo \
        "$TARGET_DIR/x86_64-apple-darwin/release/$bin" \
        "$TARGET_DIR/aarch64-apple-darwin/release/$bin" \
        -create -output "$APP/Contents/MacOS/$bin"
done

bash "$(dirname "${BASH_SOURCE[0]}")/sign-and-notarize.sh" "$APP"

zip -qr "$ZIPFILE" "$(basename "$ZIPDIR")"
rm -rf "$ZIPDIR"

echo "Built: $ZIPFILE"
