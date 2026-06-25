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
  `alloc` reuses a freed slot (a `free: Vec<SlotIndex>` stack) at its advanced
  generation or grows the arena; slot indices minted via `u32::try_from(len)`
  → `Exhausted` (no `as`). `free` resolves first (so cross-tenant/stale/double-
  free all error), then advances the generation via the M1-01
  `Generation::next()`: `Some` → `Free` + recycle, `None` → `Burned`, never
  relinked (§2.3.1.3). `resolve` checks tenant first (`CrossTenant`, kept
  distinct from `StaleHandle` per §3.11.4) then slot liveness+generation,
  returning the validated `SlotIndex`. Added `Generation::FIRST` const to
  `entity_id.rs`. Re-exported `EntityArena`/`ArenaError` from `lib.rs`.
- **Verify:** 7 new unit tests (tenant stamping, resolve live, freed→stale,
  slot-reuse bumps generation, **tenant-isolation: foreign handle rejected by
  another tenant's arena**, burn-on-generation-exhaustion via the real
  free/alloc path cycling one slot to `Generation::MAX`, double-free). `cargo
  test -p mud-core` (18 tests), `cargo clippy --workspace --all-targets -D
  warnings`, `cargo fmt --check` all green. No docs-site change (internal
  plumbing, no observable surface).
- **Next:** **M1-03** — core domain newtypes (`PlaceId`, `RegionId`,
  `ArchetypeId`, `ComponentId`, plus session/account ids as M1 needs them).
