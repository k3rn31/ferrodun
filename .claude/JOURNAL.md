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

## 2026-06-25 — M1-07 Locks DSL (parse → resolve → eval)

- **Spec:** §2.6.1–2.6.2 — Evennia-style lock DSL (`accesstype:expr`); MUST
  parse to a typed AST via `chumsky` (§2.6.2.1) and evaluate via a static
  dispatch table with no string matching at eval time (§2.6.2.2).
- **Done:** Added `crates/mud-core/src/locks/` (module dir, not the flat
  single-file style — multi-concern). Added `chumsky` 0.13 via `cargo add`
  (locked tech stack). **Two-phase pipeline**: `parser.rs` (chumsky grammar) →
  syntactic `ParsedLock`/`SyntaxExpr` (functions = name + string args, pure
  syntax); `resolve.rs` lowers known functions into the typed `Lock`/
  `ResolvedExpr`/`LockFn` (`Perm`/`Attr`/`Tag`/`Status`/`SelfRef`), enforcing
  arity, so unknown-function (`UnknownFunction`) and bad-arity (`ArityMismatch`)
  become errors at this seam (the M2 `mud check` hook). `eval.rs` walks the
  resolved tree and `match`es on `LockFn` → static dispatch, no strings;
  `LockContext` is a concrete struct (sets of perms/attrs/tags/statuses + an
  `is_self` flag, chainable `with_*`/`as_self` builders) — no trait yet (single
  use; real component stores arrive later). Grammar: `not` > `and` > `or`
  precedence (foldr/foldl), parens, whitespace-insensitive; `and`/`or`/`not`
  reserved (rejected as function names via `try_map`). **Scope decision (with
  user):** added `status` to the M1 function set so all three §2.6.1.2 normative
  examples fully evaluate (the third uses `status(drunk)`); PLAN.md M1-07
  updated to record the divergence from its original 4-function list and the
  two-phase design. `chumsky` error types are wrapped in a crate-owned
  `ParseError` (no dep leak). `LockArg` is one shared newtype — the `LockFn`
  variant already encodes the domain, so no per-function arg newtype. Arity
  extraction uses `<[String;1]>::try_from` (no unwrap/unreachable). Re-exported
  the public API from `lib.rs`.
- **Verify:** 24 new unit tests (parser: 3 normative strings → expected AST,
  precedence, parens override, whitespace, + malformed/missing-accesstype/
  unbalanced-paren/trailing-input/keyword-as-fn-name rejects; resolve: each of
  the 5 functions, all 3 normative strings, unknown-function, unary-no-arg,
  self-with-arg; eval: all 3 normative strings true/false, negation,
  disjunction). `cargo test -p mud-core` (86 tests), `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --check` all green. No docs-site change
  (locks aren't wired to any builder/operator-observable surface yet — no
  command, world-file, or `mud check` consumes them until later milestones).
- **Next:** **M1-08** — `mud-db` crate: SQLx + SQLite, per-tenant connection
  pool over a distinct file, initial migration (accounts, puppets, entities by
  `EntityKey`, location, inventory).

## 2026-06-26 — mud-core M1 housekeeping: reorg, comment cleanup, integration tests

- **Spec:** §2.3.1–2.3.2, §2.2, §2.6 — no normative change; tidy the crate at
  the M1-01…M1-07 boundary (internal structure + tests only).
