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

M1 is implemented and merged — PRs **M1-01 → M1-28**, plus the interleaved
review, crate-audit, and logging-instrumentation batches. The per-PR
breadcrumbs live in `.claude/JOURNAL.md`; the code is the source of truth.
What shipped, by area:

- **Core (`mud-core`):** `EntityId`/`TenantTag` + per-tenant generational
  arena; durable `EntityKey`; `Place` (Room) surface; `LocationOf`/`Inventory`
  hot tables; 20 Hz scheduler + `MutationCommand`; locks DSL; styled-text
  model + palette + builder markup; `PlaceKey`/`RegionKey` durable slugs;
  canonical `Direction` contract.
- **Persistence (`mud-db`):** per-tenant SQLite file; write-through
  `PersistentWorld` (arena-as-cache keyed by `EntityKey`); accounts/puppets
  repository; persisted `world_id`.
- **Wire/IPC (`mud-schema`, `mud-ipc`):** directional postcard frames; resume
  handshake; unix-socket + in-memory transports.
- **World (`mud-world`):** KDL room loader; tenant `config.toml`; mandatory
  `region.kdl` manifests; optional `palette.kdl`.
- **Rendering & i18n (`mud-net`, `mud-i18n`):** per-session ANSI renderer
  (ansi16/mono, `NO_COLOR`); `t!` lookup seam over a static `en` catalog.
- **Commands (`mud-cmd`, `mud-engine`):** CmdSet merge + trie parser; command
  pipeline; built-ins (`look`, movement, `say`, `get`/`drop`, `inventory`,
  `who`, `quit`); room presence.
- **Accounts & sessions (`mud-account`, `mud-session`):** argon2id
  credentials; login/register/puppet-select FSM; password echo suppression.
- **Net & runtime (`mud-net`, `mud-gateway`, `mudd`):** telnet core + rate
  limit; gateway library; multi-tenant single-process wiring; tenant catalogue
  + `mudd serve`/`tenant` subcommands; telnet line discipline.

Deferred refinements and known limitations that outlived M1 are tracked as
GitHub issues (milestone **0.1** and later) and are no longer inlined here.

### Remaining M1 work — the acceptance gate

- **M1-23 — M1 acceptance integration test.** Drive two scripted telnet
  sessions through login, movement, mutual visibility, and chat; assert ANSI
  + NAWS; kill and restart the process and assert credentials, location, and
  inventory persisted; run the **cross-tenant handle test** through the full
  World API (not just the arena). Locks parse + evaluate in at least one gated
  command.
  - *Spec:* §7.4 M1. *Verify:* this is the M1 gate — it must pass to claim M1.

---

## M2 — Builders without Rust

**Acceptance (§7.4 M2):** a non-programmer adds an archetype, a custom
component, a Lua command, and a prototype, and **hot-reloads with no restart
and no recompile**. `mud check` catches a broken lock string and a bad hook
signature before load. A non-English `.ftl` bundle is dropped into a
tenant's `i18n/`, hot-reloaded, and a localized engine string renders in the
tenant's configured locale (§3.14.8.1).

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
  changes routed to content migration (§2.4.3.4). Home for an **authored
  display-name** distinct from match keywords (resolves the M1-17 "display name
  = first keyword" limitation, including the silent skip of keyword-less
  entities in listings).
- **M2-E — Script-defined commands and hooks.** Lua `run` functions in the
  pipeline (§2.7 step 7); hook tables keyed by archetype with **static
  surface checking** of hook signatures, lock functions, component accesses,
  and engine-API calls at load time (§2.3.6, §2.4.4).
