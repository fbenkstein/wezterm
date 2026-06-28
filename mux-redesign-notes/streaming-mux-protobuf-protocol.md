# Streaming Mux Protobuf Protocol

## Status

Draft design notes for a new, side-by-side mux implementation.

This supersedes the earlier "protobuf as an alternative body encoding" idea in
`protobuf-protocol-design.md` for the new implementation. The old mux protocol
can remain unchanged while this protocol is developed behind an experimental
domain/server mode.

## Goals

- Improve performance and reliability for long-running sessions with many tabs
  and deep scrollback.
- Make the client/server contract explicit in protobuf IDL.
- Let the new implementation live beside the current mux implementation, with
  no big-bang migration for existing users.
- Stream PTY output bytes to client-side shadow terminal emulators.
- Keep the server authoritative for process lifecycle, canonical terminal
  state, canonical pane size, committed scrollback, and topology.
- Allow the client to speculate locally for responsiveness, then converge to
  server authority through snapshots, hashes, and explicit resync.

## Non-goals for the initial implementation

- Full multi-client interactive correctness.
- Independent per-client terminal sizes.
- A deterministic total order between PTY input, PTY output, resize ioctls,
  signals, and application repaint behavior.
- Backward compatibility with the current mux wire format.
- Replacing the current mux implementation immediately.

Concurrent clients should be tolerated, but the first implementation is
optimized for the common case: one interactive client, with a second apparent
client usually being a replacement after network timeout, laptop sleep, or
client reboot.

## Transport shape

The initial implementation should use gRPC over HTTP/2 with protobuf IDL, not
a custom protobuf envelope.

- Unary RPCs for commands with clear request/response semantics.
- Client-opened server-streaming or bidirectional streams for long-lived data
  flows.
- Protobuf messages as the durable IDL contract.
- Standard gRPC tooling for generated clients, reflection/debugging, tracing,
  flow-control behavior, and alternative client/server implementations.

The current mux protocol already pays the cost of a bespoke wire format. The
new implementation should not repeat that unless there is a concrete blocker
with gRPC. A custom envelope remains a fallback option only if the implementation
proves that gRPC cannot satisfy a required transport case, such as SSH stdio
proxying or Unix-domain-socket behavior.

See `grpc-viability-experiments.md` for the transport and performance
experiments that should validate this choice before the implementation commits
too deeply to gRPC.

## Resource model

Stable resources:

- `SessionId`
- `WindowId`
- `TabId`
- `PaneId`
- `ClientId`
- `BlobId` for image data and other large referenced payloads

The protocol should be resource-oriented: panes, tabs, and sessions are
addressed directly, not rediscovered through broad polling after every event.

Common ID and value types can be thin wrappers so the generated code preserves
type intent:

```proto
message SessionId { uint64 value = 1; }
message WindowId  { uint64 value = 1; }
message TabId     { uint64 value = 1; }
message PaneId    { uint64 value = 1; }
message BlobId    { bytes value = 1; }

message ClientId {
  string hostname = 1;
  string username = 2;
  uint32 pid = 3;
  uint64 epoch = 4;
  uint64 nonce = 5;
}

message PaneSize {
  uint32 rows = 1;
  uint32 cols = 2;
  uint32 pixel_width = 3;
  uint32 pixel_height = 4;
  uint32 dpi = 5;
}

message BlobRef {
  BlobId id = 1;
  uint64 size = 2;
  string media_type = 3;
}
```

## Responsibility split

The server owns:

- PTY creation and process lifecycle.
- Authoritative terminal emulators.
- Canonical pane/tab sizes.
- Tab/pane topology.
- Committed scrollback.
- Image/blob storage.
- Reconnect snapshots.
- Server-originated commands such as a forwarded `wezterm cli activate-pane`.

The client owns:

- UI windows and presentation.
- Active tab/pane selection for local UI purposes.
- Rendering.
- Shadow terminal emulators.
- Predictive echo overlays.
- Local resize speculation.
- Config interpretation and domain transformation.

Input is explicitly pane-addressed. Focus is metadata/signaling, not input
routing.

