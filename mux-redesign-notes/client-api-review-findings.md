# Client API Review Findings

## Status: open — blocks the next pass

This reviews `replicated-mux-types` as of the commit that introduced
`src/client.rs` (the `MuxClient -> MuxConnection -> MuxSession ->
MuxPane`/`MuxPaneTombstone` handle hierarchy) and `src/version.rs`
(interface/implementation version negotiation). Method: a codex agent did a
read-only pass over the client-to-pane API surface, deliberately excluding
fields/shapes already marked as placeholders in doc comments (`CreatePaneRequest`'s
empty body, `PaneSummary`'s minimal fields, `PaneOutput`'s tentative shape,
`MuxPaneTombstone`'s missing scrollback access, the open pane-visibility/ACL
question, `PaneSnapshot<State>`'s generic `State`), cross-checked against
`mux-redesign-notes/` itself, the archived `wezterm-grpc-mux-proto` v2 schema,
and the existing (currently-shipping) `mux`/`wezterm-client`/`codec` crates.
Every finding below was independently re-verified against the actual source
before being recorded here.

These need to be addressed before the next pass (fleshing out pane details,
event flow, and the snapshot/scrollback representation) — and that next pass
should be expected to surface *more* findings that loop back into this same
list, not just forward progress on its own topics. Treat this file as living
until it's empty.

## Findings, most severe first

1. **No operation returns a pane's initial/resync snapshot.**
   `MuxSession::get_pane` returns a live `MuxPane` with no `PaneSnapshot`;
   `PaneOutput::recv` only yields incremental `OutputEvent`s, which presume
   the caller already has a base state to apply deltas to. No method
   anywhere produces a `PaneSnapshot<State>`. This is not the excluded
   "`State`'s fields aren't fleshed out" placeholder — the *operation* to
   obtain one doesn't exist. Without it the shadow-terminal replication
   model this whole crate exists to support cannot bootstrap:
   `ReplicaTerminal::resync` takes a snapshot argument that nothing can
   supply.

2. **`ControlEvent` and `PaneLifecycleEvent` are fully defined but
   unreachable from the client topology.** Nothing in `client.rs` lets a
   caller send a `ControlEvent` (focus-scope changes, resync requests) or
   receive a `PaneLifecycleEvent` (created/exited/removed/title-changed).
   The docs say lifecycle changes are "pushed to clients instead of
   requiring a broad `ListPanes`-style poll," but there is no channel for
   that push to arrive on.

3. **`PaneSnapshot` dropped half of the continuation cursor.** It only
   carries `seqno: SequenceNo`, not `ScrollbackSeq`. `events.rs` documents
   scrollback as ordered independently of `SequenceNo`, and a snapshot
   needs to let a client resume both atomically — exactly the problem the
   superseded v2 proto's `PaneCursor` (`pane_output_seq` +
   `scrollback_seq`, deliberately paired) solved. This is a regression
   relative to the design this crate replaced, not a fresh gap.

4. **`ControlEvent::FocusScope` still has no scope id.** Flagged as an
   incidental finding in an earlier session pass and never circled back to;
   codex re-found it independently. The doc comment says the server tracks
   a *set* of focus scopes per pane, but the event is `{ pane_id, focused }`
   — no id to distinguish two scopes (e.g. two GUI windows) focusing the
   same pane.

5. **`PaneDims` dropped pixel size and DPI.** The currently-shipping
   `TerminalSize` (`term/src/terminal.rs`) is `{ rows, cols, pixel_width,
   pixel_height, dpi }`, all five load-bearing today (font rendering,
   image/graphics scaling). `PaneDims` is `{ rows, cols }` only, with
   nothing marking the smaller shape as deliberate. Reads as an oversight,
   not a stub.

6. **No layout deletion, despite the doc claiming one.**
   `MuxConnection::get_layout`/`update_layout`'s doc says blobs are
   "persisted until explicitly deleted," but there is no delete operation
   and `update_layout` takes `LayoutBlob`, not `Option<LayoutBlob>`.

7. **"Blob store" is named in `mux-design-restart.md`'s semantic-core list
   but has no representation here.** `LayoutBlob` is a distinct, specific
   concept (layout persistence only). There is no `BlobId`/`BlobRef`/
   fetch-by-id surface for the general content-addressed blobs (e.g.
   images) the old proto's `GetBlobRequest` handled.

8. **`ConnectOptions` can't carry what `connect()`'s own doc says it
   establishes.** `connect()` and `MuxConnection::client_id()` both assert
   a persistent `ClientId` and `ReconnectPolicy` get established via
   `ConnectOptions`, but the struct only has a `versions` field; everything
   else is a "fields TBD" comment. Borderline against the excluded-stub
   list, but the doc comments assert behavior the type can't currently
   satisfy, which is a step beyond an inert TBD.

9. **Version `Default` impls can silently produce a meaningless-but-passing
   value.** `InterfaceVersion`/`InterfaceVersions`/`PeerVersions`/
   `ConnectOptions` all derive `Default`, yielding interface version
   `0.0.0`. `ConnectOptions` is `#[non_exhaustive]`, which forces every
   external caller through `..Default::default()` to construct one at all.
   If either peer ever reaches for `Default::default()` instead of
   `InterfaceVersions::current_types_only()`, `check_compatible`'s
   `ExactMatch` policy sees `0.0.0 == 0.0.0` and happily accepts the
   connection — the most idiomatic way to construct the struct silently
   defeats the whole version-negotiation mechanism.

## Additional observation (not from the codex pass)

`layout.rs` says "the MVP protocol has no layout *events*," but
`events.rs` has `ControlEvent::StoreLayout { client_id, blob }` sitting
right next to `MuxConnection::update_layout()` — two different-looking
mechanisms for what may be the same operation, with nothing stating which
one is real (or whether `StoreLayout` is just the wire-level shape
`update_layout` sends, which wouldn't be a contradiction, just currently
unstated). Surfaced while verifying finding 2; worth resolving alongside it.
