# Ferrodun — Master Implementation Plan

> This is the **roadmap**. It sequences the work described in `SPEC.md`
> into small, self-contained PRs. It is descriptive of *order and shape*,
> not of requirements — when this plan and `SPEC.md` disagree, **`SPEC.md`
> wins** and this plan is corrected.

## Document roles

- **`SPEC.md`** — the normative specification. The single source of truth
  for *what* must be built. Honor RFC 2119 keywords exactly. Always read
  the cited section before implementing a PR.
- **`PLAN.md`** (this file) — the roadmap. The agreed *order* in which the
  spec is realized, decomposed into reviewable PRs. Followed top to bottom;
  amended when reality diverges.
- **`.claude/JOURNAL.md`** — the progress log. After every implementation
  PR, append one terse entry (format in `CLAUDE.md`) recording what was
  done, how it was verified, and what is left. The journal is the breadcrumb
  trail for the next session; **code is the source of truth for current
  state** when the two drift.

## How this plan is organized

The spec defines **twelve parallel workstreams** (§7.3) that integrate at
**eight milestones** (§7.4, M1–M8). Vibe-coding is sequential, so this plan
flattens the workstreams into an ordered PR sequence **grouped by the
milestone each PR serves**. A milestone is "done" when its acceptance demo
(§7.4) passes — that is the only gate (§8 rule 1).

Near-term milestones (Phase 0, M1, M2) are decomposed into concrete PRs.
Later milestones (M3–M8) are decomposed into **epics**; each epic is broken
into PRs *when the milestone is reached*, using the same conventions. This
is deliberate: planning M7 to the PR level today would be speculative, and
speculation violates the core principle below.

---

## Execution principles (binding on every PR)

1. **YAGNI — don't implement or stub it until you need it.** Build the
   minimum that the current milestone's demo requires. Do not scaffold
   future hot components, future `Place` variants, future providers, or
   future crates "to save time later." A crate is created in the PR that
   first needs it, not before. When the spec describes a capability a later
   milestone will exercise, leave a seam (a typed boundary), not a stub.
2. **Small, self-contained PRs.** Each PR is independently reviewable and
   testable, leaves the tree green (`cargo build`, `cargo clippy`,
   `cargo test`, `cargo fmt --check` all pass), and **leaves nothing half-
   built behind**. No `todo!()`, no dead `pub fn` waiting for a caller, no
   commented-out future code. If a PR cannot be finished cleanly, it is too
   big — split it.
3. **At most one crate's public API per PR** (§8 rule 3). Cross-crate
   refactors get their own PR. A consumer crate may *call* another crate's
   existing API freely; what is constrained is *changing* a second crate's
   public surface in the same PR.
4. **Wire/IPC changes start in `mud-schema`** (§8 rule 4). Define or extend
   the frame, regenerate Rust (and TS once it exists) together, never hand-
   edit generated code. Wire protocol is additive-only within a major
   version (§2.8.5).
5. **TDD where it earns its keep** (§8 rule 2). Engine logic: failing test
   first. Glue/IO: integration test is enough. Hot-reload paths and content
   features always get tests (§8 rules 8–9). Builder-content features are
   tested through the `mud test` harness, not a live server.
6. **Type-driven, newtypes mandatory, no `unwrap`** (`CLAUDE.md`, §1.7).
   Parse inputs into typed domain values at boundaries; inner code does not
   re-validate. Distinct concepts get distinct types.
7. **No silent failures** (§8 rule 5). Errors, lock denials, script/LLM
   failures, missing i18n keys, unknown markup — all surface as structured
   `tracing` events.
8. **Multi-tenancy and the Gateway/World split are present from day one**
   (§3.11.1, §7.5.5). The tenant tag lives in `EntityId` from the first
   PR that defines it; the IPC seam exists from M1 even while single-process
   mode is the default.
9. **One journal entry per implementation PR.** Append to
   `.claude/JOURNAL.md` before considering the PR complete.

### Definition of Done (per PR)

- [ ] Scope traces directly to the milestone the PR serves; nothing extra.
- [ ] The cited `SPEC.md` section(s) were read and honored (MUST/SHOULD/MAY).
- [ ] Tests written per principle 5; they fail before, pass after.
- [ ] `cargo fmt --check`, `cargo clippy` (workspace deny lints), `cargo
      test` all green.
- [ ] No `unwrap`; `expect` only in tests with a message; no `panic!/todo!/
      unreachable!` in production except behind a documented `// INVARIANT:`.
- [ ] At most one crate's public API changed.
- [ ] Journal entry appended.

---

## Roadmap at a glance

| Phase | Theme (§7.4) | Primary workstreams (§7.3) | Crates first touched |
|---|---|---|---|
| **0** | Bootstrap | — | `mudd` (workspace, CI) |
| **M1** | Walk and talk | Core runtime, Persistence, min. Networking, Game systems (accounts) | `mud-core`, `mud-db`, `mud-schema`, `mud-ipc`, `mud-net`, `mud-cmd`, `mud-world`, `mud-gateway`, `mudd` |
| **M2** | Builders without Rust | Scripting, i18n | `mud-script`, `mud-i18n`, `mud-cli` |
| **M3** | Client matrix | Networking & clients, Web | `mud-net` (MCCP2/GMCP/MSDP/MXP/MSSP), `mud-web`, `clients/` |
| **M4** | Wilderness and ships | Spatial | `mud-core` (Tile), `mud-world` (regions), `mud-vehicle` |
| **M5** | NPCs that act | NPC behavior, Combat | `mud-core` (combat prims), behavior-tree, `contribs/combat-d20` |
| **M6** | NPCs that speak | LLM dialogue & flavor | `mud-llm` |
| **M7** | Run it in production | Operations, Web & admin | multi-tenant runtime, `mud-web` admin, backup/restore |
| **M8** | 1.0 | Tutorial & docs | `games/tutorial`, docs |