- **Done:** Grouped the entity-identity subsystem into `entity/` (`id.rs`,
  `key.rs`, `arena.rs` + `mod.rs`) and split `side_tables.rs` into
  `side_tables/{location,inventory}.rs` (+ `mod.rs`). Consolidated the
  duplicated slot→index helper (`arena::slot_position` /
  `side_tables::slot_index`) into one `pub(crate) SlotIndex::to_index`. Comment
  pass: stripped development-process/roadmap meta (YAGNI tags, milestone/PR
  refs like M1-22/M4, "deferred to") while keeping design rationale and spec
  `§` anchors; fixed one stale comment (side-table test helper claimed "raw
  bits" but builds via `EntityId::new`). Public API unchanged — `lib.rs`
  re-exports the same names, now sourced from the new modules.
- **Verify:** `cargo test -p mud-core` green — 89 unit + 14 new integration
  tests across `tests/{world_simulation,locks_pipeline,navigation}.rs`
  (Scheduler+World over many ticks; full parse→resolve→evaluate lock pipeline
  incl. error surfaces; arena+Place+LocationOf composed for handle-validated
  movement). `cargo clippy -p mud-core --all-targets` and `cargo doc -p
  mud-core --no-deps` clean. No docs-site change (purely internal).
- **Next:** **M1-08** — `mud-db` crate (SQLx + SQLite), unchanged by this pass.

## 2026-06-26 — M1-08 `mud-db` SQLx + SQLite backend

- **Spec:** §2.5.1.1–2.5.1.4 (SQLx, `sqlx migrate`, **per-tenant physical file
  isolation**), §2.3.1.5 (EntityKey never reused), §3.15.1 (accounts/puppets) —
  the persistence backend and initial schema.
- **Done:** Created `crates/mud-db` (workspace member). Deps via `cargo add`:
  `sqlx` 0.9 (`default-features = false`, features `sqlite,runtime-tokio,migrate,
  macros`; `sqlite` bundles libsqlite3 so CI needs no system dep), `thiserror`;
  dev `tokio` (`macros,rt-multi-thread`) + `tempfile`. **Backend-namespaced
  layout** to anticipate Postgres without a premature single-impl trait:
  `migrations/sqlite/0001_initial.sql`, code in `src/sqlite/mod.rs`, a
  backend-agnostic `DbError` (`thiserror`, `#[from]` over `sqlx::Error` +
  `MigrateError`) at `src/error.rs`. `TenantDb` wraps one tenant's `SqlitePool`
  opened over `<data_dir>/world.db` via `SqliteConnectOptions` with
  `create_if_missing(true)` + `foreign_keys(true)` (SQLite ignores FKs
  otherwise), then runs `sqlx::migrate!("./migrations/sqlite")`; `pool()`
  accessor exposed for M1-09's write-through. Schema: `entities.entity_key
  INTEGER PRIMARY KEY AUTOINCREMENT` (AUTOINCREMENT, not plain rowid, to honor
  the §2.3.1.5 no-reuse MUST); `accounts` (username UNIQUE, password_hash,
  state); `puppets`→accounts+entities FKs; `location` (entity_key PK, place_id —
  no places table in M1); `inventory` with `item_key` as PK (item-in-≤1-container
  unrepresentable). **No `mud-core` dep yet** (no domain marshaling until M1-09).
  **§2.5.1.2 compile-time-checked `query!` macros deferred to M1-09** with the
  first real query (and the `.sqlx` cache + `SQLX_OFFLINE` CI step it needs);
  M1-08 tests use runtime `sqlx::query`/`query_scalar` (sqlx 0.9 requires
  `&'static str` SQL — table-existence check goes through `sqlite_master` with a
  bind param). Added `mud-db` to workspace `members`; updated PLAN M1-08 with the
  as-built notes.
- **Verify:** 4 `#[tokio::test]`s — migration creates all five tables, **two
  tenants' files are isolated** (write to A invisible to B), EntityKey never
  reused after delete (AUTOINCREMENT), item cannot occupy two containers (PK
  violation). `cargo test -p mud-db` (4) and `cargo test --workspace` (89 + 14
  integration + 4) green; `cargo clippy --workspace --all-targets -D warnings`
  and `cargo fmt --all --check` clean. No `unwrap`/`expect`/`panic` outside
  tests. No docs-site change (persistence is internal — no player/builder/
  operator-observable surface yet).
- **Next:** **M1-09** — write-through + boot load: arena as cache keyed by
  `EntityKey`, every mutation applies to arena + DB in one transaction, restart
  integration test. Introduces the first compile-time-checked `query!` (+ `.sqlx`
  offline cache + `SQLX_OFFLINE` CI step) and the `mud-core` dependency.

## 2026-06-26 — M1-09 write-through + boot load (cache keyed by `EntityKey`)

- **Spec:** §1.2, §2.3.1.4–2.3.1.6 (arena as a cache keyed by `EntityKey`;
  loading mints a fresh `EntityId`), §2.5.3.1–2.5.3.3 (DB is source of truth;
  write-through via `MutationCommand`) — connect the volatile `mud-core` `World`
  to the durable `mud-db` SQLite backend so a clean restart restores state.
