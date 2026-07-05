# M1-22 — `mudd` single-process wiring

**Date:** 2026-07-03
**Spec:** §2.1.3.3 (single-process IPC), §2.5.3.3 (write-through layer, amended
here), §2.5.3.5 (scheduler serialization), §2.1.3.2 (resume handshake), §5.2
(`mudd` binary), §3.19 (login flow)
**Plan:** M1-22

## Goal

Boot tenants end-to-end in a single process: for each registered tenant,
load its world, open its database, run its scheduler driver loop, run its
command pipeline, and embed a gateway over an in-memory IPC channel.
Multiple tenants run concurrently — each an isolated stack on its own
listen address. Definition of done: `cargo run -p mudd -- --tenant-dir
<dir>` serves a telnet login locally, and a registry with two tenants
serves both at once.

This PR also resolves the two design decisions PLAN.md deferred here: the
**single write path** (how `Scheduler`, `PersistentWorld`, and `Pipeline`
compose) and the **identity sources** (`world_id`, `tenant_tag`), plus the
M1-19 leftover of hydrating a mid-session-created puppet into the live world.

## Decisions (agreed in brainstorming)

1. **Single write path: `PersistentWorld` owns the `Scheduler`.** The pipeline
   stops mutating the world; all mutations flow scheduler → durable apply.
2. **SPEC §2.5.3.3 is amended** — its "same transaction" wording cannot hold
   for an in-memory arena; the real model is write-through with the database
   as the sole source of truth.
