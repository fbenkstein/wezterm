//! Identifiers shared by the authoritative server and terminal replicas.

use std::fmt;

macro_rules! newtype_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

newtype_id!(PaneId, "Identifies a pane on the authoritative server.");
newtype_id!(
    SessionId,
    "Identifies a `MuxSession`: an ephemeral, connection-scoped container \
     a client uses to manage its own pane subscriptions (get/create/\
     close). Panes are not grouped or owned by sessions at the server -- \
     any number of sessions, on any number of connections, may attach the \
     same `PaneId`. A session does not survive its connection closing, so \
     unlike `ClientId` this is not meant to be stable across reconnects."
);
newtype_id!(
    ClientId,
    "A persistent client identity, stable across reconnects. Used to key \
     reconnect and layout-blob state to \"the same client\" rather than to \
     a particular connection."
);
newtype_id!(
    ConnectionId,
    "Ephemeral id assigned by the server to one accepted transport \
     connection. Distinct from `ClientId`: a single persistent `ClientId` \
     may be associated with many `ConnectionId`s over its lifetime (one at \
     a time), as it reconnects."
);

/// Orders output events within a single pane's stream. The authoritative
/// terminal and every replica advance this in lockstep: one PTY read is one
/// output event is one `advance` call on every replica (the no-re-chunk
/// invariant — re-chunking is the one known way to desynchronize grapheme
/// clustering across replicas). `SequenceNo` is also the attach point for
/// snapshots and resync: a snapshot captured at seqno `N` is paired with
/// "the stream resumes at `N.next()`".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceNo(pub u64);

impl SequenceNo {
    pub fn next(self) -> SequenceNo {
        SequenceNo(self.0 + 1)
    }
}

/// Orders the committed-scrollback log, independently of `SequenceNo`. The
/// live viewport may be speculative; committed scrollback is authoritative
/// and server-replicated, so it gets its own ordering rather than being
/// derived from whatever a replica's shadow terminal produced while
/// speculating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScrollbackSeq(pub u64);

/// Ties a client-originated input event to the prediction it produced, so
/// a later authoritative update can retire the right prediction (confirm,
/// contradict, or time out) instead of clobbering unrelated in-flight ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputSerial(pub u64);
