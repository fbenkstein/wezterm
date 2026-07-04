//! The role split: authoritative server state vs. client-side terminal
//! replica, plus the shared determinism contract both sides implement.

use crate::events::{ControlEvent, InputEvent, OutputEvent, ResyncReason, ScrollbackCommit};
use crate::ids::{PaneId, SequenceNo};
use crate::snapshot::{PaneDims, ViewportHash};

/// The contract both sides implement identically: fed the same
/// [`OutputEvent`]s from the same starting point, the authoritative
/// terminal and every replica reach identical state (both run the same
/// `term::Terminal` code, same commit). This is the shared supertrait;
/// [`AuthoritativeTerminal`] and [`ReplicaTerminal`] add the halves that are
/// only meaningful on one side of the replication boundary.
pub trait ReplicatedTerminal {
    /// The DTO used to initialize or resynchronize this terminal; the
    /// concrete representation is supplied by the owning emulator crate,
    /// not by this one (see [`crate::PaneSnapshot`]).
    type Snapshot;

    fn pane_id(&self) -> PaneId;

    fn dims(&self) -> PaneDims;

    /// Feed one authoritative output event. Must be called exactly once per
    /// server-side PTY read, on every replica, in `SequenceNo` order (the
    /// no-re-chunk invariant) — re-chunking or reordering desynchronizes
    /// the replica from the authoritative terminal.
    fn advance(&mut self, event: &OutputEvent) -> Result<(), ReplicationError>;

    /// A fast hash over the visible rows, used for drift detection.
    fn viewport_hash(&self) -> ViewportHash;
}

/// The server-side role: owns the PTY and process lifecycle, is the sole
/// encoder of input, and is the source of truth that every replica replays
/// against.
pub trait AuthoritativeTerminal: ReplicatedTerminal {
    /// Capture a snapshot and the seqno a new subscriber's stream should
    /// resume at, as one atomic operation (capture-is-atomic-with-
    /// subscription) taken at a clean parser boundary.
    fn capture_snapshot(&self) -> Self::Snapshot;

    /// Encode and apply a raw input event, returning the bytes written to
    /// the PTY. The replica never encodes; only the authoritative side does.
    fn apply_input(&mut self, input: InputEvent) -> Result<Vec<u8>, ReplicationError>;

    /// Reflow to a new canonical size. Panes have exactly one authoritative
    /// size (they share a single PTY); the result is broadcast to every
    /// replica as `OutputEvent::Resized`.
    fn resize(&mut self, dims: PaneDims) -> Result<(), ReplicationError>;

    /// Commit any rows that scrolled off since the last call to the
    /// authoritative scrollback log, if there are new ones to commit.
    fn commit_scrollback(&mut self) -> Option<ScrollbackCommit>;
}

/// The client-side role: a shadow emulator that is *feed-only* — only
/// authoritative [`OutputEvent`]s are fed to
/// [`ReplicatedTerminal::advance`]. Local echo is a prediction overlay
/// layered on top of, and never a mutation of, this replica; it has no
/// representation in this trait because it never touches replicated state.
pub trait ReplicaTerminal: ReplicatedTerminal {
    /// Initialize (or resynchronize) from a snapshot. The caller resumes
    /// feeding `advance` starting at `snapshot.seqno.next()` (see
    /// [`crate::PaneSnapshot`]).
    fn resync(&mut self, snapshot: Self::Snapshot);

    /// Build the request for a fresh snapshot, e.g. after a viewport-hash
    /// mismatch, or on attach/reconnect.
    fn request_resync(&self, reason: ResyncReason) -> ControlEvent {
        ControlEvent::RequestResync {
            pane_id: self.pane_id(),
            reason,
        }
    }
}

/// Errors from advancing or otherwise driving a replicated terminal.
#[derive(Debug, thiserror::Error)]
pub enum ReplicationError {
    #[error("output event seqno {got:?} is not the expected next seqno {expected:?}")]
    OutOfSequence {
        expected: SequenceNo,
        got: SequenceNo,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
