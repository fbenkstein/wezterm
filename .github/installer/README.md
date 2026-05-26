# installer

A signed, double-clickable macOS installer for the WezTerm fork.

## What it does

Inside the downloaded zip, the installer sits next to `WezTerm.app`. The user
double-clicks `installer.command`, Terminal opens, and the installer:

1. Verifies a `Developer ID Application` identity exists in the login keychain.
2. Asks `[Y/n]` to proceed.
3. Strips `com.apple.quarantine` from `WezTerm.app`.
4. Re-signs the app with the user's Developer ID, with hardened runtime and timestamp.
5. Submits to Apple's notarization service via `xcrun notarytool`, using a
   stored keychain profile named `wezterm-installer`.
6. Staples the notarization ticket onto the app.
7. Asks `[Y/n]` again before installing.
8. Copies the app to `~/Applications/WezTerm.app`.

Prompts: Enter or `y` for yes; `n` or `Esc` (or Ctrl-C) for cancel.

## Why a separate installer instead of signing in CI

Apple credentials (signing identity, app-specific password) would have to live
as GitHub Actions secrets if signing happened in CI. That's a risk we
deliberately avoid. Instead, signing and notarization run on the developer's
own machine at install time, reading from the local keychain.

The installer itself is the bootstrap. It must be Developer-ID signed and
notarized so it can launch from a Gatekeeper-quarantined download without
warnings. That signing happens **once, locally**, when the installer source
changes. The resulting signed binary is committed to the `dot-github` branch
(LFS-tracked) and bundled into the zip by CI.

## Prerequisites (one-time, on your machine)

1. **Apple Developer Program membership** with a `Developer ID Application`
   certificate in the login keychain. Verify:
   ```
   security find-identity -v -p codesigning
   ```
2. **Notarization profile** stored in the keychain:
   ```
   xcrun notarytool store-credentials wezterm-installer \
       --apple-id <your-apple-id-email> \
       --team-id <your-team-id> \
       --password <app-specific-password>
   ```
   App-specific passwords are generated at https://appleid.apple.com.

## Building, signing, and shipping the installer

Run `build.sh`:

```
cd .github/installer
./build.sh
```

The script is guarded — each step (sign, notarize, staple) checks its
prerequisites and skips with a helpful message if anything is missing. So it
runs to completion even if you haven't yet set up the notarization profile;
the resulting binary will simply be signed-but-not-notarized in that case.

After it finishes, `installer.command` is staged at the path CI expects:
`.github/installer/installer.command`. Commit it (it goes through Git LFS — see
`.gitattributes`).

Re-run `build.sh` whenever you change the installer source. Otherwise the
existing signed binary is reused.

## How CI consumes it

`.github/scripts/add-installer.sh` copies `installer.command` from this
directory into the zip alongside `WezTerm.app`. There is no build step in CI;
the signed binary is a pre-built LFS artifact in the source tree.

## Layout

```
.github/installer/
  Cargo.toml
  src/main.rs
  build.sh
  README.md
  .gitignore
  .gitattributes
  installer.command   # built+signed binary; committed via Git LFS
  target/             # cargo build output; gitignored
```
