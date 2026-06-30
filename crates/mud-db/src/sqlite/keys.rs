//! Boundary conversions between the database's `i64` columns and the typed
//! domain ids (`EntityKey`, `AccountId`). Parsing happens once, here, at the persistence edge;
//! inner code works in typed ids and never re-validates. (A location's place is
//! persisted as its durable [`PlaceKey`](mud_core::PlaceKey) slug, a `TEXT`
//! column translated via [`PlaceMap`](super::PlaceMap), not through this module.)

use std::collections::HashMap;
use std::num::NonZeroU64;

use mud_account::AccountId;
use mud_core::{EntityId, EntityKey};

use crate::error::DbError;

/// Resolves a persisted `entity_key` to its loaded `EntityId`, failing if no
/// arena handle was minted for it.
pub(super) fn resolve_loaded(
    by_key: &HashMap<EntityKey, EntityId>,
    value: i64,
) -> Result<EntityId, DbError> {
    let key = entity_key_from_db(value)?;
    by_key
        .get(&key)
        .copied()
        .ok_or(DbError::DanglingReference(value))
}

/// Parses a database `i64` into an [`EntityKey`], rejecting non-positive values.
pub(super) fn entity_key_from_db(value: i64) -> Result<EntityKey, DbError> {
    nonzero_from_db(value).map(EntityKey::new)
}

/// Narrows an [`EntityKey`] to the `i64` its column stores.
pub(super) fn entity_key_to_db(key: EntityKey) -> Result<i64, DbError> {
    nonzero_to_db(key.get())
}

/// Parses a database `accounts.id` into an [`AccountId`], rejecting non-positive
/// values (defensive: ids are positive `AUTOINCREMENT` rowids).
pub(super) fn account_id_from_db(value: i64) -> Result<AccountId, DbError> {
    nonzero_from_db(value).map(AccountId::new)
}

/// Narrows an [`AccountId`] to the `i64` its column stores.
pub(super) fn account_id_to_db(id: AccountId) -> Result<i64, DbError> {
    nonzero_to_db(id.get())
}

/// `i64` → `NonZeroU64`, rejecting negative or zero values (defensive: keys are
/// positive `AUTOINCREMENT` rowids).
fn nonzero_from_db(value: i64) -> Result<NonZeroU64, DbError> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(DbError::InvalidId(value))
}

/// `NonZeroU64` → `i64`, rejecting values beyond the signed range (defensive:
/// rowids never approach `i64::MAX`).
fn nonzero_to_db(value: NonZeroU64) -> Result<i64, DbError> {
    let raw = value.get();
    i64::try_from(raw).map_err(|_| DbError::KeyOutOfRange(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_db_rejects_zero_and_negatives() {
        for corrupt in [0, -1, i64::MIN] {
            assert!(
                matches!(nonzero_from_db(corrupt), Err(DbError::InvalidId(got)) if got == corrupt),
                "a non-positive persisted id ({corrupt}) must surface as InvalidId"
            );
        }
    }

    #[test]
    fn to_db_rejects_values_past_the_signed_range() {
        let beyond = NonZeroU64::new(u64::MAX).expect("u64::MAX is non-zero");
        assert!(
            matches!(nonzero_to_db(beyond), Err(DbError::KeyOutOfRange(got)) if got == u64::MAX),
            "a key beyond i64::MAX must surface as KeyOutOfRange"
        );
    }

    #[test]
    fn round_trip_preserves_a_valid_key() {
        let key = EntityKey::new(NonZeroU64::new(42).expect("42 is non-zero"));
        let stored = entity_key_to_db(key).expect("a small key fits in i64");
        assert_eq!(entity_key_from_db(stored).expect("round-trips back"), key);
    }
}
