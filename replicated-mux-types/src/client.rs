//! The client-side connection topology: the handle hierarchy a client uses
//! to get from "I have a way to reach a server" down to "I can read and
//! write one pane's terminal." This is a different layer than
//! [`crate::terminal`]/[`crate::events`]: those model the replication
//! boundary's *data*; this module models the *object shape* a client
//! program is written against.
//!
//! The story, top to bottom:
//!
//! - [`MuxClient`] is the starting point. It knows how to reach a server
//!   (transport/address/auth are its concern, not this crate's) and
//!   [`MuxClient::connect`] yields a [`MuxConnection`].
//! - [`MuxConnection`] is one live transport connection. It offers
//!   connection-scoped commands: discover panes on the server, get/update
//!   this client's layout blob, simple maintenance queries, and open a
//!   session. Panes are not grouped or owned by anything at this level —
//!   see the next section.
//! - [`MuxSession`] is an *ephemeral, connection-scoped container a
//!   client uses to manage its own pane subscriptions* — not a durable,
//!   server-owned grouping of panes. It holds its connection internally
//!   (so a caller that only kept the `Session` still has a live
//!   connection) and exposes it back out via [`MuxSession::connection`].
//!   It offers subscription commands: create a pane, or get a handle to
//!   an existing one.
//! - Getting a pane yields either a [`MuxPane`] or a [`MuxPaneTombstone`]
//!   (see [`PaneHandle`]) depending on whether the pane is still alive.
//!   Both hold their session internally the same way a session holds its
//!   connection. [`MuxPane`] is where terminal I/O happens:
//!   [`MuxPane::send_input`] (see [`crate::events::InputEvent`], which
//!   already covers resize via `ResizeRequest`) and [`MuxPane::output`].
//!
//! # Sessions are ephemeral subscription containers, not pane groups
//!
//! An earlier draft of this module gave `SessionId` a different meaning:
//! a durable, server-owned container that panes belonged to, mirroring
//! the archived proto's `SessionSummary { session_id, panes }`. That
//! turned out to be modeling something the server doesn't actually need
//! to know about. The server only knows clients, by [`ClientId`]/
//! [`ConnectionId`], and panes, by `PaneId`; there is no server-side
//! grouping between them. Arranging panes into tabs, splits, or stacks is
//! entirely a client concern, and *that* grouping already has a home:
//! [`MuxConnection::get_layout`]/[`update_layout`](MuxConnection::update_layout)
//! (opaque to the server, keyed by [`ClientId`], persisted until the
//! client explicitly deletes it — the server does not diff or expire it).
//! A [`MuxSession`] is a much narrower thing: purely local bookkeeping for
//! *which panes this client currently has open* — subscribed to output,
//! or able to send input to. Closing a session means dropping all of
//! that: unsubscribing from every pane it's subscribed to, not deleting
//! the panes themselves or anything persisted in a layout. Layouts are
//! independent of what's currently subscribed: a layout can reference
//! panes the client isn't currently subscribed to (or that no longer
//! exist — see [`MuxPaneTombstone`]).
//!
//! Whether an implementation lets a connection open more than one session
//! at a time, or open a new one after closing the last, is deliberately
//! left to the implementation. An MVP may enforce a strict 1:1
//! connection:session relationship and fail a second
//! [`MuxConnection::open_session`] call; nothing in these traits requires
//! that, the same way nothing prevents getting the same pane twice (see
//! below) — the type doesn't need to forbid what a given deployment
//! chooses to police at runtime.
//!
//! # How much the server controls each layer
//!
//! The four layers sit at different points on a gradient of how much the
//! server can do without the client's involvement, and that gradient is
//! what "handle-like" (next section) is actually protecting against, more
//! than any particular Rust mechanism:
//!
//! - [`MuxClient`] is purely local — it doesn't correspond to a live
//!   server-side resource at all, so the server has no way to affect it.
//! - [`MuxConnection`] is mostly local, but the server can sever it (and
//!   the client's own [`MuxConnection::terminate`] can too); from then on,
//!   [`MuxConnection::list_panes`] and friends observe `Self::Error`.
//! - [`MuxSession`] is more server-controlled still — e.g. an MVP
//!   enforcing 1:1 connection:session (above) means the server's view of
//!   "is a session open" can constrain what a client is able to do
//!   locally.
//! - A pane is not an object the client owns at all: the server can kill
//!   it — becoming a [`MuxPaneTombstone`] — regardless of what the client
//!   is doing or how many handles to it exist.
//!
//! At every layer, a client has to be able to react to a state transition
//! or event it didn't ask for, at any time. That's the actual requirement
//! the next section's "handle-like" is standing in for — not any specific
//! promise about duplication.
//!
//! # Ownership: no lifetimes, handle-like objects instead
//!
//! None of these traits carry a lifetime parameter, and none of them
//! require `Clone` either — those are two separate properties, and only
//! the first is settled. A `Session` holds its `Connection`, and a
//! `Pane`/`PaneTombstone` holds its `Session`, by value, which is enough
//! on its own to rule out lifetimes: a real implementation satisfies it
//! by backing these types with `Rc`/`Arc` (or an equivalent
//! shared-ownership mechanism) internally, entirely hidden behind each
//! accessor method's signature — `fn connection(&self) -> Self::Connection`
//! never says *how* an owned `Connection` gets produced from `&self`;
//! that's the implementor's business, invisible to the trait. Whether the
//! public type is *also* `Clone` — so that generic code holding an
//! `S: MuxSession` can freely duplicate it — is a separate decision this
//! crate doesn't make: these traits are not `Clone`-bound.
//!
//! What actually needs settling is narrower: can more than one independent
//! handle to the same live entity exist at once? Yes, and the existing
//! methods already provide that without needing `Clone`: going back
//! through a parent accessor (`session.connection()`), or re-querying
//! (`connection.open_session()`, `session.get_pane(id)` — both already
//! documented as freely repeatable, not exclusive). That covers a
//! multi-threaded client (partly forced by OS restrictions on which
//! thread can do what) reasonably well: a component that only needs a
//! `Pane` can hold just that, on whatever thread it runs on, rather than
//! juggling its whole ancestry to stay able to reach it. The difference
//! from `Clone` is real, though: these calls re-validate against the
//! server (may be `async`, may fail), where `Clone` would be a cheap,
//! always-succeeds duplication of possibly-already-stale in-memory state
//! — and given a pane in particular is not something the client owns at
//! all (see the previous section), re-validating is arguably the better
//! default to reach for first. A concrete implementation is free to
//! *also* implement `Clone` on its own types if it wants cheap
//! duplication too; this crate just doesn't require it.
//!
//! Getting the same pane twice from the same session, or opening two
//! sessions on the same connection where that's allowed, is not
//! specially prevented — it's a nonsensical thing for a real client to
//! do, but preventing it would require bookkeeping this layer doesn't
//! otherwise need. Compare handles by their id accessor (`session_id()`,
//! `pane_id()`, ...), not by handle identity — there is no `PartialEq` on
//! the handles themselves.
//!
//! One consequence: [`MuxConnection::terminate`], [`MuxSession::close`],
//! [`MuxPane::close`], and [`MuxPaneTombstone::dismiss`] all take `&self`,
//! not `self` by value, even though consuming `self` looks at first like
//! it would prevent use-after-teardown. It wouldn't, `Clone`-bound or
//! not: a `Session`/`Pane` holds its own internal handle on its parent,
//! obtained independently of whatever handle a caller used to tear that
//! parent down, so consuming the one handle a caller calls
//! `terminate`/`close`/`dismiss` on can never reach the other live
//! handles anyway — the cascading invalidation already has to go through
//! shared state (see "Cascading teardown", below) regardless. Unlike the
//! live/tombstoned pane split above, there's no useful type to carve out
//! here either: a terminated `Connection` has no further valid operations
//! at all, so there's no analogous "TerminatedConnection" type worth
//! inventing the way `MuxPaneTombstone` was worth inventing. All four
//! methods are expected to be idempotent — calling one again (from the
//! same handle or another) after it already succeeded should succeed
//! trivially, not be undefined — since nothing prevents more than one
//! caller, or the same caller twice, from reaching one. A client of any
//! remote resource already has to tolerate an operation failing because
//! the other side tore it down independently (a crash, a network
//! partition, another client's `terminate`) with no local call involved
//! at all; a `Self::Error` from using a handle post-teardown is that
//! same, already-necessary case, not an extra one worth a different API
//! shape to avoid.
//!
//! # Pane lifecycle: invalid states unrepresentable
//!
//! This is the one place this module follows the archived
//! `wezterm-grpc-mux-proto` v2 schema's guiding rule directly: prefer
//! making invalid states unrepresentable over documenting the invariant
//! and trusting callers to hold it. A pane can be alive or exited
//! (tombstoned — see [`MuxPaneTombstone`]), and the operations that make
//! sense differ completely: a live pane accepts input and produces
//! output but can't be dismissed; a tombstone can be dismissed and
//! queried for its final state but can't accept input or produce more
//! output. Rather than one `Pane` type with a status flag and runtime
//! checks on every method (call `dismiss` on a live pane, or `send_input`
//! on a tombstone, and get an error back), [`MuxSession::get_pane`]
//! returns [`PaneHandle`], a sum type: [`MuxPane::close`] exists only on
//! the live variant, [`MuxPaneTombstone::dismiss`] exists only on the
//! tombstoned one. There is no method whose only job is to reject being
//! called in the wrong state.
//!
//! This can't be total, and that's fine: the pane can still die *after*
//! a client gets a [`MuxPane`], asynchronously, before the client notices
//! (see [`PaneOutput::recv`] returning `None`). Calling `send_input` on
//! that now-stale handle is a real runtime possibility — a race no type
//! system can close in a system with an independent, remote authority,
//! since the client can't be retroactively told its already-obtained
//! handle's static type was wrong. The rule this module actually
//! delivers on is narrower and still worth having: a client can never
//! *deliberately* reach for the wrong operation based on what it
//! currently believes — it cannot even write `tombstone.send_input(..)`
//! or `pane.dismiss()`, because those methods don't exist on those types.
//! [`PaneSummary::status`] (from [`MuxConnection::list_panes`]) is
//! informational only, for the same reason: it can be stale by the time
//! a client acts on it, so acting on a specific pane always goes back
//! through `get_pane` for a freshly-checked [`PaneHandle`], not through a
//! remembered status flag.
//!
//! # Cascading teardown
//!
//! [`MuxConnection`] has no `close` method (see its own docs); dropping
//! the last handle to it closes it, and [`MuxConnection::terminate`] is
//! the forceful exception. `MuxSession` is different: [`MuxSession::close`]
//! *is* an ordinary, expected, non-catastrophic action (there's no
//! separate transport for a session to abruptly lose the way a
//! connection can), so it gets an explicit method, unlike `Connection`.
//! Calling it unsubscribes from every pane this session is subscribed to,
//! regardless of how many `Pane`/`PaneTombstone` handles derived from it
//! are still held elsewhere (their next call observes `Self::Error`) —
//! the same "affects every handle, not just this one" shape as
//! `terminate`, just without the forceful connotation. It also happens
//! implicitly once the last handle to the session (including ones nested
//! inside its `Pane`/`PaneTombstone` handles) drops. `Connection::terminate`
//! cascades into this: terminating a connection closes every session
//! still open on it.
//!
//! # Sync vs. async
//!
//! Methods are `async fn` wherever the operation can or may need to do
//! real work under the hood rather than answer purely from local state:
//! every call that touches the remote authority
//! ([`MuxClient::connect`], [`MuxConnection::list_panes`]/
//! [`open_session`](MuxConnection::open_session)/
//! [`get_layout`](MuxConnection::get_layout)/
//! [`update_layout`](MuxConnection::update_layout)/
//! [`server_info`](MuxConnection::server_info),
//! [`MuxSession::create_pane`]/[`get_pane`](MuxSession::get_pane),
//! [`MuxPane::send_input`], and [`PaneOutput::recv`]), and also the three
//! teardown operations, [`MuxConnection::terminate`],
//! [`MuxSession::close`], [`MuxPane::close`], and
//! [`MuxPaneTombstone::dismiss`] — these return a `Result` and may
//! themselves need to do async teardown work (signal the server, wait for
//! the transport to actually finish closing), so a caller that wants to
//! know it actually happened can await and check it. A caller that
//! genuinely wants fire-and-forget semantics instead can get that by
//! spawning the call (`tokio::spawn`/equivalent) rather than the trait
//! baking non-async, no-error-reporting teardown in as the only option.
//!
//! This is deliberately narrower than "every drop-adjacent thing is
//! sync": only the *implicit* path — dropping the last handle — is stuck
//! being synchronous and best-effort, because Rust has no stable async
//! `Drop`. The explicit methods above don't inherit that limitation just
//! because they're conceptually related to it.
//!
//! Methods stay plain `fn` only where the answer is always available
//! immediately from state the handle already has cached locally —
//! id/version accessors, `MuxPane::dims` (kept current via
//! `OutputEvent::Resized`, not fetched on demand), and
//! `MuxPaneTombstone`'s final fields (captured once, at the moment
//! `get_pane` observed the exit, and static from then on).
//!
//! Every trait with at least one `async fn` is marked
//! `#[async_trait(?Send)]` rather than relying on native `async fn` in
//! traits, matching this workspace's existing convention (see
//! `mux::Domain`) instead of inventing a second one. `?Send` means an
//! implementation is not required to produce `Send` futures — one
//! targeting a genuinely multi-threaded executor may still choose to,
//! nothing here forbids it, but the trait doesn't demand it, the same
//! tradeoff `mux::Domain` already makes. Any concrete `impl` of these
//! traits needs the matching `#[async_trait(?Send)]` attribute too;
//! `async-trait` requires both sides to agree.
//!
//! # Least settled: pane output
//!
//! [`PaneOutput`] is the one piece of this module still up in the air,
//! though sync-vs-async is no longer the open part of that — `recv` is
//! `async fn`, per the section above. What's unsettled is the shape
//! *within* async: a plain awaitable pull (what's written here) versus a
//! `futures::Stream` versus a callback/subscription with an unsubscribe
//! token all shape the rest of a client's event loop differently. Don't
//! take this shape as settled the way the rest of this module is.
//!
//! # Still open: pane visibility
//!
//! Nothing here says whether [`MuxConnection::list_panes`]/
//! [`MuxSession::get_pane`] are scoped to panes somehow associated with
//! the connecting [`ClientId`], or are genuinely global across every
//! client on the server. That's a trust-model question independent of
//! everything above, and it is deliberately left unresolved here.
//!
//! # Version negotiation
//!
//! [`ConnectOptions`] carries this client's [`crate::version::PeerVersions`]
//! and [`MuxConnection::peer_versions`] reports the server's; see the
//! [`crate::version`] module for the full story (two independent axes, why
//! `connect()` is the only place a mismatch is fatal, and how that's
//! expected to loosen over time).

