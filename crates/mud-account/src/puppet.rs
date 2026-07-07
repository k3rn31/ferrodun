//! Puppets: the in-world characters an account owns (§3.15.1.4).

use mud_core::EntityKey;

use crate::name::PuppetName;

/// An in-world character owned by an account.
///
/// A puppet *is* an entity, so it is identified by its durable
/// [`EntityKey`] — the same key its location and inventory persist against, so a
/// puppet's whereabouts survive a restart (§7.4 M1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Puppet {
    /// The puppet's durable entity identity.
    pub key: EntityKey,
    /// The puppet's display name.
    pub name: PuppetName,
}

impl Puppet {
    /// Pairs a durable entity key with a puppet name.
    #[must_use]
    pub fn new(key: EntityKey, name: PuppetName) -> Self {
        Self { key, name }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    #[test]
    fn new_pairs_a_key_with_a_name() {
        let key = EntityKey::new(NonZeroU64::new(7).expect("non-zero key"));
        let name = PuppetName::parse("hero").expect("valid name");
        let puppet = Puppet::new(key, name.clone());
        assert_eq!(puppet.key, key);
        assert_eq!(puppet.name, name);
    }
}
