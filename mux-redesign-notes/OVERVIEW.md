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

## Current state (2026-07-05)

- **Design reset.** [`mux-design-restart.md`](mux-design-restart.md) is the
  current entry point and the thing to read first.
- **Semantic design still matters.** [`converged-design.md`](converged-design.md)
  remains the best writeup of the shadow-terminal / replicated-mux direction,
  but its transport choice is no longer the source of truth.
- **Archived branch.** The protobuf/gRPC branch, its proto crate, and the
  gRPC-specific investigation notes now live under
  [`archive/discarded/`](archive/discarded/).
- **First Rust API pass exists.** The `replicated-mux-types` crate (repo root)
  names the semantic core in Rust: the replication boundary (ids, events,
  snapshot, the authoritative/replica terminal role split, layout blobs,
  interface/implementation version negotiation) and, in `src/client.rs`, the
  client-side connection topology (`MuxClient -> MuxConnection ->
  MuxSession -> MuxPane`/`MuxPaneTombstone`). No transport, no
  implementation — traits and DTOs only.
- **A review found real gaps in that first pass.**
  [`client-api-review-findings.md`](client-api-review-findings.md) lists
  them (most serious: no operation returns a pane's initial/resync
  snapshot, and `ControlEvent`/`PaneLifecycleEvent` are unreachable from
  the client API). Status: open, blocking further depth until addressed.
- **Cross-branch note:** the deeper replicated-terminal analysis (determinism
  contract, snapshot inventory, local-echo overlay, image strategy, rollout)
  lives in `docs/mux-replicated-terminal-design.md` on the
  `mux-replicated-terminal-design` bookmark (pushed to `private-fork`), *not*
  on this `notes` branch. The converged design folds in its conclusions and
  cites it for depth. These two branches are not yet merged.

## The documents

| Doc | What it is | State |
|---|---|---|
| [`mux-design-restart.md`](mux-design-restart.md) | Summary of the current conclusion and next-step order. | **Active — entry point** |
| [`converged-design.md`](converged-design.md) | The replicated-terminal semantic design and historical transport discussion. | **Active — background** |
| [`client-api-review-findings.md`](client-api-review-findings.md) | Gaps/modeling errors found reviewing `replicated-mux-types`' client-to-pane API. | **Active — open, blocking** |
| [`archive/discarded/README.md`](archive/discarded/README.md) | Index of the protobuf/gRPC branch and other discarded notes. | **Active — archive index** |
| [`multiplexer-redesign.md`](multiplexer-redesign.md) | The original bespoke-codec shadow-emulator redesign — detailed perf analysis, testing strategy, implementation sketch. | Superseded by `converged-design.md`; useful background |
| [`mux-protocol-and-tmux-comparison.md`](mux-protocol-and-tmux-comparison.md) | Reference: the *current* wire format (framing, varbincode, transports) + native-mux-vs-`tmux -CC` comparison. | Reference / background |
| [`focus-and-identity.md`](focus-and-identity.md) | Investigation of a focus-echo feedback loop under latency; establishes that input is pane-addressed, focus is auxiliary, identities exist on both ends but notifications are identity-blind. | Investigation — feeds structural-events + identity-aware notifications; has a pre-redesign bug fix |
| [`minimal-mux-server.md`](minimal-mux-server.md) | Thesis: shrink the server to socket-listener + PTY-spawner + VT-parser; push domain logic to the client; drop SSH-in-server, extra domain types, Lua. | Adjacent track (server simplification) — informs the responsibility split |
| [`mux-server-spawn-improvements.md`](mux-server-spawn-improvements.md) | Two concrete fixes: `posix_spawn` instead of `fork` on macOS; socket activation to kill the connect-retry race. | Adjacent track — independent near-term improvements |

## Suggested reading order

1. This file.
2. [`mux-design-restart.md`](mux-design-restart.md) — the current conclusion.
3. [`converged-design.md`](converged-design.md) — the semantic design.
4. [`archive/discarded/README.md`](archive/discarded/README.md) — the archived gRPC/proto branch.
5. The rest as needed for background (`multiplexer-redesign.md`), the current
   protocol (`mux-protocol-and-tmux-comparison.md`), or adjacent tracks.

## Current implementation decisions

- **Decision process:** treat the Rust semantic model as the source of truth.
  Do not let a transport or encoding choice define the domain model.
- **Archived branch:** the protobuf/gRPC branch stays visible under
  `archive/discarded/` until a mock or PoC gives us a reason to revive it.

## Possible next steps

Not a committed plan — options, roughly ordered by how directly they advance the redesign:

1. **Resolve [`client-api-review-findings.md`](client-api-review-findings.md)**
   before going deeper on `replicated-mux-types` — the planned next depth
   pass (pane details, event flow, snapshot/scrollback representation) is
   expected to surface more findings that loop back into the same list, so
   the current ones should be settled first.
2. **Productionize the determinism golden test** (replicated-terminal step 1) —
   the spike already validated it; harden it as a permanent test.
3. **The `term` grapheme-flush fix** — move grapheme clustering off the per-call
   `Performer`; prerequisite for any phase that re-chunks the byte stream.
4. **Real-`ssh --stdio` transport retest + RTT-over-SSH** — confirm the
   loopback transport results over a real SSH link.
5. **Reconcile the branches** — bring `docs/mux-replicated-terminal-design.md`
   (on `private-fork`) together with these notes, or cross-link them, so there's
   one navigable design surface.
6. **Adjacent tracks, independently shippable:** the focus-echo bug fix
   (identity-aware notifications), the minimal-mux-server simplification, and the
   spawn improvements (`posix_spawn` + socket activation).

Future sessions may still diverge onto tangents — but that should be a choice,
not a result of missing context. Update this file when the state moves.