use crate::events::{InputEvent, OutputEvent};
use crate::ids::{ClientId, ConnectionId, PaneId, SessionId};
use crate::layout::LayoutBlob;
use crate::snapshot::PaneDims;
use crate::version::{PeerVersions, VersionMismatch};
use async_trait::async_trait;

/// The starting point: something that knows how to reach a server.
/// Transport, address, and authentication are all the implementation's
/// concern, not this trait's.
#[async_trait(?Send)]
pub trait MuxClient {
    type Connection: MuxConnection;

    /// Must be able to represent a rejected [`VersionMismatch`] — see
    /// [`connect`](MuxClient::connect)'s doc comment and the
    /// [`crate::version`] module docs.
    type Error: std::error::Error + Send + Sync + From<VersionMismatch> + 'static;

    /// Establish a connection. Roughly the old proto's `Hello` RPC: this is
    /// where a persistent [`ClientId`] and a [`ReconnectPolicy`] get
    /// asserted for the resulting connection's lifetime.
    ///
    /// Implementations are expected to exchange `options.versions` for the
    /// server's own [`PeerVersions`] as part of this call, check
    /// `options.versions.interface` against the server's via
    /// [`crate::version::InterfaceVersions::check_compatible`], and return
    /// `Self::Error::from(VersionMismatch { .. })` if it fails — this is
    /// the only axis `connect()` refuses on. An `implementation` version
    /// difference is never grounds for refusal; record it (log/metric),
    /// don't reject, and see [`MuxConnection::peer_versions`] for where a
    /// caller can observe it after the fact.
    async fn connect(&self, options: ConnectOptions) -> Result<Self::Connection, Self::Error>;
}

