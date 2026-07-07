# mud-db Error Axes, Schema Hardening & Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clarify `DbError`'s two failure axes in its docs, make the `puppets.account_id` foreign-key delete behavior explicit in the schema, and close two untested defensive paths (`parse_puppet_name` corruption in `puppets_of`, and the `world_id` `InvalidId` branch).

**Architecture:** `mud-db` persists domain types over a per-tenant SQLite file. `DbError` already mixes two distinct axes — genuine driver/infrastructure faults (`Sqlx`, `Migrate`, `BlockingTask`) versus corruption/invariant violations surfaced-not-panicked (`InvalidId`, `CorruptValue`, `DanglingReference`, …). This plan documents that split in place (no type split — see decision below), hardens one under-specified FK, and adds tests for two defensive branches that currently have no coverage.

**Tech Stack:** Rust 2024, `sqlx` (SQLite), `tokio`, `tempfile` for test DBs, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- No `unwrap()`/`expect()`/`panic!()` in production code; `expect()` in tests must carry a message.
- Libraries define error types with `thiserror`; never leak third-party error types through the public API (the `Box<dyn Error>` wrapping of `sqlx`/`tokio` errors is deliberate — preserve it).
- Database schemas are normalized to 3NF; a denormalization needs a `-- DENORMALIZED:` comment.
- Must compile clean under `cargo clippy -p mud-db --all-targets`.
- VCS is `jj`. Commit with `jj commit -m "..."`.

## Design decisions (confirm at review)

- **`DbError` is documented, not split.** Splitting into two enums (`DriverError` vs `ConsistencyError`) would ripple through every `?` and every public `Result<_, DbError>` signature for no functional gain — the two axes are already distinguishable by variant, and callers uniformly propagate. We instead group the variants with section comments and state the two axes in the type doc. (YAGNI; a split can happen if a caller ever needs to branch on the axis.)
- **No `CHECK (state IN (...))` on `accounts.state`.** The finding suggested one, but it is the wrong tool here: (1) it would duplicate `mud_account::AccountState`'s token set into SQL, and that enum is the single source of truth — the two would drift; (2) the codebase's chosen, **tested** defense against a bad state token is the load-time `parse_state → DbError::CorruptValue` path (`a_corrupt_state_token_surfaces_as_a_db_error`), and a CHECK would make that path unreachable by blocking the test's `force_state` write. We instead **document the deliberate absence** in the migration. If review prefers the CHECK, that test must be reworked and the enum's tokens pinned — raise it then.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-db`
Expected: PASS.

---

### Task 1: Document `DbError`'s two failure axes

**Files:**
- Modify: `crates/mud-db/src/error.rs`

- [ ] **Step 1: Rewrite the type doc and group the variants**

At the top of the `DbError` enum in `crates/mud-db/src/error.rs`, replace the type-level doc comment with one that names the two axes:

```rust
/// Errors raised by the persistence layer, backend-agnostic across the SQLite
/// backend and any future PostgreSQL one.
///
/// Two failure axes share this type. **Infrastructure faults** — the driver, a
/// migration, or an offloaded blocking task failed (`Sqlx`, `Migrate`,
/// `BlockingTask`) — are transient/operational. **Consistency violations** —
/// a persisted value is out of range, unparseable, dangling, or otherwise
/// impossible in a well-formed database (`InvalidId`, `CorruptValue`,
/// `KeyOutOfRange`, `EntityNotMapped`, `UnknownPlaceKey`, `PlaceNotMapped`,
/// `DanglingReference`, `LoadArenaExhausted`, `UnsupportedEffect`) — are
/// surfaced rather than panicked on, so a corrupt or newer-schema row fails
/// loudly instead of silently. Callers propagate both; no caller branches on
/// the axis today, so the two live in one enum.
```

Then add two one-line section comments inside the enum body, immediately before the first variant of each group, e.g. `// --- Infrastructure faults ---` above `Sqlx` and `// --- Consistency violations ---` above `InvalidId`. Do not reorder or rename variants.

- [ ] **Step 2: Build**

Run: `cargo test -p mud-db && cargo clippy -p mud-db --all-targets`
Expected: PASS, clippy clean (doc-only change).

- [ ] **Step 3: Commit**

```bash
jj commit -m "docs(mud-db): name DbError's two failure axes and group variants"
```

---

### Task 2: Make the `puppets.account_id` FK delete behavior explicit

**Files:**
- Modify: `crates/mud-db/migrations/sqlite/0001_initial.sql`

The `puppets.account_id` FK currently reads `REFERENCES accounts (id)` with no `ON DELETE`, leaving SQLite's implicit `NO ACTION`. Make it explicit as `RESTRICT`: deleting an account that still owns puppets must be blocked, because a puppet is itself an entity whose teardown must go through the entity destroy path (§2.5.3.1) — cascading the account delete would orphan the puppet's `entities` row. This is pre-release; the initial migration is edited in place (test DBs are rebuilt from scratch).

- [ ] **Step 1: Add `ON DELETE RESTRICT` and a rationale comment**

In `crates/mud-db/migrations/sqlite/0001_initial.sql`, change the `puppets` table's `account_id` line from:

```sql
    account_id INTEGER NOT NULL REFERENCES accounts (id),
```
to:
```sql
    -- RESTRICT (not CASCADE): a puppet is an entity; deleting an account must
    -- not orphan its puppets' `entities` rows, whose teardown goes through the
    -- entity destroy path (§2.5.3.1). Account removal must delete puppets first.
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
```

- [ ] **Step 2: Document the deliberate absence of a `state` CHECK**

