//! Version negotiation. Two independent axes, both reported at connect time
//! (see [`crate::client::ConnectOptions`] and
//! [`crate::client::MuxConnection::peer_versions`]), that must not be
//! conflated:
//!
//! - [`InterfaceVersions`] — the semver-numbered version(s) of the crate(s)
//!   that *are* the interface: today just `replicated-mux-types`
//!   ([`InterfaceVersion::types_crate_version`]); once a wire-format crate
//!   exists (`protocol`, see the module docs on `crate::client`), either as
//!   a sibling or merged into this one, its version joins or replaces it.
//!   This is the axis `connect()` is expected to refuse on when it doesn't
//!   pass [`InterfaceVersions::check_compatible`].
//! - [`ImplementationVersion`] — whatever build identity sits *behind* the
//!   interface (the existing `wezterm_version()` convention: a
//!   `<date>-<time>-<short-git-hash>` string). This axis is always
//!   advisory. It is expected to drift over time — most likely the server
//!   stays put while clients pick up new, protocol-unrelated behavior (UI,
//!   rendering, etc.) — and a real implementation should record a mismatch
//!   here (log it, expose it as a metric, show it in a diagnostics
//!   overlay) but must never use it to refuse a connection.
//!
//! # Why versions are facts, not requests
//!
//! [`ConnectOptions`] carries the client's own [`PeerVersions`] so the
//! server can decide whether to accept the connection; there is
//! deliberately no field for the client to also assert *which*
//! [`CompatibilityPolicy`] should apply. The policy is a property of the
//! interface version itself — "as of interface version N, this is how
//! strictly compatibility is checked" — not a per-connection knob a client
//! gets to choose. If the client could pick a looser policy than the
//! server intends, an old or misbehaving client could opt itself past a
//! check the server relies on. Each side applies its own copy of the
//! policy logic (baked into whatever version of this crate it was built
//! against) to the versions it was told about.
//!
//! # How this is expected to evolve
//!
//! [`CompatibilityPolicy`] has exactly one variant today,
//! [`CompatibilityPolicy::ExactMatch`], because while every change is
//! effectively breaking (nothing has shipped yet), "same version" and
//! "same behavior" are the same fact and there is nothing softer worth
//! checking. As the interface stabilizes — additions landing as minor
//! version bumps, fixes as patch bumps — a looser, semver-style policy
//! (matching major version, server minor/patch allowed to lead the
//! client's) becomes meaningful. That variant is intentionally not added
//! yet; `CompatibilityPolicy` is `#[non_exhaustive]` so it can be added
//! without breaking callers that already match on it.

/// A semver-style version triple for one interface component. Deliberately
/// not the `semver` crate's richer type (with pre-release/build-metadata
/// parsing) — both peers are always built from Cargo's own
/// `CARGO_PKG_VERSION_*` values, so there is no external version string to
/// parse, only two triples to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct InterfaceVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl InterfaceVersion {
    /// This crate's own version, straight from `Cargo.toml` via Cargo's
    /// standard `CARGO_PKG_VERSION_*` build-time env vars — no build
    /// script required. This is what a `ConnectOptions`/`PeerVersions`
    /// should use for [`InterfaceVersions::types`] until a separate
    /// protocol crate exists.
    pub fn types_crate_version() -> Self {
        InterfaceVersion {
            major: parse_cargo_version_component(env!("CARGO_PKG_VERSION_MAJOR")),
            minor: parse_cargo_version_component(env!("CARGO_PKG_VERSION_MINOR")),
            patch: parse_cargo_version_component(env!("CARGO_PKG_VERSION_PATCH")),
        }
    }
}

fn parse_cargo_version_component(s: &str) -> u32 {
    s.parse()
        .expect("Cargo always sets CARGO_PKG_VERSION_* to a valid integer")
}

/// The version(s) of the crate(s) that constitute the interface itself —
/// the axis [`CompatibilityPolicy`] governs. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct InterfaceVersions {
    /// The `replicated-mux-types` crate's own version.
    pub types: InterfaceVersion,

    /// The wire-format/protocol crate's version, once one exists
    /// separately from `types`. `None` means "not split out yet" —
    /// `types` alone is the whole interface. Both peers must agree on
    /// whether this is `Some` or `None` at the same time they agree on
    /// the version numbers themselves; a peer that has split protocol out
    /// talking to one that hasn't is itself a form of interface mismatch.
    pub protocol: Option<InterfaceVersion>,
}

impl InterfaceVersions {
    /// Convenience for today's single-crate state: `types` from this
    /// build, `protocol: None`.
    pub fn current_types_only() -> Self {
        InterfaceVersions {
            types: InterfaceVersion::types_crate_version(),
            protocol: None,
        }
    }

    /// Check `self` (usually "my own versions") against `remote` (usually
    /// "what the other peer reported") under `policy`. `Ok(())` means
    /// `connect()` may proceed; `Err` carries both sides' versions for
    /// the caller to fold into its own error type (see
    /// [`crate::client::MuxClient::Error`]'s `From<VersionMismatch>`
    /// bound) and/or log.
    pub fn check_compatible(
        &self,
        remote: &InterfaceVersions,
        policy: CompatibilityPolicy,
    ) -> Result<(), VersionMismatch> {
        let compatible = match policy {
            CompatibilityPolicy::ExactMatch => self == remote,
        };
        if compatible {
            Ok(())
        } else {
            Err(VersionMismatch {
                local: *self,
                remote: *remote,
            })
        }
    }
}

/// How strictly two peers' [`InterfaceVersions`] must agree. See the
/// module docs for why this isn't a field two peers negotiate, and how
/// it's expected to gain variants over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompatibilityPolicy {
    /// Every reported component version must be bit-for-bit identical.
    /// The only policy implemented today.
    ExactMatch,
}

impl Default for CompatibilityPolicy {
    fn default() -> Self {
        CompatibilityPolicy::ExactMatch
    }
}

/// `connect()` refused because `local` and `remote` fail the current
/// [`CompatibilityPolicy`] (see [`InterfaceVersions::check_compatible`]).
/// Only ever about [`InterfaceVersions`] — an [`ImplementationVersion`]
/// difference is never grounds for this, see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("interface version mismatch: local {local:?}, remote {remote:?}")]
pub struct VersionMismatch {
    pub local: InterfaceVersions,
    pub remote: InterfaceVersions,
}

/// Free-form build identity behind the interface — e.g. the existing
/// `wezterm_version()` convention (`<date>-<time>-<short-git-hash>`).
/// Never compared for connect-time accept/reject; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImplementationVersion(pub String);

/// Everything one peer reports about itself at connect time: bundles the
/// fatal-if-mismatched axis ([`InterfaceVersions`]) with the
/// advisory-only one ([`ImplementationVersion`]), so a caller can't
/// accidentally check compatibility against the wrong one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct PeerVersions {
    pub interface: InterfaceVersions,
    pub implementation: ImplementationVersion,
}
