//! Region identity (§2.2.7).
//!
//! A region has two identifiers with distinct lifetimes, mirroring places
//! (§2.2.6) and entities (§2.3.1.4): [`RegionId`] is the ephemeral in-process
//! handle, [`RegionKey`] is the durable authored slug. A region *groups* places
//! (§2.2.7) — it is not itself a [`Place`](crate::Place) — so it lives in its own
//! module rather than under `place`.

use std::fmt;
use std::num::NonZeroU64;

use crate::slug::first_invalid_slug_char;

/// The **ephemeral** in-process handle of a region (§2.2.7.1).
///
/// `RegionId` is to a region what [`PlaceId`](crate::PlaceId) is to a place: a
/// dense handle minted when the world is loaded and valid only for that process
/// lifetime, used on hot paths. The **durable** identity builders author is the
/// [`RegionKey`] slug; the two are never confused at compile time.
///
/// Backed by `NonZeroU64` so an absent region is representable as `Option::None`
/// for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct RegionId(NonZeroU64);

impl RegionId {
    /// Wraps a region identifier value.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the underlying identifier value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// The **durable** identity of a region (§2.2.7.1): the human-authored slug that
/// names a region in world files.
///
/// `RegionKey` is to a region what [`PlaceKey`](crate::PlaceKey) is to a place —
/// the stable identity builders author, which survives the add/remove/rename
/// authoring lifecycle. Distinct from the ephemeral [`RegionId`] handle so the two
/// cannot be confused at compile time (§1.7).
///
/// A key is a non-empty slug of lowercase ASCII letters, digits, `_`, and `-`.
/// [`parse`](RegionKey::parse) is the only constructor, so an invalid slug is
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct RegionKey(String);

impl RegionKey {
    /// Parses a slug into a [`RegionKey`].
    ///
    /// # Errors
    ///
    /// Returns [`RegionKeyError::Empty`] for an empty slug, or
    /// [`RegionKeyError::InvalidCharacter`] for any character outside
    /// `[a-z0-9_-]`.
    pub fn parse(value: &str) -> Result<Self, RegionKeyError> {
        if value.is_empty() {
            return Err(RegionKeyError::Empty);
        }
        if let Some(bad) = first_invalid_slug_char(value) {
            return Err(RegionKeyError::InvalidCharacter(bad));
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the slug text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RegionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The reason a slug could not be parsed into a [`RegionKey`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegionKeyError {
    /// The slug was empty.
    #[error("region key must not be empty")]
    Empty,
    /// The slug contained a character outside `[a-z0-9_-]`.
    #[error("region key contains an invalid character {0:?} (allowed: a-z, 0-9, '_', '-')")]
    InvalidCharacter(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_id_round_trips_through_new_and_get() {
        let value = NonZeroU64::new(7).expect("non-zero literal");
        assert_eq!(RegionId::new(value).get(), value);
    }

    // The non-zero niche keeps an optional region the same width as a RegionId.
    #[test]
    fn option_region_id_is_niche_optimized() {
        assert_eq!(size_of::<Option<RegionId>>(), 8);
    }

    #[test]
    fn region_key_parses_a_valid_slug_and_round_trips_through_display() {
        let key = RegionKey::parse("misty_mountains-1").expect("a valid slug parses");
        assert_eq!(key.as_str(), "misty_mountains-1");
        assert_eq!(key.to_string(), "misty_mountains-1");
    }

    #[test]
    fn region_key_rejects_empty_and_invalid_characters() {
        assert_eq!(RegionKey::parse(""), Err(RegionKeyError::Empty));
        assert_eq!(
            RegionKey::parse("misty mountains"),
            Err(RegionKeyError::InvalidCharacter(' '))
        );
        assert_eq!(
            RegionKey::parse("Region"),
            Err(RegionKeyError::InvalidCharacter('R'))
        );
    }
}
