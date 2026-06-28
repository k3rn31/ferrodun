//! `PlaceKey`, the durable place identity (§2.2.6).
//!
//! A place has two identifiers with distinct lifetimes. [`PlaceId`](super::id::PlaceId)
//! is the ephemeral in-process handle; `PlaceKey` is the durable identity — the
//! human-authored slug that names a place in world files and is persisted for an
//! entity's location, so it must survive a restart and the add/remove/rename
//! authoring lifecycle.

use std::fmt;

/// The **durable** identity of a [`Place`](super::Place): an authored slug
/// (§2.2.6).
///
/// `PlaceKey` is to a place what [`EntityKey`](crate::EntityKey) is to an entity —
/// the stable identity builders author and the value persisted for an entity's
/// location. Distinct from the ephemeral [`PlaceId`](super::id::PlaceId) handle so
/// the two cannot be confused at compile time.
///
/// A key is a non-empty slug of lowercase ASCII letters, digits, `_`, and `-`.
/// [`parse`](PlaceKey::parse) is the only constructor, so an invalid slug is
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct PlaceKey(String);

impl PlaceKey {
    /// Parses a slug into a [`PlaceKey`].
    ///
    /// # Errors
    ///
    /// Returns [`PlaceKeyError::Empty`] for an empty slug, or
    /// [`PlaceKeyError::InvalidCharacter`] for any character outside
    /// `[a-z0-9_-]`.
    pub fn parse(value: &str) -> Result<Self, PlaceKeyError> {
        if value.is_empty() {
            return Err(PlaceKeyError::Empty);
        }
        if let Some(bad) = value
            .chars()
            .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
        {
            return Err(PlaceKeyError::InvalidCharacter(bad));
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the slug text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlaceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The reason a slug could not be parsed into a [`PlaceKey`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlaceKeyError {
    /// The slug was empty.
    #[error("place key must not be empty")]
    Empty,
    /// The slug contained a character outside `[a-z0-9_-]`.
    #[error("place key contains an invalid character {0:?} (allowed: a-z, 0-9, '_', '-')")]
    InvalidCharacter(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_slug() {
        let key = PlaceKey::parse("town_square-2").expect("a valid slug parses");
        assert_eq!(key.as_str(), "town_square-2");
    }

    #[test]
    fn rejects_empty_and_invalid_characters() {
        assert_eq!(PlaceKey::parse(""), Err(PlaceKeyError::Empty));
        assert_eq!(
            PlaceKey::parse("town square"),
            Err(PlaceKeyError::InvalidCharacter(' '))
        );
        assert_eq!(
            PlaceKey::parse("Town"),
            Err(PlaceKeyError::InvalidCharacter('T'))
        );
        assert_eq!(
            PlaceKey::parse("café"),
            Err(PlaceKeyError::InvalidCharacter('é'))
        );
    }
}