/// Options asserted once, at connect time, for the resulting connection's
/// lifetime. Roughly the old proto's `ClientHello`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ConnectOptions {
    /// This client's own versions, reported so the server can decide
    /// whether to accept the connection. See [`crate::version`].
    pub versions: PeerVersions,
    // Placeholder — fields TBD: persistent `ClientId`, `ReconnectPolicy`.
}

/// A client's chosen policy for what happens if it reconnects with a
/// `ClientId` that already has a live connection. A `ClientId` is allowed
/// at most one live connection at a time; this is how a new connection
/// attempt resolves finding one already active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectPolicy {
    /// Reject the new connection attempt if the old one is still live.
    RefuseIfActive,
    /// Terminate the old connection (see [`MuxConnection::terminate`]) and
    /// accept this one as its replacement.
    TakeOver,
}

/// One live transport connection. See the module docs for the lifecycle
/// contract (no `close`, auto-close on last drop, `terminate` as the
/// forceful exception, and how it cascades into every open `MuxSession`).
#[async_trait(?Send)]
pub trait MuxConnection {
    type Session: MuxSession<Connection = Self>;
    type Error: std::error::Error + Send + Sync + 'static;

    /// The ephemeral id the server assigned this connection. Cached from
    /// connect time — no round trip.
    fn connection_id(&self) -> ConnectionId;

