# Multiplexer Redesign: Streaming PTY Bytes to Shadow Emulators

## Status: Draft / Proposal

## Background

WezTerm's multiplexer currently uses a pull model: the server is the sole terminal
emulator, and clients are intelligent caches that poll for dirty lines and fetch
them on demand. The server sends `GetPaneRenderChangesResponse` with dirty line
ranges; clients fetch specific line ranges via `GetLines`.

This architecture predates the requirement that client and server must be built
from the same commit. That constraint opens up a much simpler and more efficient
design.

### Current performance problems

The existing design has several compounding performance issues that worsen over
long sessions with many tabs:

1. **Scrollback rewrapping on resize** (`term/src/screen.rs: Screen::resize`):
   When the column count changes, every line in scrollback is drained and
   re-wrapped. This is O(scrollback_size) and fully synchronous. After a day of
   heavy output, a single pane can have 20,000+ lines of scrollback, making this
   the dominant cost.

2. **No debouncing of `TabResized` notifications**: A single resize triggers
   multiple `TabResized` broadcasts (from `pane.resize()`,
   `rebuild_splits_sizes_from_contained_panes()`, and
   `sync_with_pane_tree()`). Each is flushed immediately to all clients with no
   batching.

3. **`TabResized` → `ListPanes` cascade**: Each `TabResized` causes every
   connected client to call `resync()`, which sends a `ListPanes` RPC. The
   server responds by locking and reading every pane across every tab
   (dimensions, cursor, title, working dir) — O(num_panes) lock acquisitions
   per resize event, per client.

4. **Lock contention**: Background PTY output competes with these lock
   acquisitions, causing queuing.

The result: after a full day of work with many tabs open, a terminal resize can
hang for tens of seconds.

## Proposed Design

**Core idea**: since client and server are always the same version, the client
can run a full `term::Terminal` shadow emulator. The server streams raw PTY
bytes to the client; the client feeds them into its own emulator and renders
locally. The server remains the authoritative emulator (needed for attach and
reconnect), but the client no longer depends on the server for rendering.

### Invariants

- Client and server always run identical `term::Terminal` code (existing
  same-commit requirement).
- Given identical byte streams from a common snapshot point, both emulators
  will produce identical state.
- The server's emulator is ground truth. The client's is a replica.

### Session lifecycle

#### Initial attach

1. Client connects and sends `AttachPane { pane_id }`.
2. Server takes a snapshot at a clean parser boundary (see below) and sends
   `PaneSnapshot { ... }` containing the full current state.
3. Client initializes its local `term::Terminal` from the snapshot.
4. Server begins streaming subsequent PTY output as `PtyOutput { pane_id,
   seqno, data: Bytes }` messages.
5. Client feeds `data` into its local emulator and renders.

No more `GetPaneRenderChanges` polling. No more `GetLines` fetching. No more
LRU line cache.

#### Normal operation

```
PTY → server emulator → server stores authoritative state
                      → streams raw bytes to all attached clients
                      → each client feeds bytes into its shadow emulator
                      → each client renders locally
```

User input still flows client → server → PTY (unchanged).

#### Resize

```
Client window resized:
  1. Client rewraps its local emulator immediately → renders new layout (no lag)
  2. Client sends Resize { pane_id, size } to server
  3. Server rewraps its authoritative emulator asynchronously
  4. Server sends StructuralEvent::PaneResized to all *other* clients
  5. Other clients rewrap their local emulators

No ListPanes round-trip. No dirty line fetching. No lock cascade.
```

The server's rewrap still happens (required to keep authoritative state correct),
but it no longer blocks the initiating client's UI.

#### Reconnect after disconnect

Client sends `AttachPane` again. Server sends a fresh `PaneSnapshot`. Client
reinitializes from snapshot and resumes streaming.

### Drift detection

Because both sides run the same code on the same byte stream, they should never
diverge. A lightweight checksum catches any bugs:

- After each PTY output flush (or on idle), server computes a hash of the
  current **viewport** (not full scrollback — that would be O(scrollback_size))
  and includes it in the `PtyOutput` message as an optional field.
- Client computes the same hash of its local viewport.
- On mismatch, client sends `ResyncRequest { pane_id }` and server responds
  with a fresh `PaneSnapshot`.

Hash algorithm: something fast like xxHash or FxHash over the serialized
viewport lines. The viewport is typically 24–50 rows so this is cheap.

