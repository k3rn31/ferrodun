# Direction↔word Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the `Direction`↔canonical-word contract onto `Direction` in `mud-core` so `mud-engine` and `mud-world` stop hand-maintaining duplicate, divergable copies.

**Architecture:** `Direction` (in `mud-core`) gains a single authoritative forward map `name()`, an ordering constant `ALL`, and a `FromStr` impl whose inverse map is *derived* from `name()` + `ALL` (so the two directions cannot silently disagree). A domain error `ParseDirectionError` is owned by `mud-core`; `mud-world` wraps it into its existing `WorldError::UnknownDirection`. Both consumer crates delete their local helpers and call inward.

**Tech Stack:** Rust (workspace), `thiserror` for the library error type, `cargo test` / `cargo clippy`.

## Global Constraints

- Code and comments in English. Comment *why*, not *how* (project CLAUDE.md).
- `unwrap()` forbidden everywhere; `expect()` allowed **only in tests** with a descriptive message.
- Libraries define error types with `thiserror`; never leak third-party errors through public API.
- Newtype/type-driven design: exhaustive `match` with **no `_` catch-all** so new `Direction` variants surface as compile errors.
- Public error types: derive `Debug`, `thiserror::Error`; apply `#[non_exhaustive]` where the project does (matches `WorldError`, `ArenaError`, etc.).
- Must compile clean under `cargo clippy` (workspace denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`). **No lint suppressions.**
- Add dependencies only with `cargo add` — never hand-edit `Cargo.toml`. (None needed here: `thiserror` is already a `mud-core` dependency.)
- VCS is **jj (Jujutsu)**, not git. Use `jj commit -m "…"` to finish a change; do not use `git commit`.
- Design doc: `docs/superpowers/specs/2026-07-07-direction-name-consolidation-design.md`.

---

## File Structure

- `crates/mud-core/src/place/room.rs` — add `Direction::name`, `Direction::ALL`, `ParseDirectionError`, `impl FromStr for Direction`, and their tests (in the existing `#[cfg(test)] mod tests`).
- `crates/mud-core/src/place/mod.rs` — re-export `ParseDirectionError` from `room`.
- `crates/mud-core/src/lib.rs` — re-export `ParseDirectionError` at the crate root (next to `Direction`).
- `crates/mud-engine/src/builtins/movement.rs` — delete local `direction_name` + `DIRECTIONS`; call `Direction::name` / `Direction::ALL`.
- `crates/mud-engine/src/builtins/look.rs` — drop the `super::movement::{DIRECTIONS, direction_name}` import; use `Direction::ALL` / `dir.name()`.
- `crates/mud-world/src/rooms.rs` — delete local `direction_name` + `parse_direction`; call `direction.name()` and `value.parse::<Direction>()` with error mapping; relocate exhaustive tests, keep an error-mapping test.

---

### Task 1: Add the `Direction`↔word contract to `mud-core`

**Files:**
- Modify: `crates/mud-core/src/place/room.rs` (add methods/const/error near existing `opposite()` at line 44; add tests in the `#[cfg(test)] mod tests` block)
- Modify: `crates/mud-core/src/place/mod.rs:21`
- Modify: `crates/mud-core/src/lib.rs:21`
- Test: `crates/mud-core/src/place/room.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `Direction::name(self) -> &'static str` (const fn) — canonical English word.
  - `Direction::ALL: [Direction; 6]` — order `North, East, South, West, Up, Down`.
  - `struct ParseDirectionError { value: String }` — `pub`, `#[non_exhaustive]`, `thiserror::Error`, with a public accessor `value(&self) -> &str`.
  - `impl FromStr for Direction { type Err = ParseDirectionError; }`.

- [ ] **Step 1: Write the failing tests**

Add these to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/mud-core/src/place/room.rs` (the block already `use`s the `Direction` variants — reuse whatever `use super::*;` / import is present; if `FromStr` is not in scope, add `use std::str::FromStr;` inside the test module):

```rust
    #[test]
    fn every_direction_round_trips_through_its_name() {
        for dir in Direction::ALL {
            assert_eq!(
                dir.name().parse::<Direction>().expect("name must parse back"),
                dir,
            );
        }
    }

    #[test]
    fn direction_names_are_all_distinct() {
        let mut names: Vec<&str> = Direction::ALL.iter().map(|d| d.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Direction::ALL.len(), "two directions share a word");
    }

    #[test]
    fn all_lists_the_six_directions_in_canonical_order() {
        use Direction::{Down, East, North, South, Up, West};
        assert_eq!(Direction::ALL, [North, East, South, West, Up, Down]);
    }

    #[test]
    fn unknown_word_fails_to_parse_and_preserves_the_input() {
        let error = "sideways".parse::<Direction>().expect_err("not a direction");
        assert_eq!(error.value(), "sideways");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mud-core --lib place::room`
Expected: FAIL to compile — `no function or associated item named 'ALL'` / `Direction` doesn't implement `FromStr` / `name` not found.

- [ ] **Step 3: Add `name`, `ALL`, `ParseDirectionError`, and `FromStr`**

In `crates/mud-core/src/place/room.rs`, add `use std::str::FromStr;` to the file's top-level imports (alongside the existing `use super::id::PlaceId;` / `use crate::{...};` lines). Then extend the existing `impl Direction { … }` block (which currently holds `opposite()`), and add the error type + `FromStr` impl after it:

```rust
impl Direction {
    // ... existing opposite() stays here ...

    /// The six directions in canonical display/iteration order
    /// (N, E, S, W, U, D — §3.2.2).
    pub const ALL: [Direction; 6] = [
        Self::North,
        Self::East,
        Self::South,
        Self::West,
        Self::Up,
        Self::Down,
    ];

    /// The canonical English word for this direction. Invariant across
    /// locales (§3.14.5.1): it is the authored/wire token, not display text.
    ///
    /// This is the single authoritative forward map; [`FromStr`] derives the
    /// inverse from it, so the two cannot disagree.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::East => "east",
            Self::South => "south",
            Self::West => "west",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// A string did not name any [`Direction`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error("unknown direction: {value:?}")]
pub struct ParseDirectionError {
    value: String,
}

impl ParseDirectionError {
    /// The offending input that failed to parse.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl FromStr for Direction {
    type Err = ParseDirectionError;

    /// Inverse of [`Direction::name`], derived by searching [`Direction::ALL`]
    /// so it can never drift from the forward map.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|dir| dir.name() == s)
            .ok_or_else(|| ParseDirectionError { value: s.to_owned() })
    }
}
```

- [ ] **Step 4: Re-export `ParseDirectionError`**

In `crates/mud-core/src/place/mod.rs`, add `ParseDirectionError` to the `room` re-export (line 21):

```rust
pub use room::{Description, Direction, ParseDirectionError, Place, RoomData, Title};
```

In `crates/mud-core/src/lib.rs`, add it to the `place` re-export (line 21):

```rust
pub use place::{Description, Direction, ParseDirectionError, Place, PlaceId, PlaceKey, PlaceKeyError, RoomData, Title};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p mud-core --lib place::room`
Expected: PASS (including the four new tests).

- [ ] **Step 6: Clippy check**

Run: `cargo clippy -p mud-core --all-targets`
Expected: no warnings, no errors.

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(mud-core): add Direction::name, ALL, and FromStr (issue #37)"
```

---

### Task 2: Switch `mud-engine` to the `mud-core` API

**Files:**
- Modify: `crates/mud-engine/src/builtins/movement.rs` (delete `direction_name` at 73-84 and `DIRECTIONS` at 86-94; update call sites at 50 and 59)
- Modify: `crates/mud-engine/src/builtins/look.rs` (import at line 6; `exit_names` body at ~68-74)

**Interfaces:**
- Consumes: `Direction::name(self) -> &'static str`, `Direction::ALL: [Direction; 6]` (Task 1).

- [ ] **Step 1: Update `movement.rs` call sites and delete the local helpers**

In `crates/mud-engine/src/builtins/movement.rs`, replace `direction_name(self.0)` (line 50) with `self.0.name()` and `direction_name(self.0.opposite())` (line 59) with `self.0.opposite().name()`. Then delete the entire `direction_name` fn (lines 73-84) and the `DIRECTIONS` const (lines 86-94), including their doc comments.

- [ ] **Step 2: Update `look.rs` to use `Direction::ALL` and `name()`**

In `crates/mud-engine/src/builtins/look.rs`:

Replace the import at line 6:

```rust
use super::{display_name, system};
```

(i.e. drop `use super::movement::{DIRECTIONS, direction_name};` entirely.)

Add `Direction` to the existing `mud_core` import on line 3 so it reads:

```rust
use mud_core::{Direction, EntityId, Place, PlaceId, RoleName, Span, StyledText, World};
```

Rewrite `exit_names` to use the core API:

```rust
/// names of wired exits in `place`, in N/E/S/W/U/D order.
fn exit_names(place: &Place) -> Vec<&'static str> {
    Direction::ALL
        .into_iter()
        .filter(|&dir| place.neighbor(dir).is_some())
        .map(Direction::name)
        .collect()
}
```

- [ ] **Step 3: Build and test the crate**

Run: `cargo test -p mud-engine`
Expected: PASS — no references to `direction_name` or `DIRECTIONS` remain, and existing movement/look tests still pass.

- [ ] **Step 4: Clippy check**

Run: `cargo clippy -p mud-engine --all-targets`
Expected: no warnings, no errors (in particular, no unused-import warning for the removed helpers).

- [ ] **Step 5: Commit**

```bash
jj commit -m "refactor(mud-engine): use Direction::name/ALL from mud-core (issue #37)"
```

---

### Task 3: Switch `mud-world` to the `mud-core` API

**Files:**
- Modify: `crates/mud-world/src/rooms.rs` (delete `parse_direction` at 309-322 and `direction_name` at 324-334; update call sites at 238 and 287; relocate/trim tests around 393-427)
- Test: `crates/mud-world/src/rooms.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Direction::name`, `Direction::from_str` / `str::parse::<Direction>`, `ParseDirectionError` (Task 1).
- `WorldError::UnknownDirection { value: String }` is unchanged and retained.

- [ ] **Step 1: Replace the `parse_direction` call site with core `parse` + error mapping**

In `crates/mud-world/src/rooms.rs`, `parse_exit` (line 238) currently calls `parse_direction(...)`. Replace that call so the authored word is parsed via `mud-core` and the domain error is mapped into `WorldError::UnknownDirection`. The current line is:

```rust
    let direction = parse_direction(arg(node, 0).ok_or(WorldError::MissingField {
```

Change the parse expression to (keeping the surrounding `arg(node, 0).ok_or(WorldError::MissingField { … })?` exactly as-is — only the `parse_direction(...)` wrapper changes):

```rust
    let word = arg(node, 0).ok_or(WorldError::MissingField {
        // ... keep the existing MissingField fields unchanged ...
    })?;
    let direction = word.parse::<Direction>().map_err(|error| {
        WorldError::UnknownDirection {
            value: error.value().to_owned(),
        }
    })?;
```

> Note for the implementer: read lines 236-246 first and preserve the exact `MissingField { node, field }` construction that is already there; the only behavioral change is `parse_direction(word)` → `word.parse::<Direction>().map_err(...)`.

- [ ] **Step 2: Replace the `direction_name` call site**

At line 287 (inside the `DanglingExit` construction), replace `direction_name(direction).to_owned()` with `direction.name().to_owned()`.

- [ ] **Step 3: Delete the local helpers**

Delete the `parse_direction` fn (lines 309-322, with its doc comment) and the `direction_name` fn (lines 324-334, with its doc comment).

- [ ] **Step 4: Relocate the exhaustive parse tests; keep a mapping test**

In the `#[cfg(test)] mod tests` block, the existing tests call the now-deleted `parse_direction`/`direction_name` (around lines 393-427). The exhaustive per-word round-trip now lives in `mud-core` (Task 1), so remove the tests that duplicate it (`parse_direction("north") == North` … and the local round-trip loop). Replace them with a single test that proves `mud-world`'s **error mapping** is correct — i.e. an unknown authored word surfaces as `WorldError::UnknownDirection` carrying the offending value. Add:

```rust
    #[test]
    fn unknown_exit_word_maps_to_world_error() {
        let error = "sideways"
            .parse::<Direction>()
            .map_err(|error| WorldError::UnknownDirection {
                value: error.value().to_owned(),
            })
            .expect_err("sideways is not a direction");
        assert!(
            matches!(error, WorldError::UnknownDirection { ref value } if value == "sideways"),
        );
    }
```

> If a test module import referenced `parse_direction`/`direction_name` (e.g. via `use super::*;`), no import change is needed; if any test named them directly in a `use`, remove that line.

- [ ] **Step 5: Build and test the crate**

Run: `cargo test -p mud-world`
Expected: PASS — no references to `parse_direction`/`direction_name` remain; exit loading and the new mapping test pass.

- [ ] **Step 6: Clippy check**

Run: `cargo clippy -p mud-world --all-targets`
Expected: no warnings, no errors.

- [ ] **Step 7: Full workspace verification**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all tests pass; clippy clean across the workspace.

- [ ] **Step 8: Commit**

```bash
jj commit -m "refactor(mud-world): use Direction::name/FromStr from mud-core (issue #37)"
```

---

### Task 4: Journal entry

**Files:**
- Modify: `.claude/JOURNAL.md` (append at the end)

- [ ] **Step 1: Append the journal entry**

Add this entry at the bottom of `.claude/JOURNAL.md`:

```markdown
## 2026-07-07 — Consolidate Direction↔word contract (issue #37)

- **Spec:** §2.2.2, §3.2.2, §3.14.5.1 — one canonical word per direction, shared authoring/wire token.
- **Done:** Added `Direction::name`, `Direction::ALL`, `ParseDirectionError`, and `impl FromStr for Direction` in `mud-core` (inverse derived from the forward map). Deleted the duplicate `direction_name`/`DIRECTIONS`/`parse_direction` helpers from `mud-engine` and `mud-world`; `mud-world` maps the core error into its existing `WorldError::UnknownDirection`.
- **Verify:** New round-trip + uniqueness + parse tests in `mud-core`; error-mapping test in `mud-world`; `cargo test --workspace` and `cargo clippy --workspace --all-targets` clean.
- **Next:** None — pure consolidation, no behavior change, no docs impact.
```

- [ ] **Step 2: Commit**

```bash
jj commit -m "docs: journal entry for Direction consolidation (issue #37)"
```

---

## Self-Review

**Spec coverage:**
- `name()` single forward map, exhaustive no-`_` match → Task 1, Step 3. ✅
- `ALL` ordering constant → Task 1, Step 3. ✅
- `FromStr` inverse derived from `name`+`ALL` → Task 1, Step 3. ✅
- `ParseDirectionError` owned by `mud-core` → Task 1, Step 3–4. ✅
- Round-trip + word-uniqueness tests in `mud-core` → Task 1, Step 1. ✅
- `mud-engine` deletes local helpers, uses core API → Task 2. ✅
- `mud-world` deletes local helpers, keeps + maps into `WorldError::UnknownDirection` → Task 3. ✅
- Exhaustive tests moved to core; world keeps mapping test → Task 3, Step 4. ✅
- No `strum`/new dependency (YAGNI) → no `cargo add`; only pre-existing `thiserror`. ✅
- No docs/ update (internal refactor) → noted in journal (Task 4). ✅

**Placeholder scan:** No TBD/TODO; every code step shows concrete code and exact commands. The one "read first" note (Task 3, Step 1) points at exact lines and states the single behavioral change, with full replacement code given. ✅

**Type consistency:** `name`, `ALL`, `ParseDirectionError`, `value()`, `WorldError::UnknownDirection { value }` are named identically across Tasks 1–3. `str::parse::<Direction>()` used in place of importing `FromStr` at call sites (inherent method, no trait import needed). ✅
