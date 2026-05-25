#!/bin/bash
# Injects static Linux mux server binaries into a macOS app zip.
# Usage: combine-macos.sh <app-zip> <mux-binaries-dir>
#
# <mux-binaries-dir> must contain subdirectories named after Rust target triples,
# each containing a wezterm-mux-server binary. Example:
#   mux-binaries/
#     x86_64-unknown-linux-musl/wezterm-mux-server
#     aarch64-unknown-linux-musl/wezterm-mux-server
#
# Produces <basename>+mux.zip in the current directory.
set -euo pipefail

APP_ZIP="${1:?Usage: $0 <app-zip> <mux-binaries-dir>}"
MUX_DIR="${2:?Usage: $0 <app-zip> <mux-binaries-dir>}"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

unzip -q "$APP_ZIP" -d "$WORK"

APP=$(find "$WORK" -maxdepth 2 -name "WezTerm.app")
MUX_DEST="$APP/Contents/Resources/mux-binaries"
mkdir -p "$MUX_DEST"

for target_dir in "$MUX_DIR"/*/; do
    target=$(basename "$target_dir")
    mkdir -p "$MUX_DEST/$target"
    install -m 755 "$target_dir/wezterm-mux-server" "$MUX_DEST/$target/wezterm-mux-server"
done

BASENAME=$(basename "$APP_ZIP" .zip)
OUT="$(pwd)/${BASENAME}+mux.zip"
(cd "$WORK" && zip -qr "$OUT" .)

echo "Built: $OUT"
