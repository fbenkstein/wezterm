# replicated-mux-types

Transport-agnostic semantic types for the mux redesign discussed in
`mux-redesign-notes/` at the root of the [wezterm](https://wezterm.org/)
repository.

The redesign's premise is that mux clients run a full shadow
`wezterm_term::Terminal`, fed an ordered stream of raw PTY-output bytes from
the server, rather than polling for pre-rendered cells. The server stays the
authoritative emulator; clients render from their own replica and converge to
server authority via snapshots, a viewport hash, and explicit resync.

This crate names that boundary directly, in Rust, before any transport or
encoding is chosen:

- [`ReplicatedTerminal`] / [`AuthoritativeTerminal`] / [`ReplicaTerminal`] —
  the shared determinism contract and the server/client role split.
- [`PaneSnapshot`] — the snapshot/resync DTO shape, generic over the
  terminal emulator's own state representation.
- [`OutputEvent`], [`InputEvent`], [`ControlEvent`], [`PaneLifecycleEvent`] —
  the traffic that crosses the boundary.
- [`LayoutBlob`] — opaque, client-owned layout persistence.

It does not implement a terminal emulator, a wire format, or an RPC
framework. See `mux-redesign-notes/mux-design-restart.md` and
`mux-redesign-notes/converged-design.md` for the design rationale.

License: MIT