3. **Server settings are server-wide, not per-tenant:** they live in
   `$XDG_CONFIG_HOME/ferrodun/config.toml` (default `~/.config/ferrodun/`),
   overridable with `--config`. The server config carries the **tenant
   registry** — multiple tenants run concurrently in one `mudd` process
   (§2.5.1.4's whole point). The tenant's own `config.toml` stays
   `mud-world`'s file inside the tenant dir.
4. **Topology: one World task + one gateway task per tenant**, no locks
   between subsystems beyond a shared handle on that tenant's
   `PersistentWorld`. Tenants share nothing but the tokio runtime.
5. **DB errors are fail-stop.** Halt dispatch immediately, exit non-zero,
   let the supervisor restart; boot rebuilds the arena from the DB.
6. **Extend the existing `0001_initial.sql` migration** for the `server`
   table — nothing is released, so no new migration file.

## Architecture

### 1. The single write path

Today there are three independent write paths to world state:
`Scheduler::tick` (arena, no persistence), `PersistentWorld::apply` (arena +
DB, no scheduler), and `Pipeline::run_matched` (applies command effects
straight to `&mut World`). They become one:

- **`mud-core`:** `Scheduler` gains `drain()` — increments the tick counter
  and returns the queued `MutationCommand`s in arrival order. The existing
  `tick(&mut World)` remains for pure in-memory use and is re-expressible
  over `drain()`.
- **`mud-db`:** `PersistentWorld` owns a `Scheduler` and gains:
  - `submit(MutationCommand)` — enqueue;
  - `async tick() -> Result<Vec<TickEvent>, DbError>` — drain the queue,
    applying each command through the existing `apply()` (arena mutation +
    durable write, the one apply source of truth). Stops at the first
    `DbError` (fail-stop; commands later in the queue are neither applied
    nor persisted — prefix consistency).
  - `tick_number()` delegating to the inner scheduler.
- **`mud-engine`:** `Pipeline::dispatch` takes `&World` (read-only) and
  `DispatchOutcome` grows `effects: Vec<Effect>`; `run_matched` no longer
  calls `world.apply_effect`. Output and broadcast rendering was already
  pre-effect, so player-visible text is unchanged. Effects apply on the next
  50 ms tick — the §2.5.3.5 scheduler model.
- `Rejected` / `PreconditionFailed` tick events are logged with
  `tracing::warn!` in M1; surfacing them to the originating session is
  deferred until a structured channel exists (M3 GMCP note in PLAN).

### 2. Consistency model and the §2.5.3.3 amendment

The arena is a **cache rebuilt from the database at boot**
(`PersistentWorld::load`); the database is the **sole source of truth**
(§2.5.3.1). Apply order is arena-first, then durable write, because arena
rejection is a semantic outcome (stale handle, failed precondition) that must
never reach the database; `Create` is the one DB-first exception (the
`AUTOINCREMENT` key is the identity).

A failed durable write therefore leaves the arena momentarily ahead of the
DB. This is not a dual-write hazard because neither store keeps running
diverged: `tick()` returns `Err`, the World loop processes no further frames,
the process exits non-zero, and restart rebuilds the arena from the DB — the
unpersisted mutation is cleanly lost. What no ordering can eliminate is the
player having already read output for a change that evaporates; that window
is inherent to acknowledging before durability, bounded to the failing tick,
and accepted for M1 (§2.5.3.4 snapshots + WAL are the crash-recovery story).

**§2.5.3.3 is reworded to:** the write-through layer applies each
`MutationCommand` to the arena and, if and only if the arena accepts it,
performs the corresponding durable write before the next command applies; the
durable write for one command is atomic; rejected commands never reach the
database; the database is the sole source of truth and the arena a cache
rebuilt from it, with fail-stop on a failed durable write as the divergence
recovery mechanism.

### 3. CLI and configuration

Two config planes, cleanly split:

- **Server-wide config** — `$XDG_CONFIG_HOME/ferrodun/config.toml`
  (`XDG_CONFIG_HOME` env var, falling back to `~/.config`; Linux is the only
  supported deployment target for now). Optional file — defaults apply when
  absent. Keys:
  - `rate`, `burst` — shared rate-limiter settings (defaults from `mud-net`:
    10/s, 20).
  - `[[tenants]]` — the tenant registry: each entry has `dir` (the tenant
    folder) and `listen` (that tenant's telnet address). Listen addresses
    must be distinct.
- **Per-tenant config** — `<tenant-dir>/config.toml`, `mud-world`'s file:
  world content, `tenant_tag` (default 0), tenant locale.

`clap`-derived args and layering, weakest first: built-in defaults <
server `config.toml` < `MUDD_*` env vars < CLI flags. Flags:
`--config <PATH>` (server config path), `--tenant-dir <DIR>` (**replaces**
the registry with this single tenant, listening on `--listen`, default
`127.0.0.1:4000`), `--rate`, `--burst`. Booting with an empty tenant
registry and no `--tenant-dir` is a startup error.

### 4. Identity

- **`world_id`** — get-or-create in the tenant DB: the `server` table (single
  row) added to `0001_initial.sql` stores a randomly generated `NonZeroU64`,
  exposed as `TenantDb::world_id()`. Stable across restarts, as the resume
  handshake (§2.1.3.2) requires.
- **`tenant_tag`** — read from each tenant's `config.toml` (default 0) into
  `TenantTag` and passed to `World::new` via `PersistentWorld::load`. With
  multiple tenants in one process, tags must be **unique across the
  configured tenants** — validated at boot, since the tag is the §2.3.1.1
  isolation handle the cross-tenant rejection tests (M1-23) rely on.

### 5. Runtime topology and boot sequence

Per tenant, two tokio tasks: `mud_gateway::serve` on one side of
`mud_ipc::in_memory_pair()`, and that tenant's World loop on the other.
Tenants are fully independent stacks — own DB, own arena, own scheduler,
own session registry, own listener — sharing only the runtime.

Process boot: parse CLI + figment → init `tracing-subscriber` → for each
registered tenant, boot it; validate `tenant_tag` uniqueness across tenants;
then run all tenant tasks under a `JoinSet`.

Per-tenant boot order: `TenantConfig::load` + `load_world` → `TenantDb::open`
→ `world_id` get-or-create → `PlaceMap` from `rooms().place_keys()` →
`PersistentWorld::load(db, tenant, place_map)` → build `SessionService`
(world banner), `Pipeline` (tenant locale), builtin registration →
`in_memory_pair()` → spawn gateway (`TcpListener::bind(listen)`,
`GatewayConfig { world_id, rate, burst }`) → spawn World loop.

`PersistentWorld` lives in an `Arc<tokio::sync::Mutex<_>>` so the
`LoginBackend` can reach it; the mutex is uncontended in practice (one
consumer task) and tokio's flavor is safe to hold across awaits. Ctrl-C
(`tokio::signal`) breaks the loop; dropping the World endpoint shuts the
gateway down cleanly, closing client sockets.

### 6. World loop

`mud_ipc::accept_resume` first, then `select!` over `endpoint.recv()` and
`tokio::time::interval(TICK_PERIOD)`:

- `GatewayFrame::Connect` → `SessionService::connect` → send `Output` frames.
- `GatewayFrame::Input` → `SessionService::on_input(backend)`:
  - `Routing::Login` → send outputs; if the FSM closed, send
    `WorldFrame::Close` too.
  - `Routing::InWorld` → `Pipeline::dispatch(&world, …)` → send outputs;
    wrap returned effects in `MutationCommand`s and `submit` them;
    `SessionDisposition::Close` → `Close` frame + `disconnect`.
  - `Routing::Unknown` → log and drop.
- `GatewayFrame::Disconnect` → `SessionService::disconnect`.
- Tick → `persistent_world.tick()`; log events; `Err` → fail-stop.
- `recv()` yields `Ok(None)` (gateway gone) → clean shutdown.

### 7. `LoginBackend` implementation + puppet hydration

A `mudd`-local struct holding the `TenantDb` (delegating
`authenticate` / `register` / `puppets_of` / `create_puppet` to `Accounts`)
and the shared `PersistentWorld` handle:

- `resolve_puppet(key)` → `PersistentWorld::entity_id(key)`.
- `create_puppet` → `Accounts::create_puppet` (persists the puppet + entity
  rows in the start room), then the new
  **`PersistentWorld::hydrate(key) -> Result<EntityId, DbError>`**, which
  reuses `load`'s per-entity logic: mint an arena handle, replay that key's
  location and inventory rows, insert both key↔id mappings. The fresh puppet
  is resident before the FSM's `Enter` resolves it — closing the M1-19
  register → create → play gap end-to-end (§3.19).

### 8. Error handling

`mudd` is the application: `anyhow` with `.context()` at every boot step;
libraries keep their `thiserror` types. Runtime policy is fail-stop: IPC
loss, listener failure, or a `DbError` from `tick()` logs the error, stops
frame processing immediately (nothing dispatched after a failed durable
write), drops the endpoint, and exits non-zero. Restart is the supervisor's
job (systemd / container runtime / operator); no in-process restart loop.
Retry tiers only become worth having with a networked Postgres and can be
added in front of fail-stop without structural change; PLAN.md's M7-E
(Postgres backend) now records that requirement.

## Testing (TDD)

- `mud-core`: `Scheduler::drain` unit tests (order, tick increment,
  emptying).
- `mud-db`: `submit` + `tick` write-through tests (arena and DB agree after a
  tick; a rejected command persists nothing; first `DbError` stops the
  drain), `hydrate` tests (resident after hydrate; location/inventory
  replayed; unknown key errors), `world_id` get-or-create stability test.
- `mud-engine`: pipeline tests updated — dispatch over `&World`, effects
  returned in `DispatchOutcome`, no direct arena mutation.
- `mudd`: thin `main.rs` over internal modules with a testable
  `run(CliArgs)`; integration tests boot temp tenant dirs on ephemeral
  ports and connect real `TcpStream`s: (a) single tenant — register →
  create puppet → enter → one command, asserting outputs; (b) **two tenants
  concurrently** — both listeners serve logins, and the same username
  registers independently on each (per-file isolation, §2.5.1.4).
  Config-layering unit tests (defaults < file < env < flags; `--tenant-dir`
  replaces the registry; duplicate listen addresses and duplicate
  `tenant_tag`s rejected). Full restart-persistence acceptance remains
  M1-23.

## Documentation

New operator page under `docs/docs/` ("Running a server") + `nav` entry in
`mkdocs.yml`; verified with `uv run mkdocs build --strict`. Contents:

- CLI flags, the server config (`$XDG_CONFIG_HOME/ferrodun/config.toml`:
  `rate`, `burst`, the `[[tenants]]` registry), `MUDD_*` env overrides, and
  how server-wide vs per-tenant configuration split.
- **Running under a supervisor (Linux only for now):** `mudd` is fail-stop
  by design — on an unrecoverable error it exits non-zero and expects a
  supervisor to restart it (state is rebuilt from the database at boot). The
  page ships a worked systemd unit example (`Restart=on-failure`,
  `RestartSec`, plus a start-limit guard against crash loops on persistent
  faults such as a full disk) and notes the equivalent expectation for
  container runtimes (`restart: on-failure`).

## Out of scope

- Split-mode (unix-socket) deployment and its feature flag (§5.2) — later
  milestone.
- Gateway hold-open / reconnect on World loss (M7).
- Background snapshots (§2.5.3.4).
- Surfacing tick rejections to the originating session (M3 structured
  channel).
- Shared-listener / host-based tenant routing (each tenant has its own
  listen address in M1).
- Per-tenant supervision: in M1 a fatal error in any tenant fail-stops the
  whole process; keeping sibling tenants alive through one tenant's failure
  (§ "stopping tenant B must not stop A", SPEC line on tenant lifecycle) is
  a later milestone alongside admin start/stop.