## Core service sketch

```proto
syntax = "proto3";

package wezterm.streaming_mux.v1;

service StreamingMux {
  // Long-lived client control channel. The client opens it; both sides can
  // send control messages over it.
  rpc Control(stream ClientControl) returns (stream ServerControl);

  // Session/topology commands.
  rpc CreateSession(CreateSessionRequest) returns (CreateSessionResponse);
  rpc AttachSession(AttachSessionRequest) returns (AttachSessionResponse);
  rpc SpawnPane(SpawnPaneRequest) returns (SpawnPaneResponse);
  rpc SplitPane(SplitPaneRequest) returns (SplitPaneResponse);
  rpc ClosePane(ClosePaneRequest) returns (ClosePaneResponse);
  rpc ResizePane(ResizePaneRequest) returns (ResizePaneResponse);

  // Pane terminal state.
  rpc AttachPane(AttachPaneRequest) returns (PaneSnapshot);
  rpc ReadPane(ReadPaneRequest) returns (stream PaneReadEvent);
  rpc WritePane(stream PaneInput) returns (WritePaneResponse);

  // Authoritative data stores.
  rpc GetScrollback(GetScrollbackRequest) returns (GetScrollbackResponse);
  rpc GetBlob(GetBlobRequest) returns (GetBlobResponse);
}
```

The service split is illustrative. It is acceptable to group commands
differently as the IDL sharpens, but the protocol should retain the conceptual
separation between control, pane output, pane input, and authoritative stored
data.

## Control stream

The control stream carries client registration, topology notifications,
server-originated commands, focus metadata, and resync requests.

```proto
message ClientHello {
  ClientId client_id = 1;
  string wezterm_version = 2;
  repeated string capabilities = 3;
  repeated SessionId desired_sessions = 4;
}

message ServerHello {
  string server_version = 1;
  repeated string capabilities = 2;
  repeated SessionSummary sessions = 3;
}

message ClientControl {
  ClientId client_id = 1;

  oneof message {
    ClientHello hello = 10;
    ClientFocusChanged focus_changed = 11;
    ClientActivePaneChanged active_pane_changed = 12;
    ClientAck ack = 13;
    ClientRequestResync request_resync = 14;
  }
}

message ServerControl {
  uint64 control_seq = 1;
  optional ClientId origin_client_id = 2;

  oneof message {
    ServerHello hello = 10;
    TopologyChanged topology_changed = 11;
    PaneSizeChanged pane_size_changed = 12;
    PaneTitleChanged pane_title_changed = 13;
    PaneExited pane_exited = 14;
    ActivatePaneCommand activate_pane = 15;
    NotifyAlert notify_alert = 16;
    ResyncRequired resync_required = 17;
  }
}
```

`origin_client_id` allows clients to ignore or specially handle echoes of
their own actions. The server may also suppress origin echoes entirely.

The control stream has a session-level `control_seq` for notification ordering,
but that sequence does not define a total order against pane input/output
streams.

## Pane attachment and output

`AttachPane` returns a snapshot of the authoritative pane state. `ReadPane`
then streams output and other pane-specific authoritative data.

```proto
message AttachPaneRequest {
  PaneId pane_id = 1;
}

message PaneSnapshot {
  PaneId pane_id = 1;
  uint64 pane_output_seq = 2;
  uint64 scrollback_seq = 3;
  uint64 size_seq = 4;
  PaneSize canonical_size = 5;
  TerminalSnapshot terminal = 6;
  repeated BlobRef blobs = 7;
}

message ReadPaneRequest {
  PaneId pane_id = 1;
  uint64 start_after_pane_output_seq = 2;
  uint64 start_after_scrollback_seq = 3;
}

message PaneReadEvent {
  PaneId pane_id = 1;

  oneof event {
    PtyOutput output = 10;
    ScrollbackCommit scrollback_commit = 11;
    ScrollbackClear scrollback_clear = 12;
    ScrollbackDropBefore scrollback_drop_before = 13;
    PaneViewportHash viewport_hash = 14;
    PaneCheckpoint checkpoint = 15;
    ResyncRequired resync_required = 16;
  }
}

message PtyOutput {
  uint64 pane_output_seq = 1;
  bytes data = 2;
}
```