- **Done:** New `PersistentWorld` in `crates/mud-db/src/sqlite/write_through.rs`
  wraps an **untouched** `mud-core` `World` plus a one-to-one
  `EntityKey`↔`EntityId` map (`by_key`/`by_id`, the latter keyed on the full
  `EntityId` so a stale handle to a reused slot misses). `load()` rebuilds the
  world: `SELECT entities ORDER BY entity_key` → `world.create()` per key, then
  replays `location` and `inventory`. `apply(MutationCommand)` mirrors the
  scheduler's semantics (`Ok(None)` for a successful non-`Create`; `Created`/
  `Rejected`/`PreconditionFailed` events; `DbError` only for real DB failures).
  Consistency: an in-memory arena can't join a SQL tx, so "one transaction"
  (§2.5.3.3) = apply-in-memory-then-commit — `Create` is DB-first (key from
  `AUTOINCREMENT`, arena handle minted after, row rolled back on arena
  `Exhausted`); `MoveTo`/`InventoryAdd`/`InventoryRemove`/`Teardown` apply to the
  arena first (preserving `ArenaError` `CrossTenant`/`StaleHandle`
  classification) then write the DB. **Teardown = destruction:** deletes the
  entity's `entities` row, and every entity-referencing FK is declared
  `ON DELETE CASCADE` so the location/containment/items-held rows go in the same
  statement — the destroy path needs no knowledge of which tables reference an
  entity, and a dangling child row is unrepresentable (cache *eviction* is a
  separate M7 concern). One asymmetry, documented: `mud-core`'s `teardown` can't
  remove a destroyed item from a container holding it (no reverse index), so the
  arena briefly disagrees with the DB cascade and reconciles on the next `load`.
  Centralized `i64`↔`NonZeroU64` conversions (no `as`, no `unwrap`); `DbError`
  variants `InvalidId`/`KeyOutOfRange` (out-of-range each direction),
  `EntityNotMapped` (internal map miss), `DanglingReference` (corrupt load),
  `UnsupportedEffect` (`#[non_exhaustive]` guard). `mud-core`
  gained only two read accessors `MutationCommand::effect`/`precondition`.
  First compile-time `query!` macros: committed `crates/mud-db/.sqlx` offline
  cache (SQLite needs `AS "col!"` to force non-null), `cargo add mud-core` path
  dep, and `SQLX_OFFLINE: "true"` in the CI top-level `env`.
