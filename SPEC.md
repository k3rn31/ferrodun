# Ferrodun — Normative Specification

> *Ferrodun* — from **ferrum** (Latin: iron) and **dun** (Old English: dark, fortified hill).
> Iron rusts; hills endure. A MUD engine built in Rust that is meant to last.

## Preamble

This document is the **normative specification** for Ferrodun, a
pure-Rust MUD/MU* engine. It defines testable requirements for a
conformant implementation; where a decision is explicitly deferred,
the SPEC records the open decision verbatim and does not pre-empt it.

### Conformance keywords (RFC 2119 / RFC 8174)

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**,
**SHALL NOT**, **SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**,
and **OPTIONAL** in this document are to be interpreted as described
in RFC 2119 and RFC 8174 when, and only when, they appear in all
capitals.

- **MUST / MUST NOT / REQUIRED / SHALL / SHALL NOT** — absolute
  requirements or prohibitions. A non-conformant implementation is
  not Ferrodun.
- **SHOULD / SHOULD NOT / RECOMMENDED** — strong recommendations.
  Implementations MAY deviate when there is a documented reason; the
  default behavior is the recommended one.
- **MAY / OPTIONAL** — TRuly optional. Conformant implementations are
  free to include or omit the behavior.

### Scope of this document

This SPEC covers:
- Vision, non-goals, and acceptance ("done means") criteria.
- Design principles binding all implementation work.
- The two-process architecture (Gateway and World) and its IPC.
- The `Place` abstraction unifying rooms and tiles.
- The entity / component / archetype model with both hot side-tables
  and a typed component bag.
- The Lua 5.4 scripting host with sandboxing, hot-reload, and the
  `mud` standard library.
- Persistence on SQLite (dev) and PostgreSQL (production) via SQLx.
- The locks DSL.
- The command pipeline, including CmdSet merge semantics.
- Networking and MUD client compatibility (telnet/IAC, MCCP2, GMCP,
  MSDP, MXP, MSSP, TTYPE, NAWS, CHARSET, EOR/GA, TLS, SSH,
  WebSocket).
- The web layer (Axum, admin SPA, modern webclient).
- The LLM dialogue/flavor subsystem and its strict separation from
  mechanical loops.
- Wilderness regions (ASCII overworld), tile grids, FOV, transitions.
- Vehicles as mobile places.
- Combat, economy, communication, prototypes, building, observability,
  testing, multi-tenancy, content migration, and miscellaneous game
  systems.
- The authoring layers ("pyramid").
- The repository layout and the locked tech stack.
- Delivery workstreams and integration milestones M1–M8.
- Ground rules for agents working on the codebase.
- Open questions, risk register, and 1.0 acceptance contents.

### Document conventions

- Type sketches are Rust-flavored pseudo-code; they describe shape and
  intent and are not binding on field names unless the SPEC says so
  explicitly.
- Code blocks shown in examples are normative as examples; they
  illustrate the contract but do not bind file paths.
- "Tenant" means a logical, isolated game instance running inside a
  single `mudd` process; see §3.11.

---

## 0. Vision, differentiators, non-goals, and acceptance

### 0.1 Vision

0.1.1 Ferrodun MUST ship as a single binary that a hobbyist can run
on a laptop and that a hoster can operate at scale.

0.1.2 Content creators MUST work in text files and a small script
language; they MUST NOT be required to write or compile Rust to
build worlds.

0.1.3 The engine MUST ship an opinionated stack that previous MUD
frameworks bolted together over years. The stack covers, at minimum:
networking, persistence, scripting, web admin, modern web client,
and an LLM subsystem.

### 0.2 Differentiators vs. Evennia

The following are first-class engine capabilities. Each is normative.

1. **LLM-augmented NPCs as a first-class subsystem.** Scripted
   behavior trees MUST drive *actions* (move, attack, trade). LLMs
   MUST author *speech, emotes, and flavor*. NPC memory MUST include
   at least dyadic and episodic layers. LLMs MAY propose non-binding
   mood/intent hints that scripts MAY consult. Combat and other
   mechanical loops MUST NOT be gated on an LLM call.
2. **Unified spatial model.** Rooms and tiles MUST share a `Place`
   trait. Wilderness, dungeons, cities, and ship decks MUST
   interoperate. There MUST NOT be a bolted-on grid contrib that
   reimplements core spatial semantics.
3. **Vehicles as mobile places.** Ships, carts, mounts, and caravans
   MUST share one primitive. NPCs (LLM or scripted) MUST be able to
   crew them.
4. **Components definable in scripts.** New stat systems MUST NOT
   require recompiling the engine.
5. **First-class observability.** The engine MUST emit `tracing`
   spans, expose Prometheus metrics, host a live admin dashboard, and
   produce replayable command logs.
6. **Content migration tooling.** When an archetype changes, existing
   entities MUST migrate cleanly via versioned migration scripts.
7. **Multi-tenancy.** One binary MUST be able to host many isolated
   games concurrently.
8. **Type-checked wire protocol.** The wire protocol MUST be defined
   once and code-generated for Rust and TypeScript so that client and
   server cannot drift.

### 0.3 Non-goals (v1)

The following are explicitly **out of scope for 1.0**.

- A graphical 3D client.
- A distributed / sharded world spread across multiple nodes.
  Ferrodun targets a single node and scales vertically.
- Compatibility with legacy MUD codebases (Diku/Circle/ROM data
  files).
- A shipped "default game." 1.0 ships a tutorial world only.
- General-purpose programming in world files. Scripting (Lua) handles
  behavior; world files describe structure.

### 0.4 Done-means (acceptance criteria for 1.0)

A conformant 1.0 release MUST satisfy all four of the following
end-to-end criteria.

1. **Builder onboarding.** A non-programmer MUST be able to build a
   50-room area + NPCs + a shop + an ASCII wilderness region + a
   sailable ship using only world files and small scripts, and run it
   on their laptop in under 10 minutes from `cargo run`.
2. **LLM innkeeper.** An LLM innkeeper MUST remember each player
   across sessions, vary her speech based on prior interactions, and
   refuse to serve someone with low reputation. Refusal logic MUST be
   scripted; the *delivery* of the refusal MUST be LLM-authored. The
   innkeeper MUST be configurable in a world file with a ~30-line
   script.
3. **Client matrix.** Mudlet, TinTin++, MUSHclient, and BlightMud
   MUST connect cleanly with MCCP2 + GMCP + MSDP + MXP working
   (clients that natively prefer MSDP MUST be served via MSDP;
   GMCP-preferring clients via GMCP). The bundled web client MUST
   render the wilderness with tile graphics over the same protocol.
4. **Multi-tenant flag flip.** A second, isolated game instance MUST
   run in the same process with a configuration flag flip.

---

## 1. Design principles

The following principles bind every subsystem in this SPEC. They are
normative and override local convenience.

1.1 **Engine in Rust; content in data + script.** Builders MUST NOT
need a build step. The engine compiles once; content is authored
forever afterward.

1.2 **Database is the source of truth.** Memory caches the database;
memory MUST NOT invert that relationship. A read MAY be served from
memory; a write MUST be visible to the database within the rules of
§2.5 (write-through or background snapshot).

1.3 **Hot-reload is a feature, not a hack.** Scripts and world files
MUST reload without restart. Engine upgrades MUST use the
connection-preserving Gateway/World split (§2.1).

1.4 **Compose, don't branch.** Behavior MUST be layered via
archetypes, components, and hook tables. There MUST NOT be class
hierarchies. There MUST NOT be engine forks per game.

1.5 **Boring tech where possible.** Tokio, SQLx, Axum, mlua,
postcard. Bespoke runtimes MUST NOT be introduced where a mainstream
choice suffices.

1.6 **Observability is plumbing.** Every command, every script error,
every LLM call MUST be structured-logged and metered. Observability
is REQUIRED infrastructure, not an add-on.

1.7 **Type-driven development.** Rust code MUST encode invariants in
the type system so the compiler — not runtime checks — rejects
invalid states. Distinct domain concepts MUST use the newtype pattern
(e.g. `EntityId`, `PlaceId`, `TenantTag`); raw primitives MUST NOT
cross public APIs where a domain meaning exists. Inputs MUST be
parsed into typed domain values at system boundaries (network, KDL,
script bridge, DB); inner code MUST NOT re-validate. This binds the
engine's Rust code; Lua scripts are dynamically typed by construction.

---

## 2. Architecture

### 2.1 Process model

Ferrodun's runtime topology is a two-component split with a versioned
IPC channel between them. The following diagram is normative as a
high-level architectural description.

```
                        unix socket, length-prefixed
                        postcard frames, schema-versioned
                        (gameplay IPC + admin RPC sibling socket)
   ┌──────────────────┐                            ┌─────────────────┐
   │     Gateway      │ ◄──────── IPC ───────────► │      World      │
   │                  │                            │                 │
   │ telnet+IAC       │                            │ entity arena    │
   │ MCCP2/GMCP/      │                            │ command pipeline│
   │ MXP/MSDP/MSSP    │                            │ script host     │
   │ ssh              │                            │ scheduler       │
   │ websocket        │                            │ LLM subsystem   │
   │ session FSM      │                            │ persistence     │
   │                  │                            │ internal admin  │
   │ Axum (public)    │                            │ surface (priv.) │
   │  ├ SPA bundle    │ ◄──── admin RPC ─────────► │  ├ admin APIs   │
   │  ├ /metrics      │       (sibling socket,     │  ├ game REST    │
   │  ├ health/status │        not public)         │  └ game metrics │
   │  └ reverse proxy │                            │                 │
   └──────────────────┘                            └─────────────────┘
```

#### 2.1.1 Gateway responsibilities

The Gateway component MUST:

- Own all client-facing sockets, including telnet (with IAC),
  telnet-over-TLS, SSH, and WebSocket listeners.
- Implement the per-connection protocol state machine, including IAC
  negotiation, MCCP2 compression, capability negotiation (GMCP, MSDP,
  MXP, MSSP, TTYPE, NAWS, CHARSET, EOR/GA), and SSH/TLS termination
  where applicable.
- Maintain the *transport-level* view of each session, including
  GMCP package subscriptions a client has opted into.
- Survive World restarts. On loss of the IPC channel to World, the
  Gateway MUST hold all client TCP connections open, display a
  "reconnecting…" banner (or its equivalent for non-telnet clients),
  and re-handshake with World when it returns.
- Be the **sole public HTTP listener** for the deployment. Gateway
  MUST serve directly: the modern webclient SPA static bundle, a
  Prometheus `/metrics` endpoint, a health/status endpoint, and a
  "World disconnected" status surface. Gateway MUST reverse-proxy
  admin APIs, game REST endpoints, and game-metric collection to
  World's internal admin surface (§2.1.2).
- When the IPC channel to World is down, Gateway-served endpoints
  MUST continue to respond; proxied endpoints MUST return HTTP 503
  with a structured `world_unavailable` body and MUST recover
  automatically on resume handshake (§2.1.3.2).
- The `/metrics` endpoint MUST merge Gateway-local metrics
  (connections, IPC health, protocol negotiations) with the most
  recent successful scrape from World, and MUST expose a `world_up`
  gauge so scrapers can distinguish "World down" from "scrape
  failed."
- The webclient SPA opens a WebSocket back through Gateway like any
  other client.
- Enforce a **per-session command rate limit** as a leaky bucket
  with a default sustained rate of **10 commands/second** and a
  burst of **20**. Excess commands MUST be dropped at Gateway with a
  structured `rate_limited` event to the session and MUST NOT be
  forwarded to World. The limits MUST be configurable per tenant.
  This rate limit is on raw command frames; the LLM-input throttle
  (§3.1.8.2) is a separate, narrower cap on LLM-triggering inputs
  and is enforced in World.

#### 2.1.2 World responsibilities

The World component MUST:

- Own all game state (entity arena, side tables, archetypes,
  components, prototypes), the script host, persistence, and the
  scheduler.
- Own the *logical* session: account, puppet, current Place, and
  per-session script state.
- Persist logical state at natural boundaries — login, location
  change, significant script events — and SHOULD NOT persist on every
  command.
- On restart, World MUST accept that in-flight commands are lost;
  affected players MUST receive a message indicating "your last
  command was interrupted" (or equivalent).
- Expose an **internal admin/RPC surface** carrying admin APIs, game
  REST handlers, and game-metric scrapes. This surface MUST be
  reachable only from Gateway (a sibling unix socket, or a distinct
  frame class on the IPC channel — see §2.1.3.5) and MUST NOT bind a
  public port.

#### 2.1.3 IPC contract

2.1.3.1 The IPC channel between Gateway and World MUST be:
- A unix socket carrying length-prefixed `postcard` frames.
- Versioned with a schema declared in `mud-schema`.
- Multiplexed across sessions by `session_id`.
- Addressed per-World by a `world_id` declared in the resume
  handshake. A single Gateway MAY hold IPC channels to multiple
  Worlds concurrently (see §3.11.5); each channel carries its own
  `session_id` space scoped to its `world_id`.

2.1.3.2 IPC MUST include a **resume handshake** so that a freshly
started World can be re-announced the set of live sessions held by
Gateway. The handshake MUST carry the `world_id`, the schema version,
and the live `session_id` set for that World only. Cross-World
session IDs MUST NOT be conflated.

2.1.3.3 A **single-process mode** combining Gateway and World in one
binary MUST be supported for dev ergonomics and small deployments.
The split mode exists for graceful upgrades and operational isolation.

2.1.3.4 The choice between single-process and split deployment is
exposed via feature flag / runtime configuration on the `mudd`
binary.

2.1.3.5 Admin RPC and metric-scrape traffic between Gateway and World
MUST be carried either on a sibling unix socket or as a distinct frame
class on the IPC channel, addressed separately from
`session_id`-multiplexed gameplay traffic. The resume handshake
(§2.1.3.2) MUST re-establish both classes.

### 2.2 The `Place` trait

The `Place` abstraction is the most consequential design call in the
engine and binds the spatial subsystem.

2.2.1 Every spatial location in Ferrodun MUST be a `Place`. The
`Place` type MUST have exactly two variants in 1.0:

- **Room** — a discrete node with named exits and a hand-authored
  description.
- **Tile** — an `(x, y, z)` cell in a `Region` grid, with
  terrain-driven description and direction-based neighbors governed
  by terrain rules.

