#!/bin/bash
# Injects the custom installer into a macOS app zip.
# Usage: add-installer.sh <app-zip>
#
# The installer is read from .github/installer/install-wezterm relative to the
# working directory. It is placed next to WezTerm.app in the zip root so the
# user sees both side by side after unzipping.
#
# Produces <basename>+custom-install.zip in the current directory.
set -euo pipefail

APP_ZIP="${1:?Usage: $0 <app-zip>}"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

unzip -q "$APP_ZIP" -d "$WORK"

ZIPDIR=$(find "$WORK" -maxdepth 1 -mindepth 1 -type d)
install -m 755 .github/installer/install-wezterm "$ZIPDIR/install-wezterm"

BASENAME=$(basename "$APP_ZIP" .zip)
OUT="$(pwd)/${BASENAME}+custom-install.zip"
(cd "$WORK" && zip -qr "$OUT" .)

echo "Built: $OUT"
