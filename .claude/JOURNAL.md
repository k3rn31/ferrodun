# Ferrodun — Journal

Breadcrumb trail of implementation work. Newest entries at the bottom, one
per implementation PR. Format defined in `CLAUDE.md`. Code is the source of
truth when this log drifts.

## 2026-06-25 — Roadmap established

- **Spec:** §0–§11 — full normative spec reviewed.
- **Done:** Authored `PLAN.md` (master roadmap: Phase 0 + M1–M8 decomposed
  into PRs/epics, execution principles, per-PR Definition of Done).
  Updated `CLAUDE.md` to name `SPEC.md`/`PLAN.md`/`JOURNAL.md` roles.
- **Verify:** Documentation only; no code. `PLAN.md` cross-checked against
  §5 (repo layout), §7 (workstreams/milestones), §7.5 (ordering).
- **Next:** Begin **P0-01** — convert the single `ferrodun` package into a
  Cargo workspace, move `main` into `mudd`, wire CI (fmt/clippy/test).

## 2026-06-25 — P0-01 Workspace skeleton + CI

- **Spec:** §5 (repo layout), PLAN P0-01 — virtual Cargo workspace that
  builds green in CI before any domain code exists.
- **Done:** Converted root `Cargo.toml` into a virtual workspace (`resolver
  = "3"`, `[workspace.package]` version/edition, lints kept at workspace
  root). Created only `crates/mudd` (others created lazily per YAGNI); moved
  `src/main.rs` → `crates/mudd/src/main.rs` as a placeholder `main` that
  emits one line via `stdout().write_all` (avoids the denied `print_stdout`
  lint). Wired workspace lints into `mudd` via `[lints] workspace = true`.
  Added `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo
  clippy --workspace --all-targets -D warnings`, `cargo test --workspace`.
- **Verify:** Locally green — fmt clean, clippy clean under deny lints,
  `cargo test --workspace` (0 tests) ok, `cargo run -p mudd` prints
  `ferrodun mudd placeholder`.
- **Next:** **M1-01** — `EntityId` + `TenantTag` newtype with the normative
  bit layout (§2.3.1.3); first PR to create `crates/mud-core`.

## 2026-06-25 — P0-02 Documentation site (MkDocs + mike)

- **Spec:** PLAN P0-02 (cross-cutting Documentation track; consolidated docs
  are M8-C). Infrastructure, not a normative spec item.
- **Done:** Added a versioned MkDocs + Material site under `docs/`, managed
  as a **uv project** (`pyproject.toml` + `uv.lock`, pinned mkdocs-material
  9.7.6 / mike 2.2.0; `mkdocs.yml`, placeholder `docs/docs/index.md`).
  Modeled on go-gremlins but with the modern `material.extensions.emoji`
  path (the old `materialx.emoji` was removed in Material 9.4+).
  `.github/workflows/docs.yml` uses `astral-sh/setup-uv` + `uv run` with
  `UV_FROZEN=1`: PRs run `uv run mkdocs build --strict` (no deploy); `main`
  deploys the `next` version; `vX.Y.Z` tags snapshot via `mike` (major/minor
  → `latest` + default, patch → its MAJOR.MINOR line). Updated `CLAUDE.md`
  (new "Documentation site" section: keep docs current as observable
  features land) and `PLAN.md` (P0-02 + cross-cutting Documentation track).
- **Verify:** `UV_FROZEN=1 uv run mkdocs build --strict` clean locally
  against the lockfile. CI deploy and GitHub Pages serving not yet exercised
  (needs a push to `main` and Pages set to the `gh-pages` branch).
- **Next:** Repo owner must enable GitHub Pages → branch `gh-pages` (one
  time) after the first `main` deploy. Then resume **M1-01**.

## 2026-06-25 — M1-01 `EntityId` + `TenantTag`

- **Spec:** §2.3.1 (id bit layout), §2.3.7.3 (generational index) — 8-byte
  `EntityId` packing 12-bit tenant tag + 32-bit slot index + 20-bit
  generation; generation wraparound burns the slot rather than recycling.
