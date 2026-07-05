# mud-core Scheduler Write-Model Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the write-model value types (`Effect`, `Precondition`, `MutationCommand`, `TickEvent`) out of `scheduler.rs` into a new neutral `write_model.rs`, breaking the `world`↔`scheduler` module dependency cycle.

**Architecture:** Today `world.rs` depends on `Effect`/`Precondition`/`TickEvent` (via `lib.rs` re-exports of `scheduler`) while `scheduler.rs` depends on `World` — a bidirectional module cycle. The four types are plain data enums/structs with no scheduler logic; they are a shared write-model vocabulary co-owned by `World` (which applies them) and `Scheduler` (which queues them). Extracting them into `write_model.rs` (which depends only on `EntityId`/`PlaceId`/`ArenaError`) makes the dependencies flow inward: `write_model ← world ← scheduler` and `write_model ← scheduler`, with no cycle. `lib.rs` re-exports the same public names, so the crate's external API (`mud_core::Effect`, etc.) is unchanged.

**Tech Stack:** Rust 2024, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- No `unwrap()`/`expect()`/`panic!()` in production code; `expect()` in tests must carry a message.
- Must compile clean under `cargo clippy -p mud-core --all-targets` and `cargo clippy --workspace --all-targets`.
- **Public API must not change:** `mud_core::{Effect, MutationCommand, Precondition, TickEvent, Scheduler, TICK_HZ, TICK_PERIOD}` must all still resolve. Downstream crates (`mud-engine`, `mud-db`) import these from the crate root — do not break them.
- This is a move + rewire refactor. The four type *definitions* move verbatim; only their module home, imports, and `lib.rs` re-export lines change. Do not alter any variant, field, method body, or doc comment content.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0a: Confirm green**

Run: `cargo test -p mud-core`
Expected: PASS. Record the count.

- [ ] **Step 0b: Confirm the cycle exists (context, not a gate)**

Run: `grep -n "use crate" crates/mud-core/src/world.rs crates/mud-core/src/scheduler.rs`
Expected: `world.rs` imports `Effect, Precondition, TickEvent`; `scheduler.rs` imports `World`. This is the cycle being removed.

---

### Task 1: Create `write_model.rs`, move the four types, rewire `scheduler.rs` and `lib.rs`

This is one atomic task: a type cannot be defined in two modules at once, so the move, the scheduler edits, and the `lib.rs` re-export edits must land together to compile. It ends green against the existing scheduler test suite plus one new focused unit test.

**Files:**
- Create: `crates/mud-core/src/write_model.rs`
- Modify: `crates/mud-core/src/scheduler.rs`, `crates/mud-core/src/lib.rs`

**Interfaces:**
- Produces (public, re-exported unchanged by `lib.rs`): `Effect`, `Precondition`, `MutationCommand`, `TickEvent` — identical definitions to today's `scheduler.rs:44-180`.
- `scheduler.rs` after: still owns `Scheduler`, `TICK_HZ`, `TICK_PERIOD` and consumes `MutationCommand`/`TickEvent`/`World`.

- [ ] **Step 1: Write a failing unit test for the new module**

