#!/bin/bash
# Injects the custom installer into a macOS app zip.
# Usage: add-installer.sh <app-zip>
#
# Reads the pre-built+signed installer from
# .github/installer/install-wezterm.command (committed to dot-github after
# local sign+notarize — see .github/installer/README.md) and places it next to
# WezTerm.app in the zip root.
#
# Produces <basename>+custom-install.zip in the current directory.
set -euo pipefail

APP_ZIP="${1:?Usage: $0 <app-zip>}"
INSTALLER=.github/installer/install-wezterm.command

if [[ ! -f "$INSTALLER" ]]; then
    echo "error: $INSTALLER is not committed yet." >&2
    echo "Build, sign, and commit it locally — see .github/installer/README.md." >&2
    exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

unzip -q "$APP_ZIP" -d "$WORK"

ZIPDIR=$(find "$WORK" -maxdepth 1 -mindepth 1 -type d)
install -m 755 "$INSTALLER" "$ZIPDIR/install-wezterm.command"

BASENAME=$(basename "$APP_ZIP" .zip)
OUT="$(pwd)/${BASENAME}+custom-install.zip"
(cd "$WORK" && zip -qr "$OUT" .)

echo "Built: $OUT"