### Structural events

Tab/pane topology changes (splits, closes, moves, focus changes) are currently
communicated via `TabResized` → `ListPanes` round-trip. Replace this with
explicit structural event messages so the client can update its local topology
model without an RPC:

```
StructuralEvent::PaneAdded { pane_id, tab_id, ... }
StructuralEvent::PaneRemoved { pane_id }
StructuralEvent::PaneResized { pane_id, new_size }
StructuralEvent::TabCreated { tab_id, window_id }
StructuralEvent::TabClosed { tab_id }
StructuralEvent::FocusChanged { pane_id }
StructuralEvent::SplitLayout { tab_id, tree: PaneTree }
```

These replace `TabResized`, `PaneRemoved`, `TabAddedToWindow`, and
`PaneFocused` in the current protocol, and eliminate the `ListPanes` RPC from
the hot path entirely.

### New protocol messages

| Message | Direction | Replaces |
|---|---|---|
| `AttachPane { pane_id }` | C→S | implicit via `GetPaneRenderChanges` |
| `PaneSnapshot { pane_id, seqno, lines, cursor, dims, palette, ... }` | S→C | `GetLines` + `GetPaneRenderChanges` (initial) |
| `PtyOutput { pane_id, seqno, data: Bytes, viewport_hash: Option<u64> }` | S→C | `GetPaneRenderChangesResponse` + `GetLines` |
| `StructuralEvent { ... }` | S→C | `TabResized`, `PaneRemoved`, `TabAddedToWindow`, `PaneFocused` |
| `ResyncRequest { pane_id }` | C→S | (new) |

Messages that remain unchanged: `Resize`, `SendKeyDown`, `SendMouseEvent`,
`SendPaste`, `SpawnTab`, `SplitPane`, `KillPane`, `SetClipboard`,
`SetPalette`, `NotifyAlert`.

Messages that can be removed once migration is complete: `GetPaneRenderChanges`,
`GetPaneRenderChangesResponse`, `GetLines`, `GetLinesResponse`,
`ListPanes` (from hot path; keep for CLI tooling), `TabResized`, `PaneFocused`.

### What disappears from the client

- `renderable.rs`: `LineEntry` / `LruCache` machinery
- `apply_changes_to_surface()` and dirty-line merge logic
- Polling loop with exponential backoff
- Per-client `PerPane` state on the server (seqno, dimensions, sent_initial_palette, config_generation)
- Bonus lines prefetching on the server
- The `TabResized` → `resync()` → `ListPanes` call chain

## Optimistic / Predictive Rendering

The current client already implements predictive echo: when RTT exceeds a
configured threshold, `predict_from_key_event` speculatively patches the
affected cells in the line cache and renders them with a double-underline
decoration. When the server's authoritative dirty lines arrive, they simply
overwrite the prediction. There is no explicit revert — the server always wins.

### How this works in the new model

The input latency path is unchanged:

```
keyboard → client → server → PTY process → output bytes → client shadow emulator → render
```

The client still cannot know what the PTY process will do with input (echo it,
consume it silently, produce something different) until the bytes come back.
Predictive echo is still needed for the same reason.

**Key design decision: the shadow emulator is feed-only.**

The shadow emulator is only ever fed authoritative PTY bytes from the server.
It is never mutated speculatively. Predictions are rendered as a separate
overlay on top of the shadow emulator's output — the same double-underline
decoration as today. When authoritative bytes arrive and the shadow emulator
processes them, the overlay for the affected region is cleared.

This keeps the emulator simple and always-correct. Prediction remains a
render-time concern that is entirely separate from emulator state.

### Why not feed keystrokes into the shadow emulator speculatively?

The more ambitious approach — speculatively advancing the emulator on each
keystroke and rewinding on mismatch — would require either snapshotting the
full `TerminalState` before each prediction (expensive) or maintaining an undo
log of all mutations (complex). Given that predictions are inherently imprecise
(raw-mode applications control their own echo), the accuracy gain does not
justify the implementation cost.

### Timing hazards that make prediction inherently approximate

Even with full emulator state available, predictions are inherently unreliable
due to timing:

1. **State drift during round-trip**: Between the moment a key is pressed and
   the moment the echoed bytes arrive, the terminal may have received other
   output that changed the screen content or switched modes (e.g., a background
   process wrote to the terminal, or the application changed the echo mode).
   The local emulator state at prediction time may already be stale.

