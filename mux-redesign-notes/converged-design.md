# Replicated Mux: Converged Design

## Status

Draft — the converging design for the multiplexer redesign. Supersedes
`multiplexer-redesign.md` (the bespoke-codec sketch) and folds in the analysis
from the replicated-terminal design (determinism, snapshot, local echo, image
strategy, rollout). The transport branch that followed this note was later
paused and moved to `archive/discarded/`. Treat this document as the semantic
background, not as the source of truth for the transport choice. The archived
schema lives in
`archive/discarded/wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto`;
the rest of this document is transport-independent design context.

Last updated: 2026-06-29.

## Summary

Mux clients run a full `wezterm_term::Terminal` **shadow emulator** fed by an
ordered stream of raw PTY-output bytes from the server, instead of polling for
pre-rendered cells. The server stays the authoritative emulator (for attach,
reconnect, pane lifecycle, committed scrollback, blob storage). Clients render
from their own replica and speculate locally for responsiveness, converging to
server authority via snapshots, a viewport hash, and explicit resync.

This is the same model two independent passes arrived at; this document is the
reconciliation. It carries forward the deep findings of the replicated-terminal
work and adds the things that work surfaced as gaps: **viewport-hash drift
detection**, a **committed-scrollback log**, explicit pane lifecycle events, and
opaque client-owned layout persistence.

## Motivation

The current pull model (server emulates, clients poll `GetPaneRenderChanges`
for dirty lines and fetch via `GetLines`) has compounding costs that the
same-commit requirement makes unnecessary:

- **Resize is O(scrollback) and synchronous** (`term/src/screen.rs`
  `Screen::resize` rewraps every scrollback line), and triggers a
  `TabResized → ListPanes` cascade that locks every pane for every client. After
  a day of heavy output this can hang a resize for tens of seconds.
- The wire carries styled cells + hyperlink tables + image metadata — heavier
  than the underlying bytes for output-heavy programs.
- Predictive echo is a special-case cell-patching hack rather than a property
  of the architecture.

Since client and server are always the same commit, the client can just run the
same emulator. Resize becomes a local reflow with zero server round-trip;
output becomes a byte stream; predictive echo becomes a clean overlay.

## Goals / Non-goals

Goals: eliminate cell-level resync on resize; ground rendering on a correct
replica; reduce wire bandwidth; keep server-side mux semantics (cwd grouping,
titles, process inspection); let multi-client coherence fall out for free;
make session persistence materially easier (the snapshot DTO is the same data).

Non-goals (initial): cross-version wire compatibility (same-commit still
required); per-client independent terminal sizes; a deterministic total order
across input/output/resize/control; replacing the current mux immediately
(ships opt-in, side-by-side).

## Architecture: responsibility split

- **Server owns:** PTY + process lifecycle; the authoritative `Terminal`;
  canonical pane sizes; pane lifecycle; committed scrollback; blob/image
  storage; reconnect snapshots; opaque layout blobs keyed by persistent client
  id; per-pane focus membership sets that drive terminal focus events.
- **Client owns:** UI windows, tabs, split layout, zoom, overlays, rendering,
  the shadow `Terminal`, the predictive overlay, local resize speculation,
  active tab/pane selection, config interpretation.

Input is **pane-addressed**; focus is metadata, not input routing.

The server informs clients about authoritative terminal/session facts. It does
not command client UI. `wezterm cli` requests that target UI/presentation state
are forwarded opaquely to an eligible connected client; the mux server does not
inspect or implement them.

## The replication core

### Shadow terminal + determinism contract

Both sides run identical `term::Terminal` code (same-commit). Given identical
bytes from a common snapshot point, both emulators produce identical state. The
determinism contract and its one known caveat were validated by a spike (4000
fuzz streams + corpus); see the replicated-terminal design for full detail. Key
points carried over:

- `advance_bytes` is a pure function of prior state + bytes + a defined
  `EmulationConfig` slice (`normalize_output_to_unicode_nfc`, `unicode_version`,
  default `bidi_mode`) — that slice must match across replicas. Palette/theme is
  render-only and may differ per client.
- **No-re-chunk invariant:** one PTY read = one output event = one
  `advance_bytes` call on every replica. This keeps the internal `SequenceNo`
  in lockstep and avoids the one known divergence — grapheme clusters
  (combining marks / variation selectors) split across calls cluster wrong
  because `Performer` flushes per call. A proper fix (persist the cluster flush
  on `TerminalState`) is a prerequisite only for phases that re-chunk
  (coalescing, on-disk re-framing).
- The golden test compares full serialized state (not the rendered screen) and
  excludes the chunk-count-dependent `seqno`.

### Snapshot model