    /// The persistent client identity this connection was established
    /// with (see [`ConnectOptions`]). Cached from connect time — no round
    /// trip.
    fn client_id(&self) -> ClientId;

    /// The versions the server reported at connect time. Cached — no
    /// round trip. Compare `.implementation` against this build's own
    /// [`crate::version::ImplementationVersion`] to detect (and record —
    /// never reject on) drift; `.interface` already passed
    /// [`crate::version::InterfaceVersions::check_compatible`] to get this
    /// connection established at all, so it is provided here mainly for
    /// diagnostics.
    fn peer_versions(&self) -> &PeerVersions;

    /// Discover panes on the server. This is a plain query, not scoped to
    /// any session — see the module docs' "still open" note on whether
    /// it's scoped to this connection's `ClientId` or global. `status` on
    /// each summary is informational only; to act on a specific pane, use
    /// [`MuxSession::get_pane`].
    async fn list_panes(&self) -> Result<Vec<PaneSummary>, Self::Error>;

    /// Open a new session: a local container for this connection's pane
    /// subscriptions. See the module docs on why sessions are ephemeral
    /// and connection-scoped rather than durable and reattachable, and on
    /// why whether this may be called more than once is left to the
    /// implementation.
    async fn open_session(&self) -> Result<Self::Session, Self::Error>;