- **M2-Ea — Full help system (§3.8.2).** Replaces the minimal M1 in-game `help`
  (issue #67) with the full content model: DB-backed help entries plus
  file-loaded entries plus auto-generation from command docstrings, merged across
  engine *and* script-defined commands. `help` with no arguments lists
  categories; `?` stays an alias (§3.19.3); the listing is **lock-aware** so a
  viewer sees only commands they can use — hiding building commands (§3.8.1) and
  script commands gated by locks they lack. Depends on **M2-E** (so script
  commands are enumerable) and on the building/DB surfaces that back the entries.
  *Spec:* §3.8.2, §3.19.3. *Verify:* categories listed; a lock-gated command is
  hidden from a viewer who lacks the lock; DB + file entries merge with
  docstring-generated ones.
- **M2-F — Full archetype loader.** KDL archetype declaration with component
  defaults, hook table, and single-inheritance `extends` (§2.3.5); hook
  resolution statically validated at world load (§2.3.6.2). Introduces the
  item/actor archetype distinction that lets object resolution gate by entity
  kind (resolves the M1-17 "no item/actor distinction" limitation: `get`
  targets items, `look` separates actors from items).
- **M2-Fa — Targeted `look`.** Introduces an authored entity **description**
  (a component on the M2-D tagged-blob mechanism, distinct from the display-name
  and match keywords). `look <target>` resolves an entity present in the
  caller's Place via the object resolver (§2.7 step 5) and renders that entity's
  viewer-conditional description (§2.2.8); an entity without an authored
  description renders a generic fallback, and an unresolved target yields a
  structured "you don't see that here" reply — never a silent room re-render.
  Depends on **M2-D** (component mechanism) and **M2-F** (item/actor
  distinction). Resolves the M1 gap where `look` ignores its argument and always
  renders the caller's room. *Spec:* §2.2.8, §2.7. *Verify:* look-at-present-
  entity renders its description; look-at-absent-target errors; no-arg `look`
  still renders the room.
- **M2-G — Prototypes.** Prototype scripts that return a table; `spawn(...)`
  as a core engine call (§3.7).
- **M2-Ga — Line editor (§3.8.4).** A session-scoped multi-line text editor for
  composing room descriptions, mail bodies, help entries, and prototype
  descriptions. Entry is explicit: a command opens the editor on a target
  buffer, the session FSM enters editor mode, subsequent input lines append
  until a terminator, and the buffer commits to its target through a
  `MutationCommand` (§2.5.3.3). Minimum operations: append, replace line N,
  delete line N, insert before line N, show buffer with line numbers, abort,
  commit. Honors the §3.6.4 content cap. Depends on the session FSM (M1-19) and
  the mutation pipeline (M1). *Deferred:* the identical contract exposed to the
  webclient via GMCP (same `MutationCommand` path, textarea instead of
  line-by-line) lands with the webclient in **M3**. *Spec:* §3.8.4, §3.6.4.
  *Verify:* per-operation editor tests; commit routes through `MutationCommand`;
  over-cap entry rejected.
- **M2-Gb — Building commands (§3.8.1).** The in-world builder toolkit, each
  **lock-gated to builder permissions** (§2.6): `dig`, `create`, `set`,
  `examine`, `link`, `tunnel`, `typeclass`, `copy`, `delete`. Mutations flow
  through the `MutationCommand` pipeline (§2.5.3.3); multi-line description
  fields use the M2-Ga line editor. Depends on the archetype loader (M2-F) for
  `typeclass`/`copy`/`create`, prototypes (M2-G) for `create`/`spawn`, and locks
  (M1) for gating. *Spec:* §3.8.1, §2.6. *Verify:* each command's mutation
  applied and persisted; a non-builder is refused by the lock; round-trip
  `dig`→`link`→`examine`.
- **M2-Gc — Batch processors (§3.8.3).** Offline world construction from
  `.mud` files (sequences of builder commands) and `.lua` files (scripts),
  replaying through the command and script paths. Depends on the building
  commands (M2-Gb) and the Lua host (M2-A). *Spec:* §3.8.3. *Verify:* a `.mud`
  file builds a room graph; a `.lua` file runs under the sandbox; a failing line
  reports its location and halts.
- **M2-H — Hot-reload (drain-before-swap).** File watcher; new calls hit the
  new version while the old drains; atomic per-file reload; failed load keeps
  the previous version live; epoch-versioned userdata handles raise typed
  errors when stale (§2.4.3). **Hot-reload paths get tests** (§8 rule 9).
  Includes **palette hot-reload** (§3.20.3.3): the M1-13a `palette.kdl` loader
  joins the file watcher; a failed reload keeps the previous palette live and
  emits a structured error.
- **M2-I — `mud-i18n` (Fluent).** Replace the M1-14 static `en` table with
  `fluent-rs`; two-source tenant-overriding bundle discovery (§3.14.3.2);
  tenant-scoped loader; hot-reloadable bundles; per-tenant locale selection
  (§3.14.6); `mud.i18n.t` for scripts (§3.14.4.2); localized command aliases
  in the CmdSet merge (§3.14.5.2); load-time verification that every
  `t!`/`mud.i18n.t` key exists in `en` (§3.14.6.2).
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
  reserved `Core.*` handshake messages (`Hello`/`Welcome`/`Ping`/
  `Pong`/`Goodbye`) defined in `mud-schema` first (§2.8.3.3, §8 rule 4),
  including the 5 s default-profile fallback (§2.8.3.4). Includes the
  **deferred §2.1.1 obligation from M1-21**: emit the structured
  `rate_limited` event to the session when the gateway drops a throttled
  command (M1 drops silently — a bare telnet client has no structured
  channel).
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
- **M4-B — Wilderness regions.** The **tile-grid extension** of the Region
  primitive already defined in M1-12a/b (§2.2.7): a Region gains an optional
  terrain layer (ASCII *or* PNG palette), features overlay, encounters layer,
  and region scripts (§3.2.2–3.2.3). Procedural regions (`(x,y)->tile`, lazy +
  cached) and the sparse `tile_overlay` table (§3.2.3.3–3.2.3.4). Region
  *behaviours* not specific to tiles attach as their milestones arrive: PvP
  policy → M5-F (§3.12.6); LLM token budget → M6-C (§3.1.8); ambient/spawn →
  M4/M5. The `RegionKey`/`RegionId` split and folder-manifest authoring are
  reused as-is from M1, not reintroduced.
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
  The Postgres backend PR must also replace M1-22's fail-stop-on-`DbError`
  policy with a retry tier (transient network/connection errors retried with
  backoff in front of fail-stop): fail-stop is the right response to a broken
  local SQLite file, but not to a blip on a networked database.
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
- **Color/styled output** (§3.20): styled-text model + palette + builder markup
  + ANSI renderer in M1 (M1-13); palette hot-reload in M2 (M2-H); xterm256/
  truecolor tiers, TTYPE detection + webclient spans in M3/M4; per-account prefs
  + colorblind palette by M7/M8.
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
