# Agent Instructions

## Repository structure

This is a fork of [wez/wezterm](https://github.com/wez/wezterm). The `main`
branch tracks upstream and is kept in sync automatically. The `.github/`
directory is **not** part of upstream — it is maintained on the orphan branch
`dot-github` and injected into `main` by the sync workflow.

## Making changes to .github

**Never commit changes to `.github/` directly on `main`.** They will be
overwritten the next time the sync workflow runs.

All changes to workflows, scripts, and the installer must be made on the
`dot-github` branch:

```bash
git checkout dot-github
# make changes
git add .github/...
git commit -m "..."
git push origin dot-github
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
    build-macos-with-mux.yml # combined build: app + mux + custom installer
  scripts/
    build-macos.sh           # builds universal macOS .app zip
    build-mux-linux.sh       # builds static wezterm-mux-server for a musl target (uses cross)
    combine-macos.sh         # injects mux binaries into app zip → +mux
    add-installer.sh         # injects installer into +mux zip → +mux+custom-install
  installer/
    install-wezterm          # placeholder; to be replaced with a signed Rust binary
  TODO.md
```

## Artifact chain

```
build-macos.sh        → WezTerm-macos-{TAG}.zip
build-mux-linux.sh    → wezterm-mux-server (per target)
combine-macos.sh      → WezTerm-macos-{TAG}+mux.zip
add-installer.sh      → WezTerm-macos-{TAG}+mux+custom-install.zip
```