2.2.2 A `Place` MUST expose, at minimum:
- A stable identifier.
- The `Region` it belongs to. Every `Place` MUST belong to exactly one
  `Region` (§2.2.7); there is no region-less Place.
- Its current occupants (an iterator over entities present).
- A viewer-conditional description (the same place MAY look different
  to different observers, e.g. for invisibility, lighting, language).
- Its directional neighbors.
- A visibility set (which other Places are observable from here).

2.2.3 Movement, combat range, line-of-sight, pathfinding, perception,
and NPC awareness MUST operate against this shared `Place` surface
with **no special cases per variant**.

2.2.4 Rooms and tiles MUST be freely interconnectable. A city gate
room MUST be able to exit onto an overworld tile, and a tile MUST be
able to exit into a room.

2.2.5 Dispatch on `Place` MUST be static (an enum, not a trait
object). Per-tick code on hot paths MUST NOT pay a virtual-call cost
for `Place` dispatch.

2.2.6 A `Place` MUST have two identities with distinct lifetimes,
mirroring the entity split (§2.3.1.4):
- A **durable** `PlaceKey` — the human-authored slug that names the
  Place in world files. It is the only Place reference that may be
  persisted (e.g. an entity's stored location) or cross a restart, so
  it MUST be stable across the add/remove/rename authoring lifecycle.
- An **ephemeral** `PlaceId` — the in-process handle used on hot
  paths, minted when the world is loaded and valid only for that
  process lifetime. `PlaceId` values MUST NOT be persisted; loading a
  world MAY mint different `PlaceId`s for the same `PlaceKey`.

A Room's title MAY be authored; it is OPTIONAL and distinct from the
viewer-conditional description (§2.2.2).

Type sketch (illustrative):

```rust
enum Place {
    Room(RoomData),
    Tile(TileRef), // (region_id, x, y, z)
}

trait PlaceView {
    fn id(&self) -> PlaceId;
    fn region(&self) -> RegionId;
    fn occupants(&self) -> impl Iterator<Item = EntityId>;
    fn describe(&self, viewer: EntityId) -> Description;
    fn neighbor(&self, dir: Direction) -> Option<PlaceId>;
    fn visible_places(&self) -> impl Iterator<Item = PlaceId>;
}
```

#### 2.2.7 Regions

A **Region** is the single grouping primitive that every `Place` belongs
to. It is the engine's one notion of "area"; there is no separate "zone"
concept. Rooms and tiles alike belong to a Region, and the spatial
subsystem MUST NOT special-case the two.

2.2.7.1 A Region MUST have two identities with distinct lifetimes,
mirroring `Place` (§2.2.6) and entities (§2.3.1.4):
- A **durable** `RegionKey` — the human-authored slug naming the Region.
  It is the only Region reference that may be persisted or cross a
  restart, and MUST be stable across the add/remove/rename authoring
  lifecycle.
- An **ephemeral** `RegionId` — the in-process handle used on hot paths,
  minted when the world is loaded and valid only for that process
  lifetime. `RegionId` values MUST NOT be persisted.

2.2.7.2 A Region is the authoritative scope for area-level concerns. The
engine MUST allow a Region to carry, with each attribute realized by the
milestone that first consumes it:
- **Display and navigation** — a display name (surfaced on entry and
  exported to mapping clients, e.g. GMCP `Room.Area`), and an optional
  recommended level.
- **Policy** — PvP policy (safe by default, §3.12.6) and LLM token-budget
  scope (§3.1.8.1). These are *per-Region* scopes.
- **Ambient and spawn** — region scripts (weather, events), ambient
  cues (music, lighting), and spawn/reset tables.

A Region MAY carry none of these (a bare grouping with identity and a
name is valid). A **wilderness** Region additionally owns a tile grid
(§3.2.2.1); the tile grid is a property a Region MAY have, not the
definition of a Region.

2.2.7.3 **Authoring.** A Region MUST be declared by a manifest (a
`region.kdl` file) at the root of a **subfolder** of the tenant's world
directory. Every `Place` authored anywhere under that subtree belongs to
that Region. Region membership is therefore *folder-confined* but MUST
NOT be derived from the folder's **name**: identity is the manifest's
`RegionKey` slug, so renaming or moving the folder MUST NOT change the
Region's identity. Every `Place` MUST belong to a declared Region: a
`Place` covered by no manifest MUST be rejected at load, and a tenant
that authors any `Place` MUST therefore declare at least one Region. A
`region.kdl` at the world directory's **root** MUST be rejected; that
slot is reserved for a future tenant-wide Region defaults manifest. In
1.0, Regions MUST be flat: a manifest nested under another Region's
folder MUST be rejected at load.

2.2.7.4 **Builder permissions are out of scope for the engine.** Who may
edit which Region's files is delegated to the filesystem and version
control (a Region maps cleanly to one directory subtree, hence one
ownership unit). The engine MUST NOT implement per-Region authoring
permissions.

#### 2.2.8 Looking

2.2.8.1 The engine MUST provide a `look` command. With no argument it
MUST render the caller's current `Place` to the caller: its title (if
authored), the viewer-conditional description (§2.2.2), the available
exits, and the other entities present.

2.2.8.2 `look <target>` MUST resolve `<target>` among the entities
present in the caller's `Place` using the standard object-resolution
rules (§2.7 step 5, including `name.N` and disambiguation) and MUST
render the resolved entity's description. An entity MUST expose a
**viewer-conditional description** mirroring a `Place`'s (§2.2.2), with
authored entity descriptions living in the component/archetype content
home (§2.3.3); an entity without an authored description MUST render a
generic fallback rather than an error. If `<target>` resolves to no
entity present, the engine MUST emit a structured "you don't see that
here" reply and MUST NOT fall back to rendering the room.

### 2.3 Entity / Component / Archetype model

The engine is **not a classical ECS** and **not a Python-style
inheritance system**. It is a hybrid: dense hot-component arrays plus
a typed dynamic component bag, with composition via archetypes and
hook tables.

#### 2.3.1 Entity identity

Entities have **two** identifiers with distinct lifetimes: a durable
**`EntityKey`** that is the persistent identity (§2.3.1.5), and an
ephemeral **`EntityId`** that is the in-memory handle into the arena
cache (§2.3.1.1–2.3.1.4).

2.3.1.1 `EntityId` MUST encode three fields: a **tenant tag**, a
**slot index** into the per-tenant arena, and a **generation
counter** that increments on slot reuse. The slot-index + generation
pair forms the standard generational index that prevents
use-after-free across entity teardown (§2.3.7.3); the tenant tag
scopes the identifier per §3.11.

2.3.1.2 Tenant scoping MUST therefore be baked into the identifier
from day one. Cross-tenant access MUST be caught at API boundaries,
not retrofitted later.

2.3.1.3 `EntityId` MUST be **8 bytes** so that it fits in a single
machine register on 64-bit targets and packs densely in
occupant/inventory iterators on the combat hot path. The bit layout
MUST be: **12 bits tenant tag** (4096 concurrent tenants per
process), **32 bits slot index** (4G slots per tenant; well above
the §2.3.4.1 10k-entity target with headroom for arena churn), and
**20 bits generation counter** (≈1M reuses per slot before
wraparound; wraparound MUST burn the slot rather than recycle into a
collision). The field encoding is an internal `mud-core` implementation
detail: `EntityId` is an **ephemeral in-memory handle** and is never
persisted or sent on the wire (§2.3.1.4), so the layout MAY change
without a version bump.

2.3.1.4 `EntityId` is **ephemeral**: it is valid only within the
lifetime of a single arena instance (§2.5.3.2). It MUST NOT be
persisted, stored in the database, or sent on the wire. Any entity
reference that outlives the arena — on disk, on the wire, in IPC, or in
a correlation log — MUST use the entity's `EntityKey`.

2.3.1.5 Every entity MUST also have an **`EntityKey`**: its durable
identity and its database primary key. An `EntityKey`:
- MUST be unique within a tenant and MUST NOT be reused for the lifetime
  of the database, even after the entity is destroyed;
- MUST be a per-tenant monotonic 64-bit value — tenant scoping comes
  from the per-tenant database (§2.5.1.4) and the routing layer, not
  from bits in the key;
- MUST be stable across cache eviction (§2.5.3.2), World restart, and
  engine upgrade.
A reference held by a client or stored on disk MUST resolve to the same
entity for as long as that entity exists.

2.3.1.6 The arena (§2.5.3.2) is a cache keyed by `EntityKey`. While an
entity is resident the arena MUST maintain a one-to-one mapping between
its `EntityKey` and its current `EntityId`. Loading a non-resident
entity MUST mint a fresh `EntityId` for its existing `EntityKey`;
eviction MUST drop the `EntityId` and the mapping but MUST NOT affect
the `EntityKey`, and a later load MAY mint a different `EntityId` for
the same `EntityKey`.

#### 2.3.2 Entity layout

2.3.2.1 An Entity MUST be the combination of:
- Its `EntityId`.
- A typed component bag keyed by `ComponentId`, storing **two
  representations** under one interface:
  - **Rust-defined components** (§2.3.3.2) live as `Box<dyn
    Component>` trait objects.
  - **Script-defined components** (§2.3.3.2) live as a tagged
    blob (`ComponentId` + serialized payload + schema version)
    with a side-table vtable for typed access from script.
  The bag MUST expose one lookup API regardless of representation;
  callers MUST NOT need to know which storage form was used.
- Dense side-tables for **hot components**.

2.3.2.2 The following components are **hot** and MUST live in dense
`slotmap`-indexed arrays, not the bag:

- `Position`
- `Health`
- `Initiative`
- `LocationOf`
- `Inventory`

2.3.2.3 Hot components are touched on every tick / combat round. The
bag's heap-allocated dynamic dispatch is acceptable for components
such as `Shop` or `LlmFlavor` but MUST NOT be acceptable for combat
hot paths.

2.3.2.4 Adding a new hot component MUST require an engine release
(adding a new dense side-table). The bag MUST handle every other
component without engine changes.

#### 2.3.3 Components

2.3.3.1 A Component is a typed payload (e.g. `Health`, `Wallet`,
`Sails`, `LlmFlavor`).

2.3.3.2 Components MAY be:
- **Rust-defined**, provided by the engine or by plugin crates.
- **Script-defined**, declared in a script with a schema, and stored
  as a tagged blob in the bag.

2.3.3.3 Script-defined components MUST be intentionally slower than
Rust-defined components and MUST be considered unsuitable for hot-path
use. The engine MUST document this difference.

#### 2.3.4 Capacity target

2.3.4.1 The engine MUST target at least **10,000 active entities per
tenant** while maintaining a **sub-50 ms scheduler tick** at the
p99 of the scheduler's own work (entity updates, hot-component
sweeps, message routing). The tick budget MUST be measured excluding
out-of-band script and LLM work, which run on dedicated worker pools
(§2.4.7.1, §3.1.6) and MUST NOT block the scheduler loop.

2.3.4.2 Command-path scripts MUST run on the dedicated script worker
pool (§2.4.7.1), not on the scheduler thread. Script results MUST
be applied to the arena via `MutationCommand` (§2.5.3.3) on the next
scheduler tick that observes them. The 50 ms command-path script
deadline (§2.4.7.2) is a per-script ceiling and is **independent of**
the scheduler tick budget in §2.3.4.1.

2.3.4.3 The capacity target MUST be verified by a synthetic load
test during the operations hardening pass (M7). The load test MUST
report both the scheduler-tick p99 and the command-path script p99
separately.

#### 2.3.5 Archetypes

2.3.5.1 An Archetype is a **named bundle of components with defaults
and a hook table**, declared in a world file.

2.3.5.2 The following KDL block is normative as the canonical
archetype declaration form:

```kdl
archetype "innkeeper" extends "humanoid" {
    components {
        Health { max=40 }
        LlmBrain { persona="innkeepers/garrick.kdl" }
        Shop    { stock="taverns/garrick_stock.kdl" }
    }
    hooks {
        on_speak       "scripts/innkeeper/speak.lua"
        on_combat_turn "scripts/innkeeper/combat.lua"
    }
}
```

2.3.5.3 Archetypes MUST support an `extends` clause for single
inheritance of component defaults and hooks. There MUST NOT be a
multi-parent inheritance / MRO mechanism.

#### 2.3.6 Hooks

2.3.6.1 Hooks MUST dispatch through a script-defined table keyed by
archetype.

2.3.6.2 Hook resolution MUST be statically validated at world-load
time. A missing or signature-mismatched hook MUST prevent the
referencing archetype from loading and MUST emit a structured error.

#### 2.3.7 Lifecycle

2.3.7.1 Entity creation MUST go through an archetype-aware
constructor that:
- Assigns a durable `EntityKey` and, while the entity is resident, an
  `EntityId` with the current tenant tag (§2.3.1).
- Materializes the hot-component side-tables and bag entries
  prescribed by the archetype and any explicit overrides.
- Persists the entity per §2.5.

2.3.7.2 Entity mutation MUST go through `MutationCommand` (§2.5),
which keeps the arena and the DB consistent.

2.3.7.3 Entity teardown MUST release hot-component slots and bag
entries deterministically and invalidate any outstanding handles.

### 2.4 Scripting

#### 2.4.1 Language choice

2.4.1.1 The primary **and only** scripting language for builders MUST
be **Lua 5.4**, embedded via `mlua`.

2.4.1.2 LuaJIT MUST NOT be offered as a "performance opt-in." LuaJIT
is a Lua 5.1 dialect; offering it would silently change language
semantics for builders.

2.4.1.3 An **escape hatch via WASM (wasmtime)** MUST be available for
advanced plugins in any source language (Rust, Zig, AssemblyScript).
WASM is reserved for compute-heavy or polyglot extensions and MUST
NOT be presented as a primary path.

2.4.1.4 Scripting runtimes beyond `mlua` and `wasmtime` MUST NOT be
added. (See §8, rule 6.)

#### 2.4.2 Sandboxing

2.4.2.1 Scripts MUST be sandboxed by default. The following Lua
facilities MUST NOT be reachable from a sandboxed script:
- `io`
- `os`
- `package.loadlib`
- Filesystem access
- Network access

2.4.2.2 Capabilities beyond the sandbox MUST be granted explicitly
per script via a Rust-side allowlist.

#### 2.4.3 Hot reload

2.4.3.1 Scripts MUST be hot-reloadable via **drain-before-swap**:
- A file watcher MUST trigger a reload on change.
- New invocations MUST dispatch to the new version while the old
  version drains in-flight calls.
