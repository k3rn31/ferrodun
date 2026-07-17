# mud-db Crate-Internal Error Conversions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the three public `From` impls that leak `sqlx`/`tokio` types through `mud-db`'s public API, replacing them with `pub(crate)` constructors (issue #19).

**Architecture:** Rust has no `pub(crate)` trait impls, so the `From` impls must be deleted outright and every propagation site rerouted through explicit `pub(crate)` constructors on `DbError`. Pure refactor: same variants, same boxing, same messages — no behavior change and no test changes. Each task migrates one file and deletes any `From` impl whose last user it migrates, so every task compiles clean (no `dead_code` from a not-yet-used constructor) and commits green.

**Tech Stack:** Rust workspace; `jj` for VCS (NOT git — commit with `jj commit -m "..."`); crate `crates/mud-db`.

**Spec:** `docs/superpowers/specs/2026-07-17-mud-db-internal-error-conversions-design.md`

## Global Constraints

- `unwrap()` forbidden everywhere; `expect()` only in tests, with a descriptive message.
- Workspace clippy denies warnings (`unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`, `dead_code` via `-D warnings`) — an unused `pub(crate)` fn fails the build, hence the task ordering below.
- Code and comments in English; comments say *why*, not *how*.
- Do not touch test code: every `#[cfg(test)]` site already uses `.expect(...)`/`.is_err()` and never relied on the `From` impls.
- No documentation-site update: no external surface changes (per spec §3).
- VCS is jj: `jj commit -m "<msg>"` commits the working copy; never run `git commit`.

---

### Task 1: `from_sqlx` + `from_migrate` constructors; migrate `sqlite/mod.rs`

**Files:**
- Modify: `crates/mud-db/src/error.rs:106-116` (replace two `From` impls region)
- Modify: `crates/mud-db/src/sqlite/mod.rs:59-60,93,96`

**Interfaces:**
- Consumes: existing `DbError::Sqlx` / `DbError::Migrate` variants (unchanged).
- Produces: `pub(crate) fn DbError::from_sqlx(err: sqlx::Error) -> DbError` and `pub(crate) fn DbError::from_migrate(err: sqlx::migrate::MigrateError) -> DbError`, used by Tasks 2–3.

- [ ] **Step 1: Baseline — verify tests pass before refactoring**

Run: `cargo test -p mud-db`
Expected: PASS (all tests green). This is the refactor's "before" gate.

- [ ] **Step 2: Add the constructors; delete `From<sqlx::migrate::MigrateError>`**

In `error.rs`, delete ONLY this impl (`error.rs:112-116`) — its sole user, `mod.rs:60`, is migrated in this task:

```rust
impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(err))
    }
}
```

KEEP `impl From<sqlx::Error> for DbError` for now: `accounts.rs` and `persistent_world.rs` still rely on it until Tasks 2–3 migrate them; Task 3 deletes it. In place of the deleted impl, add:

```rust
impl DbError {
    /// Wraps a driver failure. Crate-internal on purpose: the only sanctioned
    /// path from a `sqlx` error into [`DbError`], so `sqlx` never appears in
    /// the public API (no public `From` impl).
    pub(crate) fn from_sqlx(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }

    /// Wraps a migration failure. Crate-internal for the same reason as
    /// [`DbError::from_sqlx`].
    pub(crate) fn from_migrate(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(err))
    }
}
```

Resulting `error.rs` tail order: enum → `impl DbError` block above → the (kept, for now) `impl From<sqlx::Error>` → `impl From<tokio::task::JoinError>` (Task 2 deletes it).

- [ ] **Step 3: Migrate the 4 sites in `sqlite/mod.rs`**

In `TenantDb::open` (`mod.rs:59-60`):

```rust
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(DbError::from_sqlx)?;
        MIGRATOR.run(&pool).await.map_err(DbError::from_migrate)?;
```

In `TenantDb::world_id` (`mod.rs:93,96`) — append `.map_err(DbError::from_sqlx)` before each `?`:

```rust
        .execute(&self.pool)
        .await
        .map_err(DbError::from_sqlx)?;
        let row = sqlx::query!(r#"SELECT world_id AS "world_id!" FROM server WHERE id = 1"#)
            .fetch_one(&self.pool)
            .await
            .map_err(DbError::from_sqlx)?;
```

- [ ] **Step 4: Verify green**

Run: `cargo test -p mud-db && cargo clippy -p mud-db --all-targets -- -D warnings`
Expected: PASS, no warnings. Both new constructors are used (`from_migrate` at `mod.rs`, `from_sqlx` at three sites), so no `dead_code`.

- [ ] **Step 5: Commit**

```bash
jj commit -m "refactor(mud-db): crate-internal sqlx conversions; drop public From<MigrateError> (#19)"
```

---

### Task 2: `from_join` constructor; migrate `sqlite/accounts.rs`

**Files:**
- Modify: `crates/mud-db/src/error.rs` (delete `From<tokio::task::JoinError>` impl, add `from_join` to the `impl DbError` block from Task 1)
- Modify: `crates/mud-db/src/sqlite/accounts.rs:71,97,108,147,152,160,167,168,188`

**Interfaces:**
- Consumes: `DbError::from_sqlx` (Task 1); existing `DbError::BlockingTask` variant.
- Produces: `pub(crate) fn DbError::from_join(err: tokio::task::JoinError) -> DbError`.

- [ ] **Step 1: Replace the `JoinError` `From` impl with a constructor**

Delete from `error.rs`:

```rust
impl From<tokio::task::JoinError> for DbError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::BlockingTask(Box::new(err))
    }
}
```

Add inside the `impl DbError` block created in Task 1, after `from_migrate`:

```rust
    /// Wraps a failed `spawn_blocking` join. Crate-internal for the same
    /// reason as [`DbError::from_sqlx`].
    pub(crate) fn from_join(err: tokio::task::JoinError) -> Self {
        Self::BlockingTask(Box::new(err))
    }
```

- [ ] **Step 2: Migrate the 9 sites in `accounts.rs`**

`register` (line 71) — the unique-violation branch keeps its logic; only the generic fallback changes from `Into` to the constructor:

```rust
            Err(err) if is_unique_violation(&err) => Ok(Err(RegisterError::UsernameTaken)),
            Err(err) => Err(DbError::from_sqlx(err)),
```

`authenticate` (lines 97, 108):

```rust
        .fetch_optional(self.db.pool())
        .await
        .map_err(DbError::from_sqlx)?;
```

```rust
        let verified =
            tokio::task::spawn_blocking(move || Credential::verify_phc(&stored, &attempt))
                .await
                .map_err(DbError::from_join)?;
```

`create_puppet` (lines 147, 152, 160, 167, 168) — same `.map_err(DbError::from_sqlx)` before each `?`:

```rust
        let mut tx = self.db.pool().begin().await.map_err(DbError::from_sqlx)?;
```

```rust
        .fetch_one(&mut *tx)
        .await
        .map_err(DbError::from_sqlx)?;
```

```rust
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_sqlx)?;
```

(twice — the `puppets` insert at 160 and the `location` insert at 167)

```rust
        tx.commit().await.map_err(DbError::from_sqlx)?;
```

`puppets_of` (line 188):

```rust
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from_sqlx)?;
```

Leave `is_unique_violation(&sqlx::Error)` (line 213) untouched — it is a private fn, not a leak.

- [ ] **Step 3: Verify green**

Run: `cargo test -p mud-db && cargo clippy -p mud-db --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(mud-db): drop public From<JoinError>; route accounts through crate-internal conversions (#19)"
```

---

### Task 3: Migrate `sqlite/persistent_world.rs`; delete `From<sqlx::Error>`

**Files:**
- Modify: `crates/mud-db/src/error.rs` (delete the last `From` impl)
- Modify: `crates/mud-db/src/sqlite/persistent_world.rs:114,131,144,288,308,322,327,332,338,365,379,398,416,431`