Attach/reconnect sends a `PaneSnapshot` and the client initializes its shadow
`Terminal` from it, then applies subsequent output. The snapshot is a protocol
**DTO**, not a serialized Rust `TerminalState` — but its field set must be
driven by the full state inventory (modes, margins, saved cursor, charsets, tab
stops, scroll region, alt-screen flag, unicode-version stack, kitty image
counter, `stable_row_index_offset`), or the shadow terminal diverges on attach.
"Minimum viable: lines + cursor + dims + palette" is too little.

- **Capture is atomic with subscription:** snapshot-at-seqno-N and "your stream
  starts at N+1" are decided in one critical section under the per-pane lock, at
  a clean parser boundary (parser not mid-sequence). Buffered-continuation is
  the simplest correct option.
- **Images travel encoded, not decoded.** `term` should retain `EncodedFile`/
  `EncodedLease` form and decode lazily in the client glyph cache, so the server
  never decodes and snapshots carry compressed bytes keyed by content hash
  (image pixels can otherwise be hundreds of MB). Large blobs travel by
  `BlobRef` / the existing blob-lease path. v1 may degrade animation to
  first-frame, provided it's explicit and reversible.
- The same DTO is the basis for **session persistence** (write to disk; restore
  via the same reconstruct path). Process resurrection is out of scope (same
  limit as tmux-resurrect).

### Local echo as a prediction overlay

The shadow emulator is **feed-only** — only authoritative PTY output is fed into
`advance_bytes`. Local echo is a **separate prediction overlay**, never a
mutation of the replica (feeding encoded keystrokes into `advance_bytes` is
wrong: encoded input ≠ program output, and `term` has no input-echo path). The
client sends raw key/mouse events; the **server is the sole encoder** — which
dissolves the who-encodes/mid-mode-flip race.

- Predictions are a per-glyph queue, each tied to the seqno it was made against,
  retired on confirm / contradict / timeout (the mosh-grade hard part).
  Worked case: typing `abc` with only `a` echoed keeps `b`,`c` shown as pending
  — they don't vanish.
- `input_serial` is the existing prior art for keeping the authoritative cursor
  from stomping outstanding predictions.