- **Verify:** `crates/mud-db/tests/restart.rs` (TDD) — `state_survives_a_clean_
  restart` (write → drop `PersistentWorld` → reopen → location + inventory intact
  via re-minted ids resolved through stable `EntityKey`s; account row persists),
  `teardown_does_not_resurrect_on_restart`, plus the apply-path branches that the
  restart test didn't cover: `failed_precondition_skips_effect_and_persists_
  nothing`, `rejected_effect_persists_nothing`, `inventory_remove_persists_across_
  restart`, `re_move_persists_only_the_last_destination`, and `teardown_of_a_
  contained_item_leaves_no_dangling_containment` (a clean reload is itself proof
  the cascade dropped the containment row). `cargo test --workspace` green
  (mud-db 4 unit + 7 integ). `cargo clippy --workspace --all-targets -D warnings`
  and `cargo fmt --all --check` clean (offline); `sqruff lint` clean against the
  CASCADE schema change. No `unwrap`/`expect`/`panic` outside tests. No docs-site
  change (persistence is internal — no player/builder/operator-observable
  surface).
- **Next:** M1-10 `mud-schema` IPC frames. The IPC boundary will carry
  `EntityKey` (§2.3.1.4) and translate to the in-memory `EntityId` via
  `PersistentWorld`'s maps; wiring the scheduler drain to `apply` is the M1-22
  driver loop. Known gaps deferred to M7: LRU cache eviction + cache-miss
  reload, background snapshot (§2.5.3.4), and rollback/crash-on DB-write failure
  (today the transient in-process inconsistency window just returns `DbError`).

## 2026-06-27 — mud-db crate review & polish

- **Spec:** §2.3.1.6, §2.5.3 — no behavior change; review pass on the M1-08/09 crate.
- **Done:** Reviewed `crates/mud-db` (layout, modules, tests). Layout judged sound
  and tests genuine (no makeshift ones). Two polish changes: (1) extracted the six
  pure `i64`↔newtype boundary conversions out of the apply/load logic into a new
  `src/sqlite/keys.rs` (`pub(super)`, no sqlx macros → `.sqlx` cache untouched);
  (2) renamed `src/sqlite/write_through.rs` → `persistent_world.rs` to match the
  `PersistentWorld` type (it owns boot load too, not only write-through).
- **Verify:** `cargo test -p mud-db` green — added 3 unit tests in `keys.rs`
  (InvalidId on zero/negatives, KeyOutOfRange past `i64::MAX`, valid round-trip)
  and one integration test `boot_load_rejects_a_corrupt_entity_key` (raw-insert a
  negative rowid → `PersistentWorld::load` fails loudly with `DbError::InvalidId`).
  `cargo clippy -p mud-db --all-targets` clean. No docs-site change (internal).
- **Next:** unchanged — M1-10 `mud-schema` IPC frames. Genuinely-unreachable arms
  (`DanglingReference` under FK enforcement, `UnsupportedEffect`, arena-exhaustion
  rollback) remain untested by design.

## 2026-06-27 — M1-10 `mud-schema` IPC frames

- **Spec:** §2.1.3 (IPC contract: postcard frames, multiplexed by `session_id`,
  schema declared in `mud-schema`), §2.7 step 2 (`SessionInput`), §2.8.3/§2.8.5.7
  (IPC frames version-locked at build time, excluded from the codegen'd wire
  protocol) — the typed frame vocabulary the Gateway↔World IPC channel speaks.
- **Done:** Created `crates/mud-schema` (leaf crate, **no `mud-core` dep**). Deps
  via `cargo add`: `serde` (derive), `postcard` (use-std), `thiserror`.
  `session.rs`: `SessionId` newtype over `NonZeroU64` (niche-friendly, the §2.1.3.1
  multiplexing key; minting is M1-11) + `SchemaVersion` newtype and the
  build-time `SCHEMA_VERSION = 1` const (carried by M1-11's resume handshake, not
  stamped per frame, §2.8.5.7). `frame.rs`: payload structs `SessionInput
  {session_id, line}` (§2.7 step 2 verbatim), `SessionOutput {session_id, text}`,
  `SessionConnect`, `SessionDisconnect`, `SessionClose`; and **two directional
  enums** —
  `GatewayFrame` (Connect/Input/Disconnect, G→W) and `WorldFrame` (Output/Close,
  W→G), both `#[non_exhaustive]` — so an illegal direction is unrepresentable
  (type-driven). `codec.rs`: `encode`/`decode` helpers over `postcard::to_stdvec`/
  `from_bytes` returning a crate-owned `SchemaError` (`thiserror`); the
  underlying `postcard::Error` is **boxed** (`Box<dyn Error + Send + Sync>` via a
  manual `From`) so the codec dependency never leaks into the public API. **No
  `EntityKey`
  crosses any M1 frame** (§2.3.1.4) — entity-bearing frames are M3+. Text payloads
  are **marker newtypes** `InputLine`/`OutputText` (mirroring `mud-core`'s
  `Description`) rather than raw `String`, per the newtype mandate; no invariant is
  enforced at the IPC boundary because §3.6.4's content cap / control-char
  stripping is command-scoped and applied downstream (M1-17), and they are
  serde-transparent so the golden-bytes encoding is unchanged. `OutputText` stays
  `String`-backed for M1; M1-13 swaps it for styled text.
  **Decision (confirmed with user):** the PLAN's "codegen scaffold emits Rust now"
  was dropped per YAGNI — IPC frames are hand-written and version-locked; the
  codegen mechanism (Rust + TS + GMCP docs, §2.8.3.1) is deferred to **M3-D** with
  the first real wire protocol. PLAN M1-10 rewritten + an *As built* note added;
  M3-D broadened from "TypeScript" to the full codegen mechanism. Length-prefixing
  and transport stay in **M1-11**.
  **Review fixes (this PR):** boxed third-party errors out of public APIs to honor
  the no-dependency-leak rule — `SchemaError::Postcard` and, in **mud-db**,
  `DbError::{Sqlx,Migrate}` now hold `Box<dyn Error + Send + Sync>` with manual
  `From` impls (was `#[from]`). Added `#[must_use]` to the frame structs/enums and
  fixed a `SessionClose` doc wording nit.
- **Verify:** 10 unit tests — round-trip per frame variant (`GatewayFrame::{Connect,
  Input,Disconnect}`, `WorldFrame::{Output,Close}`), a **golden-bytes pin** of
  `GatewayFrame::Input` (`[0x01,0x02,0x02,0x68,0x69]`) catching variant/field
  reorders, `Option<SessionId>` niche = 8 bytes, `SCHEMA_VERSION == 1`. `cargo test
  --workspace` green (mud-schema 10; 89+7+3+4 core, 7+8 db unchanged); `cargo clippy
  --workspace --all-targets -D warnings` and `cargo fmt --all --check` clean. No
  `unwrap`/`expect`/`panic` outside tests. No docs-site change (IPC is internal
  plumbing — no player/builder/operator-observable surface).
