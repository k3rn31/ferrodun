# mud-account Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the one real coverage gap in `mud-account` — `puppet.rs` has no tests — and add a lean black-box guard on the crate's public surface.

**Architecture:** `mud-account` is pure domain (accounts, state, argon2id credential, puppets). Most of the audit's suggested coverage **already exists as in-file unit tests**: `account.rs` fully tests `AccountState::login_rejection` (including `deleted_is_indistinguishable_from_unknown`), state token round-trips, and unknown-token rejection; `credential.rs` tests hash→verify, `from_phc`/`as_phc` round-trip, `verify_phc` against a stored hash, corrupt-hash rejection, and distinct salts. So this plan does **not** duplicate that behavior. It adds (1) the missing `puppet.rs` construction test and (2) a small `tests/public_api.rs` black-box test that confirms the public API composes for an external consumer (catches a broken re-export or an accidentally non-`pub` item).

**Tech Stack:** Rust 2024, `argon2`/`secrecy`, `mud-core` for `EntityKey`, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they cover; integration tests go in `tests/`.
- Must compile clean under `cargo clippy -p mud-account --all-targets`.
- Tests-only change; no production code modified.
- VCS is `jj`. Commit with `jj commit -m "..."`.

## Note on scope

The audit suggested `tests/credentials.rs` and `tests/account.rs`. Those behaviors are already unit-tested in-crate (see `credential.rs`/`account.rs` test modules). Re-implementing them in `tests/` would duplicate coverage with no added assurance. This plan therefore replaces that suggestion with a single lean `tests/public_api.rs` surface guard.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green and the existing coverage**

Run:
```bash
cargo test -p mud-account
grep -n "deleted_is_indistinguishable_from_unknown\|verify_phc_matches_a_stored_hash" crates/mud-account/src/account.rs crates/mud-account/src/credential.rs
```
Expected: tests PASS; both grep hits confirm the behavioral coverage already exists (so we don't duplicate it).

---

### Task 1: Add the `puppet.rs` construction test

**Files:**
- Modify: `crates/mud-account/src/puppet.rs`

- [ ] **Step 1: Add a `#[cfg(test)]` module**

Append to `crates/mud-account/src/puppet.rs`:

```rust
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
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-account --lib puppet`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-account): cover Puppet::new construction"
```

---

### Task 2: Add a public-surface guard

**Files:**
- Create: `crates/mud-account/tests/public_api.rs`

- [ ] **Step 1: Write the black-box test**

Create `crates/mud-account/tests/public_api.rs`:

```rust
//! Black-box guard on the crate's public surface (§3.15.1). Behavior is
//! unit-tested in-crate; this only confirms the public API composes for an
//! external consumer (no broken re-export, nothing accidentally private).
#![allow(clippy::expect_used)] // test file; mirrors `allow-expect-in-tests`

use mud_account::{AccountState, Credential, LoginError};

#[test]
fn a_credential_round_trips_through_its_phc_string() {
    let cred = Credential::hash("correct-horse").expect("hashing succeeds");
    let restored = Credential::from_phc(cred.as_phc()).expect("its own PHC parses");
    assert!(restored.verify("correct-horse"), "the right password verifies");
    assert!(!restored.verify("wrong"), "the wrong password is refused");
    assert!(
        Credential::verify_phc(cred.as_phc(), "correct-horse"),
        "verify_phc matches a stored hash"
    );
}

#[test]
fn account_state_login_rejection_is_reachable_publicly() {
    assert_eq!(AccountState::Active.login_rejection(), None);
    assert_eq!(
        AccountState::Deleted.login_rejection(),
        Some(LoginError::UnknownUser),
        "a soft-deleted account reads as unknown"
    );
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-account --test public_api`
Expected: PASS. If any imported item is not actually `pub`, that is a real surface gap — report it rather than working around it.

- [ ] **Step 3: Full crate + clippy + commit**

Run: `cargo test -p mud-account && cargo clippy -p mud-account --all-targets`
Expected: PASS, clippy clean.
```bash
jj commit -m "test(mud-account): add public-surface guard for Credential and AccountState"
```

---

## Self-review checklist

- [ ] `puppet.rs` now has a construction test; no behavioral duplication of the existing `account.rs`/`credential.rs` suites.
- [ ] `tests/public_api.rs` imports only public items and exercises them.
- [ ] Scope note explains why the suggested `tests/credentials.rs`/`tests/account.rs` were folded into one lean surface guard.
- [ ] `cargo test --workspace` green; clippy clean.