In the same file, above the `state TEXT NOT NULL DEFAULT 'active'` line in the `accounts` table, add:

```sql
    -- No CHECK on `state`: mud_account::AccountState is the single source of
    -- truth for the token set, and an unknown token is caught at load time as
    -- DbError::CorruptValue. A CHECK here would duplicate the enum and could
    -- drift from it.
```

- [ ] **Step 3: Add a test that the FK blocks deleting an account with puppets**

In `crates/mud-db/src/sqlite/accounts.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[tokio::test]
    async fn deleting_an_account_that_owns_a_puppet_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("aldous");
        let account = accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        let start = PlaceKey::parse("town_square").expect("valid slug");
        accounts
            .create_puppet(account, PuppetName::parse("hero").expect("valid name"), &start)
            .await
            .expect("puppet created");

        // Foreign keys are enabled on the pool, so the RESTRICT FK rejects the
        // delete while a puppet still references the account.
        let deleted = sqlx::query("DELETE FROM accounts WHERE username = ?")
            .bind("aldous")
            .execute(db.pool())
            .await;

        assert!(
            deleted.is_err(),
            "deleting an account with puppets must be refused by the FK"
        );
    }
```

Note: confirm `register` returns the `AccountId` used by `create_puppet` (it does — see the signature at `accounts.rs:139`). If `register`'s success payload is not the `AccountId`, fetch it via the existing account-lookup path before `create_puppet`.

- [ ] **Step 4: Run and commit**

Run: `cargo test -p mud-db && cargo clippy -p mud-db --all-targets`
Expected: PASS (the new FK test plus all existing tests), clippy clean.
```bash
jj commit -m "feat(mud-db): make puppets.account_id ON DELETE RESTRICT explicit, document no-state-CHECK"
```

---

### Task 3: Cover `parse_puppet_name` corruption in `puppets_of`

**Files:**
- Modify: `crates/mud-db/src/sqlite/accounts.rs`

- [ ] **Step 1: Write the failing test**

Add to `accounts.rs`'s test module:

```rust
    #[tokio::test]
    async fn a_corrupt_puppet_name_surfaces_as_a_db_error() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("aldous");
        let account = accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        let start = PlaceKey::parse("town_square").expect("valid slug");
        accounts
            .create_puppet(account, PuppetName::parse("hero").expect("valid name"), &start)
            .await
            .expect("puppet created");

        // Force a persisted name that PuppetName::parse rejects, standing in for
        // a manual edit or a row from a newer schema.
        sqlx::query("UPDATE puppets SET name = ?")
            .bind("not a valid name!")
            .execute(db.pool())
            .await
            .expect("force corrupt name");

        let err = accounts
            .puppets_of(account)
            .await
            .expect_err("a corrupt puppet name is corruption");
        assert!(matches!(err, DbError::CorruptValue(_)), "got {err:?}");
    }
```

Note: verify `"not a valid name!"` is actually rejected by `mud_account::PuppetName::parse` (read its parse rules). If that string is somehow valid, substitute any value the parser rejects (e.g. an empty string or one with a leading digit) — the point is only to drive `parse_puppet_name`'s error arm.

- [ ] **Step 2: Run — expect PASS**

Run: `cargo test -p mud-db a_corrupt_puppet_name`
Expected: PASS (no production change needed — this covers an existing branch at `accounts.rs:208`). If it fails because the chosen name parses as valid, pick a genuinely-invalid name per the note.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-db): cover corrupt-puppet-name path in puppets_of"
```

---

### Task 4: Cover the `world_id` `InvalidId` branch

**Files:**
- Modify: `crates/mud-db/src/sqlite/mod.rs`

- [ ] **Step 1: Write the failing test**

In `crates/mud-db/src/sqlite/mod.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[tokio::test]
    async fn a_negative_persisted_world_id_is_reported_as_invalid() {
        let dir = TempDir::new().expect("tempdir");
        let db = open_in(&dir).await;

        // First call generates and persists a valid id.
        let _ = db.world_id().await.expect("world id generated");

        // Corrupt it to a value outside the NonZeroU64 range, standing in for a
        // manual edit or a row from a newer schema.
        sqlx::query("UPDATE server SET world_id = -1 WHERE id = 1")
            .execute(db.pool())
            .await
            .expect("force corrupt world id");

        let err = db
            .world_id()
            .await
            .expect_err("a negative persisted world id is corruption");
        assert!(matches!(err, DbError::InvalidId(-1)), "got {err:?}");
    }
```

Note: `open_in` and `pool()` already exist in this module's test helpers / API. `pool()` is `pub(crate)`, so the test (same crate) may call it.

- [ ] **Step 2: Run — expect PASS**

Run: `cargo test -p mud-db a_negative_persisted_world_id`
Expected: PASS (covers the existing `u64::try_from(...).map_err(...)` branch at `mod.rs:92`). The `INSERT ... ON CONFLICT DO NOTHING` in `world_id` is a no-op on the second call, so the `SELECT` reads the corrupted `-1`.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-db): cover InvalidId branch of world_id load"
```

---

## Self-review checklist

- [ ] `DbError` doc names both axes; variants grouped with section comments; no variant renamed or reordered; `Box<dyn Error>` wrapping preserved.
- [ ] `puppets.account_id` is `ON DELETE RESTRICT` with a rationale comment; `accounts.state` carries the "no CHECK" rationale comment.
- [ ] New tests: FK-blocks-account-delete, corrupt-puppet-name, negative-world-id — all green.
- [ ] Decisions (no type split, no state CHECK) are called out above for review.
- [ ] `cargo test --workspace` green; clippy clean.