`TerminalSnapshot` is intentionally a protocol DTO rather than a serialized
Rust `TerminalState`. The exact cell encoding remains open, but the snapshot
needs to carry enough state for the client to initialize a shadow terminal at a
clean continuation point:

```proto
message TerminalSnapshot {
  PaneSize size = 1;
  uint64 viewport_top_row_id = 2;
  uint64 cursor_row_id = 3;
  uint32 cursor_col = 4;
  CursorState cursor = 5;
  TerminalModes modes = 6;
  ColorPalette palette = 7;
  repeated SnapshotRow viewport_rows = 8;
  repeated CommittedRow recent_scrollback = 9;
  repeated BlobRef blobs = 10;
}

message SnapshotRow {
  uint64 row_id = 1;
  uint32 source_cols = 2;
  bool soft_wrapped_to_next = 3;
  repeated CellRun runs = 4;
  repeated BlobRef blobs = 5;
}
```

`pane_output_seq` orders PTY output chunks for a single pane. It is not a
global session sequence and does not order output against resize RPCs or input
RPCs.

The snapshot/read contract must prevent missed or duplicated bytes:

- `PaneSnapshot.pane_output_seq` says which output byte sequence is included in
  the snapshot.
- `ReadPane(start_after_pane_output_seq = N)` starts after that sequence.
- The server must buffer, replay, or otherwise make that continuation exact.

## Pane input

Input is explicitly addressed to a pane and flows over a client-opened stream.

```proto
message PaneInput {
  PaneId pane_id = 1;
  uint64 client_input_seq = 2;

  oneof input {
    bytes raw_bytes = 10;
    KeyEvent key = 11;
    MouseEvent mouse = 12;
    Paste paste = 13;
  }
}

message WritePaneResponse {
  PaneId pane_id = 1;
  uint64 last_client_input_seq = 2;
}
```

Ordering is guaranteed within one `WritePane` stream. There is no protocol
guarantee that input is ordered relative to `ReadPane` output or `ResizePane`
requests. That matches PTY reality: input bytes, output bytes, window-size
ioctls, `SIGWINCH`, and application repaint behavior are separate channels.

`client_input_seq` is for latency measurement, predictive echo correlation,
and diagnostics. It is not used for routing.

## Resize semantics

The server owns canonical pane size. Clients may request a resize, but must be
prepared to receive non-requested authoritative size changes.

```proto
message ResizePaneRequest {
  PaneId pane_id = 1;
  PaneSize requested_size = 2;
  uint64 client_resize_seq = 3;
}

message ResizePaneResponse {
  PaneId pane_id = 1;
  uint64 client_resize_seq = 2;
  uint64 size_seq = 3;
  PaneSize canonical_size = 4;
  ResizeStatus status = 5;
}

message PaneSizeChanged {
  PaneId pane_id = 1;
  uint64 size_seq = 2;
  PaneSize canonical_size = 3;
  optional ClientId origin_client_id = 4;
}

enum ResizeStatus {
  RESIZE_STATUS_UNSPECIFIED = 0;
  RESIZE_STATUS_ACCEPTED = 1;
  RESIZE_STATUS_REJECTED = 2;
}
```

Initial multi-client behavior is deliberately simple:

- Clients accept `PaneSizeChanged` at any time.
- Resize requests from concurrent clients are not given strong ownership
  semantics in v1.
- If multiple clients are present, they naturally converge toward sizes all of
  them can render.

Client rendering policy for canonical/local size mismatch:

- If the canonical pane is larger than the local viewport, clip or pan the
  terminal view and render scrollbars where appropriate. The client may request
  a shrink to fit its viewport.
- If the canonical pane is smaller than the local viewport, render the terminal
  at canonical size and blank/pad the unused area. Do not resize the OS window
  smaller.
- Corrective resize requests should not increase either dimension in response
  to a mismatch:

```text
requested.cols = min(canonical.cols, local_viewport.cols)
requested.rows = min(canonical.rows, local_viewport.rows)
```

Corrective resize requests should be debounced to avoid resize storms during
live window resizing or reconnect.

Users already recover from too-small terminals by slightly resizing the window;
the protocol does not need a complex first-class recovery mechanism for that in
v1.

## Speculation and convergence

The client may speculate locally:

- Apply local window resizes immediately for responsiveness.
- Render predictive echo as an overlay.
- Maintain local active pane/tab presentation state.

The client shadow terminal can therefore be temporarily wrong. That is
acceptable. Terminal applications commonly repaint after resize; when they do
not, users are already accustomed to transiently garbled terminal output.

The server remains authoritative. Convergence mechanisms:

- `PaneSnapshot` on attach and reconnect.
- `PaneViewportHash` for cheap drift detection.
- `ResyncRequired` when the server knows a client should discard local state.
- Authoritative committed scrollback events.
- Explicit client `ClientRequestResync`.

The protocol should not try to prevent every temporary divergence. It should
make divergence bounded, detectable, and recoverable.

## Committed scrollback

Scrollback should not be derived solely from whatever the client shadow
terminal happened to produce while speculating. The server owns committed
scrollback and replicates it to clients.

```proto
message ScrollbackCommit {
  uint64 scrollback_seq = 1;
  repeated CommittedRow rows = 2;
}

message CommittedRow {
  uint64 row_id = 1;
  uint32 source_cols = 2;
  bool soft_wrapped_to_next = 3;
  repeated CellRun runs = 4;
  repeated BlobRef blobs = 5;
}

message ScrollbackClear {
  uint64 scrollback_seq = 1;
}

message ScrollbackDropBefore {
  uint64 scrollback_seq = 1;
  uint64 first_retained_row_id = 2;
}
```

Initial committed units can be physical terminal rows plus soft-wrap metadata.
Later versions can evolve toward more reflowable scrollback groups if needed.

The important invariant is:

```text
The live viewport may be speculative.
Committed scrollback is replicated from the server commit log.
```

## Ordering guarantees

The protocol guarantees:

- Messages are ordered within a single gRPC stream.
- PTY output bytes are ordered per pane by `pane_output_seq`.
- `PaneSnapshot` plus `ReadPane(start_after_pane_output_seq)` provides exact
  continuation with no missed or duplicated output bytes.
- Committed scrollback events are ordered by `scrollback_seq`.
- Control notifications are ordered by `control_seq`.

The protocol does not guarantee:

- A total order across `WritePane`, `ReadPane`, `ResizePane`, and `Control`.
- That resize requests are causally ordered with PTY output bytes.
- That a client shadow emulator is identical to the server emulator at every
  intermediate moment.
- That concurrent interactive clients can maintain independent canonical
  sizes.

The absence of a cross-stream total order is intentional and mirrors PTY
semantics.

## Initial lifecycle

Typical first connection:

1. Client opens `Control` and sends `ClientHello`.
2. Client creates or attaches to a session.
3. Server sends current topology over `AttachSessionResponse` and/or
   `ServerControl`.
4. Client attaches visible panes with `AttachPane`.
5. Client opens `ReadPane` from the snapshot sequence.
6. Client opens `WritePane` for interactive panes.
7. Client renders from its shadow terminal and authoritative committed
   scrollback.

Reconnect/replacement client:

1. New client opens `Control` and attaches to the existing session.
2. Server may still believe the old client is alive.
3. New client receives snapshots and current canonical sizes.
4. New client accepts unsolicited `PaneSizeChanged` events.
5. New client may request shrink-to-fit resizes using the non-increasing
   dimension rule.
6. Server eventually notices the old client is dead and drops its streams.

## Open design questions

- Exact contents of `TerminalSnapshot`.
- Whether `ReadPane` should be per-pane or eventually consolidated into a
  session-wide event stream for efficiency.
- How aggressively to send viewport hashes.
- Whether committed scrollback should start as physical rows or a more
  semantic/reflowable representation.
- How much of the current image transport can be reused as blob references.