2. **Multi-client interleaving**: If multiple clients are attached to the same
   session, another client may have sent keystrokes that reached the PTY
   before ours. The PTY processes input in arrival order, so our predicted
   outcome (based on the state we see) may be wrong because the other client's
   input ran first.

Both hazards exist in the current design too. They are fundamental to any
optimistic rendering scheme over a shared, stateful PTY — not problems
introduced by this redesign. The takeaway is that predictions should be
treated as a latency-hiding visual hint, not a correctness guarantee, and
the overlay should be cleared aggressively when authoritative bytes arrive.

The new model does enable slightly better predictions than today: since the
shadow emulator holds the full terminal state (cursor position, active modes,
etc.), the overlay logic can make more informed guesses than the current
cell-patching approach. This is an incremental improvement, not a redesign of
the prediction system.

## Open Problems

### 1. PaneSnapshot serialization

`TerminalState` is not currently serializable — it contains `Arc`, `dyn`
trait objects (clipboard handlers), and fields annotated `#[serde(skip)]`.
The snapshot should be a separate DTO:

```rust
pub struct PaneSnapshot {
    pub pane_id: PaneId,
    pub seqno: SequenceNo,
    pub lines: SerializedLines,          // existing type, viewport + scrollback
    pub cursor: StableCursorPosition,
    pub dimensions: RenderableDimensions,
    pub palette: ColorPalette,
    pub title: String,
    pub working_dir: Option<SerdeUrl>,
    pub is_alt_screen: bool,
    pub scrollback_top: StableRowIndex,  // so client line numbering matches
    // ... modes, margins, mouse tracking state
}
```

What to include in the snapshot is a design question: more state means fewer
edge-case divergences; less state means a simpler DTO and faster attach.
Minimum viable: lines + cursor + dimensions + palette.

### 2. Parser state at snapshot boundaries

The server's PTY reader and parser run on separate threads. The snapshot must
be taken at a point where the parser is not mid-sequence (mid-UTF-8 or
mid-escape). Options:

- Snapshot only when the parser's pending buffer is empty (natural idle point).
- Include the pending parse buffer in the snapshot and have the client replay
  it before starting the byte stream.
- Drain and re-parse: server buffers PTY output from the snapshot instant and
  sends it after the snapshot so the client can feed it in sequence.

The third option (buffered continuation) is simplest to reason about: snapshot
instant is "last completed parser flush", and subsequent bytes start a clean
stream.

### 3. Image data in scrollback

Images stored in scrollback are referenced by the `Line` objects but the pixel
data lives in a separate image store. The `PaneSnapshot` must either:

- Embed image data inline in `SerializedLines` (existing `compress_for_scrollback`
  path does some of this already), or
- Send a separate `ImageData` message before the snapshot.

Existing `SerializedLines` / `compress_for_scrollback` already handles this
for the current protocol; the same mechanism can be reused.

### 4. Multiple clients at different terminal sizes

Currently all clients share one PTY size. This remains true in the new design:
the server emulates at the canonical PTY size, and all shadow emulators see the
same bytes and use the same dimensions. The "smallest window constrains
everyone" UX problem is unchanged.

Per-client virtual viewports (where each client independently reflows to its
own width) would require either:
- A server-side virtual reflow layer per client, or
- Storing the semantic (pre-wrap) lines and reflowing on the client side.

This is a significant additional problem. It should be deferred to a follow-on
change.

### 5. Backward compatibility

The existing protocol can be kept on a different code path (or behind a feature
flag) during transition. Since same-commit is already required, a clean
protocol break is also acceptable — all deployments will upgrade together.

## Testing Strategy

### What the codebase already provides

The `term/` crate has the strongest test coverage in the codebase (~54 tests),
built around a `TestTerm` harness that supports configurable dimensions and
scrollback, snapshot assertions via `k9::snapshot!()`, and explicit checks on
cursor position, cell attributes, and dirty line tracking. The escape parser
(`wezterm-escape-parser/`) has ~58 tests covering parse correctness. These are
exactly the layers the shadow emulator redesign builds on.

The parts being *replaced* — the client-side line cache, `apply_changes_to_surface`,
the dirty-line pull loop, the `TabResized` → `ListPanes` cascade — have
essentially no automated tests (`wezterm-client/` has zero tests; `mux/` has
four). Removing them does not break CI, but it also means there is no existing
regression net to rely on.