Hard ordering constraints (the *only* ones, §7.5): M1 needs core +
persistence + min networking; M2 needs scripting on M1; **M5 must precede
M6**; M4 vehicles need M5 behavior trees *only if NPC crews are demoed*;
M7's graceful upgrade exercises the M1 split. Everything else is workstream-
local and may be reordered.

---

## Phase 0 — Bootstrap (pre-M1)

Goal: a clean Cargo workspace that builds green in CI, before any domain
code exists.

- **P0-01 — Workspace skeleton + CI.** Convert the single `ferrodun`
  package into a virtual Cargo workspace (`crates/`, `clients/`, `games/`
  reserved by the layout in §5 but **created lazily**). Move the current
  `main.rs` into the `mudd` binary crate as a placeholder `main`. Wire the
  existing workspace lints (already in `Cargo.toml`) and `clippy.toml` into
  every crate. Add CI running `cargo fmt --check`, `cargo clippy`, `cargo
  test`. Initialize `.claude/JOURNAL.md`.
  - *Verify:* CI green on an empty workspace; `cargo run -p mudd` prints the
    placeholder.
  - *Out of scope:* any of the spec crates beyond `mudd`. Do not pre-create
    `mud-core` et al.

- **P0-02 — Documentation site (MkDocs + mike).** Versioned docs site under
  `docs/` (MkDocs + Material), deployed to GitHub Pages via `mike`: `main`
  publishes the `next` version, each `vX.Y.Z` tag snapshots its own version
  (`.github/workflows/docs.yml`). Initial content is a placeholder home page.
  Not in §5/§7.4 as an infrastructure item, but the §3.14/§11 builder-docs
  workstream needs a home; the site is grown per the cross-cutting
  Documentation track as features land.
  - *Verify:* `uv run mkdocs build --strict` from `docs/` is clean (uv
    project: `pyproject.toml` + `uv.lock`); PR CI builds, push to `main`
    deploys `next`.
  - *Out of scope:* real builder/GMCP/API docs content (M8-C), custom theme.

---

## M1 — Walk and talk

**Acceptance (§7.4 M1):** two players connect over telnet, log in, walk
between hand-authored rooms, see each other, chat. Account credentials,
puppet location, and inventory survive a clean restart (entity references
persist via the durable `EntityKey`; the in-memory `EntityId` is re-minted on
load). ANSI + NAWS work.
Locks parse and evaluate. A tenant-isolation smoke test asserts an
`EntityId` minted in tenant A cannot be resolved/mutated/observed via
tenant B (§2.3.1, §3.11.4).

PRs are grouped by area; rough order is top-to-bottom, but core →
persistence → schema → world → net → cmd → integration is the dependency
spine.

### Core runtime (`mud-core`)

- **M1-01 — `EntityId` + `TenantTag`.** 8-byte id with the normative bit
  layout: 12 bits tenant tag, 32 bits slot index, 20 bits generation
  (§2.3.1.3). Encode/decode, generational-index semantics, and the
  "generation wraparound burns the slot rather than recycling" rule.
  - *Spec:* §2.3.1. *Verify:* unit tests on packing, round-trip, wraparound.
