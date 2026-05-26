#!/bin/bash
# Build, sign, notarize, and stage the WezTerm installer binary.
#
# Each step is guarded — if a prerequisite is missing, the script prints
# guidance and skips that step rather than failing. End state: as much of the
# pipeline as your environment supports is applied to the staged binary at
# .github/installer/installer.command.
#
# Notarization uses the `wezterm-installer` keychain profile created with:
#   xcrun notarytool store-credentials wezterm-installer \
#       --apple-id <email> --team-id <team> --password <app-specific-password>
set -euo pipefail

NOTARYTOOL_PROFILE="wezterm-installer"
INSTALLER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILT="$INSTALLER_DIR/target/release/installer"
STAGED="$INSTALLER_DIR/target/release/installer.command"
TARGET="$INSTALLER_DIR/installer.command"

cd "$INSTALLER_DIR"

# --- Build -------------------------------------------------------------------
echo "→ Building (release)..."
cargo build --release
mv -f "$BUILT" "$STAGED"

# --- Sign --------------------------------------------------------------------
IDENTITY=$(security find-identity -v -p codesigning \
    | awk -F\" '/Developer ID Application:/ {print $2; exit}')

if [[ -z "$IDENTITY" ]]; then
    echo
    echo "warning: no Developer ID Application identity in your login keychain."
    echo "         Skipping signing. The committed binary will not pass Gatekeeper."
    echo "         To enable signing, install a 'Developer ID Application' cert"
    echo "         from https://developer.apple.com/account/resources/certificates"
    echo "         and re-run."
    echo
else
    echo "→ Signing with: $IDENTITY"
    codesign --force --options runtime --timestamp \
        --sign "$IDENTITY" "$STAGED"
fi

# --- Notarize ----------------------------------------------------------------
if [[ -z "$IDENTITY" ]]; then
    echo "skipping notarization (binary is unsigned)"
elif ! xcrun notarytool history --keychain-profile "$NOTARYTOOL_PROFILE" \
        >/dev/null 2>&1; then
    echo
    echo "warning: notarytool keychain profile '$NOTARYTOOL_PROFILE' not set up."
    echo "         Skipping notarization. The signed binary will still trigger"
    echo "         Gatekeeper's 'unidentified developer' warning on first launch."
    echo "         To enable, create the profile once with:"
    echo
    echo "             xcrun notarytool store-credentials $NOTARYTOOL_PROFILE \\"
    echo "                 --apple-id <your-apple-id> \\"
    echo "                 --team-id <your-team-id> \\"
    echo "                 --password <app-specific-password>"
    echo
    echo "         App-specific passwords: https://appleid.apple.com"
    echo
    NOTARIZED=0
else
    echo "→ Notarizing (this may take several minutes)..."
    NOTARY_WORK=$(mktemp -d)
    trap 'rm -rf "$NOTARY_WORK"' EXIT
    NOTARY_ZIP="$NOTARY_WORK/notarize.zip"
    ditto -c -k --keepParent "$STAGED" "$NOTARY_ZIP"
    xcrun notarytool submit "$NOTARY_ZIP" \
        --keychain-profile "$NOTARYTOOL_PROFILE" --wait
    NOTARIZED=1
fi

# --- Staple ------------------------------------------------------------------
if [[ "${NOTARIZED:-0}" == 1 ]]; then
    echo "→ Stapling notarization ticket..."
    xcrun stapler staple "$STAGED"
fi

# --- Stage -------------------------------------------------------------------
cp -f "$STAGED" "$TARGET"
echo
echo "Staged: $TARGET"
case "${NOTARIZED:-0}" in
    1) echo "Status: signed + notarized + stapled" ;;
    *) if [[ -n "$IDENTITY" ]]; then
           echo "Status: signed (not notarized)"
       else
           echo "Status: unsigned"
       fi
       ;;
esac