- **Predicted-vs-confirmed styling** is a user setting (off / underline / dim):
  cheap because predictions are individually tracked, and the natural way to
  surface lag (mosh and today's `predict_from_key_event` both do it).

### Resize

The pane has one canonical, server-arbitrated size (status quo — shared PTY).
Client window resize reflows the local replica immediately (no lag), sends a
resize request, server reflows its authoritative emulator asynchronously and
broadcasts the new canonical size to other clients. No `ListPanes` round-trip,
no dirty-line fetch, no lock cascade. Canonical/local size mismatch is handled
by clip/pan or blank-pad, with debounced non-increasing corrective requests.

### Drift detection + resync

Lockstep *should* hold, but a cheap safety net catches residual bugs (e.g. the
combining-mark caveat): the server periodically includes a **viewport hash**
(fast hash over the visible rows — not full scrollback) with the output stream;
the client compares against its own viewport; on mismatch it requests resync and
gets a fresh snapshot. Resync is just attach mid-stream — snapshot at M, resume
at M+1 — and is also the recovery path for a slow/laggy client (coalesce by
truncation).

### Committed scrollback

The live viewport may be speculative; **committed scrollback is authoritative
and server-replicated**, not derived from whatever the client's shadow terminal
produced while speculating. The server commits scrollback rows (physical rows +
soft-wrap metadata initially; more reflowable representation later) and
replicates them as a `scrollback_seq`-ordered log (commit / clear / drop-before).
This decouples authoritative history from the speculative live view.

### Pane lifecycle, focus, and layout

Pane lifecycle changes are pushed as explicit authoritative facts (`PaneCreated`,
`PaneRemoved`, `PaneExited`, `PaneSizeChanged`, title/alert updates) so clients
can update their local presentation without a broad `ListPanes` poll.

Focus is not a server-owned active pane. The server tracks, for each pane, the
set of client focus scopes that currently focus it. Terminal focus reporting is
edge-triggered by empty-set transitions: empty -> non-empty sends focus-in to
the application; non-empty -> empty sends focus-out; adding another focused
client/scope does not send another focus-in. On abrupt disconnect the server
removes that connection's focus scopes and applies the same edge logic.

Tabs, split trees, and GUI windows are client-owned layout/presentation state,
not authoritative mux resources. For reconnect, the server persists an opaque
layout blob keyed by persistent client id, updated by the owner client via an
explicit request/response. The MVP has no layout events: a lost or delayed
layout update is recoverable because terminal state remains authoritative.
Future read-only/follower clients and layout cloning should remain possible, but
are not part of the MVP protocol.

### Flow control

With the gRPC transport (decided below), **HTTP/2 per-stream flow control
subsumes the manual ack + water-mark backpressure** scheme — a slow/flooding
pane backpressures only its own stream (proven in the viability spike), and the
server bounds a per-stream queue. The manual solicited-ack + low/high-water +
resync design from the replicated-terminal work is the fallback only if a
non-HTTP/2 transport is ever chosen.

## Transport and RPC framework

**Decision: gRPC (tonic).** A framework spike compared the options against the
host constraints (smol + main-thread `!Send` `Mux`, no tokio; transport over SSH
stdio + Unix sockets; many small interactive messages + high-throughput bursts).

Why gRPC fits *this* design specifically:

- **Per-pane stream independence is the real win.** The design pushes output for
  many panes plus control plus input concurrently. Over a single bespoke
  connection, one pane dumping output (or a large snapshot) **head-of-line-blocks
  every other pane's traffic** — a flooding pane delays another pane's keystroke
  echo. HTTP/2 multiplexes independent streams with per-stream flow control, so a
  slow/flooding pane backpressures only itself (proven in the viability spike).
  Matching this on the bespoke transport would mean re-implementing HTTP/2-style
  per-stream windowing by hand.
- **The protocol is written in protobuf already** (goal #1), with native
  unary/server-stream/client-stream/bidi mapping 1:1, plus cross-language
  clients and grpcurl/reflection tooling.
- Both required transports are proven: Unix socket directly; SSH stdio over a
  raw `AsyncRead+AsyncWrite` (h2c + one `Connected` newtype).

The price — real but contained: tonic hard-requires tokio, so the gRPC domain
runs a **dedicated tokio runtime on its own thread, bridged to the main-thread
`Mux` over `flume`** (the host runs no tokio — `promise/src/spawn.rs:40`).
Experiment 9 proved the bridge works cleanly with tidy shutdown, confined to the
experimental domain.

Alternatives considered:

- **Cap'n Proto RPC — held in reserve.** The *only* framework whose runtime
  model (`!Send`, single-threaded, `Rc`-based) matches wezterm's main-thread
  `!Send` `Mux`, so it could run on the existing smol/`promise` main thread with
  **no second runtime and no bridge** — erasing tonic's one real cost — and its
  raw-byte-stream transport is even cleaner for SSH stdio. Not the default
  because it abandons the protobuf IDL, has no first-class streaming or free flow
  control (hand-built backpressure), and trades ubiquitous gRPC tooling/clients
  for a niche ecosystem. **Revisit only if the tokio bridge proves too costly in
  practice**; the deciding experiment would be a short spike driving `RpcSystem`
  on the `promise` main thread.
- **Protobuf over the existing transport ("drop a level").** Keep the bespoke
  framing/dispatch + the working Unix/TLS/SSH transports, swap PDU bodies
  varbincode → prost; no tokio. A spike confirmed the body codec is cleanly
  separable (called in 4 places inside the `pdu!` macro) and prost bodies
  round-trip in the identical leb128 frame with no async runtime. On its own this
  is a *different goal*: it fixes the **current** mux's versioning pain (retiring
  the `CODEC_VERSION` hard-fail treadmill, enabling field-level evolution)
  without delivering the streaming redesign, and it inherits the head-of-line
  problem above. It's the right near-term move if the itch is "stop breaking the
  wire on every change," and a viable **no-tokio variant** of the streaming
  design if avoiding the second runtime ever becomes paramount (build the new
  streaming PDUs on the existing transport + the manual flow-control design).
- volo, grpcio, ttrpc, tarpc, JSON-RPC each fail a load-bearing requirement (no
  runtime win; C-core can't take SSH stdio + native build burden; no HTTP/2 flow
  control; Rust-only / no IDL; no IDL + poor binary streaming).

Low-regret note: the **prost message definitions were reusable across the
active options**, and serializing `Line`/cell attributes to protobuf was the
hard part regardless. The first schema now lives in
`archive/discarded/wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto`;
this paragraph is historical context, not a current implementation directive.

## Rollout and coexistence

Ships **opt-in behind a global flag** (default off), side-by-side with the
existing implementation, even though the end state is to be the only one.
Flag-off is byte-identical to today (old `ClientPane` path untouched; new
replica path added alongside). The server serves both from the one canonical
`Terminal`. Lifecycle: default-off → default-on once proven → remove the old
path + flag. Upstream engagement happens once the feature is proven; removal is
bundled into the merge or deferred per upstream's risk appetite.

## Open questions / follow-ups

- The transport/RPC-framework decision (the spike above).
- Exact `PaneSnapshot`/`TerminalSnapshot` field set (drive from the state
  inventory; balance attach cost vs divergence).
- Committed-scrollback granularity (physical rows now; reflowable later).
- Queue-overflow policy on a stuck stream (block / drop-and-resync / coalesce).
- The grapheme-flush fix in `term` (needed before any re-chunking phase).
- Real-`ssh --stdio` transport retest + RTT-over-SSH confirmation.
- Per-client virtual viewports (independent reflow widths) — deferred.
