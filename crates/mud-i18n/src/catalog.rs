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
    /// Holds the engine-emitted `en` strings for the M1-17 built-in commands
    /// and the M1-16 pipeline `command.*` outcomes (§3.14.6.2 requires every
    /// `t!`-referenced `en` key to exist). Keys not listed here still fall
    /// through to the literal key (§3.14.4.3). M2-I replaces this hand-built
    /// table with Fluent bundles without changing the contract.
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<Catalog> = OnceLock::new();
        BUILTIN.get_or_init(builtin_en)
    }
}

/// The `(key, en-template)` rows backing the built-in catalog (§3.14.6.2).
///
/// Module-level so the duplicate-key guard test can inspect it: the built-in
/// catalog folds these into a `HashMap`, which would silently keep the last of
/// any duplicated key — the guard turns that into a test failure instead.
const ENTRIES: &[(&str, &str)] = &[
    // look (§3.2)
    ("look.exits", "Exits: { $exits }"),
    ("look.also-here", "Also here: { $names }"),
    ("look.player-here", "{ $name } is here."),
    ("look.players-here", "{ $names } are here."),
    ("look.void", "You are nowhere in particular."),
    // movement (§3.2.2)
    ("move.no-exit", "You can't go that way."),
    ("move.depart", "{ $name } leaves { $direction }."),
    ("move.arrive-from", "{ $name } arrives from { $direction }."),
    ("move.arrive", "{ $name } arrives."),
    // presence lifecycle (§2.7 step 8): spawn/quit/disconnect
    ("presence.enter", "{ $name } appears from nowhere."),
    ("presence.leave", "{ $name } disappears."),
    // say (§3.6.3)
    ("say.speech", "You say, \"{ $message }\""),
    ("say.broadcast", "{ $name } says, \"{ $message }\""),
    ("say.nothing", "Say what?"),
    // inventory
    ("inventory.header", "You are carrying:"),
    ("inventory.empty", "You are carrying nothing."),
    // who (§3.19)
    ("who.online", "Players online: { $names }"),
    // get / drop and shared object-resolution outcomes (§2.7 step 5)
    ("get.taken", "You take { $item }."),
    ("drop.dropped", "You drop { $item }."),
    ("object.not-here", "You don't see that here."),
    ("object.not-carried", "You aren't carrying that."),
    ("object.ambiguous", "Which do you mean? { $options }"),
    // content cap (§3.6.4)
    ("content.too-long", "Your message is too long."),
    // command pipeline outcomes (§2.7 steps 5–6)
    ("command.not-found", "Unrecognized command. Type 'help'."),
    ("command.ambiguous", "Which do you mean? { $options }"),
    ("command.bad-switch", "Invalid switch: { $reason }."),
    ("command.unbound", "That command isn't available right now."),
    ("command.denied", "You can't do that."),
    // session FSM (§3.19.1)
    (
        "session.prompt",
        "Type 'login <name>' or 'register <name>'. 'help' lists commands.",
    ),
    (
        "session.help",
        "Commands: login <name>, register <name>, who, help, quit.",
    ),
    ("session.who-stub", "Nobody is listed yet."),
    ("session.unknown", "Unrecognized command. Type 'help'."),
    ("session.password", "Password:"),
    ("session.confirm", "Confirm password:"),
    ("session.login-failed", "Login failed."),
    ("session.suspended", "This account is suspended."),
    ("session.banned", "This account is banned."),
    (
        "session.server-error",
        "Something went wrong. Please try again.",
    ),
    (
        "session.no-puppets",
        "You have no characters yet. Type 'new <name>' to create one.",
    ),
    ("session.mismatch", "The passwords did not match."),
    (
        "session.name-invalid",
        "That name isn't allowed. Use letters, digits, _ ' - (1-32 chars).",
    ),
    ("session.username-taken", "That username is already taken."),
    ("session.entered", "Welcome. You are now in the world."),
    ("session.goodbye", "Goodbye."),
    (
        "session.puppet-list",
        "Your characters: { $names }. Type 'play <name>' or 'new <name>'.",
    ),
    ("session.puppet-created", "Created { $name }."),
    // quit (§3.19)
    ("quit.goodbye", "Goodbye!"),
];

/// Builds the `en` catalog for the M1-17 built-in commands.
///
/// Templates use the `{ $name }` placeholder form (see
/// [`translate`](crate::translate)). One source of truth: the built-in command
/// handlers in `mud-engine` reference exactly these keys.
fn builtin_en() -> Catalog {
    let mut catalog = Catalog::new();
    for (key, template) in ENTRIES {
        catalog.insert(Locale::EN, MessageKey::from_static(key), *template);
    }
    catalog
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
    fn the_builtin_catalog_holds_the_m1_17_keys() {
        // A populated key resolves to its en template...
        assert_eq!(
            Catalog::builtin().lookup(&Locale::EN, &MessageKey::from_static("move.no-exit")),
            Some("You can't go that way.")
        );
        // ...while an unlisted key still misses, falling through to the literal
        // key at the translate boundary (§3.14.4.3).
        assert_eq!(
            Catalog::builtin().lookup(&Locale::EN, &MessageKey::from_static("engine.boot")),
            None
        );
    }

    #[test]
    fn the_builtin_catalog_holds_the_session_keys() {
        let catalog = Catalog::builtin();
        for key in [
            "session.prompt",
            "session.help",
            "session.who-stub",
            "session.unknown",
            "session.password",
            "session.confirm",
            "session.login-failed",
            "session.suspended",
            "session.banned",
            "session.server-error",
            "session.no-puppets",
            "session.mismatch",
            "session.name-invalid",
            "session.username-taken",
            "session.entered",
            "session.goodbye",
            "session.puppet-list",
            "session.puppet-created",
        ] {
            assert!(
                catalog
                    .lookup(&Locale::EN, &MessageKey::from_static(key))
                    .is_some(),
                "missing session key: {key}"
            );
        }
    }

    #[test]
    fn the_builtin_catalog_holds_the_command_pipeline_keys() {
        let catalog = Catalog::builtin();
        for key in [
            "command.not-found",
            "command.ambiguous",
            "command.bad-switch",
            "command.unbound",
            "command.denied",
        ] {
            assert!(
                catalog
                    .lookup(&Locale::EN, &MessageKey::from_static(key))
                    .is_some(),
                "missing command pipeline key: {key}"
            );
        }
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

    #[test]
    fn the_builtin_entries_have_no_duplicate_keys() {
        // ENTRIES folds into a HashMap, which would silently keep the last of any
        // duplicated key. This guard makes an accidental duplicate a test failure
        // rather than a hard-to-spot lost template (§3.14.6.2).
        let mut seen = std::collections::HashSet::new();
        for (key, _) in ENTRIES {
            assert!(
                seen.insert(*key),
                "duplicate built-in key in ENTRIES: {key}"
            );
        }
    }
}
