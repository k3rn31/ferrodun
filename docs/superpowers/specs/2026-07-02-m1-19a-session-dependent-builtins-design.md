# M1-19a — Session-dependent built-ins (`who`, `quit`, broadcast): Design

**Date:** 2026-07-02
**Spec:** §2.7 step 8, §3.6.3, §3.19
**Status:** Draft — awaiting review

## Goal

Complete the M1 command set with the three features M1-17 deferred until the
session→entity map existed (it now does, from M1-19):

- **Cross-player broadcast** — `say` reaches co-located players; movement emits
  arrival/departure lines to the rooms entered and left.
- **`who`** — list connected players.
- **In-world `quit`** — cleanly signal the session to close.

Delivered as **one PR** (decided with the user): broadcast is the architectural
core; `who`/`quit` reuse the same session-registry read seam.

## Context

Four facts about the current codebase shape the design:

1. **`Pipeline::dispatch` returns `Vec<SessionOutput>`, but every element is
   addressed to the caller's own `session_id`.** `SessionOutput` already carries
   a `session_id`, so multi-recipient output is representable — nothing produces
   it yet. `dispatch.rs`'s `CommandReply` doc names the broadcast slot as "the
   next slot on this type," explicitly deferred to M1-19a.
2. **The entity→session map lives in `SessionService.sessions`** (`InWorldBinding
   { account, puppet: EntityId }`). The pipeline reaches the registry only
   through the `SessionResolver` port (`RegistryResolver` borrows `&sessions`).
   There is no place→sessions or roster seam.
3. **`quit` exists only pre-login** (FSM `Terminal::Closed`). In-world, a
   built-in command has no way to signal "close this connection" — `dispatch`
   returns only outputs.
4. **The M1-22 driver (which will own both `SessionService` and the gateway)
   does not exist yet.** M1-19a wires seams and tests them via the M1-19
   integration harness; the gateway socket teardown lands at M1-21/22.

## Decisions (settled with the user during brainstorming)

1. **Player display name = the real `PuppetName`**, carried into the in-world
   binding — not the M1-17 first-keyword hack. Requires threading the name
   through `Terminal::Bound` (mud-session) → `InWorldBinding` (mud-engine).
2. **`quit` is signal-only**: the pipeline's return type grows to carry a "close
   this session" disposition. The socket teardown is M1-21/22; M1-19a delivers
   the signal + goodbye.
3. **Arrival text is directional** ("Arden arrives from the east") via a new
   `Direction::opposite()` in mud-core. A directionless key (`move.arrive`) is
   also defined for the future spawn/portal path ("arrives from nowhere",
   M1-22 hydration), but M1-19a movement always has a direction.
4. **Broadcast i18n is per-world, not per-recipient.** A broadcast carries
   pre-rendered `StyledText` (rendered once in the world/caller locale) and is
   fanned to every recipient — correct under the per-world locale model. This
   contradicts the current SPEC §3.14.6 (per-session), reworked as a **separate
   follow-up task** (see below), not this PR.

## Architecture

Handlers stay **session-ignorant**. A handler emits a *domain* audience (`place`
+ `except` = the actor to exclude) plus a styled message; the **pipeline**
resolves it against the **pre-effect** world via a new `Roster` port. Pre-effect
resolution makes both movement cases fall out correctly: departure sees the mover
still in the old room (excluded via `except`); arrival sees the destination's
occupants (mover not yet added). Broadcasts resolve *before* effects apply,
consistent with the existing reply/effect-ordering contract (`dispatch.rs`).

### mud-core (`src/place/room.rs`)

`Direction::opposite(self) -> Direction` (N↔S, E↔W, U↔D). Small, reusable (M4
movement/FOV will want it). Unit-tested over all six directions + double-apply
identity.

### mud-session (`src/fsm.rs`)

`Terminal::Bound` gains `name: PuppetName`. The FSM's `PuppetSelect`/
`AwaitingEnter` states already hold the full `Puppet`, so the chosen puppet's
name threads into `AwaitingEnter` and out through `Terminal::Bound`.

### mud-engine

- **New `Roster` port** (`src/roster.rs`) — the session-registry read seam:
  - `session_of(&self, entity: EntityId) -> Option<SessionId>` — reverse map for
    fan-out.
  - `connected(&self) -> Vec<Presence>`, `Presence { name: PuppetName }` — for
    `who`.
  Implemented by `RegistryResolver` (already borrows `&sessions`); the test
  `FakeResolver` implements it too.
- **`caller.rs`** — `CallerContext` gains `name: PuppetName` (the actor's display
  name used in broadcast text); `RegistryResolver::resolve` fills it from the
  binding.
