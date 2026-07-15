# Room presence: spawn/leave announcements and players in `look` — design

**Date:** 2026-07-15
**Status:** approved

## Problem

Three presence gaps make players invisible to each other outside of movement
and `say` (SPEC §2.7 step 8, §3.6.3):

1. **Spawning is silent.** When a session binds a puppet (login or first-ever
   enter of a new puppet), nobody in the destination room is told. The
   directionless `move.arrive` key was reserved for this at M1-19a but never
   wired.
2. **Leaving the game is silent.** `quit` replies "goodbye" to the caller and
   closes the session; a socket drop removes the binding via
   `GatewayFrame::Disconnect`. Neither path tells the room, and the puppet's
   body silently remains where it stood.
3. **`look` hides players.** The "also here" list resolves names through
   `keywords_of(entity)`; player puppets carry no keywords (their name lives
   in the session binding as `PuppetName`), so they are silently filtered out
   — a known M1-17 limitation.

Room-to-room movement already broadcasts departure and directional arrival
(M1-19a) and is untouched by this design.

## Decisions

- **Disconnect semantics: the body stays, but hidden.** World state is not
  mutated on quit/drop — the puppet's location persists so re-login returns
  the player where they left. Presence is *session-based*: only connected
  players appear in `look` and in announcements. Full linkdead handling
  remains M7.
- **Quit and drop read identically to observers.** Both emit the same
  disappearance message; an observer cannot distinguish a clean quit from
  a dropped connection.
- **Announcements stay in-fiction.** No out-of-world phrasing like
  "the game": entering reads as appearing, leaving as disappearing. Both
  messages always name exactly one puppet (each enter/leave is its own
  event), so they need no plural form; the only multi-subject text is the
  `look` players line, which has distinct singular/plural keys.
- **Players and objects render differently in `look`.** Every major MUD
  lineage separates characters from things (Diku/ROM: one authored sentence
  per character and per object; Evennia: separate `Characters:` and
  `You see:` template slots). Ferrodun adopts a collapsed Diku-voice
  sentence for players — one line regardless of head-count — and keeps the
  existing keyword list for everything else.
- **New i18n keys, not `move.arrive`.** Logging in and stepping through a
  portal are different fictional events; `move.arrive` stays reserved for
  portals/teleports.

## Player-visible behavior

- **Spawn:** everyone else in the destination room sees
  `Alice appears from nowhere.` (key `presence.enter`, `system` role). The
  newcomer sees the room render, as today.
- **Quit / socket drop:** everyone else in the room sees
  `Alice disappears.` (key `presence.leave`, `system` role).
- **`look`:** after the exits line, connected players collapse into one
  Diku-voice sentence, then objects keep the current list:

  ```
  > look
  The Rusty Anchor Tavern
  A smoky taproom with a low ceiling.
  Exits: north, east
  Alice, Bob and Carol are here.
  Also here: sword, lantern.
  ```

  Singular form: `Alice is here.` (`look.player-here`); plural:
  `Alice, Bob and Carol are here.` (`look.players-here`). The English
  "and"-join is a small Rust helper; the tracked M2-I per-locale i18n rework
  owns proper list formatting and absorbs it. Disconnected puppets do not
  appear. The viewer never sees themself in the list.

## Architecture

All changes live in `mud-engine`, `mudd`, and the `en` catalog; `mud-core`
and `mud-session` are untouched.

1. **`Roster` grows a name lookup.**
   `Roster::name_of(entity) -> Option<PuppetName>` alongside `session_of` /
   `connected`, implemented by the production `RegistryResolver` over the
   live bindings. This is the single primitive both features need: "is this
   occupant a connected player, and what is their name?"

2. **`look` partitions occupants.** `render_room` gains a `&dyn Roster`
   parameter (both callers — `look` and the movement arrival render — already
   hold `ctx.roster()`). Occupants with a roster name render as the players
   sentence; the rest fall through to the existing keyword-based
   `look.also-here` list, unchanged. Until M2-F archetypes land, "has a
   session" *is* the player/object distinction available at runtime.

3. **Shared `presence` announcement helper.** The audience-resolution logic
   inside `Pipeline` (occupants of a place, minus an excluded entity, mapped
   to sessions via `Roster`, rendered per-session as `SessionOutput`s) is
   extracted into one engine function —
   `presence::announce(world, roster, place, except, message)`. The
   pipeline's existing `say`/movement fan-out is refactored onto it so the
   codebase keeps exactly one audience-resolution implementation.

4. **Three lifecycle call sites in `mudd::world_loop`.**
   - **Spawn:** the login routing result signals when a session has just
     bound a puppet (the `Routing::Login` variant carries the newly-bound
     entity); `world_loop` announces `presence.enter` to the puppet's room,
     excluding the newcomer.
   - **Quit:** when dispatch returns the `Close` disposition, `world_loop`
     announces `presence.leave` before unbinding and sending the close
     frame.
   - **Socket drop:** on `GatewayFrame::Disconnect`, before
     `sessions.disconnect()`, if the session was in-world, announce
     `presence.leave`.

   The gateway sends no `Disconnect` echo for a world-initiated close, so
   quit and drop are disjoint paths — no double announcement is possible.

## Edge cases

- **Empty room:** the helper resolves an empty audience and sends nothing.
- **Disconnect during login** (no puppet bound): no announcement.
- **Brand-new puppet** (register → create → enter): same "just bound"
  signal, same `presence.enter`.
- **Ordering:** both leave paths announce *before* unbinding so the roster
  still resolves the audience; the leaver is excluded via `except`.
- **The hidden body:** a disconnected puppet still occupies the room but has
  no keywords, so it cannot be targeted by `get`/`look <name>` — invisible
  and untargetable, consistently. **Note for M2-F:** when archetypes give
  actors display names, gate room listings and object resolution on session
  presence too, or disconnected bodies resurface.
- **`look` composition:** the players sentence and `Also here:` line render
  independently; any combination of present/absent works.

## Testing

- **`mud-engine` unit/integration:**
  - `presence::announce`: audience mapping, `except` exclusion, session-less
    occupants skipped, empty room yields no outputs.
  - `look` partition: singular/plural sentence with the and-join, objects
    stay keyword-listed, occupant with neither roster entry nor keywords is
    skipped, viewer excluded.
  - Pipeline refactor regression gate: every existing `tests/broadcast.rs`
    test stays green untouched.
- **`mudd` e2e (two telnet sessions):** Bob logs in → Alice sees
  `Bob appears from nowhere.`; `look` shows `Bob is here.`; Bob quits →
  Alice sees `Bob disappears.`; Bob reconnects and the socket is hard-dropped
  → Alice sees the same disappearance message; after either leave, `look` no
  longer shows Bob.
- **Docs:** update the Playing pages that describe `look` and session
  behavior in the same PR.

## Out of scope

- Linkdead grace periods, reconnect notices, idle handling (M7).
- Item/actor archetypes and authored long descriptions (M2-F, M5).
- Locale-aware list formatting (M2-I i18n rework).
- Any change to movement departure/arrival broadcasts (M1-19a, done).
