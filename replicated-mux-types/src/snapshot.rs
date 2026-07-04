//! Snapshot / resync semantics: what it takes to initialize or
//! resynchronize a terminal replica from the authoritative server.

use crate::ids::{PaneId, SequenceNo};

/// The canonical size of a pane, in cells. There is exactly one authoritative
/// value per pane (panes share a single PTY), though a replica may render at
/// a different local size while speculating during an in-flight resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneDims {
    pub rows: usize,
    pub cols: usize,
}

/// A fast hash over a pane's *visible* rows (not full scrollback), included
/// periodically alongside output. A replica compares this against its own
/// viewport; a mismatch means drift and is the trigger for
/// [`crate::ReplicaTerminal::request_resync`]. Drift indicates a bug (e.g.
/// the grapheme-clustering caveat of the no-re-chunk invariant); since it is
/// recoverable, normal use should not need to see it, but a debug mode can
/// surface these events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportHash(pub u64);

/// A point-in-time capture of a pane's authoritative state, used to
/// initialize (or resynchronize) a replica's shadow terminal.
///
/// This is a protocol DTO, not a serialized `wezterm_term::TerminalState` —
/// but its field set must be driven by the full state inventory (modes,
/// margins, saved cursor, charsets, tab stops, scroll region, alt-screen
/// flag, unicode-version stack, kitty image counter, stable row index
/// offset), or the replica diverges right after attach. "lines + cursor +
/// dims + palette" is not enough on its own.
///
/// The concrete representation of that state inventory is intentionally
/// *not* fixed by this crate: `State` is supplied by whichever crate owns
/// the terminal emulator (e.g. `wezterm-term`), so this crate models the
/// replication boundary rather than re-implementing terminal internals.
/// Capture is atomic with subscription: `seqno` is the point the snapshot
/// was captured at, and the caller resumes feeding the output stream at
/// `seqno.next()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot<State> {
    pub pane_id: PaneId,
    pub seqno: SequenceNo,
    pub dims: PaneDims,
    pub state: State,
}