- **Next:** **M1-11** — IPC transport: length-prefixed postcard over a unix socket
  multiplexed by `session_id`, in-memory channel for single-process mode, and the
  resume handshake carrying `world_id` + `SCHEMA_VERSION` + the live session set
  (§2.1.3.2). The handshake `world_id` type and any handshake frame land there.

## 2026-06-27 — M1-11a `mud-schema` resume-handshake vocabulary

- **Spec:** §2.1.3.1–2.1.3.2 (resume handshake carries `world_id` + schema version
  + the live session set), §2.8.5.7 (IPC frames version-locked at build time) — the
  frame *types* the M1-11 transport will carry, defined before the transport that
  carries them (§8 rule 4: wire/IPC changes start in `mud-schema`).
- **Done:** Split M1-11 into **M1-11a** (this PR, `mud-schema` types) and **M1-11b**
  (the `mud-ipc` transport crate) to keep each PR to one crate's public API (PLAN
  principle #3) and honor §8 rule 4. Added `WorldId` to `session.rs` — a `NonZeroU64`
  newtype mirroring `SessionId` (niche-friendly, `new`/`get`, `#[must_use]`), the
  §2.1.3.1 per-World address; minting is config-driven (M1-12/M1-22), so only the type
  lands here. Gave `SchemaVersion` a public `new` (it had only `get`) so the handshake
  can carry/compare a *peer's* announced version, distinct from this build's
  `SCHEMA_VERSION` const. Added to `frame.rs`: payloads `ResumeHandshake { world_id, schema_version,
  live_sessions: Vec<SessionId> }` (the §2.1.3.2 announcement, G→W) and `HandshakeAck
  { world_id, schema_version }` (W→G confirmation), plus the directional variants
  `GatewayFrame::Resume(ResumeHandshake)` and `WorldFrame::ResumeAck(HandshakeAck)`.
  The handshake direction maps onto the existing directional split exactly (Gateway
  announces, World acks), so it rides the existing enums rather than a new control
  enum; both enums are already `#[non_exhaustive]`, so appending is wire-additive and
  the M1-10 golden-bytes pin (which targets `Input` = index 1) is unperturbed. Re-exported
  the new public items from `lib.rs`.
- **Verify:** `cargo test -p mud-schema` (16: +`WorldId` round-trip/order/`Option` niche
  = 8 bytes, +`SchemaVersion::new` round-trip, +`GatewayFrame::Resume`/`WorldFrame::
  ResumeAck` postcard round-trips; the existing `input_frame_has_a_stable_encoding` golden
  test still passes). `cargo clippy -p mud-schema --all-targets -D warnings` and `cargo
  fmt` clean. No `unwrap`/`expect`/`panic` outside tests. No docs-site change (IPC is
  internal plumbing).
- **Next:** **M1-11b** — the `mud-ipc` crate: `Endpoint` duplex trait + `InMemoryEndpoint`
  (single-process channel) + `SocketEndpoint` (length-prefixed postcard over a unix
  socket), and the resume-handshake exchange (`announce_sessions`/`accept_resume`) written
  generically over `Endpoint` so both transports share one code path. Adds the SPEC §5
  `mud-ipc` layout line and the first async runtime (tokio).

## 2026-06-27 — M1-11b `mud-ipc` transport + resume handshake + single-process mode

- **Spec:** §2.1.3.1 (length-prefixed postcard over a unix socket, multiplexed by
  `session_id`), §2.1.3.2 (resume handshake: `world_id` + schema version + live session
  set), §2.1.3.3 (single-process in-memory channel, same frame contract) — the transport
  that carries the M1-11a frame vocabulary.