- **M1-02 — Generational arena (per tenant).** A `slotmap`-style arena that
  allocates `EntityId`s with the current tenant tag, resolves live handles,
  and invalidates stale handles on slot reuse (§2.3.7.3). `EntityId` is the
  **ephemeral** arena handle; durable identity (`EntityKey`), the
  `EntityKey`↔`EntityId` mapping, and LRU eviction live in the write-through
  cache layer (M1-09, §2.3.1.4–2.3.1.6). Cross-tenant resolution returns an
  error, never another tenant's entity (§3.11.4).
  - *Spec:* §2.3.1–2.3.2, §3.11.4. *Verify:* alloc/free/stale-handle tests;
    **the tenant-isolation unit test** (A's id not resolvable via B).
- **M1-03 — `EntityKey` (durable entity identity).** The durable, per-tenant
  monotonic 64-bit identity and DB primary key (§2.3.1.5); the only entity
  reference that crosses the disk/wire/IPC boundary (§2.3.1.4). A newtype
  distinct from the ephemeral `EntityId` (§2.3.1.1) so the two cannot be
  confused at compile time (§1.7). Per-tenant monotonic minting and the
  `EntityKey`↔`EntityId` mapping live in M1-08/M1-09; this PR adds only the
  type. Per YAGNI, the other ids the foundation once bundled here move to their
  first consumer: `PlaceId`/`RegionId` to M1-04, `ArchetypeId`/`ComponentId` to
  the M2 archetype/component-bag work.
  - *Spec:* §1.7, §2.3.1.4–2.3.1.5. *Verify:* compile-level; `EntityKey`
    (durable) and `EntityId` (ephemeral) are distinct types — misuse is a type
    error; `Option<EntityKey>` is niche-optimized to 8 bytes.
- **M1-04 — `Place` enum (Room only) + spatial surface.** Static-dispatch enum
  with the single `Room` variant for M1 (Tile deferred to M4, §2.2.1).
  Introduces the `PlaceId`/`RegionId`/`Direction` newtypes (moved from M1-03)
  the surface uses (`Direction` = n/e/s/w + up/down). `Place` exposes the
  §2.2.2 surface — `id`, `region`, `describe(viewer)`, `neighbor`,
  `visible_places` — as **inherent methods** that `match` on the variant, so
  dispatch is static by construction (§2.2.5, no trait object). The `PlaceView`
  *trait* is **deferred to M4**: with one variant it would be single-impl
  (YAGNI); `Tile` gives it a genuine second implementor. `occupants()` is
  **deferred to M1-05**: occupancy's authoritative home is the dense
  `LocationOf` side-table (§2.3.2.2), so adding it to the static `Place` now
  would duplicate that index (build-then-rip).
  - *Spec:* §2.2. *Verify:* describe/neighbor/visible-places unit tests against
    a fixture room graph. *Out of scope:* Tile + the `PlaceView` trait (M4),
    `occupants` (M1-05), viewer-conditional invisibility beyond a trivial hook.
- **M1-05 — Hot side-tables (M1 subset).** Dense `LocationOf` and
  `Inventory` tables only — the two M1 needs. `Position`, `Health`,
  `Initiative` are **not** added until their milestone (§2.3.2.2 lists all
  five as hot, but YAGNI: add each dense table when first used). Adds
  `occupants()` to the `Place` surface (deferred from M1-04), resolving a
  Place's occupants through the `LocationOf` reverse index.
  - *Spec:* §2.3.2.2–2.3.2.4. *Verify:* occupants-of-place (via the `Place`
    surface) and inventory-of-entity round-trips.
- **M1-06 — Scheduler tick + `MutationCommand` (M1 subset).** 20 Hz fixed
  tick (§3.16.2). `MutationCommand` enum with only the variants M1 needs
  (move entity between Places, inventory add/remove, create/teardown
  entity). Per-entity serialization, arrival-order application, precondition
  carrying (§2.5.3.5).
  - *Spec:* §2.5.3.3, §2.5.3.5, §3.16.2. *Verify:* serialization +
    last-writer-wins + precondition-failed tests.
  - *Out of scope:* the **wall-clock driver loop** — this PR ships a
    deterministic logical `tick()` plus the `TICK_HZ`/`TICK_PERIOD` cadence
    constants only. The 50 ms timed loop that calls `tick()` is deferred to
    M1-22 (no async runtime is wired before then).
- **M1-07 — Locks DSL.** `chumsky` parser → typed AST; static-dispatch
  evaluation table (no string matching at eval time). Lock functions for
  M1: `perm`, `attr`, `tag`, `self`, **`status`** (§2.6.1.2). `status` is
  included so all three normative example strings (one uses `status(drunk)`)
  fully evaluate. Two-phase pipeline: a `chumsky` grammar produces a purely
  syntactic AST (functions = name + string args), then a separate **resolve**
  pass lowers known functions into the typed `LockFn` enum — keeping parse
  pure-syntax and giving M2's `mud check` a clean seam for unknown-function /
  arity / tag-lint diagnostics (§2.6.2.3–2.6.2.4). Inline typed builder seam
  may be deferred to when scripts need it (M2).
  - *Spec:* §2.6.1–2.6.2. *Verify:* parse + eval table tests over the three
    normative example strings. *Out of scope:* `mud check` CLI validation
    (M2 with `mud-cli`), LSP (§2.6.3, post-1.0).

### Persistence (`mud-db`)

- **M1-08 — SQLx + SQLite backend.** `mud-db` crate; SQLx with compile-time
  checked queries; `sqlx migrate` setup; initial migration for accounts,
  puppets, entities (keyed by a per-tenant monotonic `EntityKey`, §2.3.1.5),
  location, inventory. **Per-tenant connection pool over
  a distinct SQLite file** (§2.5.1.4) — no shared DB, no tenant column.
  - *Spec:* §2.5.1. *Verify:* migration applies; per-tenant file isolation
    test. *Out of scope:* Postgres backend (added when prod is exercised,
    M7-ish), `sqlite-vec` (M6).
  - *As built:* layout is **namespaced by backend** to anticipate Postgres
    without a premature trait (single-impl rule): SQLite migrations under
    `migrations/sqlite/`, code under `src/sqlite/`; a backend-agnostic `DbError`
    at the crate root; Postgres will land as sibling `migrations/postgres/` +
    `src/postgres/`, and the unifying seam emerges with that second implementor.
    The §2.5.1.2 **compile-time-checked `query!` macros** (and the `.sqlx`
    offline cache + `SQLX_OFFLINE` CI step they require) are deferred to **M1-09**
    with the first real write-through query — M1-08 has no app queries, so its
    tests use runtime `sqlx::query` and need no CI change.
  - *SQL tooling:* migrations are linted by **sqruff** (pinned in `mise.toml`),
    gated in CI (`sqruff lint crates/mud-db/migrations`) and wired into Zed via
    the sqruff LSP. Dialect is per-directory through sqruff's hierarchical
    `.sqruff` discovery: the root `.sqruff` sets `dialect = sqlite`; the Postgres
    backend PR adds `migrations/postgres/.sqruff` (`dialect = postgres`).
- **M1-09 — Write-through + boot load (cache keyed by `EntityKey`).** Every
  mutation flows through `MutationCommand` and applies to arena + DB in one
  transaction (§2.5.3.3). The arena is a cache keyed by `EntityKey`: loading an
  entity mints a fresh `EntityId` for its durable `EntityKey` and installs the
  `EntityKey`↔`EntityId` mapping (§2.3.1.6). World state loads from DB on boot
  so a clean restart restores accounts, location, and inventory.
  - *Spec:* §1.2, §2.3.1.4–2.3.1.6, §2.5.3. *Verify:* restart integration test
    (write → drop process → reload → state intact), asserting a persisted
    `EntityKey` resolves to the same entity after restart; `EntityId` values are
    **not** expected to survive restart (re-minted on load). *Out of scope:*
    LRU eviction + cache-miss reload beyond what boot-load exercises (deferred
    until working sets exceed the cache, M7-ish); background snapshot (§2.5.3.4
    — crash recovery, deferred until M7 hardening; clean restart needs only
    write-through).
  - *As built:* the `EntityKey`↔`EntityId` mapping and write-through live in a
    `PersistentWorld` in **`mud-db`** (`src/sqlite/write_through.rs`), wrapping an
    untouched `mud-core` `World`; the arena-as-cache (§2.3.1.6) is the map →
    `EntityId` → arena slot. `mud-core` gained only two read accessors
    (`MutationCommand::effect`/`precondition`) so `mud-db` can inspect a command.
    Since an in-memory arena cannot enlist in a SQL transaction, "one
    transaction" (§2.5.3.3) is realized as **apply-in-memory-then-commit**:
    `Create` is DB-first (key from `AUTOINCREMENT`), all other effects apply to
    the arena first (preserving the precise `ArenaError` classification) then
    write the DB. **Teardown deletes the entity's `entities` row** (destruction,
    not eviction — a destroyed entity must not resurrect; key non-reuse still
    holds via `AUTOINCREMENT`); every entity-referencing FK is `ON DELETE
    CASCADE`, so its dependent rows go with it and the destroy path stays
    table-agnostic. First compile-time `query!` macros land here with the
    committed `crates/mud-db/.sqlx` offline cache and `SQLX_OFFLINE: "true"` in
    CI; `Effect`/`Precondition` being `#[non_exhaustive]` forces a defensive
    wildcard arm (`DbError::UnsupportedEffect`).