    /// This connection's own layout blob (keyed by its `client_id()`
    /// implicitly — unlike the old proto's `GetLayoutRequest`, there's no
    /// need to pass the id back in since the connection handle already
    /// carries it). Opaque to the server, persisted until explicitly
    /// deleted, independent of what's currently subscribed via any
    /// session.
    async fn get_layout(&self) -> Result<Option<LayoutBlob>, Self::Error>;

    async fn update_layout(&self, blob: LayoutBlob) -> Result<(), Self::Error>;

    /// A stand-in for "some maintenance commands" — a basic liveness/version
    /// check. More maintenance operations are expected to accrete here.
    async fn server_info(&self) -> Result<ServerInfo, Self::Error>;

    /// Forcefully and immediately terminate this connection, regardless of
    /// how many `Connection`/`Session`/`Pane`/`PaneTombstone` handles are
    /// still live — this closes every open `Session` (see
    /// [`MuxSession::close`]) as part of terminating. Their next call
    /// observes `Self::Error`. Contrast with ordinary drop-based teardown
    /// (dropping the last handle), which is graceful, requires no explicit
    /// call, and — unlike this method — can't be `async` or report
    /// failure (see the module docs' "Sync vs. async" section). A caller
    /// that wants fire-and-forget semantics here anyway can spawn the
    /// call instead of awaiting it.
    async fn terminate(&self) -> Result<(), Self::Error>;
}

/// A stand-in maintenance response. Does not repeat version info — see
/// [`MuxConnection::peer_versions`] for that.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ServerInfo {
    // Placeholder — fields TBD: uptime, etc.
}