**Interfaces:**
- Consumes: `DbError::from_sqlx` (Task 1).
- Produces: `mud-db` public API free of third-party types; compiler-verified completeness (a missed site fails to build once the impl is gone).

- [ ] **Step 1: Delete the remaining `From` impl from `error.rs`**

```rust
impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }
}
```

Delete it FIRST: from here on the compiler enumerates every unmigrated site as a build error, which is the completeness check.

- [ ] **Step 2: Migrate the 14 sites in `persistent_world.rs`**

Every site is the same mechanical shape — `.await?` on a sqlx call becomes `.await.map_err(DbError::from_sqlx)?`. Sites (all in production code; the file's tests use `.expect`):

- `load`: `fetch_all` at lines 114, 131, 144
- `hydrate`: `fetch_optional` at lines 288, 308
- `apply_create`: `begin()` at 322, `fetch_one` at 327, `tx.commit()` at 332, `tx.rollback()` at 338
- `persist_move` 365, `persist_clear_location` 379, `persist_inventory_add` 398, `persist_inventory_remove` 416, `persist_teardown` 431 — each an `.execute(...).await?`

Representative before/after (line 114; apply identically at every listed site):

```rust
        let keys =
            sqlx::query!(r#"SELECT entity_key AS "entity_key!" FROM entities ORDER BY entity_key"#)
                .fetch_all(db.pool())
                .await
                .map_err(DbError::from_sqlx)?;
```

Transaction sites in `apply_create`:

```rust
        let mut tx = self.db.pool().begin().await.map_err(DbError::from_sqlx)?;
```

```rust
            Ok(id) => {
                tx.commit().await.map_err(DbError::from_sqlx)?;
```

```rust
            Err(error) => {
                tx.rollback().await.map_err(DbError::from_sqlx)?;
```

- [ ] **Step 3: Verify the compiler finds nothing left**

Run: `cargo build -p mud-db && cargo test -p mud-db && cargo clippy -p mud-db --all-targets -- -D warnings`
Expected: PASS. Any `?`-conversion build error here means a missed site — fix it with the same `map_err` pattern.

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(mud-db): drop public From<sqlx::Error>; conversions now fully crate-internal (#19)"
```

---

### Task 4: Definition-of-Done verification and journal entry

**Files:**
- Modify: `.claude/JOURNAL.md` (append entry)

**Interfaces:**
- Consumes: Tasks 1–3 complete.
- Produces: verified DoD for issue #19; journal breadcrumb.

- [ ] **Step 1: DoD greps — no third-party type in the public API**

Run: `rg -n "impl From<sqlx|impl From<tokio" crates/mud-db/src`
Expected: no matches (exit code 1).

Run: `rg -n "pub fn|pub struct|pub enum|pub trait" crates/mud-db/src | rg "sqlx|tokio"`
Expected: no matches. (The `pub(crate)` constructors and `pool()` are crate-internal and do not count.)

- [ ] **Step 2: Full workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all PASS — the issue's Definition of Done.

- [ ] **Step 3: Append journal entry to `.claude/JOURNAL.md`** (newest at bottom)

```markdown
## 2026-07-17 — mud-db crate-internal error conversions (#19)

- **Spec:** §1.7 (no third-party errors across public APIs); design doc 2026-07-17-mud-db-internal-error-conversions-design.md
- **Done:** removed public `From<sqlx::Error>`, `From<sqlx::migrate::MigrateError>`, `From<tokio::task::JoinError>` from `DbError`; added `pub(crate)` `from_sqlx`/`from_migrate`/`from_join`; rerouted 27 propagation sites in sqlite/{mod,accounts,persistent_world}.rs. Pure refactor, no behavior change. `JoinError` impl included beyond the issue text (same defect). TenantConfig Deserialize finding split off as #73.
- **Verify:** `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --all --check`, DoD greps for `impl From<sqlx|tokio`.
- **Next:** #73 (mud-world TenantConfig raw-deserialization bypass).
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "docs(journal): log #19 crate-internal error conversions"
```
