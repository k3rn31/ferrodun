//! Engine-string lookup seam (§3.14.4).
//!
//! Every engine-emitted player-facing string resolves through one typed
//! boundary — the [`t!`] macro over [`translate`] — instead of being assembled
//! inline in Rust (§3.14.4.1: sentences are single keyed messages so translators
//! can reorder them). Keys and locales are parsed into typed domain values
//! ([`MessageKey`], [`Locale`]) at the call site (§3.14.4.4); a missing key falls
//! back to `en` then to the literal key, warning on the way (§3.14.4.3).
//!
//! For M1 the backing [`Catalog`] is a static, hand-built `en` table: this ships
//! the stable call-site contract so M2-I can swap the internals for Fluent
//! (hot-reload, per-tenant overrides, locale resolution) **without changing a
//! single call site**.

mod catalog;
mod key;
mod locale;
mod translate;

pub use catalog::Catalog;
pub use key::MessageKey;
pub use locale::Locale;
pub use translate::translate;

/// Resolves a translatable string against the built-in catalog (§3.14.4.1).
///
/// ```ignore
/// t!(locale, "command.not-found");
/// t!(locale, "command.moved", dir = "north", room = name);
/// ```
///
/// Takes a [`Locale`] by value, a `'static` key literal, and zero or more named
/// arguments. Argument values are rendered through [`Display`](std::fmt::Display)
/// and interpolated into `{ $name }` placeholders. Resolution and fallback are
/// [`translate`]'s; this macro only parses the literals into typed values and
/// supplies [`Catalog::builtin`].
#[macro_export]
macro_rules! t {
    ($locale:expr, $key:expr $(, $name:ident = $value:expr )* $(,)?) => {{
        // Own each rendered value so the &str slice borrows live for the call.
        $( let $name = ::std::string::ToString::to_string(&$value); )*
        $crate::translate(
            $crate::Catalog::builtin(),
            &$locale,
            &$crate::MessageKey::from_static($key),
            &[ $( (::core::stringify!($name), $name.as_str()) ),* ],
        )
    }};
}

#[cfg(test)]
mod tests {
    use crate::Locale;

    #[test]
    fn macro_falls_back_to_the_literal_for_an_unlisted_key() {
        // `engine.boot` is not in the builtin catalog, so it renders literally
        // (§3.14.4.3).
        assert_eq!(t!(Locale::EN, "engine.boot"), "engine.boot");
    }

    #[test]
    fn macro_threads_named_args_into_the_literal_fallback() {
        let who = "Sam";
        assert_eq!(t!(Locale::EN, "{ $who } waves", who = who), "Sam waves");
    }
}
