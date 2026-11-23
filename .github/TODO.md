# TODO / Ideas

## Linux builds: consider Flatpak

Replace the per-distro `.deb`/`.rpm` workflows with a single Flatpak build.

- One workflow instead of 8+ distro×variant combinations
- `.flatpak` bundle artifact mirrors the macOS zip approach
- Use `flatpak-cargo-generator.py` (flatpak-builder-tools) to generate Cargo
  sources from `Cargo.lock`; borrow manifest structure from the existing
  Flathub listing (`org.wezfurlong.wezterm`)

**Caveats:** sandbox is largely bypassed in practice (`--filesystem=host`,
`--share=network`) because wezterm needs full fs and network access as a
terminal emulator — so users gain cross-distro convenience but not real
sandboxing. Not a blocker, just worth knowing.
