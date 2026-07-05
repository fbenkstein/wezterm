//! Input, output, and control events: the traffic that crosses the
//! replication boundary between the authoritative server and a replica.

use crate::ids::{ClientId, InputSerial, PaneId, ScrollbackSeq, SequenceNo};
use crate::layout::LayoutBlob;
use crate::snapshot::{PaneDims, ViewportHash};
use wezterm_input_types::{KeyEvent, MouseEvent};

/// Authoritative facts about a single pane's output stream, in `SequenceNo`
/// order. Everything here is server -> replica.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputEvent {
    /// One PTY read, fed verbatim to [`crate::ReplicatedTerminal::advance`]
    /// by every replica. Never re-chunked or coalesced online: that would
    /// break the `SequenceNo` lockstep the determinism contract depends on.
    /// (A slow/lagging replica is instead recovered via resync, which *is*
    /// allowed to coalesce by truncation.)
    Bytes { seqno: SequenceNo, bytes: Vec<u8> },
    /// Periodic drift-detection hash of the visible rows. See
    /// [`ViewportHash`].
    Viewport {
        seqno: SequenceNo,
        hash: ViewportHash,
    },
    /// The canonical size changed; a server-arbitrated resize completed.
    Resized { dims: PaneDims },
    /// A unit of the authoritative, server-replicated scrollback log.
    ScrollbackCommitted(ScrollbackCommit),
}

/// Raw input as produced by the user. The server is the sole encoder — it
/// turns this into the bytes a program actually receives — which removes
/// the who-encodes/mid-mode-flip race a client-side encoder would create.
/// Input is pane-addressed: sending it does not require or imply that the
/// pane has server-side "focus" (see [`ControlEvent::FocusScope`]).
/// Direction: replica -> server.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    Key {
        serial: InputSerial,
        event: KeyEvent,
    },
    Mouse {
        serial: InputSerial,
        event: MouseEvent,
    },
    Paste {
        serial: InputSerial,
        text: String,
    },
    /// A client's local resize speculation. The replica reflows its shadow
    /// terminal immediately on the local window resize; this message asks
    /// the server to reflow the authoritative emulator and broadcast the
    /// new canonical size (delivered back as `OutputEvent::Resized`) to
    /// every replica.
    ResizeRequest {
        dims: PaneDims,
    },
}

/// Out-of-band coordination between a replica and the authoritative server
/// that is neither terminal input nor terminal output.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// replica -> server: a replica detected drift, or is attaching or
    /// reconnecting, and asks for a fresh [`crate::PaneSnapshot`] plus the
    /// seqno its stream should resume at.
    RequestResync {
        pane_id: PaneId,
        reason: ResyncReason,
    },
    /// replica -> server: a client focus scope started or stopped focusing
    /// a pane. The server tracks a *set* of focus scopes per pane rather
    /// than one server-owned "active" pane, and edge-triggers focus-in/out
    /// to the application on empty <-> non-empty transitions of that set.
    FocusScope { pane_id: PaneId, focused: bool },
    /// replica -> server: persist the caller's opaque layout blob, keyed by
    /// its persistent [`ClientId`], for retrieval after reconnect.
    StoreLayout {
        client_id: ClientId,
        blob: LayoutBlob,
    },
}

/// Why a replica is asking for resync. Purely informational/diagnostic:
/// normal use recovers silently, but a debug mode can be configured to
/// surface these events for inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncReason {
    Attach,
    Reconnect,
    ViewportHashMismatch,
    SlowConsumerCoalesced,
}

/// One unit of the authoritative, server-replicated scrollback log. Rows
/// committed here are final; the live viewport may still be speculative,
/// so committed scrollback is never derived from a replica's local state.
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollbackCommit {
    Rows { seq: ScrollbackSeq, bytes: Vec<u8> },
    Clear { seq: ScrollbackSeq },
    DropBefore { seq: ScrollbackSeq },
}

/// Explicit authoritative pane lifecycle facts, pushed to clients instead of
/// requiring a broad `ListPanes`-style poll after every change. Direction:
/// server -> all interested clients (not scoped to one pane's output
/// stream, since e.g. `Created` announces a pane a client isn't yet
/// subscribed to).
#[derive(Debug, Clone, PartialEq)]
pub enum PaneLifecycleEvent {
    Created {
        pane_id: PaneId,
        dims: PaneDims,
    },
    /// A tombstone for `pane_id` was explicitly dismissed (see
    /// `crate::client::MuxPaneTombstone::dismiss`) and the id is now gone
    /// for good -- no `get_pane` will ever succeed for it again. This
    /// is *not* fired when the pane's process exits; that's `Exited`,
    /// below.
    Removed {
        pane_id: PaneId,
    },
    /// The pane's process exited. The pane does not disappear: it becomes
    /// a tombstone (see `crate::client::MuxPaneTombstone`) whose final
    /// properties and content remain readable but which no longer accepts
    /// input, resizes, or produces further output. It stays a tombstone,
    /// occupying `pane_id`, until a client calls `dismiss` on it (see
    /// `Removed`) -- it is not garbage-collected on its own.
    Exited {
        pane_id: PaneId,
        exit_code: Option<i32>,
    },
    TitleChanged {
        pane_id: PaneId,
        title: String,
    },
}
