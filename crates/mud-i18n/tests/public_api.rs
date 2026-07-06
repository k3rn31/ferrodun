//! Black-box guard on the crate's public surface (§3.14.4). The fallback matrix
//! and interpolation are unit-tested in `translate.rs`; this only confirms the
//! public API composes for an external consumer (no broken re-export, the `t!`
//! macro reaches the built-in catalog).
#![allow(clippy::expect_used)] // test file; mirrors `allow-expect-in-tests`

use mud_i18n::{t, translate, Catalog, Locale, MessageKey};

#[test]
fn the_public_translate_path_composes_over_a_caller_built_catalog() {
    // Exercises Catalog::new/insert, a non-en target locale, and interpolation
    // through the exported `translate` — the whole seam an external caller sees.
    let mut catalog = Catalog::new();
    let de = Locale::from_static("de");
    catalog.insert(
        de.clone(),
        MessageKey::from_static("greet"),
        "Hallo { $who }",
    );

    assert_eq!(
        translate(&catalog, &de, &MessageKey::from_static("greet"), &[("who", "Sam")]),
        "Hallo Sam"
    );
    // Absent everywhere -> literal key falls through (the public miss contract).
    assert_eq!(
        translate(&catalog, &de, &MessageKey::from_static("absent.key"), &[]),
        "absent.key"
    );
}

#[test]
fn the_macro_resolves_a_real_built_in_key() {
    // Nothing else drives a populated built-in key through `t!` end-to-end;
    // lib.rs's macro tests only cover the literal-key fallback.
    assert_eq!(t!(Locale::EN, "move.no-exit"), "You can't go that way.");
    assert_eq!(
        t!(Locale::EN, "move.depart", name = "Sam", direction = "north"),
        "Sam leaves north."
    );
}