- Termination of a runaway old call MUST be cooperative: a debug-hook
  deadline MUST raise an error at the next safe instruction boundary.
- The cooperative mechanism MUST be acknowledged as **sufficient for
  normal scripts but unable to interrupt a blocked host call**. Such
  cases MUST be logged and the offending coroutine MUST be abandoned.

2.4.3.2 Userdata handles MUST be versioned per script-load epoch.
Stale handles MUST raise a typed error rather than corrupt state.

2.4.3.3 Reload MUST be atomic per script file. A failed load MUST
keep the previous version live.

2.4.3.4 **Component schemas are immutable across hot-reload.** A
script-defined component's schema (its `ComponentId`, field set, and
field types per §2.3.3.2) MUST NOT change as a side effect of
reloading the script that declared it. In-flight `MutationCommand`s
(§2.5.3.3) queued by the previous script version MUST therefore
remain applicable to the arena and DB without re-validation.
Changing a component schema is a **content migration** (§2.5.4) and
MUST be performed via a versioned migration script on world reload,
not via the in-process hot-reload path. The script host MUST detect
a schema-change attempt during hot-reload, reject the reload (per
§2.4.3.3 the previous version stays live), and emit a structured
error directing the author to the migration flow.

#### 2.4.4 Static surface checking

2.4.4.1 At script load time the engine MUST verify:
- Every hook signature.
- Every referenced lock function.
- Every component access.
- Every engine-API call.

2.4.4.2 Verification failures MUST prevent registration of the
script.

#### 2.4.5 `mud` standard library

2.4.5.1 The engine MUST expose a Rust-implemented `mud` stdlib to
bridge gaps in Lua's small standard library. At minimum it MUST
include:

- `mud.json` — JSON encode/decode.
- `mud.time` — structured time primitives.
- `mud.random` — seeded RNG.
- `mud.tbl` — table helpers including `map`, `filter`, `reduce`.
- String utilities.
- Search and create helpers.
- The entity / component API.

2.4.5.2 Builders SHOULD rarely need to touch raw Lua stdlib.

#### 2.4.6 Custom module loader

2.4.6.1 `require` MUST be reimplemented to resolve only within the
game directory's script tree, watched by `mud-world`.

2.4.6.2 Arbitrary path loading MUST be rejected with a structured
error.

#### 2.4.7 Execution model

2.4.7.1 Scripts MUST run on a dedicated worker pool.

2.4.7.2 Hard deadlines MUST apply, with defaults of:
- 50 ms for command-path scripts.
- 5 s for background scripts.

2.4.7.3 Runaway scripts MUST be terminated under the cooperative
deadline mechanism and MUST emit a structured error event.

2.4.7.4 Lua GC MUST be tuned for low latency: incremental collection
with bounded steps.

#### 2.4.8 Failure modes

- Script load failure: previous version remains; structured error;
  optional admin alert.
- Script runtime error: caller receives an error result; event is
  logged with `command_id`; player MUST see a non-leaky generic
  message unless the script explicitly opts to surface details.
- Deadline exceeded: see §2.4.7; coroutine abandoned if blocked in
  host call.

### 2.5 Persistence

#### 2.5.1 Backends

2.5.1.1 The engine MUST support both:
- **SQLite** (embedded) for development and small games.
- **PostgreSQL** for production (concurrent access, replication).

2.5.1.2 Database access MUST go through **SQLx** with compile-time
checked queries.

2.5.1.3 Schema migrations MUST be managed by `sqlx migrate` and MUST
be versioned in the repository.

2.5.1.4 **Per-tenant DB topology.** Each tenant (§3.11.3) MUST have
its own physically isolated database, not a shared database with a
tenant column. Specifically:
- On **SQLite**, each tenant MUST own a distinct database file
  under the tenant's data directory. Cross-tenant queries are
  therefore impossible by construction.
- On **PostgreSQL**, each tenant MUST own a distinct **database**
  (not merely a distinct schema in a shared database). The
  connection pool MUST be per-tenant. Role-based access MUST be
  configured so that the engine's per-tenant DB role cannot connect
  to another tenant's database. Schema-per-tenant in a shared DB is
  REJECTED because a misconfigured search-path or a `SET ROLE` slip
  leaks across tenants, and pgvector indexes plus content
  migrations are operationally simpler per-database.
- Row-level tenant columns are REJECTED for the same reason: a
  missing `WHERE tenant_id = ?` is a silent data leak.
The tenant tag in `EntityId` (§2.3.1.1) is therefore a
**defense-in-depth** check on top of physical DB isolation, not the
sole tenant boundary.

2.5.1.5 **Place references are persisted by durable key.** A stored
location (or any persisted Place reference) MUST record the Place's
durable `PlaceKey` (§2.2.6), never its ephemeral `PlaceId`. Rooms are
authored content held in memory, not rows, so a persisted location is
a soft reference resolved against the loaded world at boot; a slug
that names no loaded room MUST surface as a structured error, not a
silent relocation.

#### 2.5.2 Vector storage

2.5.2.1 Vector storage MUST be implemented via:
- `sqlite-vec` extension on SQLite.
- `pgvector` on PostgreSQL.

2.5.2.2 Vector storage is used by the LLM memory subsystem (§3.1).

#### 2.5.3 Write model

2.5.3.1 The database MUST be the source of truth (cf. §1.2). Entities
MUST be stored keyed by their `EntityKey` (§2.3.1.5).

2.5.3.2 An in-memory `slotmap` arena MUST cache hot entities, keyed by
`EntityKey`, with an LRU eviction policy. Eviction MUST release the
entity's `EntityId` handle and its `EntityKey`↔`EntityId` mapping
(§2.3.1.6); it MUST NOT delete the entity, whose source of truth is the
database (§2.5.3.1).

2.5.3.3 A **write-through layer** MUST maintain arena/DB consistency:
every mutation MUST go through a `MutationCommand`. The layer applies
each command to the arena and, if and only if the arena accepts it,
performs the corresponding durable write before the next command
applies; the durable write for one command MUST be atomic, and a
command the arena rejects MUST NOT reach the database. The database is
the sole source of truth; the arena is a cache rebuilt from it at boot.
On a failed durable write the engine MUST stop applying further
mutations and terminate (fail-stop): restart-and-rebuild is the
divergence recovery mechanism. (Entity creation MAY be database-first
where the database allocates the durable identity.)

2.5.3.4 The engine MUST perform a background snapshot every N seconds
for crash recovery beyond the SQLite/PG WAL. `N` is configurable.

2.5.3.5 **Concurrent mutation semantics.** `MutationCommand`s
targeting an entity MUST be **serialized per entity** by the
scheduler: at most one `MutationCommand` against a given `EntityId`
is in flight at a time, and they apply in scheduler arrival order.
Within a single scheduler tick, mutations against the same entity
MUST be applied in arrival order; the *last writer wins* on
conflicting field updates, but no field MUST ever observe a
torn write. Mutations against *different* entities MAY proceed in
parallel. Composite operations that must read-then-write
atomically (e.g. "take the rusty sword if it is still here") MUST
be expressed as a single `MutationCommand` carrying both the
precondition and the effect; the scheduler MUST evaluate the
precondition against the arena at apply time and MUST emit a
structured `precondition_failed` event if it fails, rather than
applying a partial effect. Scripts MUST NOT assume that a read
performed outside a `MutationCommand` is still valid by the time
their write applies. This is the engine's only concurrency contract;
there is no optimistic-CAS API.

#### 2.5.4 Content migrations

2.5.4.1 When an archetype's component set changes, a Lua migration
script MUST run over existing entities of that archetype on world
load.

2.5.4.2 Content migrations MUST be versioned alongside schema
migrations.

2.5.4.3 A **dry-run mode** MUST be available that reports the
proposed changes without writing.

### 2.6 Locks and permissions

#### 2.6.1 DSL

2.6.1.1 The engine MUST keep Evennia's DSL surface for locks — it is
familiar and battle-tested.

2.6.1.2 The following examples are normative as DSL shape:

```
get:perm(player) and not attr(cursed)
edit:self() or perm(admin)
helm:tag(crew) and not status(drunk)
```

#### 2.6.2 Implementation requirements

2.6.2.1 The DSL MUST be parsed to a typed AST using `chumsky`.

2.6.2.2 Evaluation MUST be via a static dispatch table — no string
matching at evaluation time.

2.6.2.3 The `mud check` CLI MUST statically validate lock strings at
world-load time against:
- Known lock functions.
- Declared permission names.

2.6.2.4 Tag references are necessarily runtime (tags are applied
dynamically by scripts). `mud check` MUST warn — but MUST NOT error
— on tag names that no archetype or script ever produces. The warning
is a lint, not a load failure.

2.6.2.5 Locks MAY be defined inline in scripts via a typed builder
for hot paths. The typed builder MUST compose only over the same
known lock-function and permission-name registries used by §2.6.2.3.
Builder-constructed locks are therefore statically valid by
construction (a misnamed function or permission is a Lua error at
script-load time, surfaced by §2.4.4); `mud check` MUST report the
inline-builder call sites it discovered alongside the world-load lock
audit.

#### 2.6.3 LSP (deferred)

2.6.3.1 An LSP server providing autocomplete on lock functions and
permissions, hover docs, and jump-to-definition is **post-1.0**. It
MUST be tracked separately from the 1.0 milestone.

### 2.7 Command pipeline

The command pipeline MUST follow the eight steps below in order:

1. Player input arrives at Gateway and is decoded from
   telnet / IAC / MCCP2.
2. Gateway forwards a `SessionInput { session_id, line }` frame over
   IPC to World.
3. World resolves `session → account → puppet entity → location
   stack`.
4. **CmdSet merge**: World MUST collect commands from account, puppet,
   location, containers, and channels. Merge rules MUST be **Union /
   Replace / Remove**, matching Evennia's semantics; that model works.
   When two sources contribute the same command name and the merge
   rule does not itself resolve the collision (i.e. both are Union),
   precedence MUST follow this fixed order, highest first: **account
   → puppet → containers (innermost first) → location → channels**.
   The intent is that the player's own bindings beat anything the
   world layers on, that containers the player is *inside* beat the
   surrounding room, and that channel-contributed commands (§3.6.1)
   never shadow a more local command. Replace and Remove operate
   regardless of precedence — they are explicit overrides authored
   against a known command. Localized command aliases (§3.14.5.2)
   participate in the merge at the precedence of the source that
   contributed them.
5. Parse: trie lookup with prefix matching, aliases, and switches.
   **Object disambiguation** on command arguments MUST follow a
   fixed rule. Candidate objects are gathered from the caller's
   inventory, current Place occupants, and exits, deduplicated by
   `EntityId`. Matching is by alias (§3.7) with case-insensitive
   prefix match. When multiple candidates match a single token, the
   parser MUST accept the **ordinal suffix** form `name.N` (1-based,
   e.g. `get sword.2`), MUST accept the keyword `all` to apply to
   every match (e.g. `get all sword`), and otherwise MUST emit a
   numbered disambiguation prompt listing the candidates in the
   gather order above (inventory before room before exits). The
   prompt MUST NOT block the session FSM: it MUST be a one-shot
   message and the next command from the player MUST be re-parsed
   fresh, optionally referencing the candidates by their ordinal.
   Cross-room or cross-region targeting MUST require an explicit
   command, never implicit fuzzy matching.
6. **Lock check** against the caller, which MAY be a player **or** an
   NPC (see §3.1).
7. Dispatch: the command's `run` function executes. It MAY be:
   - Rust-native (built-ins), or
   - Lua-defined.
   If the command targets an LLM-augmented NPC (§3.1), dispatch MUST
   fork: the scripted behavior runs synchronously on the command path
   and a dialogue request is queued in parallel onto the LLM
   subsystem per §3.1.6. The command MUST complete on the scripted
   result; the LLM response MUST arrive later as an asynchronous
   `say` / `emote` event addressed to the same `command_id`.
8. Output rendering MUST be per session, honoring color capabilities,
   screen width from NAWS, and GMCP for structured data. Output is
   routed back to the player via Gateway.

2.7.1 Every command run MUST be traced with a `command_id`. Failures,
lock denials, script errors, and LLM tool calls MUST share this
`command_id` for log correlation.

### 2.8 Networking and MUD client compatibility

#### 2.8.1 Hard requirement

2.8.1.1 Existing MUD clients MUST "just work." This is a hard
requirement, not a stretch goal.

#### 2.8.2 Gateway protocol support matrix

The Gateway MUST support:

- **Telnet** with full IAC negotiation, including:
  - **MCCP2** (compression) — critical for large map data.
  - **GMCP** — structured out-of-band data (map, vitals, room
    contents). The engine MUST define its own GMCP message
    namespace, MUST document it, and MUST version it.
  - **MSDP** — alternative structured out-of-band channel for clients
    preferring it.
  - **MXP** — clickable links and styled output.
  - **MSSP** — server status for crawlers.
  - **TTYPE** — client identification.
  - **NAWS** — screen size; drives pagination and wilderness viewport
    sizing.
  - **CHARSET** — UTF-8 negotiation, with fallback transliteration
    for legacy clients.
  - **EOR / GA** — prompt framing. Every output block MUST be followed
    by one EOR/GA prompt frame. The Gateway MUST precede every output
    block with a blank line, MUST terminate a completed message block
    with CRLF, and MUST leave an input-prompt block (e.g. `Password:`)
    unterminated so the cursor rests on the prompt line.
  - **ECHO** (RFC 857) — server-claimed around password entry to suppress
    the client's local echo; released as soon as the secret line is
    consumed. The server never echoes normal input.
- **Telnet over TLS** on a separate port.
- **SSH** via `russh`. Account authentication via SSH keys MUST be
  available, and SSH support MAY be optional per deployment.
- **WebSocket** via `tokio-tungstenite`, carrying the same logical
  protocol with a JSON/CBOR envelope for the modern webclient.

#### 2.8.3 Wire protocol (`mud-schema`)

2.8.3.1 The structured wire protocol (map, vitals, NPC actions,
combat events) MUST be defined once in `mud-schema` and code-generated
to:
- Rust types used by World and Gateway.
- TypeScript types used by the webclient.
- GMCP message documentation auto-rendered for client plugin authors.

2.8.3.2 Generated code MUST NOT be hand-edited (cf. §8 rule 4).