### Wire/IPC seam (`mud-schema` types, `mud-ipc` transport) and Gateway/World split

- **M1-10 — `mud-schema` IPC frames.** `mud-schema` crate; postcard IPC
  frame types for M1: `SessionInput`, `SessionOutput`, connect/disconnect,
  schema version (§2.1.3.1). Ships **hand-written `serde`-derived postcard
  frame types — no codegen**: §2.8.5.7 version-locks IPC frames at build time
  and excludes them from the code-generated wire protocol, and the wire
  protocol that §2.8.3.1 actually generates (map/vitals/NPC actions → Rust +
  TS + GMCP docs) does not exist until M3. The **codegen mechanism is deferred
  to M3** (see M3-D).
  - *Spec:* §2.1.3, §2.8.3. *Verify:* frame round-trip encode/decode tests.
  - *As built:* two **directional enums** `GatewayFrame` (Connect/Input/
    Disconnect, G→W) and `WorldFrame` (Output/Close, W→G) make an illegal
    direction unrepresentable; both `#[non_exhaustive]`. `SessionId` is a
    `NonZeroU64` newtype (niche-friendly); `SCHEMA_VERSION` is a build-time
    const (1), carried by the M1-11 resume handshake, not stamped per frame.
    `mud-schema` is a **leaf crate** (no `mud-core` dep): **no M1 frame carries
    an `EntityKey`** — `SessionInput`/`SessionOutput` carry text, connect/
    disconnect carry only a `SessionId`; entity-bearing frames arrive in M3+.
    Text payloads are **marker newtypes** (`InputLine`, `OutputText`, mirroring
    `mud-core`'s `Description`) not raw `String`, per the newtype mandate — no
    invariant enforced here since §3.6.4's cap/stripping is command-scoped and
    downstream (M1-17); `OutputText` is `String`-backed for M1 and M1-13 swaps it
    for styled text. `encode`/`decode` helpers wrap postcard (`SchemaError` via
    `thiserror`); length-prefixing is M1-11.
- **M1-11 — IPC transport + resume handshake + single-process mode.** Split
  into two PRs so each touches one crate's public API (principle #3) and the
  wire/IPC change starts in `mud-schema` (§8 rule 4): **M1-11a** defines the
  handshake frame *types*, **M1-11b** builds the transport that carries them in
  the new `mud-ipc` crate.
  - **M1-11a — `mud-schema` resume-handshake vocabulary.** Add `WorldId` (a
    `NonZeroU64` newtype mirroring `SessionId`, the §2.1.3.1 per-World address);
    the `ResumeHandshake { world_id, schema_version, live_sessions }` and
    `HandshakeAck { world_id, schema_version }` payloads; and the directional
    variants `GatewayFrame::Resume` (G→W announce) / `WorldFrame::ResumeAck`
    (W→G). Both enums are `#[non_exhaustive]`, so appending is wire-additive and
    the M1-10 golden-bytes pin is unperturbed.
    - *Spec:* §2.1.3.1–2.1.3.2, §2.8.5.7. *Verify:* `WorldId` niche/round-trip;
      handshake-frame round-trips; golden pin unchanged.
  - **M1-11b — `mud-ipc` transport + single-process mode.** New `mud-ipc` crate
    (the IPC transport's home — kept out of the tokio-free, codegen-source
    `mud-schema` leaf, §5.1). Length-prefixed postcard over a unix socket,
    multiplexed by `session_id` (`SocketEndpoint` over `tokio-util`'s
    `LengthDelimitedCodec` with a `MAX_FRAME_BYTES` cap); **single-process mode**
    via an in-memory channel (`InMemoryEndpoint` over `tokio::mpsc`) with the same
    frame contract (§2.1.3.3). A duplex `Endpoint` trait (two impls = the
    legitimate single-vs-split seam) lets the resume-handshake exchange
    (`announce_sessions`/`accept_resume`, carrying `world_id` + `SCHEMA_VERSION` +
    the live session set, §2.1.3.2) be written once over both transports.
    Feature-flag/config selection of split vs. single (§2.1.3.4) is the `mudd`
    binary's call (M1-22); M1-11b ships both transports.
    - *Spec:* §2.1.3. *Verify:* in-proc and unix-socket transports pass the same
      frame round-trip; resume-handshake replays a live session set;
      schema/world-id mismatch and frame-size cap rejected.
    - *Out of scope:* admin RPC sibling socket (§2.1.3.5 — M7); World-restart
      "reconnecting" banner (M7); per-session demultiplexing (M1-21/22).

### World loading (`mud-world`) and config

- **M1-12 — `mud-world` KDL room loader + tenant config.** `mud-world`
  crate; parse hand-authored rooms (id, description, named exits) from KDL;
  load tenant `config.toml` via `figment`; load the welcome banner (§3.19.1).
  Minimal archetype handling: a built-in `player` puppet shape (full KDL
  archetype + `extends` + hooks land in M2).
  - *Spec:* §2.3.5 (minimal), §4.1, §3.19.1. *Verify:* loads the M1 fixture
    world; malformed KDL yields a structured load error.

### Styled output and engine strings (minimal seams)

- **M1-13 — Styled text + ANSI renderer (minimal).** Transport-neutral
  styled-text spans in `mud-core` (§3.20.1); a KDL palette with the baseline
  roles (§3.20.3.2); per-session ANSI renderer in `mud-net` defaulting to
  `ansi16` with `NO_COLOR` → `mono` and fixed downsample tables (§3.20.5).
  No raw escapes in internal pipelines (§3.20.1.2).
  - *Spec:* §3.20.1–3.20.5. *Verify:* snapshot tests for ansi16 + mono
    rendering of a styled fixture. *Out of scope:* truecolor/xterm256 tiers
    beyond the downsample tables, webclient semantic spans (M3), per-account
    color prefs (M7-ish), colorblind palette (ship by 1.0).
- **M1-14 — Engine-string lookup seam.** Route engine-emitted player strings
  through a minimal `t!`-style lookup backed by a static `en` table. This
  establishes the §3.14.4 boundary (typed keys, `en` fallback, missing-key
  `tracing` warning) **without** Fluent. M2 swaps the backing store to
  `fluent-rs` + hot-reload + per-tenant overrides; **call sites do not
  change.**
  - *Spec:* §3.14.4 (boundary only). *Verify:* missing key emits a warning
    and falls back to the literal key. *Out of scope:* `.ftl` bundles,
    hot-reload, locale resolution (all M2).

### Command pipeline (`mud-cmd`) and built-ins

- **M1-15 — `mud-cmd` CmdSet + parser.** CmdSet model; trie parser with
  prefix matching, aliases, switches (§2.7 step 5); merge semantics
  Union/Replace/Remove with the fixed precedence order (§2.7 step 4).
  - *Spec:* §2.7 steps 4–5. *Verify:* merge precedence + prefix-match tests.
  - *Out of scope:* full object disambiguation prompt/ordinals (add when
    multiple matching items exist — minimal `name`/single-match for M1).
- **M1-16 — Command pipeline in World.** Resolve `session → account →
  puppet → location stack`, merge CmdSets, lock-check the caller, dispatch
  to a Rust-native `run`, render output per session (§2.7 steps 3–8). Every
  run carries a `command_id` for trace correlation (§2.7.1).
  - *Spec:* §2.7. *Verify:* end-to-end command dispatch unit/integration
    test with a fake session.
- **M1-17 — Built-in commands (M1 set).** Rust-native: `look`, movement
  (`north/east/south/west/up/down` + aliases), `say`, `who`, `quit`,
  `get`/`drop`, `inventory`. `say`/`emote` honor the 4 KiB content cap and
  control-char/ANSI stripping (§3.6.4) and render through palette roles
  (§3.20.4).
  - *Spec:* §2.7, §3.6.3–3.6.4, §3.20.4. *Verify:* per-command behavior
    tests; content-cap rejection test.

### Accounts and sessions (`mud-core` domain + `mud-db` storage + FSM)

- **M1-18 — Accounts + login.** Account domain types (tenant-scoped,
  §3.15.1.1); `argon2id` credential hashing with per-account salt
  (§3.15.1.2); **open-registration** mode only for M1 (invite-only deferred
  to M7); explicit puppet-selection step (§3.15.1.4); account states with
  suspended/banned rejected at login (§3.15.1.5, enforcement minimal).
  - *Spec:* §3.15.1. *Verify:* register → login → wrong-password reject →
    restart → login-again tests. *Out of scope:* recovery flow, invite
    tokens, moderation states machinery (M7).
- **M1-19 — Session FSM (login states).** In `mud-net`: pre-login banner →
  register/login → puppet select → in-world. Pre-login `help` listing the
  small command set (§3.19.1, §3.19.3). Linkdead/idle handling minimal
  (full linkdead reattach is M7-grade; M1 just needs clean connect/quit).
  - *Spec:* §3.19.1, §3.19.3, §2.7 step 1. *Verify:* FSM transition tests.

### Networking and integration (`mud-net`, `mud-gateway`, `mudd`)

- **M1-20 — `mud-net` telnet core.** Telnet/IAC negotiation for the M1
  subset: NAWS (drives width/pagination), CHARSET/UTF-8 with legacy
  transliteration fallback, EOR/GA prompt framing, TTYPE (§2.8.2). Line
  decoder; per-session command **rate limit** leaky bucket (10/s sustained,
  burst 20) at the gateway boundary (§2.1.1).
  - *Spec:* §2.8.2 (subset), §2.1.1. *Verify:* IAC negotiation unit tests;
    rate-limit drop test. *Out of scope:* MCCP2/GMCP/MSDP/MXP/MSSP (M3),
    TLS/SSH/WebSocket (M3).
- **M1-21 — `mud-gateway` binary.** Owns the telnet listener; decodes input
  and forwards `SessionInput` over IPC; renders `SessionOutput` back to the
  client (§2.1.1). Holds connections and shows a reconnect banner on IPC
  loss — banner deferred to M7; M1 gateway assumes World is up in single-
  process mode.
  - *Spec:* §2.1.1. *Verify:* gateway↔World loopback test in single-process
    mode.
- **M1-22 — `mudd` single-process wiring.** Boot a tenant: load world
  (M1-12), open DB pool (M1-08), start the scheduler (M1-06), run the
  command pipeline (M1-16), embed the gateway (M1-21) via the in-proc IPC
  channel (M1-11). **Starting the scheduler = owning a `mud_core::World` plus
  a `mud_core::Scheduler` and running the driver loop M1-06 deferred:** every
  `mud_core::TICK_PERIOD` (50 ms / `TICK_HZ`), call `scheduler.tick(&mut
  world)` and consume the returned `Vec<TickEvent>` (`Created` reports minted
  handles; `PreconditionFailed`/`Rejected` are surfaced to the caller). The
  async **runtime** first appears at M1-11b (the `mud-ipc` unix-socket
  transport); M1-22 adds the scheduler **timer / driver loop** on top of it.
  M1-06 ships only the logical `tick()` and the cadence constants.
  - *Spec:* §2.1.3.3, §5.2. *Verify:* `cargo run -p mudd` serves a telnet
    login locally.
- **M1-23 — M1 acceptance integration test.** Drive two scripted telnet
  sessions through login, movement, mutual visibility, and chat; assert ANSI
  + NAWS; kill and restart the process and assert credentials, location, and
  inventory persisted; run the **cross-tenant handle test** through the full
  World API (not just the arena). Locks parse + evaluate in at least one
  gated command.
  - *Spec:* §7.4 M1. *Verify:* this is the M1 gate — it must pass to claim M1.

---

## M2 — Builders without Rust

**Acceptance (§7.4 M2):** a non-programmer adds an archetype, a custom
component, a Lua command, and a prototype, and **hot-reloads with no restart
and no recompile**. `mud check` catches a broken lock string and a bad hook
signature before load. A non-English `.ftl` bundle is dropped into a
tenant's `i18n/`, hot-reloaded, and a localized engine string renders to a
session whose locale resolves to it (§3.14.8.1).

Depends on M1 core (§7.5.2). Epics → PRs:

- **M2-A — `mud-script` Lua host + sandbox.** Embed Lua 5.4 via `mlua`;
  strip `io`/`os`/`package.loadlib`/filesystem/network (§2.4.2); Rust-side
  capability allowlist; dedicated script worker pool with the 50 ms
  command-path / 5 s background deadlines and cooperative debug-hook
  termination (§2.4.7); low-latency incremental GC (§2.4.7.4).
- **M2-B — `mud` stdlib.** `mud.json`, `mud.time`, `mud.random` (seeded),
  `mud.tbl`, string/search/create helpers, the entity/component API
  (§2.4.5). `mud.time` exposes `wall/tick/game/after/every` (§3.16.4).
- **M2-C — Custom module loader.** Reimplement `require` to resolve only
  within the tenant script tree; reject arbitrary paths with a structured
  error (§2.4.6).
- **M2-D — Script-defined components.** Tagged-blob bag representation with
  schema + version (§2.3.2.1, §2.3.3.2); one lookup API across Rust- and
  script-defined components; **schemas immutable across hot-reload**, schema
  changes routed to content migration (§2.4.3.4).
- **M2-E — Script-defined commands and hooks.** Lua `run` functions in the
  pipeline (§2.7 step 7); hook tables keyed by archetype with **static
  surface checking** of hook signatures, lock functions, component accesses,
  and engine-API calls at load time (§2.3.6, §2.4.4).
- **M2-F — Full archetype loader.** KDL archetype declaration with component
  defaults, hook table, and single-inheritance `extends` (§2.3.5); hook
  resolution statically validated at world load (§2.3.6.2).
- **M2-G — Prototypes.** Prototype scripts that return a table; `spawn(...)`
  as a core engine call (§3.7).
- **M2-H — Hot-reload (drain-before-swap).** File watcher; new calls hit the
  new version while the old drains; atomic per-file reload; failed load keeps
  the previous version live; epoch-versioned userdata handles raise typed
  errors when stale (§2.4.3). **Hot-reload paths get tests** (§8 rule 9).
- **M2-I — `mud-i18n` (Fluent).** Replace the M1-14 static `en` table with
  `fluent-rs`; two-source tenant-overriding bundle discovery (§3.14.3.2);
  tenant-scoped loader; hot-reloadable bundles; locale resolution per session
  (§3.14.6); `mud.i18n.t` / `mud.i18n.locale_of` for scripts (§3.14.4.2);
  localized command aliases in the CmdSet merge (§3.14.5.2); load-time
  verification that every `t!`/`mud.i18n.t` key exists in `en` (§3.14.6.2).
- **M2-J — `mud-cli` + `mud check`.** `mud-cli` crate; `mud check` statically
  validates lock strings against known lock functions and permission names,
  warns (not errors) on unknown tags (§2.6.2.3–2.6.2.4), and validates hook
  signatures before load (§7.4 M2).
- **M2-K — Content migrations.** Versioned Lua migration scripts run over
  existing entities of a changed archetype on world load, versioned
  alongside schema migrations, with a **dry-run mode** (§2.5.4, §3.13).
- **M2-L — M2 acceptance.** End-to-end demo in the `mud test` harness:
  add archetype + component + Lua command + prototype, hot-reload, and a
  non-English locale render. `mud check` catches a broken lock and a bad
  hook.

---

## M3 — Client matrix

**Acceptance (§7.4 M3):** Mudlet, TinTin++, MUSHclient, and BlightMud all
connect cleanly with MCCP2 + GMCP + MSDP + MXP. The webclient SPA renders
the same game over WebSocket. SSH and TLS ports are live.

Epics (decompose into PRs when reached):

- **M3-A — MCCP2** compression in the telnet stack (§2.8.2).
- **M3-B — GMCP** with the engine's documented, versioned namespace; the
  reserved `Core.*` handshake messages (`Hello`/`Welcome`/`Locale`/`Ping`/
  `Pong`/`Goodbye`) defined in `mud-schema` first (§2.8.3.3, §8 rule 4),
  including the 5 s default-profile fallback (§2.8.3.4).
- **M3-C — MSDP** as the alternative out-of-band channel; **MXP** clickable
  links/styling; **MSSP** server status; round out TTYPE/NAWS/CHARSET edge
  cases (§2.8.2).
- **M3-D — Wire-protocol codegen (Rust + TS + GMCP docs).** Establish the
  code-generation mechanism §2.8.3.1 mandates — defined once in `mud-schema`,
  generated to Rust types, TypeScript types (into `clients/schema-ts/`), and
  auto-rendered GMCP docs. M1-10 deliberately left this out (its IPC frames are
  hand-written and version-locked, §2.8.5.7); the mechanism first earns its
  keep here, with the structured wire protocol (map/vitals/NPC actions) and a
  TS consumer (the M3-G webclient). Unknown-field tolerance on generated
  decoders both sides (§2.8.3.1, §2.8.5.7).
- **M3-E — Telnet-over-TLS** (`rustls`) and **SSH** (`russh`, key auth,
  optional per deployment) (§2.8.2).
- **M3-F — WebSocket transport** (`tokio-tungstenite`) carrying the logical
  protocol with a JSON/CBOR envelope; webclient semantic color spans
  (§3.20.5.3).
- **M3-G — `mud-web` + webclient SPA skeleton.** Axum on the Gateway public
  listener; Svelte+TS webclient that connects over WS and renders rooms/chat
  using `schema-ts` types (§2.9.1, §2.9.3).
- **M3-H — Reference client matrix in CI.** Headless harnesses for the four
  clients (§2.8.4); this is the M3 gate and the standing regression guard
  from M3 onward (§10 risk row).

---

## M4 — Wilderness and ships

**Acceptance (§7.4 M4):** walk from a hand-authored city room onto an
overworld tile, board a sailable ship, cross water to another city. Viewport
renders with per-player FOV. GMCP map data drives webclient tile graphics.

Epics:

- **M4-A — `Place::Tile` + coordinate system.** Add the `Tile` variant and
  the dense `Position` hot table now that it's needed; signed-32 `(x,y,z)`
  with z as floor-stacking (§2.2.1, §3.2.2.0). Rooms↔tiles freely
  interconnect via the single `move()` primitive (§2.2.4, §3.2.5). With a
  second variant the `visible_places` arms now need an enum/`Either` iterator
  to unify; revisit the visibility set's type while doing so: M1 stores it as
  `Vec<PlaceId>`, which permits duplicates even though §2.2.2 calls it a *set*.
  Decide between dedup-on-build and a true set type, weighing the
  order-preservation a `Vec` gives display against `HashSet` set semantics —
  and drop `visible_places_yields_the_authored_set`'s exact-ordering assertion
  if visibility becomes unordered.
- **M4-B — Regions.** `mud-world` region loader: terrain layer (ASCII *or*
  PNG palette), features overlay, encounters layer, region scripts
  (§3.2.2–3.2.3). Procedural regions (`(x,y)->tile`, lazy + cached) and the
  sparse `tile_overlay` table (§3.2.3.3–3.2.3.4).
- **M4-C — Viewport, FOV, fog-of-war.** NAWS-sized viewport centered on the
  player; terrain+entity glyphs with ANSI/truecolor; engine-side FOV reused
  for NPC perception (§3.2.4).
- **M4-D — GMCP map frames.** Structured map data in `mud-schema` →
  webclient tile renderer with ASCII fallback (§3.2.4.4, §2.9.3.2); the
  per-tenant tile asset pipeline + `tiles.kdl` manifest + `Core.AssetsChanged`
  hot-reload (§2.9.3.3).
- **M4-E — `mud-vehicle`.** Vehicles as mobile places-and-entities; movement
  coupling carries occupants and emits ambience; controls as locked commands
  on a control entity (`steer`/sails); per-vehicle terrain predicate;
  boarding/docking temporary exits; vehicle persistence; nesting rejected
  in v1 (§3.3). Player-piloted ship needs no NPC crew (§7.5.4).

---

## M5 — NPCs that act

**Acceptance (§7.4 M5):** scripted NPCs perceive, decide, move, fight, and
trade using behavior-tree primitives and the d20-flavored reference combat
rules. **No LLM.** M5 establishes the mechanical substrate and **must
precede M6** (§7.5.3).

Epics:

- **M5-A — Behavior-tree primitives + perception** (workstream 6). Scripted
  decide-loop that never blocks on the network; perception reuses M4 FOV.
- **M5-B — Combat primitives.** Add `Health` and `Initiative` hot tables now
  that combat needs them; initiative/round scheduler driven by the 20 Hz
  tick; damage-type × resistance matrix; status effects with duration + tick
  hooks; range bands from `Place` distance; hooks `on_attack`,
  `on_damage_taken`, `on_death`, `on_round_start` (§3.4.2).
- **M5-C — Death primitives.** `on_death` hook; default `Corpse` archetype in
  `mud-core` with inventory transfer + decay timer; `RespawnPoint`/
  `Respawnable` reposition via `MutationCommand`; admin revive (§3.4.4).
- **M5-D — `contribs/combat-d20`.** D&D-flavored + basic-d20 + classless
  reference rule sets as optional contribs (§3.4.3, §5.3).
- **M5-E — Economy.** `Wallet`/`PriceTag`/`Shop`/`MarketOrder`/`Ledger`
  components; KDL stock files + Lua restock; tagged-integer currencies
  (signed 64-bit, scripted conversion, no baked-in set); journaled ledger;
  auction/player-market primitives (§3.5).
- **M5-F — Equipment, factions, PvP, parties/pets.** `Equipped` over
  `Inventory` with archetype slot tables; factions as tag sets +
  relationship matrices; `PvpPolicy` tag with **safe zones default**;
  parties as degenerate vehicles, pets/followers as `FollowTarget` NPCs
  (§3.12).

---

## M6 — NPCs that speak

**Acceptance (§7.4 M6):** an LLM innkeeper **layered on an M5 scripted NPC**
remembers each player across sessions, references prior interactions, refuses
low-reputation characters (refusal scripted, delivery LLM-authored), and
keeps working when the provider is killed mid-session — fallback lines take
over without breaking play.

Epics (all in `mud-llm`):

- **M6-A — Provider abstraction.** Anthropic, OpenAI, Google, local Ollama,
  per-NPC selectable; SSE streaming with word-by-word render; replay/
  deterministic mode for tests (§3.1.9, §3.1.8.5). *Resolve §9 open question:
  tutorial default provider, before this lands.*
- **M6-B — Action/speech split + async delivery.** Script decides actions
  synchronously on the command path; dialogue request queued in parallel;
  response delivered later as a `say`/`emote` event keyed to the same
  `command_id` (§2.7 step 7, §3.1.2, §3.1.6). **Combat never `await`s an LLM
  future** (§3.1.2.3).
- **M6-C — Constrained output + guardrails.** Typed `{say?,emote?,to?,
  mood_hint?}` shape, server-validated, no tool use (§3.1.5); last-known-
  value mood hints with archetype defaults (§3.1.2.2); per-NPC/zone/tenant/
  server token budgets, per-player throttle (1 / 2 s, collapse to latest),
  3 s soft deadline → fallback (§3.1.8).
- **M6-D — Memory layers + vector store.** Six-layer prompt assembly
  (§3.1.3); dyadic + episodic memory; vector storage via `sqlite-vec` /
  `pgvector` (§2.5.2); shared per-tenant world-event log (§3.1.3 #6);
  summarization-based eviction (§3.1.8.4). *Resolve §9 vector-granularity
  question via load test.*
- **M6-E — Fallback line tables + locale.** Per-archetype KDL fallback tables
  keyed by `(archetype, locale)` with `mood_default` (§3.1.6.3.1); LLM
  locale awareness in the persona slice (§3.14.7).
- **M6-F — LLM observability.** Per-call tracing span (prompt length, tokens,
  latency, fallback, throttle) + minimal call inspector landing with this
  workstream (§3.1.10).

---

## M7 — Run it in production

**Acceptance (§7.4 M7):** admin dashboard shows live commands, script errors,
LLM calls, and metrics; **two isolated games run on one binary**; graceful
upgrade preserves connections across a World restart (Gateway HTTP stays up,
admin SPA loads with a "World disconnected" banner, `/metrics` exposes
`world_up`); online per-tenant backup + restore work against a live second
tenant; moderation tooling and account deletion/export are demoable; security
and performance passes complete.

Epics:

- **M7-A — Multi-tenant runtime activation.** Flip on the tenant machinery
  wired since M1 — arena, script loader, file watcher, scheduler, DB pool,
  metrics labels all per-tenant; single Gateway multiplexes Worlds by
  port/hostname (§3.11.5–3.11.6). **No data-model change** (§3.11.6).
- **M7-B — Gateway/World graceful upgrade.** Split-mode hardening: connection
  hold + reconnect banner on IPC loss; resume handshake reattach;
  "last command interrupted" message; admin RPC sibling socket (§2.1.3.5,
  resolve §9). Gateway-served HTTP + merged `/metrics` with `world_up`
  (§2.1.1).
- **M7-C — `mud-web` admin SPA.** Entity browser, live command log, script
  editor with reload, LLM call inspector, metrics dashboards; admin auth
  reusing the account system with `perm(admin)` re-checked in World
  (§2.9.2). Account/moderation browser + action panel (suspend/ban/kick/
  silence/revert) + report queue, all journaled (§3.15.5).
- **M7-D — Backup/restore.** `mud backup`/`mud restore` online, per-tenant,
  with PITR; restore drains one tenant without stopping others and runs
  schema + content migrations forward (§3.18).
- **M7-E — Privacy + lifecycle.** Soft/hard account deletion, data-export
  endpoint, retention windows (§3.17); invite-only registration + recovery
  tokens (§3.15.1.3); full linkdead reattach + idle/liveness (§3.15.2–3.15.3);
  background snapshot (§2.5.3.4); Postgres backend + per-DB roles (§2.5.1.4).
- **M7-F — Hardening.** Synthetic load test proving 10k entities / sub-50 ms
  tick p99 with command-path script p99 reported separately (§2.3.4.3);
  security pass (Lua sandbox fuzzing, §10); performance pass.

---

## M8 — 1.0

**Acceptance (§7.4 M8, §11):** the tutorial world covers city, overworld,
dungeon, ship, LLM innkeeper, shop, auction, combat encounter, and a hot-
reload demo; builder docs, GMCP spec, and release process are published; the
§0.4 "done means" criteria are met.

Epics:

- **M8-A — Tutorial world content** (`games/tutorial`): city rooms,
  overworld region, dungeon, sailable ship, LLM innkeeper + scripted-only
  twin NPC, shop, auction, ledger, d20 combat encounter, hot-reload demo,
  multi-tenant config sample (§11).
- **M8-B — `mud test` suite** passing against the tutorial (§3.10, §11).
- **M8-C — Docs + release:** builder docs, auto-rendered GMCP spec from
  `mud-schema`, contribution guide, release process; verify §0.4 end-to-end
  (builder onboarding < 10 min, LLM innkeeper, client matrix, multi-tenant
  flag flip).

---

## Cross-cutting tracks (advance continuously, not as a milestone)

These are not separated into their own milestone; each PR in them rides with
the milestone that first needs it, but they are called out so they are not
forgotten:

- **Observability** (§3.9, workstream-pervasive): every command, script
  error, and LLM call gets a `tracing` span sharing its `command_id`; the
  Prometheus `/metrics` surface grows with each subsystem (§2.7.1, §3.9).
  Plumbing, not an add-on (§1.6).
- **i18n** (§3.14): the seam ships in M1 (M1-14), Fluent backing in M2
  (M2-I), LLM-locale awareness in M6 (M6-E). Every new engine-emitted player
  string goes through `t!`, never string concatenation (§3.14.4.1).
- **Color/styled output** (§3.20): minimal in M1 (M1-13), tiers + webclient
  spans in M3/M4, per-account prefs + colorblind palette by M7/M8.
- **Testing harness** (`mud-test`, §3.10): the in-memory `mud test` harness
  is built out as content features arrive (commands → M2, NPCs → M5, LLM
  replay → M6) and is the home for all content-feature tests (§8 rule 8).
- **Documentation site** (`docs/`, scaffolded in P0-02): the versioned MkDocs
  site grows as user/builder/operator-facing surface lands. Any PR that adds
  or changes observable behavior (a command, config key, script API, network
  feature, CLI subcommand, deployment knob) updates the relevant page under
  `docs/docs/` in the same PR (cf. `CLAUDE.md` → Documentation site). The
  consolidated builder/GMCP/release docs pass is M8-C.

## Open decisions to resolve before their PRs (§9)

- **Tutorial LLM provider** (Ollama vs. hosted) — before **M6-A**.
- **Vector-store granularity** (per-NPC table vs. global table + id column) —
  pick after a load test, before **M6-D**.
- **Tile size in bytes** (terrain code + flags; default 4) — before **M4-B**.
- **Admin RPC transport** (sibling unix socket vs. new postcard frame class;
  spec leans sibling socket) — before **M7-B**.

These are recorded so the relevant PR opens by resolving them, not by
guessing (§9 "this SPEC does not pre-empt them").
