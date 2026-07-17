# mud-db Crate-Internal Error Conversions — Design

**Date:** 2026-07-17
**Status:** Approved (design); implementation pending
**Issue basis:** #19 (milestone 0.1)
**Spec basis:** SPEC §1.7 — third-party error types must not leak across a
crate's public API

## 1. Motivation and scope

An audit of `mud-db`'s public API (during review of #18) found the error
*variants* clean — `DbError::Sqlx`, `Migrate`, and `BlockingTask` box their
sources as `Box<dyn std::error::Error + Send + Sync>`, and the `SqlitePool`
never crosses the crate boundary. What leaks is the *conversion path*: three
public `From` impls in `crates/mud-db/src/error.rs` name third-party types in
a public trait implementation:

- `impl From<sqlx::Error> for DbError`
- `impl From<sqlx::migrate::MigrateError> for DbError`
- `impl From<tokio::task::JoinError> for DbError`

Rust has no `pub(crate)` trait impls — a `From` impl is as public as the
types it connects — so downstream crates observe `DbError:
From<sqlx::Error>`, making `sqlx` (and `tokio`) public dependencies of
`mud-db`. The `JoinError` impl is not named in issue #19 but is the same
defect and is included here.

**In scope:** removing the three impls and rerouting every propagation site
through crate-internal constructors. Pure refactor: no behavior change, no
new variants, same boxing, same error messages.

**Out of scope:** the secondary audit finding that `TenantConfig`
(`mud-world`) derives `Deserialize` publicly, allowing `load()` validation to
be bypassed — tracked separately as #73.

## 2. Design

### 2.1 Constructors (`crates/mud-db/src/error.rs`)

Delete the three `From` impls. Add one `impl DbError` block with three
`pub(crate)` constructors, each boxing its argument into the existing
variant:

```rust
impl DbError {
    pub(crate) fn from_sqlx(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }

    pub(crate) fn from_migrate(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(err))
    }

    pub(crate) fn from_join(err: tokio::task::JoinError) -> Self {
        Self::BlockingTask(Box::new(err))
    }
}
```

Variant doc comments stay as they are; they already explain the boxing
rationale. The constructors carry doc comments noting they are the only
sanctioned conversion path.

Alternatives considered and rejected: a `pub(crate)` extension trait on
`Result` (terser call sites, but trait machinery purely for ergonomics and
less greppable) and an internal error newtype preserving `?` (most invasive —
re-plumbs private function signatures).

### 2.2 Call sites (~28)

Mechanical sweep of `crates/mud-db/src/sqlite/{mod,accounts,persistent_world}.rs`:

- Every `?` on a sqlx result becomes `.map_err(DbError::from_sqlx)?`.
- The single migration site (`MIGRATOR.run(&pool).await?`, `sqlite/mod.rs`)
  uses `from_migrate`.
- The single `spawn_blocking(...).await?` site (`sqlite/accounts.rs`) uses
  `from_join`.
- Sites with existing custom `map_err` logic (the unique-violation handling
  around `is_unique_violation` in `accounts.rs`) keep that logic and route
  only their generic fallback through the new constructors.
  `is_unique_violation(&sqlx::Error)` is private and stays untouched.

### 2.3 Error handling

Unchanged by construction: the same variants are produced with the same boxed
sources, so `Display`/`source()` output and every caller's behavior are
identical before and after.

## 3. Testing and verification

Refactor discipline: the full test suite passes before and after, with no
test changes required. Removing the `From` impls is self-enforcing — any
missed propagation site is a compile error, so the compiler is the test for
completeness.

Definition of Done (from #19, extended to `tokio`):

1. `rg "impl From<sqlx|impl From<tokio" crates/mud-db/src` → no matches.
2. No `pub` item in `mud-db` names a `sqlx` or `tokio` type (variants,
   signatures, trait impls).
3. `cargo test --workspace` clean.
4. `cargo clippy --workspace --all-targets -- -D warnings` clean.
5. `cargo fmt --all --check` clean.

No documentation-site update: the change has no external surface
(commands, config, network, CLI all unaffected).
