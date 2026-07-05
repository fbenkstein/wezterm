//! Client-owned layout persistence.

/// Opaque, client-owned presentation state — tabs, split tree, GUI window
/// arrangement, zoom — persisted by the server purely so it can be handed
/// back to the same client after a reconnect. The server stores and returns
/// these bytes verbatim; it never inspects or interprets them. Keyed by a
/// persistent [`crate::ClientId`] rather than a connection, since the owner
/// reconnects under a new connection but the same identity.
///
/// The MVP protocol has no layout *events*: a lost or delayed layout update
/// is recoverable because terminal state remains authoritative regardless of
/// what the layout blob says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutBlob(pub Vec<u8>);
