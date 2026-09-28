# Agent Instructions

## Repository structure

This is a fork of [wez/wezterm](https://github.com/wez/wezterm). The `main`
branch tracks upstream and is kept in sync automatically. The `.github/`
directory is **not** part of upstream — it is maintained on the orphan branch
`dot-github` and injected into `main` by the sync workflow.

## Making changes to .github

**Never commit changes to `.github/` directly on `main`.** They will be
overwritten the next time the sync workflow runs.

All changes to workflows and scripts must be made on the `dot-github` branch:

```bash
jj new dot-github
# make changes
jj describe -m "..."
jj bookmark set dot-github -r @
jj git push --remote fork --branch dot-github
```

The sync workflow (`sync-upstream.yml`) will carry them into `main` on its
next run, or it can be triggered manually via `workflow_dispatch`.

## Layout

```
.github/
  workflows/
    sync-upstream.yml        # merges upstream/main, replaces .github from dot-github
    build-macos.yml          # standalone macOS app build (workflow_dispatch)
    build-mux-linux.yml      # standalone static Linux mux server builds (workflow_dispatch)
    build-macos-with-mux.yml # combined build: mux binaries embedded in app, signed + notarized
  scripts/
    build-macos.sh           # builds universal macOS .app zip; embeds mux binaries if MUX_BINARIES_DIR set
    build-mux-linux.sh       # builds static wezterm-mux-server for a musl target (uses cross)
    sign-and-notarize.sh     # signs and notarizes a .app with Developer ID; ad-hoc fallback if secrets absent
```

## Artifact chain

```
build-mux-linux.sh    → wezterm-mux-server (per target, uploaded as inter-job artifact)
build-macos.sh        → WezTerm-macos-{TAG}.zip  (with mux binaries embedded when MUX_BINARIES_DIR is set)
sign-and-notarize.sh  → signs + notarizes the .app inside the zip before it is zipped
```

## Signing and notarization

`sign-and-notarize.sh` runs as part of `build-macos.sh`. It requires five
repository secrets: `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_SPECIFIC_PASSWORD`. If any are absent
it falls back to ad-hoc signing.
