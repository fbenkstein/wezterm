# Streaming Mux Protobuf Protocol

## Status

The authoritative protocol draft has moved from this markdown sketch to the
protobuf schema:

```text
../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto
```

This note remains as a short guide to the design intent behind that schema. Do
not copy message definitions back into this file; update the `.proto` directly.

## MVP Shape

The schema describes a new side-by-side mux implementation over gRPC/tonic. The
old mux protocol remains unchanged while this is developed behind an
experimental server/domain mode.

Core ownership split:

- Server owns PTY/process lifecycle, authoritative terminal emulators,
  canonical pane size, pane lifecycle, committed scrollback, blob storage,
  reconnect snapshots, opaque layout blobs keyed by persistent client id, and
  per-pane focus membership sets.
- Client owns GUI windows, tabs, split layout, zoom, overlays, active tab/pane
  selection, rendering, shadow terminal emulators, predictive echo overlays,
  local resize speculation, config interpretation, and domain transformation.

The server informs clients about authoritative terminal/session facts. It does
not command client UI. GUI-oriented `wezterm cli` requests are forwarded
opaquely to an eligible connected client.

## Important Decisions Captured In The Proto

- Panes and sessions are server resources. GUI windows, tabs, and split trees
  are not authoritative mux resources in the MVP.
- Layout is opaque client-owned reconnect metadata. The server persists it but
  does not interpret it and does not emit layout events.
- There is no `SplitPane` RPC in the MVP. The primitive operation is pane
  creation/destruction; clients update layout separately. If high-latency links
  make this awkward, add a generic transaction/batch later rather than making
  split layout server-owned.
- A stable `PersistentClientId` is part of the protocol from the start. The
  server refuses concurrent primary connections for the same persistent id
  unless the reconnect policy requests takeover.
- Focus is tracked as per-pane membership sets of `FocusScopeId`; terminal focus
  events are emitted only on empty/non-empty transitions.
- Clipboard changes from OSC 52 are terminal-originated events consumed by
  clients. The server does not store clipboard contents.
- Explicit OSC 8 hyperlinks are terminal cell metadata and must be preserved in
  snapshots/scrollback. Auto-detected links and URL opening are client/UI policy.

## Open Design Questions

- Exact contents of `TerminalSnapshot`; the `.proto` currently includes
  explicit first-pass fields plus an opaque escape hatch.
- Whether `ReadPane` should remain per-pane or eventually become a consolidated
  session-wide event stream.
- How aggressively to send viewport hashes.
- Whether committed scrollback starts as physical rows or a more semantic /
  reflowable representation.
- How much of the current image transport can be reused as blob references.
- Whether blob and snapshot transfer need streaming/chunked variants before the
  first useful implementation.
