# mud-i18n Catalog Guard & Public-API Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the one guard the built-in catalog is missing — a test that a duplicate key in the `ENTRIES` table fails loudly instead of being silently collapsed by the backing `HashMap` — and a lean black-box guard that the crate's public surface (`translate`, `Catalog`, `MessageKey`, `Locale`, and the `t!` macro over the built-in catalog) composes for an external consumer.

**Architecture:** `translate`'s three-step fallback (target locale → `en` → literal key) and the interpolator are already exhaustively unit-tested in `translate.rs`; re-asserting them from `tests/` would duplicate coverage with no added assurance. Two things are genuinely uncovered: (1) `ENTRIES` is a `Vec`-into-`HashMap` table, so two rows sharing a key would silently keep the last with no test to catch it — but `ENTRIES` is a *function-local* `const`, invisible to any test. This plan lifts it to a module-level `const` (a trivial, testability-motivated move) and adds a duplicate-key guard. (2) Nothing drives a *real* built-in key end-to-end through `t!` — `lib.rs`'s macro tests only exercise the literal-key fallback. The public-API guard closes that.

**Tech Stack:** Rust 2024, `std::sync::OnceLock` (built-in catalog), `tracing` (miss warnings), workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they cover; integration tests go in `tests/`.
- Must compile clean under `cargo clippy -p mud-i18n --all-targets`.
- VCS is `jj`. Commit with `jj commit -m "..."`.

## Note on scope

The audit suggested a `tests/public_api.rs` covering target-locale hit, `en` fallback, literal-key fallback, interpolation, and a non-`en` locale. All five are already unit-tested in `translate.rs` (`resolves_a_key_in_the_target_locale`, `falls_back_to_en_when_the_locale_lacks_the_key`, `falls_back_to_the_literal_key_when_absent_everywhere`, the `interpolates_*` tests, and `from_static("fr")`-driven cases). Re-implementing them verbatim in `tests/` adds no assurance. This plan therefore keeps the black-box guard lean — one composed round-trip through the public path plus the genuinely-missing built-in-key-through-`t!` case — and spends the real effort on the duplicate-key guard.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-i18n`
Expected: PASS.

---

### Task 1: Guard the built-in `ENTRIES` table against duplicate keys

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs`

**Interfaces:**
- Produces: a module-level `const ENTRIES: &[(&str, &str)]` (moved out of `builtin_en`), referenced by both `builtin_en` and the new guard test.

- [ ] **Step 1: Lift `ENTRIES` to a module-level `const`**

In `crates/mud-i18n/src/catalog.rs`, move the `ENTRIES` table out of `builtin_en` so a test can see it. Change the top of `builtin_en` from:

```rust
fn builtin_en() -> Catalog {
    const ENTRIES: &[(&str, &str)] = &[
        // look (§3.2)
        ("look.exits", "Exits: { $exits }"),
```

to a module-level const placed **immediately above** `builtin_en`, and have `builtin_en` reference it:

```rust
/// The `(key, en-template)` rows backing the built-in catalog (§3.14.6.2).
///
/// Module-level so the duplicate-key guard test can inspect it: the built-in
/// catalog folds these into a `HashMap`, which would silently keep the last of
/// any duplicated key — the guard turns that into a test failure instead.
const ENTRIES: &[(&str, &str)] = &[
    // look (§3.2)
    ("look.exits", "Exits: { $exits }"),
    // ... (the remaining rows unchanged) ...
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
```

Move **all** the existing rows verbatim (do not retype the templates — cut the array body from inside the fn and paste it into the module-level const). The only structural change is where the `const` lives; the fn body shrinks to the loop.

- [ ] **Step 2: Verify nothing broke**

Run: `cargo test -p mud-i18n --lib`
Expected: PASS — the existing `the_builtin_catalog_holds_the_m1_17_keys` and `the_builtin_catalog_holds_the_session_keys` tests still pass, proving the table moved without content change.

- [ ] **Step 3: Write the failing duplicate-key guard test**

Append to the `#[cfg(test)] mod tests` block in `crates/mud-i18n/src/catalog.rs`:

```rust
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
```

- [ ] **Step 4: Run the guard**

Run: `cargo test -p mud-i18n --lib the_builtin_entries_have_no_duplicate_keys`
Expected: PASS (the table currently has no duplicates — the guard protects future edits). To confirm the guard actually bites, temporarily paste a second `("quit.goodbye", "x")` row into `ENTRIES`, re-run, see it FAIL with the `duplicate built-in key` message, then remove the temporary row.

- [ ] **Step 5: Full crate + clippy**

Run: `cargo test -p mud-i18n && cargo clippy -p mud-i18n --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 6: Commit**

```bash
jj commit -m "test(mud-i18n): guard built-in ENTRIES against duplicate keys"
```

---

### Task 2: Add a lean public-surface guard

**Files:**
- Create: `crates/mud-i18n/tests/public_api.rs`

**Interfaces consumed (public):** `mud_i18n::{translate, Catalog, MessageKey, Locale}` and the `mud_i18n::t!` macro.

- [ ] **Step 1: Write the black-box test**

Create `crates/mud-i18n/tests/public_api.rs`:

```rust
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
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-i18n --test public_api`
Expected: PASS. If any imported item is not actually `pub` (or `t!` is not reachable as `mud_i18n::t`), that is a real surface gap — report it rather than working around it. If a built-in template's exact text differs from the assertion, correct the test to the real template in `ENTRIES` (do **not** change production text).

- [ ] **Step 3: Full crate + clippy**

Run: `cargo test -p mud-i18n && cargo clippy -p mud-i18n --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 4: Commit**

```bash
jj commit -m "test(mud-i18n): add public-surface guard for translate and the t! macro"
```

---

## Self-review checklist

- [ ] `ENTRIES` moved to module scope with content **unchanged** (existing built-in-key tests still pass); the duplicate-key guard references it and bites when a duplicate is injected.
- [ ] `tests/public_api.rs` imports only public items and the `t!` macro; it does not re-duplicate `translate.rs`'s fallback/interpolation matrix.
- [ ] Scope note explains why the five suggested scenarios collapse to one composed round-trip plus the built-in-key-through-`t!` case.
- [ ] `cargo test --workspace` green; clippy clean.