- **Done:** Created `crates/mud-core` (first domain crate). Added
  `entity_id` module with newtypes `TenantTag` (parsed, `0..=4095`),
  `SlotIndex` (full u32), `Generation` (parsed, `0..=2^20-1`), and
  `EntityId` (single `u64`). Packing/extraction via documented bit
  constants; `Generation::next()` returns `Option` (`None` = wraparound →
  arena must burn the slot, encoding the §2.3.1.3 rule in the type).
  `EntityIdError` via `thiserror`. `to_bits`/`from_bits` for the
  persistence/wire seam. Bit-field extraction narrows via masked `as`; the
  `& MASK` bounds the value so clippy's `cast_possible_truncation` range
  analysis passes with no suppression. `TryFrom<u16>`/`TryFrom<u32>` delegate
  to the parsed constructors for boundary ergonomics; handle types are
  `#[must_use]`. Wired `mud-core` into the workspace members.
- **Verify:** 11 unit tests (8-byte size, per-field pack/unpack, per-field
  bit isolation — one field max + neighbors zero, mutation-checked to fail
  under an overlapping layout, raw-bits round-trip, golden-value persistence
  encoding + all-fields-max→`u64::MAX` to pin the normative layout, `TryFrom`
  parity with `new`, out-of-range rejection for tenant and generation,
  `next()` increment, wraparound→`None`). `cargo test -p mud-core`, `cargo
  clippy --workspace --all-targets -D warnings`, `cargo fmt --check` all
  green.
- **Next:** **M1-02** — per-tenant generational arena (`slotmap`-style):
  alloc with current tenant tag, resolve live handles, invalidate on slot
  reuse, cross-tenant resolution returns an error (the tenant-isolation
  unit test).

## 2026-06-25 — M1-02 Per-tenant generational arena

- **Spec:** §2.3.1–2.3.2 (generational index), §2.3.7.3 (teardown
  invalidates handles), §3.11.4 (tenant isolation at API boundaries) — the
  liveness authority that mints/validates `EntityId` handles.
- **Done:** Added `crates/mud-core/src/arena.rs` with `EntityArena` (one per
  tenant) and `ArenaError`. Hand-rolled rather than pulling `slotmap`, to keep
  the normative 12/32/20 bit layout and the burn-on-wraparound rule exact.
  `EntityArena` is a non-generic liveness registry (no component payload; those
  are separate side-tables, M1-05): `new`/`tenant`/`alloc`/`free`/`resolve`.
  Slot liveness is `enum SlotState { Live(Generation), Free(Generation),
  Burned }` (one `Vec<SlotState>` + a `free: Vec<SlotIndex>` stack) — `Burned`
  carries no generation so a retired-but-non-terminal slot is unrepresentable.
  `alloc` reuses a freed slot at its advanced generation or grows the arena;
  slot indices minted via `u32::try_from(len)` → `Exhausted` (no `as`). `free`
  checks tenant (`ensure_owned`) then the live generation in a single slot
  lookup, then advances via the M1-01 `Generation::next()`: `Some` → `Free` +
  recycle, `None` → `Burned`, never relinked (§2.3.1.3). `resolve` checks
  tenant first (`CrossTenant`, kept distinct from `StaleHandle` per §3.11.4)
  then slot liveness+generation, returning the validated `SlotIndex`. The
  reuse path's free-list lookup is a guarded `// INVARIANT:` `unreachable!`
  (not a dishonest `StaleHandle`, since `alloc` takes no handle). Added
  `Generation::FIRST` const to `entity_id.rs`. Re-exported `EntityArena`/
  `ArenaError` from `lib.rs`.
- **Verify:** 8 new unit tests (tenant stamping, resolve live, freed→stale,
  slot-reuse bumps generation, **stale handle cannot free a reused slot**
  (mutation-side use-after-free guard), **tenant-isolation: foreign handle
  rejected by another tenant's arena**, burn-on-generation-exhaustion via the
  real free/alloc path cycling one slot to `Generation::MAX`, double-free,
  out-of-range→stale). `cargo test -p mud-core` (20 tests), `cargo clippy
  --workspace --all-targets -D warnings`, `cargo fmt --check` all green. No
  docs-site change (internal plumbing, no observable surface).
- **Next:** **M1-03** — core domain newtypes (`PlaceId`, `RegionId`,
  `ArchetypeId`, `ComponentId`, plus session/account ids as M1 needs them).

## 2026-06-25 — EntityKey/EntityId split (durable vs ephemeral identity)

