//! The message backing store (§3.14.4.3).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::key::MessageKey;
use crate::locale::Locale;

/// A store of message templates keyed by `(locale, key)` (§3.14.4.3).
///
/// For M1 this is a static, in-memory table; M2-I replaces the internals with
/// Fluent bundles ([`PLAN.md`] M2-I) **without changing the lookup contract or
/// any call site**. The store is injectable — [`translate`](crate::translate)
/// takes a `&Catalog` — so tests and (later) tenant-scoped loaders can supply
/// their own.
#[derive(Debug, Default)]
#[must_use]
pub struct Catalog {
    messages: HashMap<Locale, HashMap<MessageKey, String>>,
}

impl Catalog {
    /// An empty catalog. Build one up with [`insert`](Self::insert).
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `template` for `(locale, key)`, replacing any prior entry.
    pub fn insert(&mut self, locale: Locale, key: MessageKey, template: impl Into<String>) {
        self.messages
            .entry(locale)
            .or_default()
            .insert(key, template.into());
    }

    /// The template registered for `(locale, key)`, if any.
    #[must_use]
    pub fn lookup(&self, locale: &Locale, key: &MessageKey) -> Option<&str> {
        self.messages.get(locale)?.get(key).map(String::as_str)
    }

    /// The process-wide built-in `en` catalog (§3.14.2.1).
    ///
    /// Empty for M1: there are no engine-emitted strings yet, so every lookup
    /// falls through to the literal key (§3.14.4.3). M2-I populates this from
    /// Fluent bundles. The first real entries arrive with their call sites in
    /// the command pipeline (M1-16/17).
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<Catalog> = OnceLock::new();
        BUILTIN.get_or_init(Catalog::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_a_registered_template() {
        let mut catalog = Catalog::new();
        catalog.insert(Locale::EN, MessageKey::from_static("greeting"), "Hello");

        assert_eq!(
            catalog.lookup(&Locale::EN, &MessageKey::from_static("greeting")),
            Some("Hello")
        );
    }

    #[test]
    fn lookup_misses_an_unregistered_key() {
        let catalog = Catalog::new();

        assert_eq!(
            catalog.lookup(&Locale::EN, &MessageKey::from_static("absent")),
            None
        );
    }

    #[test]
    fn the_builtin_catalog_is_empty_for_m1() {
        assert_eq!(
            Catalog::builtin().lookup(&Locale::EN, &MessageKey::from_static("anything")),
            None
        );
    }

    #[test]
    fn insert_overwrites_an_existing_entry() {
        let mut catalog = Catalog::new();
        catalog.insert(Locale::EN, MessageKey::from_static("greeting"), "Hello");
        catalog.insert(Locale::EN, MessageKey::from_static("greeting"), "Hi");

        // The contract is "replacing any prior entry": the second template wins.
        assert_eq!(
            catalog.lookup(&Locale::EN, &MessageKey::from_static("greeting")),
            Some("Hi")
        );
    }

    #[test]
    fn lookup_is_isolated_across_locales() {
        let fr = Locale::from_static("fr");
        let mut catalog = Catalog::new();
        catalog.insert(Locale::EN, MessageKey::from_static("hello"), "Hello");
        catalog.insert(fr.clone(), MessageKey::from_static("hello"), "Bonjour");
        catalog.insert(fr.clone(), MessageKey::from_static("bye"), "Au revoir");

        assert_eq!(
            catalog.lookup(&Locale::EN, &MessageKey::from_static("hello")),
            Some("Hello")
        );
        assert_eq!(
            catalog.lookup(&fr, &MessageKey::from_static("hello")),
            Some("Bonjour")
        );
        // A key present only in fr does not leak into en.
        assert_eq!(
            catalog.lookup(&Locale::EN, &MessageKey::from_static("bye")),
            None
        );
    }
}
