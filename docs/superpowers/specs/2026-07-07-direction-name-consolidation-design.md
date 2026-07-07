# Consolidate the `Direction`↔word contract onto `Direction` (issue #37)

**Date:** 2026-07-07
**Issue:** #37 — Duplicated `direction_name` helper in `mud-engine` and `mud-world`
**Status:** Approved design

## Problem

The mapping between a `Direction` and its canonical English word is spread across
three sites in two crates, with the forward and inverse maps hand-written
independently so they can silently disagree:

| Site | What | Direction |
|---|---|---|
| `crates/mud-engine/src/builtins/movement.rs:75` | `direction_name` | `Direction → &str` |
| `crates/mud-engine/src/builtins/movement.rs:87` | `DIRECTIONS: [Direction; 6]` | ordering constant |
| `crates/mud-world/src/rooms.rs:325` | `direction_name` | `Direction → &str` (identical copy) |
| `crates/mud-world/src/rooms.rs:310` | `parse_direction` | `&str → Direction` (**inverse** map) |

`mud-world` uses the words to parse KDL exit definitions; `mud-engine` uses them
to render exit lists and phrase move broadcasts. It is one shared authoring/wire
contract. Nothing structurally prevents the forward and inverse maps from
drifting apart — only a local round-trip test in `rooms.rs` guards one side.

`Direction` already lives in `crates/mud-core/src/place/room.rs` next to
`opposite()`, and both `mud-world` and `mud-engine` already depend on `mud-core`.

## Design principle

`Direction` is a domain concept; its word form is intrinsic to the type, not to
any consumer. So the contract belongs on `Direction` in `mud-core`, with the two
consumer crates (both boundary/adapter layers) calling inward.

The core idea is to **eliminate the divergence bug by construction, not by
test**: define a single authoritative forward map and *derive* the inverse from
it, so the two directions cannot disagree.

## What lands in `mud-core`

On `Direction` in `crates/mud-core/src/place/room.rs`:

- `pub const fn name(self) -> &'static str` — the single authoritative forward
  map. An exhaustive `match` with **no `_` catch-all**, so adding a `Direction`
  variant is a compile error at this one canonical site.
- `pub const ALL: [Direction; 6]` — the canonical display/iteration order
  (`North, East, South, West, Up, Down` — matches both existing orderings).
- `impl FromStr for Direction` — the inverse map, **derived** by searching `ALL`
  for the variant whose `name()` equals the input. Because the inverse is
  computed from the forward map, the two cannot drift.
- `pub struct ParseDirectionError { value: String }` (via `thiserror`) — the
  domain-level "this string is not a direction" error, owned by `mud-core`.
  Carries the offending value so callers can build their own messages.

`FromStr` is chosen over an inherent `parse()` because it is the idiomatic
"parse a string into this type" trait: it yields `.parse::<Direction>()` for
free and expresses parse-at-the-boundary intent.

### Tests in `mud-core` (single owner of the invariant)

- **Round-trip:** for every `Direction::ALL`, `d.name().parse() == Ok(d)`.
- **Word uniqueness:** the six words returned by `name()` are all distinct
  (guards that `ALL` is complete and no two variants share a word).

## Changes in the consumer crates

### `mud-engine` (`builtins/movement.rs`, `builtins/look.rs`)

- Delete the local `direction_name` fn and `DIRECTIONS` const.
- Replace `direction_name(x)` call sites with `x.name()`.
- Replace `DIRECTIONS` with `Direction::ALL`; update the `look.rs` import.

### `mud-world` (`rooms.rs`, `error.rs`)

- Delete the local `direction_name` and `parse_direction` fns.
- Replace `direction_name(direction)` with `direction.name()`.
- Replace `parse_direction(v)` with
  `Direction::from_str(v).map_err(|_| WorldError::UnknownDirection { value: v.to_owned() })`.
- **Keep** `WorldError::UnknownDirection`. This is the clean-architecture
  boundary move: the world-loading layer wraps the core error with its own
  authoring context ("unknown exit direction: …") and stays in control of its
  public error surface, rather than leaking `mud-core`'s `ParseDirectionError`
  shape through `WorldError`.
- The exhaustive parse/round-trip tests move to `mud-core` (the single owner).
  `mud-world` keeps only a small test that its error *mapping* is correct: an
  unknown word yields `WorldError::UnknownDirection { value }` with the offending
  value preserved.

## Non-goals / YAGNI

- No `strum` (or any macro crate) to count variants at compile time. The
  no-catch-all `match` in `name()` is the compile-time tripwire; the round-trip
  and uniqueness tests cover completeness. Introducing a dependency for this is
  not warranted.
- No change to the set of directions, their order, or their words. This is a
  pure consolidation — behavior is unchanged.

## Definition of Done

- `name`, `ALL`, `FromStr`, and `ParseDirectionError` live on/near `Direction`
  in `mud-core`; round-trip and uniqueness tests pass there.
- No `direction_name`, `DIRECTIONS`, or `parse_direction` remains in
  `mud-engine` or `mud-world`; both call the `mud-core` API.
- `WorldError::UnknownDirection` still carries the offending value, verified by a
  mapping test.
- Workspace builds clean under `cargo clippy` (no new lint suppressions) and all
  tests pass.
- No observable player/builder/operator behavior changes, so no `docs/` update
  is required (internal refactor).
