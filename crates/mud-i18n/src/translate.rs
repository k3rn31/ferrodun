//! The lookup boundary: resolve a key to player-facing text (§3.14.4).

use crate::catalog::Catalog;
use crate::key::MessageKey;
use crate::locale::Locale;

/// Resolves `key` to a player-facing string in `locale`, interpolating `args`.
///
/// This is the §3.14.4 system boundary. Resolution falls back in order
/// (§3.14.4.3): (a) `(locale, key)`, (b) `(en, key)`, then (c) the literal key
/// text. A fall-through to the literal emits a structured `tracing` warning —
/// missing keys MUST NOT be silently swallowed (§8 rule 5).
///
/// `args` are named `(name, value)` pairs interpolated into `{ $name }`
/// placeholders. Pass `&[]` when the message takes no arguments.
#[must_use]
pub fn translate(
    catalog: &Catalog,
    locale: &Locale,
    key: &MessageKey,
    args: &[(&str, &str)],
) -> String {
    let template = catalog
        .lookup(locale, key)
        .or_else(|| catalog.lookup(&Locale::EN, key))
        .unwrap_or_else(|| {
            // A miss is operator-facing telemetry, never shown to the player; the
            // literal key still renders so the message is legible (§3.14.4.3).
            tracing::warn!(key = %key, locale = %locale, "missing i18n key; falling back to literal key");
            key.as_str()
        });

    interpolate(template, args)
}

/// Substitutes `{ $name }` / `{$name}` placeholders with provided argument values.
///
/// A deliberately minimal stand-in for Fluent's placeable parsing (arriving at
/// M2-I): only literal named-variable placeholders are recognised, with or
/// without surrounding spaces. A placeholder with no matching arg is left as-is;
/// an unreferenced arg is ignored.
///
/// NOTE: this throwaway folds substitutions sequentially, so a value
/// substituted for one arg can still contain another arg's placeholder and be
/// re-scanned. Fluent (M2-I) resolves placeables from the template alone and
/// never re-scans argument values; do not treat this loop as that contract.
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() || !template.contains('{') {
        return template.to_owned();
    }

    args.iter().fold(template.to_owned(), |acc, (name, value)| {
        acc.replace(&format!("{{ ${name} }}"), value)
            .replace(&format!("{{${name}}}"), value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    fn catalog_with(locale: Locale, key: &'static str, template: &str) -> Catalog {
        let mut catalog = Catalog::new();
        catalog.insert(locale, MessageKey::from_static(key), template);
        catalog
    }

    #[test]
    fn resolves_a_key_in_the_target_locale() {
        let catalog = catalog_with(Locale::from_static("fr"), "hello", "Bonjour");

        assert_eq!(
            translate(
                &catalog,
                &Locale::from_static("fr"),
                &MessageKey::from_static("hello"),
                &[]
            ),
            "Bonjour"
        );
    }

    #[test]
    fn falls_back_to_en_when_the_locale_lacks_the_key() {
        let catalog = catalog_with(Locale::EN, "hello", "Hello");

        // Requested in fr, absent there, present in en (§3.14.4.3a).
        assert_eq!(
            translate(
                &catalog,
                &Locale::from_static("fr"),
                &MessageKey::from_static("hello"),
                &[]
            ),
            "Hello"
        );
    }

    #[test]
    fn falls_back_to_the_literal_key_when_absent_everywhere() {
        let catalog = Catalog::new();

        assert_eq!(
            translate(
                &catalog,
                &Locale::EN,
                &MessageKey::from_static("totally.unknown"),
                &[]
            ),
            "totally.unknown"
        );
    }

    #[test]
    #[traced_test]
    fn a_missing_key_warns() {
        let catalog = Catalog::new();

        let _ = translate(
            &catalog,
            &Locale::EN,
            &MessageKey::from_static("missing.key"),
            &[],
        );

        // §3.14.4.3 / §8 rule 5: misses are never silently swallowed.
        assert!(logs_contain("missing i18n key"));
        assert!(logs_contain("missing.key"));
    }

    #[test]
    #[traced_test]
    fn an_en_fallback_does_not_warn() {
        let catalog = catalog_with(Locale::EN, "hello", "Hello");

        // Resolving via the en fallback is a hit, not a miss: the warning fires
        // only when the key is absent everywhere (§3.14.4.3).
        let _ = translate(
            &catalog,
            &Locale::from_static("fr"),
            &MessageKey::from_static("hello"),
            &[],
        );

        assert!(!logs_contain("missing i18n key"));
    }

    #[test]
    fn interpolates_named_args_with_and_without_spaces() {
        let catalog = catalog_with(Locale::EN, "greet", "Hi { $who } and {$who}");

        assert_eq!(
            translate(
                &catalog,
                &Locale::EN,
                &MessageKey::from_static("greet"),
                &[("who", "Sam")]
            ),
            "Hi Sam and Sam"
        );
    }

    #[test]
    fn leaves_an_unprovided_placeholder_intact_and_ignores_extra_args() {
        let catalog = catalog_with(Locale::EN, "greet", "Hi { $who }");

        assert_eq!(
            translate(
                &catalog,
                &Locale::EN,
                &MessageKey::from_static("greet"),
                &[("other", "x")]
            ),
            "Hi { $who }"
        );
    }
}
