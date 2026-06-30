//! Validated name newtypes for the account domain.
//!
//! A [`Username`] identifies an account within its tenant; a [`PuppetName`]
//! names an in-world character. Both share one validation rule (length bounds +
//! a restricted character set) so a name is parsed once, at the boundary, and
//! never re-validated downstream.

use std::fmt;

/// Inclusive character-count bounds shared by account and puppet names.
const MIN_LEN: usize = 1;
const MAX_LEN: usize = 32;

/// Rejected-name reasons, shared by both name newtypes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// The name was empty or exceeded [`MAX_LEN`] characters.
    #[error("name must be {min}–{max} characters, got {got}", min = MIN_LEN, max = MAX_LEN)]
    Length {
        /// The offending length, in characters.
        got: usize,
    },
    /// The name contained a character outside the allowed alphabet.
    #[error("name contains an invalid character: {ch:?}")]
    InvalidChar {
        /// The first disallowed character encountered.
        ch: char,
    },
}

/// Returns the validated character count, or the first rule the input breaks.
fn validate(raw: &str) -> Result<(), NameError> {
    let len = raw.chars().count();
    if !(MIN_LEN..=MAX_LEN).contains(&len) {
        return Err(NameError::Length { got: len });
    }
    if let Some(ch) = raw.chars().find(|ch| !is_allowed(*ch)) {
        return Err(NameError::InvalidChar { ch });
    }
    Ok(())
}

/// A character permitted in a name: ASCII alphanumeric or one of `-`, `_`, `'`.
///
/// Deliberately conservative — it keeps names renderable on a bare telnet client
/// and free of control characters, whitespace, or markup, so a name can never
/// carry styling or layout tricks into another player's output (cf. §3.20.7).
fn is_allowed(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '\'')
}

/// An account's login name, unique within its tenant (§3.15.1.1).
///
/// Constructed only through [`Username::parse`], which enforces the shared name
/// rule; downstream code treats the value as already-valid. Names are stored and
/// matched **case-sensitively** (`Aldous` and `aldous` are distinct accounts);
/// the spec mandates no case-folding within a tenant. If a tenant later wants
/// case-insensitive login, fold here at the parse boundary rather than at each
/// query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(String);

impl Username {
    /// Parses `raw` into a [`Username`].
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] if `raw` is empty, longer than 32 characters, or
    /// contains a character outside `[A-Za-z0-9_'-]`.
    pub fn parse(raw: impl Into<String>) -> Result<Self, NameError> {
        let raw = raw.into();
        validate(&raw)?;
        Ok(Self(raw))
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of an in-world puppet owned by an account (§3.15.1.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PuppetName(String);

impl PuppetName {
    /// Parses `raw` into a [`PuppetName`] under the shared name rule.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] under the same conditions as [`Username::parse`].
    pub fn parse(raw: impl Into<String>) -> Result<Self, NameError> {
        let raw = raw.into();
        validate(&raw)?;
        Ok(Self(raw))
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PuppetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_name() {
        let name = Username::parse("aldous").expect("a lowercase name is valid");
        assert_eq!(name.as_str(), "aldous");
    }

    #[test]
    fn accepts_the_allowed_separators_and_digits() {
        for raw in ["o'brien", "rusty_knight", "mage-42", "Aldous7"] {
            assert!(Username::parse(raw).is_ok(), "{raw:?} should be valid");
        }
    }

    #[test]
    fn rejects_an_empty_name() {
        assert_eq!(
            Username::parse(""),
            Err(NameError::Length { got: 0 }),
            "an empty name has no identity"
        );
    }

    #[test]
    fn rejects_a_name_past_the_length_cap() {
        let too_long = "a".repeat(MAX_LEN + 1);
        assert_eq!(
            Username::parse(&too_long),
            Err(NameError::Length { got: MAX_LEN + 1 })
        );
    }

    #[test]
    fn accepts_a_name_at_the_length_cap() {
        let at_cap = "a".repeat(MAX_LEN);
        assert!(Username::parse(&at_cap).is_ok());
    }

    #[test]
    fn rejects_whitespace_and_control_and_markup() {
        // Space, tab, a control char, and a markup-ish brace must all be refused
        // so a name cannot smuggle styling or layout into another player's line.
        for ch in [' ', '\t', '\n', '{', '<', '\u{1b}'] {
            let raw = format!("ad{ch}ous");
            assert_eq!(
                Username::parse(&raw),
                Err(NameError::InvalidChar { ch }),
                "{ch:?} must be rejected"
            );
        }
    }

    #[test]
    fn puppet_name_shares_the_rule() {
        assert!(PuppetName::parse("Gandalf").is_ok());
        assert_eq!(PuppetName::parse(""), Err(NameError::Length { got: 0 }));
    }
}