There are no end-to-end integration tests that spin up a mux server and client
together. Verifying the full client↔server interaction will require either
interactive testing or writing new integration tests as part of this work.

### What can be tested with unit tests

The following can be covered using the existing `TestTerm` harness and
`codec/` test patterns, without any new framework:

- **`PaneSnapshot` round-trip**: construct a `TestTerm` in a known state,
  serialize to `PaneSnapshot`, reconstruct a new `Terminal` from it, assert
  identical viewport and cursor. Covers the serialization DTO and the
  attach path.

- **Byte-stream replay**: take a `TestTerm` snapshot, then feed a sequence of
  PTY bytes into both the original and a freshly-reconstructed terminal,
  assert identical state after each flush. Directly validates the core
  invariant that identical bytes from a common snapshot produce identical
  state.

- **Drift detection**: introduce a deliberate mutation to one emulator, assert
  the viewport hash diverges, trigger a resync, assert convergence.

- **Resize local handling**: assert that resizing a `TestTerm` directly
  (without a server round-trip) produces the correct rewrapped output —
  this already has coverage in `term/src/test/`.

- **`PtyOutput` codec**: extend the existing codec smoke tests to cover
  `PtyOutput` and `PaneSnapshot` PDU serialization.

### What requires interactive or integration testing

- **Latency and hang regression**: the original motivation (tens-of-second
  hangs on resize after a day of work) can only be validated against a real
  remote session with real scrollback depth.

- **Multi-client interleaving**: two clients sending input concurrently and
  both converging to correct state.

- **Network interruption and reconnect**: client disconnect mid-stream,
  reconnect, snapshot resync, correct rendering afterward.

- **Prediction overlay behaviour**: visual correctness of the double-underline
  decoration and its clearing on authoritative bytes — inherently a rendering
  concern.

### Recommendation

Write the unit tests listed above alongside each implementation phase (see
Implementation Sketch below). They are natural extensions of the existing
`TestTerm` pattern and catch the most likely correctness bugs (snapshot
incompleteness, byte-boundary splits, hash collisions). Rely on interactive
testing for latency, multi-client, and reconnect scenarios.

## Session Persistence and Resurrection

WezTerm does not currently have a session persistence mechanism. This redesign
makes one materially easier to implement.

The `PaneSnapshot` DTO — required anyway for client attach and reconnect — is
exactly what session persistence needs: a serializable, self-contained
representation of terminal state that can be written to disk and used to
reconstruct a `Terminal` from scratch. Solving snapshot serialization for the
multiplexer solves it for persistence at the same time.

- **Save**: write `PaneSnapshot` to disk for each pane, plus the tab/window
  topology (already needed for `StructuralEvent`). This is the same data the
  server would send to a new attaching client.
- **Restore**: the server reconstructs a `Terminal` from the snapshot using the
  same code path it uses for reconnecting clients.

The PTY process itself cannot be resurrected — only the terminal display state
(scrollback, cursor, layout) is recoverable. Running processes must be
re-launched. This is the same limitation as tmux-resurrect and is not affected
by this redesign.

**Follow-up**: research existing WezTerm session management plugins (e.g.
community Lua scripts that save/restore tab layouts) to understand how they
interact with the current multiplexer API and how this redesign would affect
them.

## Implementation Sketch

Rough phases:

1. **Add `PaneSnapshot` DTO** and serialization. Can be done independently
   without changing any behavior.

2. **Add `PtyOutput` streaming on the server**: after sending a `PaneSnapshot`,
   start forwarding raw bytes to the client in addition to continuing to feed
   the server-side emulator. Client ignores the stream initially (still uses old
   poll path) — allows testing the stream in parallel.

3. **Add shadow emulator on the client**: initialize from `PaneSnapshot`,
   consume `PtyOutput` bytes, render from local emulator. Keep the old path
   as fallback behind a flag.

4. **Replace resize path**: client handles resize locally, sends `Resize` to
   server, receives `PtyOutput` continuation. Remove `TabResized` → `ListPanes`
   cascade.

5. **Add drift detection**: compute viewport hash on server, check on client,
   trigger `ResyncRequest` on mismatch.

6. **Replace structural events**: add `StructuralEvent` messages, remove
   `TabResized` etc. from the protocol.

7. **Remove old protocol messages** once all paths migrate.