2.8.3.3 The wire protocol MUST reserve a `Core.*` GMCP namespace for
the session handshake and session-scope metadata. The following
messages are normative for 1.0; their full schemas live in
`mud-schema`:
- `Core.Hello` (client → server, REQUIRED on connect within 5 s of
  IAC negotiation completing): carries client identification, the
  set of supported wire protocol major versions (§2.8.5.3), and a
  capability bitset (MCCP2, GMCP, MSDP, MXP, MSSP, TTYPE, NAWS,
  CHARSET, EOR/GA support — already negotiated via IAC but
  re-declared here for non-telnet transports).
- `Core.Welcome` (server → client, REQUIRED response): carries the
  selected major version, the server's package list, the
  `session_id`, and the tenant's locale (§3.14.6.1). The locale is
  fixed for the world; the client cannot switch it.
- `Core.Ping` / `Core.Pong` (REQUIRED on both sides): liveness on
  idle connections; see §3.15.3.
- `Core.Goodbye` (either direction, OPTIONAL): graceful close with
  a structured reason code. Absent close MUST be treated as
  linkdead (§3.15.2).

2.8.3.4 If `Core.Hello` does not arrive within 5 s of negotiation
completing, the server MUST assume the **default profile**: wire
protocol major version 1 and no GMCP packages subscribed. The
tenant's locale (§3.14.6.1) applies regardless — locale is not part
of the client profile. The connection MUST proceed; missing
`Core.Hello` MUST NOT be a disconnect.

#### 2.8.4 Reference client matrix

2.8.4.1 Mudlet, TinTin++, MUSHclient, and BlightMud MUST be tested in
CI via headless client harnesses where possible.

2.8.4.2 Real client sessions against each of the above MUST be
manually verified before each release.

#### 2.8.5 Wire protocol compatibility policy

2.8.5.1 Within a major version, the wire protocol MUST be
**additive-only**:
- New messages and fields MAY be added.
- Renames, removals, and semantic changes MUST NOT occur.

2.8.5.2 Every message MUST carry a `schema_version`.

2.8.5.3 Clients MUST announce their supported versions on connect via
a GMCP `Core.Hello` message. The server MUST select the highest
mutually supported version per session.

2.8.5.4 Deprecated fields MUST be retained for one full major version
with a `deprecated_in` annotation surfaced in generated documentation.

2.8.5.5 A breaking change MUST require a major version bump and a
documented client migration guide.

2.8.5.6 Old clients MUST be able to continue connecting at the prior
major version until the server explicitly drops it; the server MUST
NOT drop a major version sooner than **6 months** after its
successor.

2.8.5.7 **Clients and servers MUST ignore unknown fields** on any
wire message within a major version. This is the consumer-side
counterpart of the additive-only producer rule (§2.8.5.1) and is
what makes additive growth safe: if a future version adds `tool_calls`
to an LLM dialogue frame (§3.9.3), or a new field to a map frame, an
older client MUST decode the frame successfully and silently drop
the field rather than erroring. Generated decoders (Rust and
TypeScript, §2.8.3.1) MUST be configured to permit and discard
unknown fields. This rule applies to JSON/CBOR-framed traffic, GMCP
payloads, and MSDP payloads. It does NOT apply to `postcard` IPC
frames (§2.1.3.1), which are version-locked to the schema declared
in `mud-schema` at build time.

### 2.9 Web layer

#### 2.9.1 Foundations

2.9.1.1 The web layer MUST be built on **Axum** and MUST use
**utoipa** to generate OpenAPI documentation for REST endpoints.

2.9.1.2 The web layer MUST be split across Gateway and World per
§2.1.1 and §2.1.2. Gateway owns the public HTTP listener, both SPA
bundles (the modern webclient and the admin SPA), `/metrics`,
health/status, and reverse-proxying. Serving the admin SPA's static
bundle from Gateway is what allows the dashboard to load during a
World restart (§7.4, M7); only its proxied API/WS calls return 503
until the resume handshake. World owns handlers that require game
state (admin APIs, game REST, game metrics) behind its internal
admin surface. The `mud-web` crate is a library linked into both
binaries.

#### 2.9.2 Admin SPA

2.9.2.1 The admin SPA MUST be implemented in **Svelte with
TypeScript**, the same stack as the modern webclient (§2.9.3). A
single frontend toolchain MUST cover both SPAs so that
`mud-schema`-generated TypeScript types, the GMCP/WS client, theming,
and shared UI primitives are reused across them.

2.9.2.2 The admin SPA MUST include, at minimum:
- An entity browser.
- A live command log.
- A script editor with a reload button.
- An LLM call inspector.
- Metrics dashboards.

2.9.2.3 **Admin authentication and authorization.** Access to the
admin SPA, admin REST endpoints, and any game-state-mutating admin
RPC MUST be gated by an explicit `perm(admin)` permission on the
caller's account. Admin auth MUST reuse the player account system —
there MUST NOT be a separate parallel credential store — but the
`admin` permission MUST be grantable only by another admin or by a
documented bootstrap mechanism (e.g. a CLI subcommand running with
local DB access). Sessions to the admin SPA MUST authenticate over
the same account login flow used by players, and the SPA MUST
present an admin-permission token (or session cookie) that Gateway
forwards on the proxied admin RPC channel (§2.1.3.5) so that World
can re-check `perm(admin)` on every admin call. **Authorization MUST
be enforced in World**, not in Gateway; Gateway MAY short-circuit
unauthenticated requests but MUST NOT be the sole authority. Failed
admin auth MUST emit a structured `tracing` event (§3.9.1) and MUST
be rate-limited at Gateway. Per-tenant admin scoping MUST be
respected: an admin in tenant A MUST NOT receive an admin handle in
tenant B (§3.11.4).

#### 2.9.3 Modern webclient

2.9.3.1 The webclient MUST be built fresh (not a port of a legacy
MUD web client).

2.9.3.2 The webclient MUST:
- Use Svelte with TypeScript.
- Be mobile-first, themeable, and accessible.
- Include a tile-graphics renderer for the wilderness. Tile graphics
  MAY be optional and MUST fall back to ASCII glyphs.
- Be GMCP / structured-data aware: vitals, map, room panel, inventory
  panel, NPC dialogue UI.
- Expose a documented, stable plugin API.

2.9.3.3 **Tile asset pipeline.** Tile sprites for the webclient
MUST live in a per-tenant `assets/tiles/` directory and MUST be
mapped to terrain codes (§3.2.2) by a `tiles.kdl` manifest at the
tenant root. Sprites MUST be PNG. The webclient MUST fetch the
manifest at handshake time over the same Gateway HTTP listener
(§2.1.1) and MUST cache sprites keyed by a content hash carried in
the manifest. Adding or replacing a sprite MUST be a hot-reload
operation: the file watcher (§2.4.3) MUST detect the change, the
manifest's content hash MUST update, and a `Core.AssetsChanged`
message MUST be pushed to connected webclients so they re-fetch.
The manifest schema lives in `mud-schema`.

---

## 3. Subsystems

### 3.1 LLM-augmented NPCs

The LLM subsystem (`mud-llm`) MUST be a first-class engine subsystem,
not a contrib.

#### 3.1.1 Scope

3.1.1.1 The LLM subsystem's scope MUST be **deliberately narrow**:
LLMs author speech and flavor; scripts decide actions.

3.1.1.2 Game mechanics — combat, movement, trade, locks, reputation —
MUST NOT be gated on an LLM call.

#### 3.1.2 The action / speech split

3.1.2.1 A **scripted behavior tree** MUST decide what the NPC *does*:
when to attack, when to flee, when to refuse service, where to walk,
what to trade. This layer:
- MUST be authoritative.
- MUST always run.
- MUST NEVER block on the network.

3.1.2.2 The **LLM dialogue/flavor layer** decides what the NPC *says
and how it emotes*. It:
- MUST receive the script's decision as context (e.g. "you have
  decided to refuse this customer").
- MUST produce in-character speech and emotes.
- MAY produce non-binding mood hints (e.g. `{annoyed: 0.7}`) that the
  script MAY read on the next tick. Mood hints MUST be exposed to
  scripts as **last-known-value** with an archetype-declared default
  used until the first hint arrives; a read of a mood hint MUST
  return immediately and MUST NOT block on, await, or otherwise be
  influenced by the in-flight LLM call. If the LLM is slow, failed,
  budget-exhausted, or has never produced a hint for this NPC, the
  declared default value MUST be returned. This preserves §3.1.2.3:
  no behavior-tree path — including paths that consult mood hints —
  may couple combat or other mechanical loops to LLM latency.

3.1.2.3 Consequently, combat rounds, shop transactions, and movement
loops MUST run at engine speed regardless of LLM latency or
availability. Specifically, the combat round scheduler MUST NOT
`await` any LLM future; LLM responses MUST be delivered
asynchronously via the dialogue channel.

#### 3.1.3 Memory layers

3.1.3.1 The LLM subsystem MUST assemble prompts from six memory
layers in the following priority order:

1. **System / persona** — static: name, role, voice, hard rules (e.g.
   "never reveal you are AI"). Authored in world files.
2. **World facts slice** — current Place description, present
   entities, time of day, weather, recent room events. Cheap, MUST
   always be included.
3. **Dyadic memory** — per `(npc, player)` rolling summary plus
   recent verbatim turns. "Remembers you."
4. **Episodic memory** — vector-indexed log of significant past
   events (gifts received, quest beats, scripted-flag changes),
   retrieved by similarity to the current turn.
5. **Reputation scalars** — cheap numeric `trust / fear / affection`
   per player, maintained by scripts. MUST be read into the prompt
   as context and MUST also be read by the behavior tree for action
   decisions. This is the inter-subsystem contract between scripts
   and the LLM: scripts write reputation scalars; the LLM subsystem
   reads them at prompt-assembly time.
