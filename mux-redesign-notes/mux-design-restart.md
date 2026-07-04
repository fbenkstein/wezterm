# Mux Design Restart

## Status

This is the current summary of the mux redesign discussion after the gRPC /
protobuf branch was paused.

The short version: the `wezterm-grpc-mux-proto` branch was useful as a
pressure test, but it should not be the source of truth for the next
iteration. The right next move is to define the mux in Rust first, from
semantics outward, then choose transport and encoding as projections of that
model.

## Current conclusion

- Keep SSH as the bootstrap and auth path. It is boring, well understood, and
  already solves identity and transport security.
- Treat QUIC as an attractive transport candidate, not a design starting
  point. It matters for latency, stream independence, and fairness, but only
  after the semantics are stable.
- Do not let protobuf/gRPC drive the protocol shape. The `v2` schema is a
  useful inventory of prior decisions, but it already encodes transport-shaped
  assumptions and RPC boundaries.
- Re-derive the next iteration from first principles in Rust.

## What the Rust interface should define first

The Rust model should name the domain objects and invariants directly:

- session
- pane
- snapshot
- output event
- input event
- control event
- layout blob
- blob store
- reconnect policy
- resync contract

That is the semantic core. Transport comes after that.

## What to keep from the previous work

- The shadow-terminal direction from `converged-design.md` still looks right.
- The split between authoritative server state and client-owned UI state still
  looks right.
- The observations about resize, scrollback, image handling, and local echo are
  still relevant.
- The archived gRPC/protobuf branch remains useful as a reference for what was
  tried and why it felt off.

## Recommended next step

1. Write the Rust API for the mux domain first.
2. Make the semantics explicit in the type system and traits.
3. Revisit transport only after the domain model feels clean.
4. Use protobuf only if it later proves to be a good projection of that model.

## Archived material

The following branch is now preserved under
`archive/discarded/wezterm-grpc-mux-proto/` and should be treated as historical
reference until a mock or PoC gives us a reason to revive it:

- the proto crate itself
- the protobuf/gRPC protocol notes
- the gRPC viability notes
- the investigation tracker that was specific to that branch