/// An ephemeral, connection-scoped container for this client's own pane
/// subscriptions — see the module docs' "Sessions are ephemeral
/// subscription containers" section for why this is not a durable,
/// server-owned grouping of panes. Holds its connection internally (so a
/// caller that only kept the `Session` still keeps the connection alive)
/// and hands it back out via [`MuxSession::connection`].
#[async_trait(?Send)]
pub trait MuxSession {
    type Connection: MuxConnection<Session = Self>;
    type Pane: MuxPane<Session = Self>;
    type Tombstone: MuxPaneTombstone<Session = Self>;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Cached locally, assigned when this session was opened — no round
    /// trip.
    fn session_id(&self) -> SessionId;

    /// Accessor to the connection this session is holding alive. A caller
    /// does not need to separately hold onto a `Connection` to keep it
    /// open — this session's own internal handle already does that. A
    /// cheap local clone — no round trip.
    fn connection(&self) -> Self::Connection;

    /// Create a new pane. Always returns a live handle directly (not
    /// [`PaneHandle`]) — a freshly created pane is alive by construction,
    /// so there is no tombstoned case to represent here. The word
    /// "spawn" survives one layer down, in [`CreatePaneRequest`], for the
    /// process-launch details specifically (the old proto's
    /// `SpawnCommand`) — it's an accurate name for *that*, even though
    /// the pane-creation operation itself doesn't need the connotation.
    async fn create_pane(&self, request: CreatePaneRequest) -> Result<Self::Pane, Self::Error>;

    /// Get a handle to an existing pane, live or tombstoned — see the
    /// module docs' "Pane lifecycle" section for why the result is a sum
    /// type rather than one type with a status flag. Nothing prevents
    /// getting the same pane more than once, from this session or
    /// another.
    async fn get_pane(
        &self,
        pane_id: PaneId,
    ) -> Result<PaneHandle<Self::Pane, Self::Tombstone>, Self::Error>;

    /// Unsubscribe from every pane this session is subscribed to,
    /// regardless of how many `Pane`/`PaneTombstone` handles derived from
    /// it are still held elsewhere. See the module docs' "Cascading
    /// teardown" section for how this differs from [`MuxConnection`]
    /// having no explicit `close`, and for the implicit drop-based
    /// equivalent (which, unlike this method, can't be `async` or report
    /// failure). A caller that wants fire-and-forget semantics here
    /// anyway can spawn the call instead of awaiting it.
    async fn close(&self) -> Result<(), Self::Error>;
}

/// Summary of a pane, as returned by [`MuxConnection::list_panes`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PaneSummary {
    pub pane_id: PaneId,

    /// Informational only — see the module docs' "Pane lifecycle"
    /// section. Do not branch on this to decide whether to call
    /// `send_input`/`close`/`dismiss`; call [`MuxSession::get_pane`] and
    /// match on the freshly-checked [`PaneHandle`] instead.
    pub status: PaneStatus,
    // Placeholder — fields TBD: canonical `PaneDims`, title, advisory
    // `PaneMetadata`-equivalent.
}

/// Informational snapshot of a pane's lifecycle state. See
/// [`PaneSummary::status`] — this is a display-only readout, not a
/// capability check; the operations that actually differ between these
/// two states live on [`MuxPane`] and [`MuxPaneTombstone`] respectively,
/// reachable only via [`MuxSession::get_pane`]'s [`PaneHandle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaneStatus {
    Live,
    Tombstoned { exit_code: Option<i32> },
}

/// Request to create a new pane, as passed to [`MuxSession::create_pane`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CreatePaneRequest {
    // Placeholder — fields TBD: requested `PaneDims`, the process spawn
    // command (argv/env/cwd), and the determinism-sensitive terminal
    // config inventory (the old proto's `SpawnCommand`/`TerminalConfig`).
}

/// What [`MuxSession::get_pane`] hands back: the pane's actual current
/// lifecycle state, encoded as a variant rather than a flag on one type.
/// See the module docs' "Pane lifecycle" section.
#[derive(Debug, Clone)]
pub enum PaneHandle<P, T> {
    Live(P),
    Tombstoned(T),
}

/// A live pane: holds its session internally the same way `Session` holds
/// its `Connection`. This is where terminal I/O happens. Obtained from
/// [`MuxSession::create_pane`] or the `Live` case of
/// [`MuxSession::get_pane`].
#[async_trait(?Send)]
pub trait MuxPane {
    type Session: MuxSession;
    type Output: PaneOutput;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Cached locally from when this handle was obtained — no round trip.
    fn pane_id(&self) -> PaneId;