Create `crates/mud-core/src/write_model.rs` with only a test, so it fails to compile (module/types don't exist yet — this is the intended red):

```rust
//! The write-model vocabulary shared by [`World`](crate::World) (which applies
//! it) and [`Scheduler`](crate::Scheduler) (which queues it): the primitive
//! [`Effect`], its optional [`Precondition`] guard, the [`MutationCommand`] that
//! pairs them, and the [`TickEvent`] outcomes an apply produces (§2.5.3.3,
//! §2.5.3.5, §3.16.2).
//!
//! These are plain data with no scheduling or apply logic, so both the domain
//! aggregate and the scheduler can depend on them without a module cycle.

use crate::{ArenaError, EntityId, PlaceId};

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn entity_place() -> (EntityId, PlaceId) {
        // EntityId is minted by the arena; for a pure builder test we only need a
        // PlaceId, which has a public constructor.
        let place = PlaceId::new(NonZeroU64::new(10).expect("non-zero place id"));
        // A dummy entity id via the arena keeps this independent of scheduler.
        let mut arena = crate::EntityArena::new(
            crate::TenantTag::new(1).expect("tenant tag in range"),
        );
        let entity = arena.alloc();
        (entity, place)
    }

    #[test]
    fn command_carries_effect_and_optional_precondition() {
        let (entity, place) = entity_place();
        let effect = Effect::MoveTo { entity, place };
        let bare = MutationCommand::new(effect);
        assert_eq!(bare.effect(), effect);
        assert_eq!(bare.precondition(), None);

        let guard = Precondition::LocatedIn { entity, place };
        let guarded = MutationCommand::new(effect).with_precondition(guard);
        assert_eq!(guarded.precondition(), Some(guard));
        assert_eq!(guarded.effect(), effect);
    }
}
```

Note: verify the arena's constructor/alloc names before running — read `crates/mud-core/src/entity/arena.rs` for the exact `EntityArena::new`/`alloc` signatures and adjust `entity_place()` to match (the arena is the only way to mint an `EntityId`). If minting an entity is awkward, drop `entity` from the `MoveTo`/`LocatedIn` and instead test with `Effect::Create` (no entity needed) plus a `Precondition` built from an arena-minted id — the point is only to exercise `new`/`with_precondition`/`effect`/`precondition`.

- [ ] **Step 2: Run the test — expect a compile failure**

Run: `cargo test -p mud-core --lib write_model`
Expected: FAIL to compile — `write_model` is not declared in `lib.rs` yet, and `MutationCommand`/`Effect`/`Precondition` are still in `scheduler`.

- [ ] **Step 3: Move the four type definitions into `write_model.rs`**

Cut `Effect` (`scheduler.rs:44-88`), `Precondition` (`:90-110`), `MutationCommand` + its `impl` (`:112-149`), and `TickEvent` (`:151-180`) from `scheduler.rs` and paste them into `write_model.rs` **above** the `#[cfg(test)]` block, verbatim (all doc comments, `#[non_exhaustive]`, `#[must_use]` intact). The `use crate::{ArenaError, EntityId, PlaceId};` line is already present at the top of `write_model.rs`.

- [ ] **Step 4: Fix `scheduler.rs` imports**

Replace `scheduler.rs`'s top import block:

```rust
use std::collections::VecDeque;
use std::time::Duration;

use crate::{ArenaError, EntityId, PlaceId, World};
```

with exactly what the remaining production code (the `Scheduler` impl) uses:

```rust
use std::collections::VecDeque;
use std::time::Duration;

use crate::World;
use crate::write_model::{MutationCommand, TickEvent};
```

Then update the module doc's intralinks if needed (the `[`MutationCommand`]` / `[`Effect`]` links still resolve through the crate, so no change required). In `scheduler.rs`'s `#[cfg(test)] mod tests`, the tests previously reached `Effect`/`Precondition`/`EntityId`/`PlaceId`/`ArenaError` via `use super::*`; those types no longer live in `scheduler`, so add an explicit import inside the test module, just after `use super::*;`:

```rust
    use crate::{ArenaError, EntityId, Effect, PlaceId, Precondition, TenantTag};
```

and delete the now-redundant standalone `use crate::TenantTag;` line (folded into the line above). Leave `use std::num::NonZeroU64;` as-is.

- [ ] **Step 5: Declare the module and re-point `lib.rs` re-exports**

In `crates/mud-core/src/lib.rs`: add `mod write_model;` to the module list (alphabetical: after `mod world;` is fine, or keep the block ordered — place it wherever reads cleanly). Change the scheduler re-export block from:

```rust
pub use scheduler::{
    Effect, MutationCommand, Precondition, Scheduler, TICK_HZ, TICK_PERIOD, TickEvent,
};
```

to:

```rust
pub use scheduler::{Scheduler, TICK_HZ, TICK_PERIOD};
pub use write_model::{Effect, MutationCommand, Precondition, TickEvent};
```

- [ ] **Step 6: Run the new test and the whole crate**

Run: `cargo test -p mud-core`
Expected: PASS — the new `write_model` test plus every existing scheduler/world test, same total as Step 0a **plus one** (the new test).

- [ ] **Step 7: Verify the cycle is gone and downstream still builds**

Run:
```bash
grep -n "use crate" crates/mud-core/src/scheduler.rs   # must no longer be the only inbound edge; world no longer imported by data types
cargo clippy -p mud-core --all-targets
cargo test --workspace
```
Expected: `write_model.rs` imports only `ArenaError`/`EntityId`/`PlaceId` (no `World`); clippy clean (this catches any leftover unused import in `scheduler.rs`); the workspace suite (including `mud-engine` and `mud-db`, which consume these types from the crate root) passes unchanged.

- [ ] **Step 8: Commit**

```bash
jj commit -m "refactor(mud-core): extract write-model types into write_model.rs, break world<->scheduler cycle"
```

---

## Self-review checklist

- [ ] `write_model.rs` contains `Effect`, `Precondition`, `MutationCommand`, `TickEvent` verbatim + one builder unit test; it imports only `ArenaError`/`EntityId`/`PlaceId` — **not** `World`.
- [ ] `scheduler.rs` no longer defines those four types; its production imports are `World` + `write_model::{MutationCommand, TickEvent}`; its tests import `Effect`/`Precondition`/etc. explicitly.
- [ ] `lib.rs` re-exports the same seven public names as before (four from `write_model`, three from `scheduler`).
- [ ] `world.rs` is untouched (still imports the types from the crate root).
- [ ] `cargo test --workspace` green; clippy clean; `mud-engine`/`mud-db` unchanged.