- **Done:** Created `crates/mud-ipc` (new SPEC §5 crate; layout line added). Deps via
  `cargo add`: `tokio` (net/io-util/sync/rt/macros), `tokio-util` (codec), `futures`,
  `bytes`, `serde` (trait bounds), `thiserror`, `tracing`, `mud-schema`; dev `tokio`
  (rt-multi-thread/macros/time) + `tempfile`. **First async runtime in the workspace.**
  `transport.rs`: an `Endpoint` duplex trait (associated `Outbound`/`Inbound` frame types
  encode direction; methods declared `-> impl Future + Send` so consumers can spawn
  endpoints across threads — impls satisfy it with `async fn`, sidestepping both
  `async_fn_in_trait` and `manual_async_fn`). Two impls (the legitimate single-vs-split
  seam): `InMemoryEndpoint` over a pair of `tokio::mpsc` channels passing **typed frames
  directly** (no serialization), built crosswise by `in_memory_pair()`; `SocketEndpoint`
  over `Framed<UnixStream, LengthDelimitedCodec>` with `max_frame_length(MAX_FRAME_BYTES =
  1 MiB)` — an explicit untrusted-payload bound, also re-checked on send → `FrameTooLarge`.
  `connect`/`accept` build the Gateway/World socket endpoints (World binds + accepts, per
  §2.1.1/§2.1.2). `handshake.rs`: `announce_sessions` (Gateway sends `GatewayFrame::Resume`,
  awaits `WorldFrame::ResumeAck`, validates) / `accept_resume` (World awaits the resume,
  validates `schema_version == SCHEMA_VERSION` and `world_id == expected`, acks, returns the
  live set to re-adopt) — written generically over `Endpoint`, so one implementation runs
  over both transports. Mismatches emit a `tracing::warn` then a typed error (no silent
  failure). `error.rs`: `IpcError` (`thiserror`, `#[non_exhaustive]`) —
  `Io`/`Codec`/`SchemaMismatch`/`WorldIdMismatch`/`FrameTooLarge`/`UnexpectedFrame`/
  `PeerClosed`; no third-party error in a public variant. Multiplexing is a property of the
  frames (each carries its `session_id`); demuxing into per-session sinks is the M1-21/22
  consumer's job, not the transport's. Feature-flag split-vs-single selection (§2.1.3.4) is
  M1-22; M1-11b ships both transports.
- **Verify:** `crates/mud-ipc/tests/transport.rs` (TDD) — 9 `#[tokio::test]`s: round-trips
  both directions over **both** transports (shared generic helper = the same-contract
  proof), resume handshake replays `{1,2,3}` over both, schema-version mismatch and
  world-id mismatch rejected, in-memory and socket peer-close → `recv` `Ok(None)`, oversized
  frame → `FrameTooLarge`. `cargo test --workspace` green (mud-ipc 9; 16 mud-schema; others
  unchanged); `cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --all
  --check` clean. No `unwrap`/`expect`/`panic` outside tests. No docs-site change (IPC is
  internal plumbing — no player/builder/operator-observable surface).
- **Next:** **M1-12** — `mud-world` KDL room loader + tenant config (the `world_id` minted
  from tenant config feeds M1-11's handshake). The scheduler driver loop wiring the in-proc
  channel to `World` is M1-22; per-session demux + rate-limit live in M1-20/21.

## 2026-06-27 — Architecture checkpoint: unify the write-model dispatch

- **Spec:** §2.5.3.3, §2.5.3.5 — no behavior change; a cross-crate review at the
  M1-11b boundary found the write model duplicated.
- **Done:** Fanned out a 3-agent review of `crates/`. Verdict: architecture sound
  (inward layering, leaf `mud-schema`, 3NF schema, newtype discipline, no
  unwrap/panic). One real finding, invisible per-crate: the `Effect → World-op +
  TickEvent` dispatch and the `Precondition` semantics were implemented twice —
  once in `mud-core`'s `scheduler.rs` (free `apply`/`holds`) and again in
  `mud-db`'s `PersistentWorld`, whose copies were forced into `#[non_exhaustive]`
  catch-alls (`_ => false`, `_ => UnsupportedEffect`) that would silently mis-handle
  a future variant. Centralized both as the single source of truth on `World`:
  new `World::apply_effect(Effect) -> Option<TickEvent>` and `World::satisfies(
  Precondition) -> bool`. `Scheduler::tick` now calls them (free `apply`/`holds`
  deleted). `PersistentWorld` routes its in-memory mutation/precondition through
  them and deleted its own `holds`, keeping only the per-effect durable SQL write
  (and the explicit `Create` DB-first path). `Effect::Create` remains the one
  documented exception (DB-first for the `AUTOINCREMENT` key).
- **Verify:** `cargo test --workspace` green (mud-core 92 unit +3 new direct
  tests for `apply_effect`/`satisfies`; mud-db 7+8 unchanged; others unchanged);
  `cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --all
  --check` clean. No docs-site change (internal plumbing).
- **Next:** M1-12 (unchanged). The remaining split — `Scheduler` ordering vs.
  `PersistentWorld` durability are still two apply paths — is now a recorded
  **open design decision for M1-22** (PLAN M1-22): compose them into one path via
  the shared `World::apply_effect` seam (candidate: `PersistentWorld` owns the
  `Scheduler`; a `mud-core` `MutationSink` port is the clean alternative, deferred
  under the trait-for-one-impl rule).