6. **Shared world-event log** — a tagged, per-tenant, append-only log
   of significant public events (e.g. "Bran killed the guard captain
   at noon"). NPCs MUST subscribe by tag / place; entries surface in
   context when relevant. Info flow is scoped (NPCs only see entries
   from places/topics they care about) and the mechanism is a simple
   tagged feed, not a cross-NPC summarizer.

#### 3.1.4 Multi-party in-room

3.1.4.1 When multiple players are present in the same Place, the LLM
context MUST include a participant table and dyadic memory for every
present player.

3.1.4.2 The system prompt MUST instruct the LLM to address
participants by name.

#### 3.1.5 No tool use

3.1.5.1 The LLM MUST NOT use tools. Its output MUST be constrained to
speech and emote tokens — a small typed JSON shape of the form:

```
{ say?, emote?, to?, mood_hint? }
```

3.1.5.2 The output MUST be validated server-side. Anything not
matching the typed shape MUST be rejected.

3.1.5.3 Mechanical actions remain the behavior tree's exclusive
responsibility.

#### 3.1.6 Asynchronous dialogue generation

3.1.6.1 On player input directed at an LLM NPC, the engine MUST:
- Trigger the script first. The script MAY, for example, open a shop
  menu, refuse service, or queue an attack.
- In parallel, queue the dialogue request.

3.1.6.2 When the dialogue response returns, the resulting speech /
emote MUST be delivered as a normal `say` / `emote` event from the
NPC.

3.1.6.3 If the LLM times out or the budget is exhausted, the
engine MUST use the script's **fallback line table** — a
per-archetype keyed phrase set including, at minimum: `greeting`,
`refused_service`, `combat_taunt`. Additional keys MAY be defined per
archetype.

3.1.6.3.1 Fallback line tables MUST be authored **in KDL** under the
archetype's world directory, alongside the archetype declaration
(§2.3.5.2). They are static content; authoring them in Lua is
REJECTED because they are pure data, not behavior. The following
KDL block is normative as the canonical fallback-table form:

```kdl
fallback_lines archetype="innkeeper" locale="en" {
    greeting        "Aye, what'll it be?"
    refused_service "I'll not serve your kind here."
    combat_taunt    "You'll regret crossing me." "Come on then!"
}
```

A key whose value is a list of strings MUST be interpreted as a
phrase pool; the engine MUST pick from it via `mud.random`
(§2.4.5.1). Per §3.14.7.2 each `(archetype, locale)` pair is a
distinct table; the `en` table MUST be present and MUST contain
every key referenced by the archetype's behavior, and non-`en`
tables MAY be incomplete and fall back per §3.14.4.3. The
declared mood-hint default (§3.1.2.2) MUST also live in this
block as a `mood_default { ... }` child node so that defaults and
fallback lines are co-located with the archetype that uses them.

#### 3.1.7 Combat interaction

3.1.7.1 The behavior tree MUST pick the attack, target, and ability
deterministically every round.

3.1.7.2 A flavor LLM call MAY fire at most once every N rounds per
NPC (configurable, default N=3) to produce a taunt or grunt.

3.1.7.3 This flavor call MUST NOT block the round and MUST be
skippable under load.

#### 3.1.8 Budgets and guardrails

Budgets and guardrails MUST be enforced on the Rust side, never
delegated to script-only enforcement.

3.1.8.1 The engine MUST enforce token budgets at the following scopes:
- Per NPC.
- Per region (§2.2.7).
- Per tenant.
- Per server.

3.1.8.2 A **per-player throttle** MUST apply to LLM-triggering
inputs. The default is **1 dialogue request per player per 2 s**;
excess inputs MUST collapse to a single request carrying the latest
input.

3.1.8.3 The soft deadline for dialogue MUST default to **3 s**.
Beyond the deadline, the fallback line MUST fire.

3.1.8.4 Memory size MUST be capped with summarization-based eviction.

3.1.8.5 A **replay / deterministic mode** MUST be available for
testing with recorded responses.

#### 3.1.9 Provider abstraction

3.1.9.1 The subsystem MUST abstract over at least the following
providers, and MUST allow per-NPC selection:
- Anthropic.
- OpenAI.
- Google.
- Local Ollama.

3.1.9.2 Streaming MUST be supported via SSE; partial responses MUST
render word-by-word in the client.

#### 3.1.10 Observability

3.1.10.1 Every LLM call MUST emit a tracing span recording at minimum:
- Prompt length.
- Tokens used.
- Latency.
- Whether a fallback was triggered.
- Whether the throttle hit.

3.1.10.2 A minimal call inspector (structured log stream + admin
endpoint) MUST land with the LLM workstream itself. The richer admin
UI MAY follow with the web workstream.

#### 3.1.11 Failure modes

- Provider unavailable: fallback line table; logged; metric
  incremented.
- Output schema violation: response rejected; fallback line; logged.
- Budget exhausted: dialogue requests dropped to fallback;
  player-visible behavior MUST still be playable.

### 3.2 Wilderness — ASCII overworld

3.2.1 The wilderness MUST be a **core** subsystem, not a contrib.
Bolting it on creates seams; the `Place` abstraction in §2.2 is the
unified primitive.

#### 3.2.2 Region structure

3.2.2.0 **Coordinate system.** Tile coordinates `(x, y, z)` MUST be
signed 32-bit integers. The origin `(0, 0, 0)` is region-local and
its world meaning is region-defined; tile distances and movement
deltas MUST NOT cross regions without an explicit transition
(§3.2.5). `x` increases eastward and `y` increases northward; the
"north / east / south / west" direction map (§2.2.2) is fixed to
this convention. `z` is **floor-stacking**, not altitude: `z+1` is
"the floor above," and movement between z-layers MUST be authored
as an explicit vertical exit (stairs, ladder, shaft). Airships and
similar (§3.3.4.3) MUST model altitude as a component on the
vehicle, not as a z-coordinate. The maximum region extent MUST be
`±2^31 - 1` per axis; procedural regions (§3.2.3.3) MAY use the
full range, hand-authored regions SHOULD stay well inside it.

3.2.2.1 A **wilderness Region** is a Region (§2.2.7) that additionally
owns a 2D (optionally 3D z-layer) tile grid with:
- A **terrain layer** — forest / road / water / mountain / etc.
  encoded as bytes.
- A **features overlay** — cities, dungeons, landmarks; transitions
  to Rooms.
- An **encounters layer** — spawn tables per terrain.
- **Region scripts** — region-level events (weather, ambushes).

The tile grid is a property a Region MAY have, not the definition of a
Region (§2.2.7.2): a Region of rooms carries identity, name, and policy
without a grid.

#### 3.2.3 Authoring

3.2.3.1 Terrain maps MUST be authorable as ASCII art **or** as a PNG
with a palette mapping to terrain codes. Both MUST be supported.

3.2.3.2 Features and encounters MUST be authored in KDL referencing
the terrain map.

3.2.3.3 **Procedural regions** MUST be supported: a script returning
`(x, y) -> tile`, called lazily and cached. This makes infinite
wilderness possible without a 10M-row table.

3.2.3.4 **Sparse overlay**: any modification to a procedural tile —
tree chopped, road built, item dropped — MUST be stored as a row in
a `tile_overlay` table keyed by `(region, x, y)`. Reads MUST be
`procedural(x, y)` merged with the overlay. Entity placements MUST
use the standard entity store with a `Position` component; only
terrain *edits* and tile-attached state hit the overlay table.

#### 3.2.4 Rendering per player

3.2.4.1 The engine MUST render a viewport window centered on the
player's tile, sized from NAWS.

3.2.4.2 Rendering MUST combine terrain glyphs plus entity glyphs with
ANSI color (or 24-bit color if negotiated).

3.2.4.3 FOV and fog-of-war MUST be handled engine-side. The same FOV
rules MUST apply to NPC perception.

3.2.4.4 Structured map data MUST be sent via GMCP for clients that
render tile graphics. The webclient MUST render tile graphics by
default.

#### 3.2.5 Transitions

3.2.5.1 A tile MUST be able to host a doorway to a Room (city gate →
city interior).

3.2.5.2 A room exit MUST be able to land on a tile.

3.2.5.3 A single `move()` primitive MUST underlie both. The player
MUST NOT need to know or care which variant they are entering.

### 3.3 Vehicles

3.3.1 Vehicles MUST be generalized to **mobile containers that are
also places**.

3.3.2 A ship is simultaneously:
- An **entity** with a position in the overworld (water tiles).
- A **place** with its own interior (rooms: cabin / deck / hold, or
  its own tile grid for large decks).
- A **vehicle** with controls (helm, sails) that affect overworld
  position.

3.3.3 One primitive MUST cover cart, river boat, ocean ship, airship,
train, dragon-mount, and party caravan.

#### 3.3.4 Mechanics

3.3.4.1 **Movement coupling.** When a vehicle moves on the overworld,
its occupants MUST move with it. The engine MUST emit ambience to
passengers (e.g. "the deck rocks; the coastline drifts past"). The
helm MUST see the overworld view; below-decks MUST see only the ship
interior.

3.3.4.2 **Controls as commands on a control entity.** A helm MUST
expose commands such as `steer <direction>`, locked to whoever is at
the helm. The same pattern MUST apply to sails, oars, and engines.
NPCs MAY crew via their scripted behavior tree. An LLM-augmented
crew member MAY *narrate* the work (e.g. "Garrick spits over the
rail and hauls the mainsheet") but MUST NEVER *decide* it.

3.3.4.3 **Terrain predicates per vehicle.** Ships MUST move on water,
carts on roads, airships MUST ignore terrain (consuming fuel). Each
vehicle MUST declare its movement predicate.

3.3.4.4 **Boarding and disembarking.** Special exits MUST appear when
adjacent terrain or another vehicle is in range. Docking MUST create
a temporary exit between ship deck and dock tile.

3.3.4.5 **Combat at sea.** Vehicles MUST be able to be combatants.
Ships MUST have HP; crews MUST have HP. Cannons MUST be tools on
deck; targeting MUST use the same combat system, with range derived
from tile distance.

3.3.4.6 **Persistence.** Vehicles MUST save position, contents, and
crew. A ship that logs out MUST reappear with its cargo and
offline-stashed passengers.

3.3.5 Parties MUST be modeled as degenerate vehicles: mobile
container, no controls, follows a leader. Mounts MUST be modeled as
one-seat vehicles.

#### 3.3.6 Nesting restriction

3.3.6.1 **Nested vehicles MUST NOT be supported in v1.** A vehicle's
occupants MUST be non-vehicle entities.

3.3.6.2 Boarding a vehicle while piloting another MUST be rejected
with a clear error.

3.3.6.3 Lifting this restriction requires transitive movement
coupling and recursive "leave vehicle" semantics, which are out of
scope for 1.0. The decision to support nesting in a later version is
deferred.

### 3.4 Combat

3.4.1 The combat subsystem MUST be **modular, not opinionated**. The
engine provides primitives; rules live in scripts.

3.4.2 The engine MUST provide:
- Initiative / round scheduler.
- Damage type plus resistance matrix.
- Status effects with duration plus tick hooks.
- Range bands (melee, near, far) computed from `Place` distance.
- Hooks: `on_attack`, `on_damage_taken`, `on_death`,
  `on_round_start`.

3.4.3 Reference rule sets MUST be shipped: D&D-flavored, basic d20,
classless attribute-only. Builders MUST be free to pick a reference
set or write their own.

3.4.4 **Death primitives.** Whether death is permanent, penalized,
or trivial is a per-game choice and MUST be scripted. The engine
MUST provide only the primitives:
- The `on_death` hook (§3.4.2) fires when `Health` reaches zero or
  the entity is explicitly killed via `MutationCommand`.
- A **`Corpse` archetype** MUST ship in `mud-core` as the default
  `on_death` outcome: the deceased entity's `Inventory` is
  transferred to a freshly spawned corpse entity placed in the
  same `Place`, with a decay timer (default 10 minutes, scriptable)
  after which the corpse and any remaining contents are destroyed.
  Scripts MAY override this in their own `on_death` hook.
- A **`RespawnPoint` component** MAY be attached to any `Place`
  marking it as a valid respawn destination. A `Respawnable`
  component on a puppet MUST reference a respawn-point selection
  strategy (nearest, fixed, scripted); the engine resolves it and
  emits a `MutationCommand` repositioning the puppet at the
  selected `Place`.
- Whether players respawn at all, lose XP, drop items, or
  perma-die MUST be implemented in script over these primitives.
The engine MUST NOT make death an irreversible state machine; an
admin command MUST be able to revive any entity (§2.9.2.3).

### 3.5 Economy

3.5.1 The engine MUST provide the following components:
- `Wallet`
- `PriceTag`
- `Shop`
- `MarketOrder`
- `Ledger`

3.5.2 Per-shop stock files MUST be authorable in KDL with restock
scripts in Lua.

3.5.3 Auction-house and player-market primitives MUST be included;
their rules MUST be scriptable.

3.5.4 The ledger MUST be journaled: every transaction MUST be
durable, auditable, and surfaced in the admin UI.

3.5.5 **Currency model.** Currencies MUST be modeled as **tagged
integer balances** in `Wallet` and `Ledger`. A `Currency` is a
named tag (e.g. `gold`, `silver`, `copper`, `credits`); a balance
is a signed 64-bit integer in the currency's smallest unit. The
engine MUST NOT bake in a specific currency set, denomination
hierarchy, or fantasy/sci-fi flavor — currencies and their tags
MUST be declared in a tenant's world files. Conversion between
currencies MUST be **scripted**, not engine-resolved: a game that
wants 1 gold = 100 silver authors a Lua conversion script invoked
by shop and ledger code. Floating-point currency is REJECTED;
fractional units MUST be modeled by choosing a smaller smallest
unit (e.g. cents, not dollars). Negative balances MUST be allowed
(debt is a game design choice) but `Wallet` mutations MAY be
locked against going negative via script.

### 3.6 Communication

3.6.1 **Channels** MUST be persistent pub/sub with subscribers,
mute/ban, nicks, and history. LLM NPCs MAY subscribe (with locks).
Each channel MUST contribute a CmdSet to every subscriber, exposing
at minimum a channel-named send command (e.g. `gossip <text>`) plus
administrative aliases (`/who`, `/history`, `/mute`). Channel CmdSet
contribution is the mechanism by which the command pipeline (§2.7
step 4) discovers channel commands; the merge follows the standard
Union / Replace / Remove rules.

3.6.2 **Tells / pages** MUST support direct messaging.

3.6.3 **Say / emote / pose** are in-Place communication and MUST be
consumed by present NPCs; LLM NPCs MUST hear them as input.

3.6.4 **Content limits.** Every player-authored text message — say,
emote, pose, tell, channel send, mail body, prototype description
written through a building command — MUST be capped at **4 KiB**
of UTF-8 after normalization. Messages exceeding the cap MUST be
rejected at the command pipeline boundary (§2.7 step 5) with a
structured error; the engine MUST NOT silently truncate. Control
characters other than `\n` MUST be stripped before delivery; ANSI
escape sequences in *player* input MUST be stripped, while engine-
or builder-emitted ANSI is preserved (the color model layered over
this, including markup escaping in player input, is §3.20). The
engine MUST NOT ship a
profanity filter — moderation is a policy decision and belongs to
the deployment (see §3.15.5) — but the engine MUST expose a
`on_player_text` hook that scripts and contribs MAY use to
implement filtering, link policy, or replacement.

### 3.7 Prototypes

3.7.1 Prototypes MUST be **scripts that return a table**, not pure
data. This gives expression power (`$random(1,6)`, parent references)
without embedding a mini-language in KDL.

3.7.2 The following Lua block is normative as the canonical prototype
form:

```lua
-- scripts/prototypes/rusty_sword.lua
return {
    parent = "weapon",
    name = "rusty sword",
    aliases = {"sword"},
    damage = mud.random.roll(1, 6),
    durability = 50 + mud.random.roll(0, 20),
}
```

3.7.3 `spawn("rusty_sword")` MUST be a core engine call.

### 3.8 Building, help, and batch processors

3.8.1 The following building commands MUST exist and MUST be
lock-gated to builder permissions: `dig`, `create`, `set`, `examine`,
`link`, `tunnel`, `typeclass`, `copy`, `delete`.

3.8.2 Help MUST be DB-backed entries plus file-loaded entries plus
auto-generation from command docstrings. Help MUST be lock-aware:
the viewer MUST see only commands they can use.

3.8.3 Batch processors MUST support `.mud` files (commands) and
`.lua` files (script) for offline world construction.

3.8.4 **Line editor.** The engine MUST ship a session-scoped line
editor for composing multi-line text (room descriptions, mail
bodies, help entries, prototype descriptions). Entry MUST be
explicit: a command opens the editor on a target buffer, the
session FSM enters editor mode, subsequent input lines are appended
until a terminator command is received, and the buffer is committed
to the target via a `MutationCommand` (§2.5.3.3). The editor MUST
support, at minimum: append, replace line N, delete line N, insert
before line N, show buffer with line numbers, abort, and commit.
The editor MUST honor the §3.6.4 content cap. The same editor
contract MUST be exposed to the webclient via a GMCP message so
the webclient can render a textarea instead of a line-by-line UI;
the committed buffer travels the same `MutationCommand` path.

### 3.9 Observability

3.9.1 The engine MUST emit `tracing` spans pervasively, with
structured spans for sessions, commands, scripts, and LLM calls.

3.9.2 Prometheus `/metrics` MUST expose at minimum: connections,
commands/s, script timings, LLM tokens, DB latency, queue depths.

3.9.3 The live admin MUST surface: command log stream, script error
feed, LLM call inspector with prompt + response. The inspector
schema MUST reserve a `tool_calls` field; in 1.0 it MUST always be
empty per §3.1.5, but the field is reserved so a future post-1.0
tool-use mode can populate it without a wire-protocol break (cf.
§2.8.5.1 additive-only policy).

3.9.4 The engine MUST journal every input with enough context for
replay against a snapshot for debugging.

### 3.10 Testing for builders

3.10.1 `mud test` MUST spin up an in-memory instance, load the game
directory, and run all `*.test.lua` files. It MUST allow assertions
on game state, command outcomes, and NPC reactions. The `mud-test`
crate MUST provide the in-memory harness as a library; the `mud-cli`
crate MUST provide the `mud test` subcommand wrapping it.

3.10.2 LLM tests MUST use replay mode (recorded responses) for
determinism.

3.10.3 Snapshot tests MUST exist for room descriptions and wilderness
rendering.

### 3.11 Multi-tenancy

3.11.1 Multi-tenancy MUST be designed in from the first commit and
MUST NOT be retrofitted.

3.11.2 The tenant tag MUST be in `EntityId` (§2.3.1). The arena,
script loader, file watcher, scheduler, DB connection pool, and
metrics labels MUST all be tenant-aware from day one.

3.11.3 One binary MUST host many games. Each game MUST have its own
DB, world directory, script directory, settings, and port set.

3.11.4 Tenant isolation MUST be enforced at API boundaries: every
entity / component lookup MUST check the caller's tenant. The
engine's public API MUST NOT expose a way to acquire a handle from
one tenant and apply it inside another.

3.11.5 A single Gateway MUST be able to multiplex multiple Worlds by
routing on destination port or hostname.

3.11.6 Early milestones MAY run a single tenant; the **multi-tenant
runtime** MUST activate at milestone M7. The data model MUST NOT
change to support this activation.

### 3.12 Other game systems

The following are core but do not warrant their own subsystem.

3.12.1 **Equipment & body slots.** An `Equipped` component MUST layer
on top of `Inventory`. Slot tables (head, torso, main-hand, etc.)
MUST be declared in archetype files so that different games can
model different bodies. Equipment bonuses MUST flow through the
standard hook table.

3.12.2 **Quests.** The engine MUST provide quest primitives only: a
`Quest` component on the player carrying a state machine plus
scripted predicates for "advance" and "complete." A quest-editor UI
MUST NOT ship in 1.0; quests MUST be authored as scripts.
World-event log entries MUST be able to trigger advancement.

3.12.3 **Mail / offline messaging.** A durable per-account inbox MUST
exist, usable for player mail, system notices, and offline tells.
Channels MUST continue to handle online history; mail MUST handle
the offline case.

3.12.4 **Parties, followers, pets.** Parties MUST be degenerate
vehicles (mobile container, no controls, follows a leader). Pets and
followers MUST be NPCs with a `FollowTarget` component and a behavior
tree resolving leader movement. No new primitives MAY be introduced.

3.12.5 **Factions & reputation.** Reputation scalars MUST be the
same scalars used by the LLM memory layer (§3.1.3 #5) and MUST also
be readable by locks and behavior trees. Factions MUST be modeled as
tag sets with relationship matrices, authored in KDL.

3.12.6 **PvP.** PvP MUST be opt-in per region (§2.2.7) via a `PvpPolicy`
tag. The combat system MUST consult this tag before resolving
player-vs-player damage. **Safe regions MUST be the default.**

3.12.7 **Crafting.** Crafting MUST NOT be a core subsystem. It MUST
be supported through prototypes plus scripted recipes. A reference
contrib crate MUST cover the common case (gather → combine →
produce).

### 3.13 Content migration

3.13.1 When an archetype's component set changes, existing entities
MUST be migrated.

3.13.2 The engine MUST provide:
- Versioned archetype definitions.
- Migration scripts (Lua) per version bump, run automatically on
  world reload.
- A **dry-run mode** that reports what would change without writing.

### 3.14 Internationalization (i18n)

The engine MUST be translatable. World file *content* (room
descriptions, item names, NPC dialogue authored in KDL/Lua) is
whatever language the builder writes and is out of scope for this
section. This section governs **engine-emitted strings**, **built-in
command surfaces**, and the **opt-in API** that allows builders to
localize their own strings using the same machinery.

#### 3.14.1 Scope

3.14.1.1 The following MUST be translatable:
- Engine-emitted player-facing strings: command errors, lock denial
  messages, system notices, prompts, paginator chrome, batch-processor
  reports, "your last command was interrupted" (§2.1.2) and similar.
- Built-in command names and aliases (see §3.14.5).
- Built-in help entries for engine commands.
- LLM fallback line tables (§3.1.6.3).
- Builder-authored strings that explicitly opt into the i18n API.

3.14.1.2 The following MUST NOT be translated by the engine:
- Lock DSL keywords (`perm`, `attr`, `tag`, etc.) — they are code,
  not prose (cf. §2.6).
- Archetype names, component names, hook names, prototype identifiers,
  `ComponentId`s — they are stable identifiers.
- GMCP / MSDP message keys, schema field names, structured wire data
  (§2.8.3) — also identifiers.
- Log, trace, and metric labels — they are operator-facing telemetry,
  not player-facing UI.
- Builder-authored content that has not been routed through the i18n
  API.

#### 3.14.2 Default language and extensibility

3.14.2.1 The default and reference locale MUST be `en` (English).
Every translatable key MUST have an English value; a missing English
value is a load-time error (§3.14.6.2).

3.14.2.2 Adding a new locale MUST require only dropping a locale
bundle into the appropriate directory and reloading. It MUST NOT
require recompiling the engine, modifying Rust code, or restarting
the server.

#### 3.14.3 Storage and format

3.14.3.1 Locale bundles MUST use **Fluent** (`.ftl`) via the
`fluent-rs` crate. Fluent natively supports plural, gender, and
selector forms, which trivial key→string maps do not.

3.14.3.2 Bundles MUST be discovered from two locations, with later
sources overriding earlier ones for the same key:
- **Engine bundles** under `crates/mud-core/i18n/<locale>.ftl` (and
  per-crate equivalents for any crate that emits player-facing
  strings). These ship with the engine.
- **Per-tenant bundles** under `games/<tenant>/i18n/<locale>.ftl`.
  These let a tenant override engine strings or add tenant-specific
  keys.

3.14.3.3 The locale-bundle loader MUST be tenant-scoped (§3.11): one
tenant's overrides MUST NOT bleed into another tenant.

3.14.3.4 Locale bundles MUST be hot-reloadable under the same
file-watcher discipline as scripts (§2.4.3). A failed reload of a
bundle MUST keep the previous version live and MUST emit a structured
error.

#### 3.14.4 Lookup API

3.14.4.1 Rust callers MUST resolve translatable strings through a
`t!(key, args...)` macro (or equivalent function) that takes a
session or locale handle, the key, and named arguments. Concatenating
fragments to build player-facing sentences in Rust MUST NOT be done;
sentences MUST be single keyed messages so translators can reorder
them.

3.14.4.2 The `mud` stdlib (§2.4.5) MUST expose `mud.i18n.t(key,
args)` so scripts can localize builder-authored strings using the
same bundles.

3.14.4.3 A missing key MUST fall back, in order: (a) the same key in
`en`, then (b) the literal key string. A missing-key event MUST emit
a structured `tracing` warning with the key, locale, and tenant.
Missing keys MUST NOT be silently swallowed (cf. §8 rule 5).

3.14.4.4 The lookup API MUST be a system boundary in the sense of
§1.7: keys, locale identifiers, and argument bundles MUST be parsed
into typed domain values, not raw strings, before they reach inner
code.

#### 3.14.5 Built-in commands

3.14.5.1 Built-in commands MUST retain a **canonical English name**
(e.g. `look`, `north`, `say`) used in documentation, scripts, hook
references, and the admin UI. The canonical name MUST be invariant
across locales.

3.14.5.2 Each locale bundle MAY declare additional **localized
aliases** for built-in commands (e.g. `regarder` for `look` in
`fr`). The CmdSet parser (§2.7 step 5) MUST accept localized aliases
contributed by the tenant's locale, merged via the standard
Union / Replace / Remove rules.

3.14.5.3 Command help output MUST render in the tenant's locale
when a translation exists, falling back per §3.14.4.3.

#### 3.14.6 Tenant locale and load-time verification

3.14.6.1 A tenant's effective locale MUST be a single value
configured per tenant (§3.11), defaulting to `en`. The locale is a
property of the world, not of a session or account: every session
connected to a tenant renders engine-emitted strings in that one
locale. There MUST be no per-session or per-account locale resolution
and no mid-session switching — the world's content language and the
engine's UI language are one builder-owned choice.

3.14.6.2 At world load, the engine MUST verify that every key
referenced via `t!` or `mud.i18n.t` exists in the `en` bundle. A
missing English key MUST be a load-time error. Non-English bundles
MAY be incomplete; missing keys fall back per §3.14.4.3.

#### 3.14.7 LLM interaction

3.14.7.1 The LLM subsystem (§3.1) MUST be locale-aware. The tenant's
locale MUST be included in the system / persona prompt slice (§3.1.3
#1) so generated speech matches the world's locale.

3.14.7.2 Fallback line tables (§3.1.6.3) MUST be keyed by locale.
A missing locale-specific fallback MUST fall back to `en` per
§3.14.4.3.

3.14.7.3 Localization MUST NOT be implemented by routing strings
through an LLM at runtime. Translation is bundle-driven (§3.14.3).

#### 3.14.8 Acceptance

3.14.8.1 A conformant 1.0 release MUST be able to add a second
locale (e.g. `fr`) by dropping a `.ftl` bundle into the tenant's
`i18n/` directory and reloading (the tenant then renders engine
strings in that locale), with no engine recompilation and no
restart. This MUST be demonstrated end-to-end against at least one
non-English locale during the tutorial demo, even if the tutorial
content itself ships in English only.

### 3.15 Sessions, accounts, and linkdead

#### 3.15.1 Accounts

3.15.1.1 An **account** is the durable per-player identity. A
session (§2.7) MUST resolve to exactly one account after login.
Accounts MUST be tenant-scoped (§3.11.4): the same username in two
tenants is two unrelated accounts.

3.15.1.2 Credentials MUST be stored as **argon2id** hashes with a
per-account salt. Plain or reversibly-encrypted password storage
is REJECTED. Login attempts MUST be rate-limited at Gateway
(default: 5 attempts per source per minute, leaky-bucket); the
rate limit is independent of and stricter than the §2.1.1
command rate limit.

3.15.1.3 Account creation MUST be possible in one of two modes per
tenant, configurable: **open registration** (anyone connecting may
register) or **invite-only** (registration requires a one-shot
invite token issued by an admin via §2.9.2.3). Email is OPTIONAL;
if present, it MUST be used for password recovery. Recovery MUST
issue a single-use, time-limited token (default 1 hour). If email
is not configured for the tenant, password recovery MUST be an
admin-mediated reset, not a self-service flow.

3.15.1.4 An account MAY own multiple **puppets** (the in-world
characters). Puppet selection at login MUST be an explicit step;
auto-selecting the most-recent puppet MAY be offered as a
preference. A puppet's name MUST be **unique within its tenant**,
compared **case-insensitively**, across all accounts — creating a
puppet whose name collides with an existing one (in any account)
MUST be rejected. Uniqueness is tenant-scoped (§3.11.4): the same
puppet name in two tenants is unrelated. Account and puppet names
MUST NOT consist entirely of digits. Puppet selection MUST accept
the puppet's name (compared case-insensitively) and MUST accept the
1-based ordinal at which the puppet appears in the displayed
character list; because no valid name is all digits, a digit-only
argument always denotes an ordinal.

3.15.1.5 Account states MUST be one of: `active`, `suspended`
(temporary, admin-issued, with reason and expiry), `banned`
(permanent), `deleted` (see §3.17). Suspended and banned accounts
MUST be rejected at login with a non-leaky message.

3.15.1.6 At most **one session per account** MUST be active at a
time. When an already-authenticated account logs in again on a new
connection, the new session MUST take over and the prior session
MUST be disconnected (displaced); the engine MUST NOT run two
concurrent sessions for the same account. A displaced session's
puppet is handled as on any disconnect (§3.15.2).

#### 3.15.2 Linkdead handling

3.15.2.1 When a session's transport fails (TCP reset, WS close,
SSH disconnect) without a `Core.Goodbye` (§2.8.3.3), the session
MUST be marked **linkdead** and the puppet MUST enter a
linkdead state at its current `Place`. The puppet MUST remain
in-world; commands queued for it MUST be dropped. The default
linkdead timeout MUST be **5 minutes**, configurable per tenant.

3.15.2.2 During linkdead the puppet MUST remain a valid target for
combat, scripts, and other players' actions. PvP regions (§3.12.6)
MUST treat a linkdead puppet identically to a connected one — the
spec MUST NOT introduce a safe-by-linkdead loophole. Safe regions
(the default) protect linkdead puppets exactly as they protect
connected ones.

3.15.2.3 If the player reconnects within the linkdead timeout and
authenticates to the same account + puppet, the session MUST
reattach to the existing puppet at its current `Place`. No state
MUST be lost; any speech/emote/combat events that occurred during
linkdead MUST be replayable via the recent-events buffer of the
puppet's `Place` (bounded; default 50 lines).

3.15.2.4 On linkdead timeout, the engine MUST invoke the puppet's
`on_disconnect` hook. The default implementation MUST persist the
puppet at its current `Place` and unlist it from the active-session
view. Builders MAY override to teleport to an inn-style "safe"
location; the engine MUST NOT do so by default because it would
quietly invalidate combat in progress.

#### 3.15.3 Idle and liveness

3.15.3.1 Sessions MUST exchange `Core.Ping` / `Core.Pong`
(§2.8.3.3) at a default interval of **60 s**. A session with no
pong response for **2 intervals** MUST be declared linkdead.

3.15.3.2 An **idle threshold** (default 15 minutes of no input)
MUST surface as a flag on the session and as a tag on the puppet;
scripts MAY consult it. The engine MUST NOT auto-disconnect idle
sessions; that policy belongs to the deployment.

#### 3.15.4 Quit semantics

3.15.4.1 An explicit `quit` command MUST close the session
gracefully (sending `Core.Goodbye`) and MUST invoke
`on_disconnect`. There is **no "safe quit anywhere" guarantee**:
where the puppet ends up is the script's responsibility. The
reference combat rules MUST refuse `quit` while the puppet is in
combat unless a `force-quit` admin override is invoked.

3.15.4.2 For vehicles (§3.3.4.6), quitting while aboard a vehicle
MUST stash the puppet on that vehicle. Re-login MUST place the
puppet wherever the vehicle currently is — including "still at
sea, still in cargo hold."

#### 3.15.5 Moderation tooling

3.15.5.1 The admin SPA (§2.9.2.2) MUST provide, in addition to the
listed features:
- An **account browser** with filter by state, search by name,
  view login history, view recent commands.
- A **moderation action panel** issuing: `suspend` (with reason
  and duration), `ban`, `kick` (force-disconnect a live session),
  `silence` (revoke `say` / channel send for a duration),
  `revert` (revert a `MutationCommand` from the journal §3.9.4).
- A **content moderation queue** of player-reported messages
  (mail, channels, say): the engine MUST expose a `report`
  command and queue reports for admin review.
- All moderation actions MUST be journaled with the acting admin's
  account, target, reason, and timestamp; the journal MUST be
  surfaced in the admin UI and MUST NOT be deletable through the
  UI.

### 3.16 Time

3.16.1 **Wall-clock time** is the operating-system clock. It MUST
be used for: deadlines (§2.4.7.2), LLM latency budgets (§3.1.8.3),
linkdead timeouts (§3.15.2.1), session pings (§3.15.3.1), and
snapshot intervals (§2.5.3.4). It MUST always be UTC at the
engine's API boundary; locale-aware formatting belongs to §3.14.

3.16.2 **Scheduler tick.** The engine MUST run a scheduler tick at
a fixed **20 Hz** (50 ms period; the §2.3.4.1 budget). All
periodic engine work — combat round advancement, status-effect
ticks, respawn timers, weather updates, scripted timers — MUST be
driven by the tick, not by wall-clock timers. The tick rate MUST
NOT be configurable per tenant in 1.0: builders rely on a fixed
cadence for combat balance and reproducible tests.

3.16.3 **In-game time.** Each tenant MUST expose an `in-game clock`
that advances at a configurable ratio against wall-clock (default
1 in-game minute per real minute, i.e. 1:1; common alternatives
8:1 or 24:1). The clock MUST expose: hour, minute, day-of-month,
month, year. The calendar (month names and lengths, year zero)
MUST be configured in the tenant's KDL.

3.16.4 **`mud.time` API.** The script-side `mud.time` module
(§2.4.5.1) MUST expose:
- `mud.time.wall()` — UTC wall-clock as seconds since epoch.
- `mud.time.tick()` — current scheduler tick number since world
  load (monotonically increasing).
- `mud.time.game()` — in-game clock as a structured record.
- `mud.time.after(ms, fn)` — schedule `fn` to fire after a
  wall-clock delay, on the script worker pool, **not** on the
  scheduler thread.
- `mud.time.every(ticks, fn)` — schedule `fn` to fire every N
  scheduler ticks; this is the right primitive for periodic
  in-world effects.
Scripts MUST NOT spin or busy-wait on time; the engine MUST
enforce this via the §2.4.7.2 deadlines.

3.16.5 Time-zone, daylight saving, and locale-aware date
formatting MUST be done at the rendering boundary (the `t!` /
`mud.i18n.t` lookup, §3.14.4) and MUST NOT leak into engine
internals.

### 3.17 Privacy, data export, and account deletion

3.17.1 The engine MUST support an account-deletion flow with two
semantically distinct modes, configurable per tenant:
- **Soft delete** (default): the account state moves to `deleted`
  (§3.15.1.5). The puppet's name and authored content (room
  descriptions, mail sent, channel history) remain but the
  account becomes unloggable. This preserves world history and
  is required for any game where player-authored content is
  load-bearing.
- **Hard delete**: PII (email, login history, IP records) MUST be
  purged. The puppet's authored content MUST be reassigned to a
  tombstone "deleted player" entity. Dyadic LLM memory referencing
  this player (§3.1.3 #3) MUST be purged; episodic memory entries
  MUST be redacted by replacing the player identifier with the
  tombstone. The deletion MUST be journaled.

3.17.2 The engine MUST expose a **data-export endpoint**
returning, for the requesting account: account fields (excluding
the password hash), puppet list, mail, channel messages sent by
this account, and a manifest of LLM memory entries that reference
the account. Export MUST be admin-mediated or self-service per
tenant policy.

3.17.3 Channel and mail content MUST be retained for at most a
configurable retention window (default 90 days for channel
history, indefinite for mail). After the window, channel history
MUST be purged; mail MUST require explicit user deletion.

### 3.18 Backup and restore

3.18.1 The engine MUST ship a `mud backup` and `mud restore` CLI
subcommand pair. Backup MUST be **online** (no service interruption)
and tenant-scoped: a backup MUST contain exactly one tenant's
database plus its world directory snapshot pinned to a commit hash
or file tree hash.

3.18.2 Backups MUST be in a **streaming, append-friendly format**
(SQLite: `.backup` API; Postgres: `pg_basebackup` plus WAL).
Point-in-time recovery to any moment within the WAL retention
window MUST be possible.

3.18.3 Backup cadence MUST default to **hourly incremental + daily
full**, configurable. The retention default MUST be 7 daily fulls
+ 24 hourly incrementals, configurable.

3.18.4 Restore MUST be a **per-tenant** operation. Restoring tenant
A MUST NOT require stopping tenant B in the same `mudd` process;
the engine MUST drain tenant A, replace its DB and world directory,
and reload. Players in tenant A MUST see a "world restoring"
banner and MUST be disconnected gracefully; players in tenant B
MUST be unaffected.

3.18.5 The restore process MUST run schema migrations and content
migrations forward to the engine's current versions, with the
content-migration dry-run (§2.5.4.3) available beforehand.

### 3.19 Onboarding

3.19.1 A brand-new connection — before login — MUST present:
- A **welcome banner** authored per tenant in KDL.
- A short prompt indicating how to register (or that the tenant is
  invite-only) and how to log in.
- A `help` command available pre-login that lists the small set of
  pre-login commands (register, login, who, quit).

3.19.2 On first login of a new puppet, the engine MUST run a
scripted **`on_first_login`** hook. The default tutorial-world
implementation MUST guide the player through movement, looking,
speaking, and inventory; other tenants MAY replace it.

3.19.3 The `help` command MUST be discoverable by `?` as an alias,
and `help` with no arguments MUST list categories. Lock-awareness
(§3.8.2) MUST hide commands the viewer cannot use.

### 3.20 Color and styled output

The engine MUST treat color and text attributes as a **render-time
concern**, resolved once per session at the output boundary (§2.7
step 8). Engine code, scripts, and builder content MUST NOT emit raw
terminal escape sequences as their primary styling mechanism; they
emit **markup** that the renderer compiles to the session's
negotiated target.

#### 3.20.1 Internal representation

3.20.1.1 All player-facing output MUST be representable as **styled
text**: a sequence of spans, each a string plus an optional style
(foreground, background, and attribute flags — bold, underline,
italic, reverse, blink-equivalent). This representation is
transport-neutral; it is compiled to ANSI for telnet/SSH and to
structured spans for WebSocket clients.

3.20.1.2 The engine MUST NOT carry raw escape sequences through
internal pipelines. Escape generation MUST happen only in the
per-session telnet renderer.

#### 3.20.2 Markup

3.20.2.1 Styled text MUST be authorable via a compact markup with two
forms (syntax illustrative):
- **Semantic role** — `{error}…{/}`, `{channel.gossip}…{/}`.
  Resolved through the active palette (§3.20.3). This is the
  RECOMMENDED form.
- **Direct style** — named color `{fg=cyan}…{/}`, background
  `{bg=…}`, attributes `{b}…{/}`. For builder flavor where a semantic
  role does not apply. Named colors resolve through the active palette
  (§3.20.3) so a tenant restyle reaches them.

3.20.2.2 Unknown role or color names MUST resolve to "unstyled" and
MUST emit a structured `tracing` warning (cf. §3.14.4.3 missing-key
handling). They MUST NOT abort rendering.

3.20.2.3 Markup tags are stable identifiers and MUST NOT be
translated (cf. §3.14.1.2). Translatable strings (§3.14) MAY contain
markup; the RECOMMENDED practice is to apply roles at the emission
site and keep prose in the bundle.

3.20.2.4 Which direct-style tags a given authored field admits is an
engine policy per field, not a global of the markup language. Authored
room fields (title, description) MUST express color only as
palette-curated **named colors**; a raw `#rrggbb` literal in field
markup (e.g. `{fg=#1a53ff}`) MUST NOT be accepted, so every authored
color resolves through the palette (§3.20.3.4) and a tenant restyle
reaches it. A field MAY further restrict attributes. A disallowed tag
MUST degrade per §3.20.2.2 — kept as literal text with a structured
warning — never abort. Direct truecolor literals in markup are
reserved for a later milestone and a non-field context.

#### 3.20.3 Palette

3.20.3.1 A **palette** maps semantic roles and named colors to
concrete styles. Palettes MUST be authored in KDL and MUST follow the
same two-source, tenant-overriding discovery as locale bundles
(§3.14.3.2): an engine default palette, overridable per tenant. The
following block is normative as shape:

```kdl
palette "default" {
    role "error"           fg="#ff5555"
    role "system"          fg="#7aa2f7"
    role "alert"           fg="#ffffff" bg="#aa0000" bold=true
    role "say"             fg="#cdd6f4"
    role "emote"           fg="#bac2de"
    role "tell"            fg="#f5c2e7"
    role "channel.gossip"  fg="#a6e3a1"

    color "cyan" "#00ffff"   // named color for direct markup
}
```

3.20.3.2 The engine MUST define a baseline set of roles for its own
output, including at minimum: `error`, `system`, `alert`, `prompt`,
`say`, `emote`, `tell`. A tenant palette MAY add roles; channels
(§3.6.1) reference roles by name.

3.20.3.3 Palettes MUST be hot-reloadable under the §2.4.3
file-watcher discipline; a failed reload MUST keep the previous
palette live and emit a structured error.

3.20.3.4 Authored colors MUST be stored as 24-bit and downsampled per
session (§3.20.5). Builders author in truecolor regardless of any
client's capability.

#### 3.20.4 Channel and message colors

3.20.4.1 Each channel (§3.6.1) MUST be able to declare its display
role in its KDL definition (e.g. `color="channel.gossip"`). Absent an
explicit role, a channel MUST fall back to a default channel role.

3.20.4.2 In-Place communication (`say` / `emote` / `pose`, §3.6.3)
and direct messages (`tell`, §3.6.2) MUST render through their
corresponding engine roles so a tenant restyles them by overriding
the palette, not by editing scripts.

#### 3.20.5 Capability tiers and negotiation

3.20.5.1 A session's color target MUST be one of four tiers:
**`mono`**, **`ansi16`**, **`xterm256`**, **`truecolor`**.

3.20.5.2 The tier MUST be resolved in order, first match wins:
1. An explicit per-account preference (§3.20.6).
2. `NO_COLOR` signalled by the client → `mono`.
3. TTYPE / terminal identification (§2.8.2) and the `Core.Hello`
   capability bitset (§2.8.3.3).
4. The tenant default tier (configurable; default `ansi16` for
   maximum compatibility).

3.20.5.3 The WebSocket / webclient transport MUST always receive
**semantic spans** carrying the role name and the resolved truecolor
value, so the webclient themes via CSS and falls back to the inline
value when no theme rule matches (§2.9.3.2). Tier downsampling
(§3.20.5.4) MUST NOT be applied to the webclient path.

3.20.5.4 The telnet renderer MUST downsample deterministically:
truecolor → `xterm256` → `ansi16` by nearest-color match, and `mono`
MUST drop color while preserving attributes the terminal supports
(bold / underline), falling back to plain text otherwise. Downsampling
tables MUST be fixed so rendering is reproducible for snapshot tests
(§3.10.3).

#### 3.20.6 Player preferences and accessibility

3.20.6.1 An account MUST be able to persist a color preference (tier
override and/or named palette selection), resolved first-match-wins
from: the account preference, then the tenant's default palette
(§3.20.3), then the engine default. Unlike locale (§3.14.6), color
stays per-account and switchable mid-session without disconnect:
color carries no meaning and is applied per-connection at the render
edge (§3.20.5.4), and the color tier and colorblind-safe palette
(§3.20.6.3) depend on the individual terminal and player.

3.20.6.2 The engine MUST honor `NO_COLOR`.

3.20.6.3 The engine MUST ship at least one **colorblind-safe**
alternate palette selectable per account. Shipping it is REQUIRED;
making it the default is a deployment choice.

#### 3.20.7 Player-input safety

3.20.7.1 This refines §3.6.4. Raw ANSI in player input MUST continue
to be stripped. Color **markup** in player-authored text MUST be
escaped (rendered literally) by default, so players cannot inject
styling into others' output. A tenant MAY grant a limited, lock-gated
markup subset for player text (e.g. RP emotes) via the
`on_player_text` hook (§3.6.4).

3.20.7.2 Engine- and builder-emitted markup is trusted and compiled
normally. Raw ANSI emitted by builders is preserved for backward
compatibility but is NOT downsampled and does NOT reach the webclient
as theme data; builders SHOULD use markup.

---

## 4. Authoring layers (the pyramid)

The following diagram is normative as the authoring stack. Higher
layers depend on lower layers; users of lower layers MUST NOT be
required to touch higher ones to author content.

```
                        ┌────────────────┐
                        │  Rust engine   │   <- engine devs only
                        └────────────────┘
                  ┌──────────────────────────┐
                  │   Plugin crates (Rust)   │   <- optional contrib
                  └──────────────────────────┘
            ┌──────────────────────────────────────┐
            │       Lua scripts (behavior)          │  <- world devs
            └──────────────────────────────────────┘
      ┌────────────────────────────────────────────────────┐
      │  World files: KDL (structure) + ASCII/PNG (maps)    │  <- builders
      └────────────────────────────────────────────────────┘
```

4.1 **KDL** MUST be used for static structure: rooms, archetypes,
channels, help, config.

4.2 **ASCII art / PNG** MUST be supported for wilderness terrain
maps.

4.3 **Lua scripts** MUST be used for behavior: hooks, commands,
prototypes, NPC brains, migrations.

4.4 **Rust plugin crates** MAY be authored when scripting is not
enough: custom components, performance-critical systems.

---

## 5. Repository layout

The following layout is normative for the 1.0 source tree.

```
crates/
  mud-schema      // wire protocol; codegen source
  mud-ipc         // Gateway↔World IPC transport: length-prefixed
                  // postcard framing, unix-socket + in-memory channel,
                  // resume handshake (carries mud-schema frames)
  mud-core        // entity arena, components, archetypes, Place trait,
                  // locks, scheduler, hook dispatch
  mud-db          // SQLx schema, write-through cache, snapshots,
                  // migrations
  mud-net         // telnet+IAC+MCCP2+GMCP+MXP+MSDP+MSSP, SSH, WS
  mud-gateway     // gateway binary
  mud-script      // mlua integration, sandbox, mud stdlib, hot-reload,
                  // deadlines, custom module loader
  mud-world       // KDL parsers, prototype loader, file watcher,
                  // region+tile loader, content migrations
  mud-cmd         // cmdset trie, parser, dispatch
  mud-llm         // memory layers, vector store, tool dispatcher,
                  // budget enforcer, provider drivers
  mud-vehicle     // Place-as-vehicle, movement coupling, controls
  mud-web         // axum routes, OpenAPI, admin handlers, webclient
                  // assets, metrics. Linked into both mud-gateway
                  // (public listener, static, proxy, Gateway metrics)
                  // and mudd/World (internal admin surface,
                  // game-state handlers, game metrics). See §2.9.1.2.
  mud-i18n        // Fluent-based locale bundles, lookup API, session
                  // locale resolution, hot-reload
  mud-test        // in-memory harness for `mud test`
  mud-cli         // `mud` CLI: check, test, migrate, fmt, lsp
  mudd            // the server binary (combines gateway + world or
                  // runs split via feature flag)
  contribs/       // optional crates: combat-d20, crafting,
                  // turn-battle, named-rp, etc.

clients/
  webclient/      // Svelte + TS SPA (player-facing)
  admin/          // Svelte + TS SPA (admin dashboard)
  schema-ts/      // generated TS types, shared by both SPAs

games/
  tutorial/       // shipped tutorial world (the demo)
    world/*.kdl
    scripts/*.lua
    maps/*.txt
    config.toml
```

5.1 Each crate in `crates/` MUST own a single concern as labeled.

5.2 The `mudd` binary MUST support both single-process and split-mode
deployments via feature flag (cf. §2.1.3.3).

5.3 The `contribs/` directory MUST be the home of optional crates;
none MAY be made a hard runtime dependency of the engine core.

---

## 6. Tech stack (locked unless stated)

The following table is normative for 1.0. Each row is a locked choice
unless this SPEC or §9 marks it as deferred.

| Concern | Choice | Why |
|---|---|---|
| Async runtime | Tokio | de facto standard |
| TLS | rustls | pure Rust, audited |
| SSH | russh | pure Rust |
| WebSocket | tokio-tungstenite | mature, idiomatic |
| DB | SQLx + SQLite/Postgres | compile-checked queries; either backend |
| Vector store | sqlite-vec / pgvector | embedded; no extra service |
| Schema | postcard for IPC, JSON+CBOR for wire | small, fast, versioned |
| Scripting | Lua 5.4 via mlua; WASM via wasmtime for polyglot/perf-critical plugins | MUD lingua franca; mature tooling; one dialect to learn |
| Parser | chumsky | locks DSL, command parser |
| KDL parser | kdl (kdl-rs) | reference KDL v2 parser for world files |
| Web | Axum + utoipa | OpenAPI for free |
| Admin/Webclient | Svelte + TypeScript (single stack for both SPAs) | one toolchain, shared schema-ts types and UI primitives |
| Observability | tracing + tracing-subscriber + opentelemetry; metrics + prometheus exporter | standard stack |
| Logs | structured JSON to file + stdout | parseable |
| LLM clients | reqwest + provider SDKs where they exist | swappable |
| CLI | clap | standard |
| Config | figment (toml/env merge) | flexible |
| Password hashing | argon2 (argon2id) | memory-hard, modern default |
| i18n | fluent-rs | plural/gender forms; pure Rust; bundle-based |
| Migrations | sqlx migrate + bespoke content migrations | versioned |
| Test harness | cargo test + custom in-memory `mud test` | both engine and content |

6.1 Substitutions outside this table MUST be motivated by a documented
deficiency in the locked choice and MUST be reviewed by a maintainer.

---

## 7. Delivery shape — workstreams and milestones

7.1 The delivery plan MUST be organized as **parallel workstreams**,
not a linear waterfall.

7.2 Workstreams MUST progress independently behind a small set of
**integration milestones**. Each milestone is a demoable slice of
the eventual 1.0, **not** a stage gate that prevents work in other
workstreams.

### 7.3 Workstreams

There are exactly twelve workstreams.

1. **Core runtime** — entity model, `Place`, archetypes / components,
   cmdset pipeline, locks, scheduler.
2. **Persistence** — SQLite/Postgres backends, write-through cache,
   snapshots, content migrations.
3. **Scripting** — Lua host, sandbox, `mud` stdlib, hot-reload,
   script-defined commands / components / hooks, prototypes.
4. **Networking & clients** — telnet/IAC, MCCP2, GMCP / MSDP / MXP /
   MSSP / TTYPE / CHARSET / NAWS, TLS, SSH, WebSocket, reference
   client matrix.
5. **Spatial** — wilderness regions, terrain maps, viewport / FOV,
   procedural regions, vehicles, movement coupling.
6. **NPC behavior** — behavior-tree primitives, perception, scripted
   combat / trade / movement logic. This is the mechanical substrate
   for any NPC, LLM-augmented or not.
7. **LLM dialogue & flavor** — provider abstraction, persona / prompt
   assembly, constrained dialogue output, async delivery, fallback
   lines, dyadic / episodic memory, world-event log, budgets /
   throttles. Layers *on top of* a scripted NPC; never required for
   an NPC to act.
8. **Web & admin** — Axum services, admin UI, modern webclient,
   metrics, LLM call inspector.
9. **Game systems** — accounts (registration, login, recovery),
   sessions and linkdead handling, time model and scheduler tick,
   channels and tells, combat reference rules, economy (shops /
   ledger / auctions), building commands, help, batch processors,
   paginator / table / form / editor helpers, line editor.
10. **Operations** — multi-tenant routing, per-tenant isolation,
    graceful upgrade, backups (online per-tenant) and restore,
    moderation tooling (suspend/ban/kick/silence/report queue),
    privacy / data export / account deletion, throttles, security
    and performance hardening.
11. **Tutorial & docs** — tutorial world content, builder docs, GMCP
    spec, contribution guide.
12. **Internationalization** — Fluent bundles for engine strings,
    locale resolution, localized command aliases, LLM locale
    awareness, fallback line locale keying.

### 7.4 Milestones

Milestones are *integration points* that prove the workstreams
compose. They are not exhaustive feature lists; a workstream MAY land
features between milestones provided the next milestone slice still
works.

#### M1 — Walk and talk

Two players connect over telnet, log in, walk between hand-authored
rooms, see each other, and chat. The following state MUST survive a
clean restart: account credentials, puppet location, and inventory.
Conversation history and other later-introduced state are out of
scope for M1. ANSI and NAWS MUST work. Locks MUST parse and
evaluate. A tenant-isolation smoke test MUST exist from M1 onward:
even when only one tenant is active by default, an automated test
MUST construct two tenants and assert that an `EntityId` minted in
tenant A cannot be resolved, mutated, or observed via tenant B's
handles. This locks the tenant-tag contract (§2.3.1, §3.11.4) in
before the multi-tenant runtime activates at M7.

#### M2 — Builders without Rust

A non-programmer MUST be able to add an archetype, a custom
component, a Lua command, and a prototype, and hot-reload — with no
restart and no recompile. `mud check` MUST catch a broken lock
string and a bad hook signature before load. A non-English locale
bundle (`.ftl`) MUST be added to a tenant's `i18n/` directory,
hot-reloaded, and a localized engine string MUST render to a session
whose locale resolves to it (§3.14.8.1). The remaining LLM-side
locale work (§3.14.7) lands with M6.

#### M3 — Client matrix

Mudlet, TinTin++, MUSHclient, and BlightMud MUST all connect cleanly
with MCCP2 + GMCP + MSDP + MXP. The webclient SPA MUST render the
same game over WebSocket. SSH and TLS ports MUST be live.

#### M4 — Wilderness and ships

Walk from a hand-authored city room onto an overworld tile, board a
sailable ship, cross water to another city. The viewport MUST render
with FOV per player. GMCP map data MUST drive tile graphics in the
webclient.

#### M5 — NPCs that act

Scripted NPCs MUST perceive, decide, move, fight, and trade using
behavior-tree primitives and the d20-flavored reference combat
rules. **No LLM involvement at this milestone.** M5 establishes the
mechanical substrate.

#### M6 — NPCs that speak

An LLM-augmented innkeeper, layered on top of an M5-style scripted
NPC, MUST remember each player across sessions, reference prior
interactions, refuse service to low-reputation characters (refusal
scripted; delivery LLM-authored), and MUST keep working when the
provider is killed mid-session — fallback lines MUST take over
without breaking play.

#### M7 — Run it in production

The admin dashboard MUST show live commands, script errors, LLM
calls, and metrics. **Two isolated games MUST run on one binary.**
Graceful upgrade MUST preserve connections across a World restart.
Across a World restart, Gateway-served HTTP MUST remain reachable;
the admin SPA MUST load, render a "World disconnected" banner,
continue scraping Gateway-side metrics, and automatically reattach
game-state panels on resume handshake. Prometheus `/metrics` MUST NOT
gap for Gateway-local metrics and MUST expose `world_up` for
World-side scrape state. **Online per-tenant backup and per-tenant
restore (§3.18) MUST work end-to-end against a live second tenant.**
Moderation tooling (§3.15.5) MUST be wired into the admin SPA:
suspend, ban, kick, silence, and the report queue MUST be demoable.
Account deletion in both soft and hard modes (§3.17.1) and the
data-export endpoint (§3.17.2) MUST be demoable. Security and
performance passes MUST complete.

#### M8 — 1.0

The tutorial world MUST cover city, overworld, dungeon, ship, LLM
innkeeper, shop, auction, combat encounter, and a hot-reload demo.
Builder docs, GMCP spec, and release process MUST be published.
The "done means" criteria in §0.4 MUST be met.

### 7.5 Dependency notes

The following are the only hard ordering constraints called out by
this SPEC; everything else is workstream-local.

7.5.1 M1 requires core runtime + persistence + minimum networking.

7.5.2 M2 requires scripting on top of M1's core.

7.5.3 M5 (scripted NPCs) MUST precede M6 (LLM flavor). The LLM
workstream MAY develop provider / memory / prompt machinery in
parallel, but the integration demo at M6 requires a real scripted
NPC to dress up.

7.5.4 M4's vehicles MUST require M5's behavior-tree primitives only
if NPC crews are demoed. A player-piloted ship MUST require
neither.

7.5.5 M7's graceful upgrade exercises the Gateway / World split that
MUST have been in the design since M1.

---

## 8. Vibe-coding ground rules

The following ground rules bind agents working on this codebase.
They are normative; deviations require explicit maintainer approval.

1. **Milestone integration demos are the gates; workstreams are
   not.** Per §7.1–7.2, workstreams progress independently and
   milestones (M1–M8) are integration points proving the workstreams
   compose. Agents MUST NOT block intra-workstream work on another
   workstream's progress, but a milestone's acceptance demo MUST
   pass before claiming that milestone complete. Hard ordering
   constraints are limited to those enumerated in §7.5.
2. **TDD where it makes sense.** Engine code: yes. Glue code:
   integration tests are enough.
3. **Every PR touches at most one crate's public API.** Cross-crate
   refactors get their own PR.
4. **Wire protocol changes go through `mud-schema` first**, then
   regenerate Rust + TS together. **Never hand-edit generated
   code.**
5. **No silent failures.** Script errors, lock denials, and LLM
   failures MUST surface as structured events. Use `tracing` over
   `eprintln!`.
6. **No vendored scripting runtime beyond `mlua` + `wasmtime`.**
   Surface-area discipline. **No second script language sneaks in.**
7. **No magic.** No macros that hide control flow. Prefer explicit
   dispatch tables over derive-trait sleight of hand.
8. **Tests for content features run in the `mud test` harness**, not
   via a live server.
9. **Hot-reload paths get tests.** A reload that leaves the world
   broken is a regression.
10. **Document GMCP messages in `mud-schema`'s sources.** They are
    the public client API.

---

## 9. Open questions (deferred decisions)

The following decisions MUST be resolved before the corresponding
workstream lands. They are recorded here as open. **This SPEC does
not pre-empt them.**

- **LLM**: default provider for the tutorial — Ollama (zero-cost
  local) or a hosted provider with a server-op-supplied key?
- **LLM**: vector store granularity — one table per NPC vs. one
  global table with an NPC id column. Pick after a load test.
- **Spatial**: tile size in bytes (terrain code + flags). Default 4
  bytes; verify against the largest realistic region.
- **Web/IPC**: carry the Gateway↔World admin RPC (§2.1.3.5) on a
  sibling unix socket vs. as a new frame class on the existing
  postcard IPC channel. Lean: sibling socket, to keep gameplay frame
  schemas in `mud-schema` clean and let admin traffic use a richer
  envelope (e.g. JSON-RPC).

---

## 10. Risk register

The following risks are tracked. Mitigations are normative as
engineering practice.

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Lua GC pauses on tick-tight code | Medium | Medium | Incremental GC; per-script deadlines; WASM/native escape hatch for hot paths |
| LLM provider drift (API changes) | High | Low | Provider abstraction; pinned versions; replay tests |
| LLM latency degrades player experience | Medium | Low | LLM never on mechanical path; async delivery; fallback lines; per-player throttle |
| Session-resume protocol bugs lose state | Medium | High | Checkpoint at every command boundary; integration tests for kill-mid-command; player-visible "last command interrupted" message |
| MUD client incompatibility surfaces late | Medium | High | Continuous reference-client matrix in CI from M3 onward |
| Content migration framework painful | Medium | Medium | Build dry-run + good error messages early |
| Vector store performance at 10k+ NPCs | Low | Medium | Per-tenant scoping; pgvector for prod |
| Two-process IPC complexity | Medium | Medium | Single-process mode for dev; integration tests |
| Sandbox escape from Lua | Low | High | Strip dangerous stdlib; capability allowlist; fuzz host bridge |
| Wire protocol churn breaking clients | Medium | High | Versioned schema; never break, only add |

---

## 11. What 1.0 ships

A conformant 1.0 release MUST ship:

- A single `mudd` binary.
- A `mud` CLI.
- A tutorial world.

The tutorial world MUST demonstrate all of the following:

- Telnet with MCCP2 / GMCP / MSDP / MXP working across Mudlet,
  TinTin++, MUSHclient, BlightMud.
- The modern webclient with map tile rendering.
- A city with hand-authored rooms; an overworld region; a dungeon; a
  sailable ship traversing the overworld.
- An LLM-augmented innkeeper (scripted behavior + LLM dialogue /
  flavor) with dyadic memory and episodic recall; and a
  scripted-only NPC with the same behavior surface but no LLM
  dependency.
- A shop, an auction, and a ledger.
- A combat encounter using the d20-flavored reference rules.
- Hot-reload of a script, demonstrating the no-restart workflow.
- Admin dashboard showing live command log, script errors, LLM
  calls, and metrics.
- Multi-tenant config sample running two isolated games on one
  binary.
- A `mud test` suite that passes against the tutorial.

Builders MUST be able to fork the tutorial and have their own world
running in an evening.

End of specification.
