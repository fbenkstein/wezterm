# Replicated Mux: Converged Design

## Status

Draft — the converging design for the multiplexer redesign. Supersedes
`multiplexer-redesign.md` (the bespoke-codec sketch) and folds in the analysis
from the replicated-terminal design (determinism, snapshot, local echo, image
strategy, rollout). The **transport / RPC-framework layer is pending** a
framework spike (see [Transport](#transport-and-rpc-framework-pending)); the
rest of this document is transport-independent.

Last updated: 2026-06-29.

## Summary

Mux clients run a full `wezterm_term::Terminal` **shadow emulator** fed by an
ordered stream of raw PTY-output bytes from the server, instead of polling for
pre-rendered cells. The server stays the authoritative emulator (for attach,
reconnect, topology, committed scrollback, blob storage). Clients render from
their own replica and speculate locally for responsiveness, converging to
server authority via snapshots, a viewport hash, and explicit resync.

This is the same model two independent passes arrived at; this document is the
reconciliation. It carries forward the deep findings of the replicated-terminal
work and adds the three things that work surfaced as gaps: **viewport-hash
drift detection**, a **committed-scrollback log**, and **structural/topology
events**.

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
  canonical pane/tab sizes; tab/pane topology; committed scrollback; blob/image
  storage; reconnect snapshots; server-originated commands (e.g. forwarded
  `wezterm cli activate-pane`).
- **Client owns:** UI windows, rendering, the shadow `Terminal`, the predictive
  overlay, local resize speculation, local active-pane selection, config
  interpretation.

Input is **pane-addressed**; focus is metadata, not input routing.

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

### Structural / topology events

Topology changes (splits, closes, moves, focus, layout) are pushed as explicit
**structural events** (`PaneAdded/Removed`, `TabCreated/Closed`,
`PaneResized`, `FocusChanged`, `SplitLayout`) so the client updates its local
topology model with no RPC, eliminating the `TabResized → ListPanes` cascade.

### Flow control

If the transport is HTTP/2 (gRPC), **per-stream flow control subsumes the
manual ack + water-mark backpressure** scheme — a slow pane backpressures only
its own stream (proven in the viability spike), and the server bounds a
per-stream queue. If the transport keeps the existing communication layer, the
manual solicited-ack + low/high-water + resync design from the replicated-
terminal work applies instead. **This choice depends on the transport decision
below.**

## Transport and RPC framework (pending)

This is the one open layer. gRPC/tonic is **viable** (all gating experiments
pass — Unix socket, SSH-stdio-shaped raw stream, ~16µs RTT, HTTP/2 flow control,
and the in-process tokio↔`promise` bridge). But before committing, a spike is
evaluating three framings:

1. **Full gRPC (tonic):** standard IDL, generated/alt-language clients,
   reflection tooling, HTTP/2 streaming + flow control for free. Cost: a
   dedicated tokio runtime bridged to the main-thread `promise` executor (proven
   workable), and `tonic`/`prost` deps (already largely in the lock).
2. **Other RPC frameworks** (Cap'n Proto RPC, tarpc, volo, ttrpc, …) — may fit
   the non-tokio host or arbitrary-byte-stream transport better.
3. **Drop a level:** keep the existing transport/dispatch (Unix/TLS/SSH-stdio,
   all working, no tokio) and only swap the PDU **body** encoding
   varbincode → protobuf (prost). Minimal change; keeps smol/promise; loses the
   gRPC streaming/flow-control framework (we'd keep the existing PDU
   multiplexing + the manual backpressure design).

The spike's recommendation fills in this section: the framing, the IDL, the
flow-control mechanism, and the SSH/Unix transport adaptation. Everything above
is independent of which option wins.

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