- **Spec:** §2.3.1 (new §2.3.1.4–2.3.1.6), §2.3.7.1, §2.5.3.1–2.5.3.2 — a
  design fix, not new code. The shipped `EntityId` (M1-01) was wrongly billed
  as the persisted + wire identity, but it is a generational arena index whose
  slots are reused under the LRU cache (§2.5.3.2) — it cannot be durable.
- **Done:** Split entity identity into two types. `EntityId` stays the
  **ephemeral** in-memory arena handle (dense O(1) side-table index on the hot
  path); a new durable **`EntityKey`** (per-tenant monotonic, DB primary key,
  the only entity ref that leaves the World process) carries persistent
  identity. SPEC §2.3.1 restructured; §2.3.7.1/§2.5.3 updated so the arena is a
  cache keyed by `EntityKey` with an `EntityKey`↔`EntityId` mapping. PLAN:
  `EntityKey` added to M1-03; mapping + key assignment scoped to M1-08/M1-09;
  M1-09 restart test now asserts `EntityKey` (not `EntityId`) stability.
  Corrected `entity_id.rs` doc comments + renamed the persistence test to
  `packs_to_the_documented_bit_layout`. Kept `to_bits`/`from_bits` as an
  internal encoding utility (the layout-tiling tests depend on them).
- **Verify:** `cargo test -p mud-core` (20 tests), `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --check` all green. Docs are SPEC/PLAN
  only; no mkdocs surface (`EntityKey`/`EntityId` are internal).
- **Next:** implement the `EntityKey` newtype in **M1-03**; the
  `EntityKey`↔`EntityId` mapping, key assignment, and LRU live in **M1-09**.

## 2026-06-25 — M1-03 `EntityKey` (durable entity identity)

- **Spec:** §2.3.1.4–2.3.1.5, §1.7 — the durable, per-tenant monotonic 64-bit
  identity / DB primary key; the only entity ref that crosses disk/wire/IPC,
  distinct from the ephemeral `EntityId`.
