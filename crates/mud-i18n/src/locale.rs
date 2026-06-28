//! Locale identifiers (§3.14.2).

use std::borrow::Cow;
use std::fmt;

/// A locale identifier such as `en` or `fr` (§3.14.2.1).
///
/// A newtype over a string rather than an enum: adding a locale MUST be pure
/// data — dropping in a bundle and reloading, with no recompile (§3.14.2.2) — so
/// the type stays open over arbitrary identifiers. The boundary parses a raw tag
/// into a `Locale` once, sparing inner code from re-validating (§3.14.4.4). M1
/// only constructs `'static` tags; the `Cow` leaves room for owned tags resolved
/// at runtime when locale resolution lands (M2-I, §3.14.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct Locale(Cow<'static, str>);

impl Locale {
    /// The default and reference locale, `en` (§3.14.2.1).
    pub const EN: Self = Self::from_static("en");

    /// A locale from a `'static` string, without allocating.
    pub const fn from_static(tag: &'static str) -> Self {
        Self(Cow::Borrowed(tag))
    }

    /// The locale tag text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_is_the_reference_locale() {
        assert_eq!(Locale::EN.as_str(), "en");
    }
}
