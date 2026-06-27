//! Session identity and the IPC schema version (§2.1.3.1).

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

/// Identifies one client session on the IPC channel (§2.1.3.1).
///
/// The IPC channel between Gateway and World is multiplexed by `SessionId`;
/// every gameplay frame carries the session it belongs to. Each World's
/// `SessionId` space is scoped to its `world_id` (§2.1.3.1), so ids are never
/// compared across Worlds.
///
/// Minted by the Gateway when a client connects (the minting logic lives in the
/// transport, M1-11). Backed by `NonZeroU64`: ids are 1-based, so an absent
/// session is representable only as `Option::None` (which takes the niche for
/// free), never as a meaningless id `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[must_use]
pub struct SessionId(NonZeroU64);

impl SessionId {
    /// Wraps a session id value.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the underlying id value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// The version of the IPC frame schema declared in this crate (§2.1.3.1,
/// §2.8.5.2).
///
/// Unlike the structured wire protocol (§2.8.5), `postcard` IPC frames are
/// version-locked at build time (§2.8.5.7): Gateway and World are built against
/// the same schema, so the version is not stamped on every gameplay frame. It is
/// declared once here and announced by the resume handshake (§2.1.3.2, M1-11) so
/// a freshly started World and a running Gateway can confirm they agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[must_use]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Returns the underlying version number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The IPC schema version this build speaks (§2.1.3.1).
pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

#[cfg(test)]
mod tests {
    use super::*;

    fn session(value: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(value).expect("test session id must be non-zero"))
    }

    // The non-zero niche encodes "no session" as `None` for free: an optional id
    // stays the same width as an id, with no sentinel value to reserve.
    #[test]
    fn option_session_id_is_niche_optimized() {
        assert_eq!(size_of::<Option<SessionId>>(), 8);
    }

    #[test]
    fn round_trips_through_new_and_get() {
        let value = NonZeroU64::new(7).expect("non-zero literal");
        assert_eq!(SessionId::new(value).get(), value);
    }

    #[test]
    fn orders_by_value() {
        assert!(session(1) < session(2));
    }

    #[test]
    fn schema_version_is_one() {
        assert_eq!(SCHEMA_VERSION.get(), 1);
    }
}