    /// Accessor to the session this pane is holding alive, mirroring
    /// [`MuxSession::connection`]. A cheap local clone — no round trip.
    fn session(&self) -> Self::Session;

    /// The pane's current dimensions. Kept current locally via
    /// [`crate::events::OutputEvent::Resized`] rather than fetched on
    /// demand — no round trip.
    fn dims(&self) -> PaneDims;

    /// Send one input event. Resizing goes through here too — see
    /// [`crate::events::InputEvent::ResizeRequest`] — rather than a
    /// separate method.
    async fn send_input(&self, input: InputEvent) -> Result<(), Self::Error>;

    /// The authoritative output stream for this pane. Returns the handle
    /// itself synchronously — the subscription was already established
    /// when this `Pane` was obtained (`create_pane`/`get_pane`); waiting
    /// for the next event happens in [`PaneOutput::recv`], not here. See
    /// the module docs: this shape (and `PaneOutput`'s) is the least
    /// settled part of this hierarchy.
    fn output(&self) -> Self::Output;

    /// Forcefully end this pane's process. Only reachable through a live
    /// handle — there is no id-based "close a pane you haven't gotten a
    /// handle to" operation, and no way to call this on an
    /// already-tombstoned pane (see [`MuxPaneTombstone::dismiss`]
    /// instead); both are invalid by construction, not by runtime check.
    /// Per the tombstone model (see
    /// `crate::events::PaneLifecycleEvent::Exited`), this still results
    /// in a tombstone afterward, the same as any other exit.
    async fn close(&self) -> Result<(), Self::Error>;
}

/// A tombstone: the read-only remains of a pane whose process has
/// exited. Holds its session internally the same way [`MuxPane`] does.
/// Obtained from the `Tombstoned` case of [`MuxSession::get_pane`]. See
/// the module docs' "Pane lifecycle" section: there is deliberately no
/// `send_input`, `output`, or `close` here, and no `dismiss` on
/// [`MuxPane`] — each type only offers the operations valid for its
/// state.
#[async_trait(?Send)]
pub trait MuxPaneTombstone {
    type Session: MuxSession;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Cached locally from when this handle was obtained — no round trip.
    fn pane_id(&self) -> PaneId;

    /// Accessor to the session this tombstone is holding alive, mirroring
    /// [`MuxPane::session`]. A cheap local clone — no round trip.
    fn session(&self) -> Self::Session;

    /// This pane's dimensions at the moment it exited. Captured once, when
    /// `get_pane` observed the exit, and static from then on — no round
    /// trip.
    fn dims(&self) -> PaneDims;

    /// Captured once, same as `dims()` — no round trip.
    fn exit_code(&self) -> Option<i32>;

    // Placeholder — final content/scrollback access is not fleshed out
    // here; it likely reuses `crate::PaneSnapshot`/a `GetScrollback`
    // equivalent rather than inventing a new shape (and, unlike the
    // fields above, would plausibly need to be `async` to fetch on
    // demand rather than cached in full up front).

    /// Permanently delete this tombstone. After this, `pane_id()` is
    /// gone for good — see `crate::events::PaneLifecycleEvent::Removed`
    /// — and a future `get_pane` for it fails. Only reachable through a
    /// tombstone handle; there is no way to call this on a live pane, by
    /// construction.
    async fn dismiss(&self) -> Result<(), Self::Error>;
}

/// Tentative — see the module docs' "least settled" note. A plain
/// awaitable pull is the simplest shape to write down, not a considered
/// choice between this, a `futures::Stream`, and a callback/subscription
/// shape.
#[async_trait(?Send)]
pub trait PaneOutput {
    /// Wait for the next authoritative output event, or return `None`
    /// once this stream has permanently ended (pane exited — see
    /// [`crate::events::PaneLifecycleEvent::Exited`] — or this particular
    /// attachment was invalidated and needs a fresh `get_pane` — see
    /// `ReadPaneResyncRequired` in the archived proto for prior art on
    /// the latter).
    async fn recv(&mut self) -> Option<OutputEvent>;
}
