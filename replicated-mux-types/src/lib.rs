//! `replicated-mux-types` models the semantic boundary of the mux redesign
//! described under `mux-redesign-notes/`: the transport-independent contract
//! between the authoritative server terminal and a client-side terminal
//! replica.
//!
//! This crate is deliberately *not* a transport or encoding layer: it takes
//! no position on gRPC vs. Cap'n Proto vs. anything else, and it does not
//! reimplement `wezterm_term`'s terminal state. The cross-cutting facts that
//! are already settled — identifiers, the snapshot/resync framing, the
//! input/output/control events, and the authoritative/replica role split —
//! live here. The terminal emulator's own state representation stays
//! generic (`PaneSnapshot<State>`) so this crate does not need to depend on
//! `wezterm-term` or fix the still-open "exact snapshot field set" question.
//!
//! Start with [`ReplicatedTerminal`], [`AuthoritativeTerminal`], and
//! [`ReplicaTerminal`] in the [`terminal`] module for the role split, then
//! [`PaneSnapshot`] for the snapshot/resync contract and the `events`
//! module for the traffic that crosses the boundary.
//!
//! The [`client`] module is a different layer: the client-side connection
//! topology (`MuxClient` -> `MuxConnection` -> `MuxSession` ->
//! `MuxPane`/`MuxPaneTombstone`) that a client program is actually written
//! against, built out of the types above. A session is an ephemeral,
//! connection-scoped container for a client's own pane subscriptions, not
//! a server-owned grouping of panes — panes are not grouped at the server
//! at all. [`version`] is what makes that interface itself versioned: the
//! semver-numbered interface-version axis `connect()` can refuse on, kept
//! separate from the advisory-only implementation-version axis behind it.
//!
//! See `mux-redesign-notes/mux-design-restart.md` and
//! `mux-redesign-notes/converged-design.md` for the design rationale this
//! crate is derived from. The archived `wezterm-grpc-mux-proto` protobuf
//! schema was intentionally not used as a source for this API.

mod client;
mod events;
mod ids;
mod layout;
mod snapshot;
mod terminal;
mod version;

pub use client::*;
pub use events::*;
pub use ids::*;
pub use layout::*;
pub use snapshot::*;
pub use terminal::*;
pub use version::*;