- **Done:** Added `crates/mud-core/src/entity_key.rs` with `EntityKey`, a
  newtype over `NonZeroU64` (Copy/Eq/Ord/Hash, `#[must_use]`, `new`/`get`).
  `NonZeroU64` so an unassigned ref is only `Option::None` (niche, no sentinel
  0) and the constructor stays infallible — no `EntityKeyError`/`thiserror`
  needed (the zero-check is std's `NonZeroU64::new` at the boundary). Re-exported
  from `lib.rs`. **Strict-YAGNI scope:** M1-03 ships only `EntityKey`; the other
  ids the old foundation bundled here were redistributed in `PLAN.md` —
  `PlaceId`/`RegionId` → M1-04, `ArchetypeId`/`ComponentId` → M2. Per-tenant
  minting + the `EntityKey`↔`EntityId` mapping stay in M1-08/M1-09 (not added).
  Distinctness from `EntityId` is enforced by the type system (no shared
  conversion), so no `trybuild` harness.
- **Verify:** 4 new unit tests (8-byte size, `Option<EntityKey>` niche = 8
  bytes, `new`/`get` round-trip, monotonic ordering). `cargo test -p mud-core`
  (24 tests), `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt
  --check` all green. No docs-site change (`EntityKey` is internal, no
  observable surface yet).
- **Next:** **M1-04** — `Place` enum (Room only) + `PlaceView`, introducing the
  `PlaceId`/`RegionId` newtypes.

## 2026-06-25 — M1-04 `Place` enum (Room only) + spatial surface

- **Spec:** §2.2 — the spatial surface. Every location is a `Place`; one shared
  read surface with no per-variant special cases (§2.2.3) and **static dispatch
  only** (§2.2.5, no trait object).
- **Done:** Added `crates/mud-core/src/place.rs`. Newtypes `PlaceId`/`RegionId`
  over `NonZeroU64` (mirroring `EntityKey`: 1-based so `Option<PlaceId>` takes
  the niche, which `neighbor` returns); `Direction` enum n/e/s/w + up/down
  (vertical exits per §3.2.2.0, not a `z` coord); `Description` newtype over
  `String`. `Place` (only `Room` for M1, Tile→M4) exposes the §2.2.2 surface —
  `id`, `region`, `describe(viewer)`, `neighbor`, `visible_places` — as
  **inherent methods** that `match` on the variant and delegate to private
  accessors on the variant payload (`RoomData`). Dispatch is static *by
  construction* (an enum with inherent methods can't be a trait object), so
  §2.2.5 holds with no trait gymnastics. **No `PlaceView` trait yet:** with one
  variant it would be single-impl (our rules forbid a trait for a single impl);
  it lands in M4 when `Tile` is its second implementor. `RoomData` stores exits
  as six explicit `Option<PlaceId>` fields (duplicate-direction exit
  unrepresentable; `neighbor` is a plain `match`, no `indexing_slicing`) built
  via `new` + chainable `with_exit`/`with_visible_places`. `describe` ignores
  `viewer` for M1 (documented trivial passthrough; param locks the signature).
  **Scope call:** `occupants()` deferred to M1-05 — occupancy's authoritative
  home is the dense `LocationOf` side-table (§2.3.2.2), so putting it on the
  static `Place` now would duplicate that index. `PLAN.md` updated accordingly
  (M1-04 = `Place` inherent surface, `PlaceView` trait → M4; M1-05 adds
  `occupants()` to the surface against `LocationOf`). Re-exported `Place`/
  `RoomData`/`PlaceId`/`RegionId`/`Direction`/`Description` from `lib.rs`.
- **Verify:** 6 new unit tests (neighbor wired/unwired, visible-places set,
  viewer-independent describe, id/region, `Option<PlaceId>` niche = 8 bytes),
  all exercised through the `Place` surface against a fixture room graph (so
  `Place`→variant delegation is covered by every test). `cargo test -p
  mud-core` (30 tests), `cargo clippy --workspace --all-targets -D warnings`,
  `cargo fmt --check` all green. No docs-site change (`Place` is internal
  plumbing — no command, world-file, or other observable surface yet).
- **Next:** **M1-05** — hot side-tables (`LocationOf`, `Inventory`); add
  `occupants()` to the `Place` surface resolving through `LocationOf`.

## 2026-06-25 — M1-05 Hot side-tables + `Place::occupants`

- **Spec:** §2.3.2.2–2.3.2.4 (hot components in dense slot-indexed arrays),
  §2.2.2 (the Place surface's occupants) — the two M1 hot side-tables and the
  occupancy join deferred from M1-04.
- **Done:** Added `crates/mud-core/src/side_tables.rs` with `LocationOf` and
  `Inventory`, the only two hot components M1 needs (`Position`/`Health`/
  `Initiative` deferred to their own milestones per YAGNI). Both are **pure
  storage keyed by `SlotIndex`**, not liveness authorities — the arena resolves
  handles (rejecting stale/cross-tenant) before a table is indexed (§2.3.2
  separation, documented on the module). `LocationOf` is a dense forward array
  (`Vec<Option<PlaceId>>` by slot) plus a reverse occupant index
  (`HashMap<PlaceId, Vec<EntityId>>`) kept in lockstep: `place` moves an entity
  out of its old Place's list before recording the new one (reverse-index
  consistency on move); `remove` clears both halves (teardown-ready, §2.3.7.3);
  `location`/`occupants` are the reads. Reverse removal matches **by slot** (a
  slot is in ≤1 Place), so it stays correct across slot reuse; empty reverse
  `Vec`s are pruned. `Inventory` is a dense `Vec<Vec<EntityId>>` by slot with
  `insert` (dedups within a container)/`remove`/`contents`; cross-container
  exclusivity is left to the M1-06 mutation layer. Growth via `resize`/
  `resize_with` with `checked_add` (no overflow); cell access after grow uses a
  documented `// INVARIANT:` `unreachable!` mirroring `arena.rs`. Completed the
  Place surface with inherent `Place::occupants(&self, &LocationOf)` — a Place
  doesn't own occupancy, so the table is passed in and the join keys on
  `self.id()` (the §2.2.2 sketch's bare `occupants(&self)` is illustrative).
  Re-exported `Inventory`/`LocationOf` from `lib.rs`; refreshed the place.rs
  module-doc note to point at the now-landed method.
- **Verify:** 12 new unit tests (location round-trip, occupants listing,
  unlocated/empty cases, **move keeps the reverse index consistent**, move
  evicts only the moved entity while other occupants stay, a reused slot
  supersedes its stale handle, `remove` clears both halves, inventory
  round-trip, dedup on duplicate insert, remove-drops-item, and
  `Place::occupants` joining through `LocationOf`). `cargo test -p mud-core`
  (42 tests), `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt
  --check` all green. No docs-site change (internal plumbing — no command,
  world-file, or operator-observable surface yet).
- **Next:** **M1-06** — scheduler tick + `MutationCommand` (M1 subset). Entity
  teardown there will consume `LocationOf::remove`; move-between-Places will
  drive `LocationOf::place`; inventory add/remove will drive `Inventory`.

## 2026-06-25 — M1-06 Scheduler tick + `MutationCommand` (M1 subset)

- **Spec:** §2.5.3.3 (write-through single channel), §2.5.3.5 (per-entity
  serialization, arrival order, last-writer-wins, precondition-carrying
  composite mutations), §3.16.2 (fixed 20 Hz tick) — the write-model
  orchestration over the M1 domain primitives.
- **Done:** Added `crates/mud-core/src/world.rs` and `scheduler.rs`. **`World`**
  is the per-tenant mutable aggregate bundling `EntityArena` + `LocationOf` +
  `Inventory` (fields private); it exposes the *apply* surface (`create`,
  `teardown`, `move_to`, `inventory_add`, `inventory_remove`) and the
  *precondition* read surface (`is_located_in`, `contains`). Every op resolves
  the handle through the arena before touching a side-table, so the §2.3.2
  liveness/separation rule holds and stale/cross-tenant handles are rejected as
  `ArenaError`; predicates return `false` for a non-live handle rather than
  reading the table. `teardown` = `arena.free` + `LocationOf::remove` +
  `Inventory::clear` — releases both hot-component slots so a reused slot leaks
  no state (§2.3.7.3); removing the entity from a container that holds it *as an
  item* is deferred (no reverse item→container index yet).
  **`scheduler.rs`** holds the mutation vocabulary — `Effect` (Create / Teardown
  / MoveTo / InventoryAdd / InventoryRemove, primitive by design), an orthogonal
  optional `Precondition` (LocatedIn / Contains), `MutationCommand`
  (`new(effect)` + `.with_precondition(..)`), and `TickEvent` (Created /
  PreconditionFailed / Rejected) — plus the `Scheduler` (FIFO `VecDeque` +
  monotonic `tick` counter; `submit`/`tick`/`tick_number`). `tick(&mut world)`
  drains the whole queue in arrival order; a carried precondition is evaluated
  at apply time and on failure emits `PreconditionFailed` and skips the effect
  (no partial effect, §2.5.3.5); effects dispatch to the matching `World` method,
  mapping `Ok(EntityId)`→`Created`, `Ok(())`→no event, `Err`→`Rejected`.
  **Per-entity serialization + arrival order + last-writer-wins all hold by
  construction** from the single-threaded sequential drain; parallel execution
  of different entities is a §2.5.3.5 MAY, so no per-entity lock was built
  (YAGNI), and the §2.3.4.1 per-tick budget is not enforced yet. Cadence pinned
  via `TICK_HZ = 20` / `TICK_PERIOD = 50ms` constants; the **wall-clock driver
  loop is deferred to M1-22** (no async runtime yet) — documented in the module
  doc and both PLAN entries (M1-06 *Out of scope*, M1-22 wiring contract).
  `tick_number()` is the §3.16.4 `mud.time.tick()` source. Re-exported all new
  public items from `lib.rs`.
- **Verify:** 20 new unit tests (8 `World`: create→usable, teardown→stale +
  location cleared, move records location, foreign-handle reject, inventory
  add/contains + remove, predicates-false-for-stale, teardown clears a reused
  slot's inventory; 10 `Scheduler`: create mints usable entity, arrival-order/
  last-writer-wins, two-entity independence, precondition fail→skip+event,
  precondition pass→apply, inventory effects through tick, `Contains`
  precondition gate, teardown command, foreign-handle→Rejected, tick counter
  increments, cadence constants) plus 1 `Inventory::clear` slot-reuse test.
  `cargo test -p mud-core` (62 tests), `cargo clippy --workspace --all-targets
  -D warnings`, `cargo fmt --check` all green. No `unwrap`/`expect`/`panic`
  outside tests. No docs-site change (internal plumbing — no player/builder/
  operator-observable surface yet).
- **Next:** **M1-07** — Locks DSL (`chumsky` parser → typed AST, static-dispatch
  eval table; lock fns `perm`/`attr`/`tag`/`self`).
