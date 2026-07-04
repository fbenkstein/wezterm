# Mux Redesign — Overview

Entry point for the multiplexer-redesign notes. Start here.

## What this is

An effort to redesign WezTerm's multiplexer so that **mux clients run a full
`wezterm_term::Terminal` shadow emulator** fed by a stream of raw PTY bytes,
instead of polling the server for pre-rendered cells. The server stays
authoritative; clients render from their own replica and speculate locally for
responsiveness. Motivation: the current pull model makes resize O(scrollback)
and synchronous (tens-of-second hangs after a day of use), carries heavy
pre-rendered cells on the wire, and bolts on predictive echo as a hack.

## Current state (2026-07-04)

- **Design: converging.** [`converged-design.md`](converged-design.md) is the
  current design and the thing to read first. Its replication core is written;
  the transport/RPC-framework layer is **decided: gRPC (tonic)**.
- **Transport viability: settled → GO.** All gating experiments pass (Unix
  socket, SSH-stdio-shaped raw stream, ~16µs RTT, HTTP/2 per-stream flow
  control, and the in-process tokio↔`promise` runtime bridge). Recorded in
  [`grpc-viability-experiments.md`](grpc-viability-experiments.md). gRPC chosen
  over Cap'n Proto RPC (the reserve option) and "protobuf-over-existing-
  transport" (a different goal — near-term versioning fix).
- **Protocol schema: started.** The authoritative protobuf/gRPC schema now
  lives in
  [`../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto`](../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto).
  [`streaming-mux-protobuf-protocol.md`](streaming-mux-protobuf-protocol.md) is
  now an index/design-intent note pointing at that schema.
- **Nothing implemented yet** — this is all design/spike work.
- **Cross-branch note:** the deeper replicated-terminal analysis (determinism
  contract, snapshot inventory, local-echo overlay, image strategy, rollout)
  lives in `docs/mux-replicated-terminal-design.md` on the
  `mux-replicated-terminal-design` bookmark (pushed to `private-fork`), *not*
  on this `notes` branch. The converged design folds in its conclusions and
  cites it for depth. These two branches are not yet merged.

## The documents

| Doc | What it is | State |
|---|---|---|
| [`converged-design.md`](converged-design.md) | The reconciled design: replication core + the gRPC transport decision. **Read first.** | **Active — source of truth** |
| [`grpc-viability-experiments.md`](grpc-viability-experiments.md) | gRPC viability experiment plan + recorded results (verdict: GO). | **Active — settled** |
| [`../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto`](../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto) | The authoritative protobuf/gRPC schema for the experimental mux. | **Active — protocol source** |
| [`streaming-mux-protobuf-protocol.md`](streaming-mux-protobuf-protocol.md) | Short protocol intent/index note that points to the `.proto`. | Active reference |
| [`multiplexer-redesign.md`](multiplexer-redesign.md) | The original bespoke-codec shadow-emulator redesign — detailed perf analysis, testing strategy, implementation sketch. | Superseded by `converged-design.md`; useful background |
| [`protobuf-protocol-design.md`](protobuf-protocol-design.md) | Protobuf as a *body encoding* over the existing transport ("drop a level"). | Superseded for the new streaming impl; still relevant as the near-term versioning-fix option |
| [`mux-protocol-and-tmux-comparison.md`](mux-protocol-and-tmux-comparison.md) | Reference: the *current* wire format (framing, varbincode, transports) + native-mux-vs-`tmux -CC` comparison. | Reference / background |
| [`focus-and-identity.md`](focus-and-identity.md) | Investigation of a focus-echo feedback loop under latency; establishes that input is pane-addressed, focus is auxiliary, identities exist on both ends but notifications are identity-blind. | Investigation — feeds structural-events + identity-aware notifications; has a pre-redesign bug fix |
| [`minimal-mux-server.md`](minimal-mux-server.md) | Thesis: shrink the server to socket-listener + PTY-spawner + VT-parser; push domain logic to the client; drop SSH-in-server, extra domain types, Lua. | Adjacent track (server simplification) — informs the responsibility split |
| [`mux-server-spawn-improvements.md`](mux-server-spawn-improvements.md) | Two concrete fixes: `posix_spawn` instead of `fork` on macOS; socket activation to kill the connect-retry race. | Adjacent track — independent near-term improvements |

## Suggested reading order

1. This file.
2. [`converged-design.md`](converged-design.md) — the design.
3. [`grpc-viability-experiments.md`](grpc-viability-experiments.md) — why gRPC, and what's proven.
4. [`../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto`](../wezterm-grpc-mux-proto/proto/wezterm/streaming_mux/v1/streaming_mux.proto) — protocol/IDL detail.
5. [`streaming-mux-protobuf-protocol.md`](streaming-mux-protobuf-protocol.md) — protocol intent/index note.
6. The rest as needed for background (`multiplexer-redesign.md`), the current
   protocol (`mux-protocol-and-tmux-comparison.md`), or adjacent tracks.

## Possible next steps

Not a committed plan — options, roughly ordered by how directly they advance the redesign:

1. **Wire codegen for `wezterm-grpc-mux-proto`.** The schema crate exists and
   owns the `.proto`; the next step is tonic/prost generation using vendored
   protoc or `protox`, without requiring a system protobuf compiler.
2. **Prototype the experimental gRPC domain** end-to-end — the 7 production
   touch points from the viability study (tokio runtime + `flume` bridge, server
   listener, `GrpcClientDomain`, client config/connect, SSH adapter, build
   codegen, IDL crate), behind the opt-in flag.
3. **Productionize the determinism golden test** (replicated-terminal step 1) —
   the spike already validated it; harden it as a permanent test.
4. **The `term` grapheme-flush fix** — move grapheme clustering off the per-call
   `Performer`; prerequisite for any phase that re-chunks the byte stream.
5. **Real-`ssh --stdio` transport retest + RTT-over-SSH** — confirm the
   loopback transport results over a real SSH link.
6. **Reconcile the branches** — bring `docs/mux-replicated-terminal-design.md`
   (on `private-fork`) together with these notes, or cross-link them, so there's
   one navigable design surface.
7. **Adjacent tracks, independently shippable:** the focus-echo bug fix
   (identity-aware notifications), the minimal-mux-server simplification, and the
   spawn improvements (`posix_spawn` + socket activation). The drop-a-level
   protobuf-body swap is also a standalone near-term option to retire the
   `CODEC_VERSION` hard-fail treadmill on the *current* mux.

Future sessions may still diverge onto tangents — but that should be a choice,
not a result of missing context. Update this file when the state moves.
