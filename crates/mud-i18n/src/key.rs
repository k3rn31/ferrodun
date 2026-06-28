//! Message keys for translatable strings (§3.14.4).

use std::borrow::Cow;
use std::fmt;

/// A translatable-message key such as `command.not-found` (§3.14.4.1).
///
/// A newtype so keys cannot be confused with the resolved player-facing text or
/// any other string: the [`t!`](crate::t) macro parses a literal into a
/// `MessageKey` at the call site, the boundary required by §3.14.4.4. Keys are
/// `'static` literals in M1; the `Cow` leaves room for keys assembled at runtime
/// once the `mud.i18n.t` script API lands (M2-I, §3.14.4.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct MessageKey(Cow<'static, str>);

impl MessageKey {
    /// A key from a `'static` string, without allocating — the macro path.
    pub const fn from_static(key: &'static str) -> Self {
        Self(Cow::Borrowed(key))
    }

    /// The key text. Also the last-resort fallback rendering (§3.14.4.3).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_exposes_its_text() {
        assert_eq!(
            MessageKey::from_static("command.not-found").as_str(),
            "command.not-found"
        );
    }
}
