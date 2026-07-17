# Single `resolve_puppet` per successful Enter

**Date:** 2026-07-17
**Issue:** #33
**Status:** Approved

## Problem

For a successful `Enter`, the engine resolves the same `EntityKey` twice in
`crates/mud-engine/src/session/mod.rs`:

1. `perform` calls `backend.resolve_puppet(key)` to decide
   `EffectResult::Entered` vs `EffectResult::BackendError` — then discards the
   resolved `EntityId`.
2. `apply_terminal` re-resolves the same key to build the `InWorldBinding`,
   with a fallback arm (drop the session) for a failure the code itself
   documents as a "vanishing chance".

The second lookup cannot meaningfully differ from the first (synchronous
sequence, nothing mutates in between). The redundant call and the
runtime-handled "impossible" state violate the project's type-driven-design
rule: encode the invariant in types instead.

## Design

Carry the resolved `EntityId` through the FSM types so the bind reuses it.
No observable behavior changes; the failure path (resolve fails in `perform`
→ `BackendError` → `ServerError` message → back to puppet select) is
untouched.

### `mud-session` type changes

- `EffectResult::Entered` → `EffectResult::Entered { entity: EntityId }`.
  `EntityId` comes from `mud-core`, already a dependency; the crate stays
  pure and carries the value opaquely.
- `Terminal::Bound` replaces `puppet: EntityKey` with `entity: EntityId`:

  ```rust
  Terminal::Bound {
      account: AccountId,
      entity: EntityId,   // was: puppet: EntityKey
      name: PuppetName,
  }
  ```

  The `EntityKey` leaves `Bound` entirely — its only consumer was the
  redundant re-resolution. `AwaitingEnter` keeps `chosen: EntityKey`
  because it is still needed to emit `Effect::Enter`.
- The `(AwaitingEnter, Entered { entity })` arm copies `entity` straight
  into the terminal.

### `mud-engine` changes

- `perform`'s Enter arm: `Some(entity) => EffectResult::Entered { entity }`.
- `apply_terminal`'s `Bound` arm binds directly with `entity`. The second
  `resolve_puppet`, the "vanishing chance" comment, and the
  `None => drop session` arm are deleted — a `Bound` terminal that cannot
  bind is now unrepresentable.
- The deleted resolve was `apply_terminal`'s only `.await`, so it becomes a
  plain sync method and drops its `backend` parameter.

## Alternatives considered

- **Engine-side stash:** `drive` keeps a local `Option<EntityId>` and hands
  it to `apply_terminal`. Leaves `mud-session` untouched, but the
  "`Bound` implies stash is `Some`" invariant lives outside the type
  system, so the impossible-`None` arm survives as runtime handling.
  Rejected.
- **Resolve only in `apply_terminal`:** removes the duplication but changes
  behavior — on resolve failure the player would see `EnteredWorld` and
  then be dropped, instead of `ServerError` + puppet select. Rejected.

## Testing

- `mud-session` (`login_flow.rs`, fsm unit tests): feed
  `Entered { entity }` and assert `Bound` carries that same `EntityId`
  through — the pass-through is the new behavior under test.
- `mud-engine`: any test exercising the deleted second-resolve-fails path
  is removed with the code path. A test asserts `InWorldBinding.puppet`
  equals the id the backend returned and that `resolve_puppet` was called
  exactly once for a successful Enter (counting fake backend).

## Non-changes

- No `SPEC.md` impact — internal plumbing only.
- No docs-site update — no observable surface changes.
- Journal entry on completion as usual.
