#!/bin/bash
# Builds a statically-linked wezterm-mux-server for a Linux musl target.
# Usage: build-mux-linux.sh <rust-musl-target>
# Requires: cross (https://github.com/cross-rs/cross) and Docker.
set -euo pipefail

TARGET="${1:?Usage: $0 <rust-musl-target>}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

cd "$REPO_ROOT"
cross build --release --locked --target "$TARGET" -p wezterm-mux-server

BIN="$REPO_ROOT/target/$TARGET/release/wezterm-mux-server"
file "$BIN"
echo "Built: $BIN"
