//! The tenant catalogue: the operator-side registry that assigns each
//! tenant its listen port and runtime tenant tag (design:
//! docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md).

use std::fmt;

use serde::{Deserialize, Serialize};

/// A tenant's name: lowercase ASCII alphanumeric plus `-`/`_`, starting with
/// an alphanumeric. It doubles as the tenant's folder name under
/// `tenants_dir`, so the grammar is deliberately filesystem-safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantName(String);

impl TenantName {
    /// Parses a tenant name, rejecting anything but lowercase ASCII
    /// alphanumerics, `-`, and `_` (the first character must be
    /// alphanumeric).
    ///
    /// # Errors
    ///
    /// Returns an error naming the offending value when the grammar is
    /// violated.
    pub fn parse(value: &str) -> anyhow::Result<TenantName> {
        let mut chars = value.chars();
        let starts_alnum = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_valid = chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !(starts_alnum && rest_valid) {
            anyhow::bail!(
                "invalid tenant name {value:?}: use lowercase letters, digits, `-`, `_`, starting with a letter or digit"
            );
        }
        Ok(TenantName(value.to_owned()))
    }

    /// The name as authored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TenantName {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        TenantName::parse(&value)
    }
}

impl From<TenantName> for String {
    fn from(name: TenantName) -> String {
        name.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_slug_parses() {
        let name = TenantName::parse("my-game_2").expect("valid slug");
        assert_eq!(name.as_str(), "my-game_2");
    }

    #[test]
    fn uppercase_is_rejected() {
        assert!(TenantName::parse("MyGame").is_err());
    }

    #[test]
    fn a_leading_separator_is_rejected() {
        assert!(TenantName::parse("-game").is_err());
        assert!(TenantName::parse("_game").is_err());
    }

    #[test]
    fn empty_and_path_like_names_are_rejected() {
        assert!(TenantName::parse("").is_err());
        assert!(TenantName::parse("a/b").is_err());
        assert!(TenantName::parse("..").is_err());
    }
}
