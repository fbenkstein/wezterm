#!/bin/bash
# Builds a statically-linked wezterm-mux-server for a Linux musl target.
# Usage: build-mux-linux.sh <rust-musl-target>
# Requires: cross (https://github.com/cross-rs/cross) and Docker.
set -euo pipefail

TARGET="${1:?Usage: $0 <rust-musl-target>}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

cd "$REPO_ROOT"
# OpenSSL is needed in two places (see .github/Cross.toml for the host side):
#   - musl *target*: the openssl/vendored feature builds it from source
#     (openssl-src is already in Cargo.lock, so --locked holds);
#   - gnu *host*: the git2 build-dependency (wezterm-version) pulls openssl-sys,
#     linked against the libssl-dev that Cross.toml's pre-build installs.
export CROSS_CONFIG="$REPO_ROOT/.github/Cross.toml"
cross build --release --locked --target "$TARGET" -p wezterm-mux-server --features openssl/vendored

BIN="target/$TARGET/release/wezterm-mux-server"
file "$BIN"
echo "Built: $BIN"
