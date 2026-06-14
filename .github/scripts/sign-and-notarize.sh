#!/bin/bash
# Sign (and optionally notarize+staple) a macOS .app bundle.
#
# Usage: sign-and-notarize.sh <path-to.app>
#
# Real signing: set these env vars before calling:
#   APPLE_CERTIFICATE_P12          base64-encoded .p12 export of Developer ID cert+key
#   APPLE_CERTIFICATE_PASSWORD     password protecting the .p12
#   APPLE_ID                       Apple ID email for notarytool
#   APPLE_TEAM_ID                  10-char team ID (e.g. AB12CD34EF)
#   APPLE_APP_SPECIFIC_PASSWORD    app-specific password from appleid.apple.com
#
# If APPLE_CERTIFICATE_P12 is unset, falls back to ad-hoc signing (--sign -)
# which lets the app run on Apple Silicon but will not pass Gatekeeper.
#
# Notarization is skipped if any of APPLE_ID / APPLE_TEAM_ID /
# APPLE_APP_SPECIFIC_PASSWORD is unset (signing still proceeds).
set -euo pipefail

APP="${1:?Usage: $0 <path-to.app>}"

# --- Ad-hoc fallback ---------------------------------------------------------
if [[ -z "${APPLE_CERTIFICATE_P12:-}" ]]; then
    echo "→ APPLE_CERTIFICATE_P12 not set — falling back to ad-hoc signing"
    codesign --force --deep --sign - "$APP"
    exit 0
fi

# --- Import certificate into a temporary keychain ----------------------------
KEYCHAIN_PATH=$(mktemp -u "$TMPDIR/wezterm-XXXXXX.keychain-db")
KEYCHAIN_PASSWORD=$(uuidgen)
P12_FILE=$(mktemp "$TMPDIR/cert-XXXXXX.p12")

cleanup() {
    security delete-keychain "$KEYCHAIN_PATH" 2>/dev/null || true
    rm -f "$P12_FILE"
}
trap cleanup EXIT

echo "$APPLE_CERTIFICATE_P12" | base64 --decode > "$P12_FILE"

security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security import "$P12_FILE" \
    -k "$KEYCHAIN_PATH" \
    -P "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD must be set}" \
    -T /usr/bin/codesign
security set-key-partition-list \
    -S apple-tool:,apple:,codesign: \
    -s -k "$KEYCHAIN_PASSWORD" \
    "$KEYCHAIN_PATH"

# Add the temp keychain to the user search list so codesign can find the identity.
# shellcheck disable=SC2046
security list-keychains -d user -s "$KEYCHAIN_PATH" \
    $(security list-keychains -d user | tr -d '"')

# --- Sign --------------------------------------------------------------------
echo "→ Signing with Developer ID Application..."
codesign --force --deep \
    --options runtime \
    --timestamp \
    --sign "Developer ID Application" \
    "$APP"

echo "→ Verifying signature..."
codesign --verify --deep --strict "$APP"

# --- Notarize ----------------------------------------------------------------
if [[ -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" || -z "${APPLE_APP_SPECIFIC_PASSWORD:-}" ]]; then
    echo "→ Skipping notarization (APPLE_ID / APPLE_TEAM_ID / APPLE_APP_SPECIFIC_PASSWORD not set)"
    echo "Status: signed (not notarized)"
    exit 0
fi

echo "→ Notarizing (this may take a few minutes)..."
NOTARY_WORK=$(mktemp -d "$TMPDIR/notary-XXXXXX")
trap 'cleanup; rm -rf "$NOTARY_WORK"' EXIT

NOTARY_ZIP="$NOTARY_WORK/notarize.zip"
ditto -c -k --keepParent "$APP" "$NOTARY_ZIP"
xcrun notarytool submit "$NOTARY_ZIP" \
    --apple-id "$APPLE_ID" \
    --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --wait

# --- Staple ------------------------------------------------------------------
echo "→ Stapling notarization ticket..."
xcrun stapler staple "$APP"

echo "Status: signed + notarized + stapled"