- **`session/mod.rs`** — `InWorldBinding` gains `name`; `apply_terminal` stores
  it from `Terminal::Bound`; `RegistryResolver` implements `Roster`.
  (`SessionService::disconnect` already exists for the driver's close path.)
- **`dispatch.rs`**:
  - `Broadcast { place: PlaceId, except: EntityId, message: StyledText }`.
  - `CommandReply` gains `broadcasts: Vec<Broadcast>` (`with_broadcast`) and a
    private `disposition: SessionDisposition` (`Remain`/`Close`) via a `closing()`
    builder.
  - `CommandContext` gains `roster: &dyn Roster` (`roster()` — for `who`) and a
    `caller_name()` accessor (for broadcast text).
- **`pipeline.rs`**:
  - `dispatch` returns `DispatchOutcome { outputs: Vec<SessionOutput>,
    disposition: SessionDisposition }`; the resolver bound becomes
    `SessionResolver + Roster`.
  - `run_matched` order: run handler (read-only world) → render caller output →
    resolve broadcasts pre-effect (`world.occupants_of(place)` filter `!= except`,
    map via `roster.session_of`, flatten `message.to_plain_string()`) → apply
    effects → carry the reply disposition into the outcome.
- **`builtins.rs`**:
  - `Say` — keep the caller echo; add a broadcast to `{ location, except: caller,
    say.broadcast(name, message) }`.
  - `Move` — add departure `{ from, except: caller, move.depart(name, dir) }` and
    arrival `{ to, except: caller, move.arrive-from(name, opposite(dir)) }`.
  - New `Quit` — `CommandReply::to_caller(quit.goodbye).closing()`; table entry
    `("quit", &["q"], …)`.
  - New `Who` — `ctx.roster().connected()` rendered via `who.online` (joined
    names, mirroring `look.also-here`); table entry `("who", &[], …)`.

### mud-i18n (`src/catalog.rs`, additive `en` keys)

`say.broadcast`, `move.depart`, `move.arrive-from`, `move.arrive` (directionless,
future path), `dir.north`…`dir.down` (localizable direction words),
`who.online`, `quit.goodbye`.

### §3.6.3 (present NPCs hear say/emote) — design note, no code

The audience is *occupants* (entities); fan-out to sessions naturally skips
session-less entities (NPCs). When NPCs land (M5) the same occupants list feeds
NPC perception, so the audience model is already the right shape. Nothing to
build in M1-19a; the rationale is recorded in the journal.

## Error handling & conventions

- New `Roster`/`Broadcast`/`SessionDisposition`/`DispatchOutcome` are
  workspace-internal, matched exhaustively (no `#[non_exhaustive]`, per the M1-19
  precedent for co-located internal enums). `PipelineError` keeps its
  `#[non_exhaustive]`.
- No third-party error leaks; no `unwrap`/`expect`/`panic` outside tests. Fan-out
  uses `world.occupants_of` + `.filter` + `.filter_map` (no indexing).
- Broadcast rendering flattens `StyledText` → plain text for M1, matching the
  caller-reply flattening (the styled-text-over-IPC swap stays deferred to
  M1-21/22).

## Scope boundaries (deferred)

- Gateway socket teardown on `quit` (consuming the close disposition) — M1-21/22.
- Directionless `move.arrive` wiring for mid-session spawn/portal — M1-22
  hydration.
- Per-recipient locale rendering — not applicable (locale is per-world).
- Linkdead/idle/ping — M7.

## Follow-up task (separate PR, right after M1-19a): i18n per-world locale rework

Locale is a per-tenant/world property, not per-session. This must be reworked in
spec **and** implementation as its own task (do **not** fold into M1-19a):

1. **SPEC §3.14** — rewrite §3.14.6 as per-tenant locale (single tenant-configured
   locale; `en` default/reference; no per-session resolution order); remove
   §3.14.6.3 mid-session switching; remove GMCP `Core.Locale` (§2.8.3.3 / §2.1.4)
   as a client switch (the announced locale becomes the tenant's); §3.14.7.1 LLM
   speech → tenant locale; remove `mud.i18n.locale_of` (§3.14.4.2); §3.14.5.2/.3
   aliases/help → "the tenant's locale"; §3.14.8.1 acceptance → "renders in the
   tenant's configured locale." Keep §3.14.2 (`en`) and §3.14.3 (bundle
   discovery).
2. **PLAN §M2-I** — drop "locale resolution per session," `locale_of`, and
   "active session's locale"; restate as tenant-locale selection.
3. **Implementation** — remove per-caller locale plumbing (`CallerContext.locale`
   and the per-session `locale` threaded through the pipeline/render) in favor of
   one tenant/world locale sourced from tenant config (wired at M1-22). `t!` /
   `Catalog` stay keyed by `Locale`; only how the locale is *sourced* changes.

## Testing

- **mud-core** — `Direction::opposite` over all six + double-apply identity.
- **mud-session** — `Terminal::Bound` carries the puppet name through the enter
  and create-then-enter paths.
- **mud-engine** (unit/integration via the `tests/builtins.rs` harness + a fake
  `Roster`): two co-located sessions — A `say` → B receives `say.broadcast`, A
  gets the echo; A moves out of B's room → B sees `move.depart`; A moves into
  C's room → C sees `move.arrive-from` (opposite direction); broadcast excludes
  the actor; a session-less occupant is skipped. `who` lists both puppet names;
  `quit` → `DispatchOutcome` disposition `Close` + `quit.goodbye`.
- **Integration** (`tests/session_login.rs` style, real `mud-db` backend) — a
  two-session broadcast end-to-end.
- **Gates** — `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -D warnings`, `cargo fmt --all --check`, `uv run mkdocs build --strict`.

## Documentation

Update `docs/docs/playing/commands.md`: add `who` and `quit`, and note that
`say` and movement are now heard by other players in the room.

## Known tension

Principle #3 (one crate's public API per PR) is stretched: mud-core
(`Direction::opposite`), mud-session (`Terminal::Bound`), and mud-engine public
surfaces all change. The user chose one-PR scope including the mud-core and
mud-session touches; both are additive and small. Flagged, not blocking.
