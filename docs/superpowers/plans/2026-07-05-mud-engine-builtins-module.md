# mud-engine `builtins/` Module Reorganization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the 467-line flat `crates/mud-engine/src/builtins.rs` into a `builtins/` module grouped by command family, with zero behavior change.

**Architecture:** Pure structural refactor. `builtins.rs` becomes `builtins/mod.rs` (registration wiring + two genuinely shared helpers), and each command family moves to its own submodule (`look`, `movement`, `say`, `session`, `items`). Handlers and helpers become `pub(super)` so `mod.rs`'s `table()` and sibling modules can reach them. No public API changes — `pub use builtins::register` in `lib.rs` is unaffected. Every command still runs through the same `Pipeline`, so the existing `tests/` suite is the regression oracle: it must stay green after every task.

**Tech Stack:** Rust 2024, workspace clippy lints (`unwrap_used`/`expect_used`/`panic`/`indexing_slicing` denied), `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- No `unwrap()`/`expect()`/`panic!()`/`todo!()`/`unreachable!()` in production code.
- Must compile clean under `cargo clippy -p mud-engine --all-targets` (workspace denies the lints above).
- **This is a move-only refactor:** do not change any function body, string, or logic. Only relocate items and adjust `mod`/`use`/visibility. If a `cargo test` behavior changes, you introduced a bug.
- Add dependencies with `cargo add` — but this plan needs none.
- VCS is `jj` (Jujutsu), not git branches. Commit with `jj commit -m "..."`.
- The Direction↔name duplication (`direction_name`, `DIRECTIONS`) is a **separately-tracked issue** — do NOT deduplicate it here. Move those two items verbatim into `movement.rs` and leave them as-is.

---

## Baseline (do this once, before Task 1)

- [ ] **Step 0a: Confirm the suite is green before touching anything**

Run: `cargo test -p mud-engine`
Expected: PASS (all tests). Record the count; every later task must match it.

- [ ] **Step 0b: Confirm clippy is clean**

Run: `cargo clippy -p mud-engine --all-targets`
Expected: no warnings, no errors.

---

## Target file layout

```
crates/mud-engine/src/builtins/
  mod.rs        register(), table(), system(), display_name()   (wiring + shared helpers)
  look.rs       Look, render_room(), append(), exit_names(), occupant_names()
  movement.rs   Move, direction_name(), DIRECTIONS
  say.rs        Say
  session.rs    Who, Quit
  items.rs      Get, Drop, ShowInventory, take_all(), drop_all(), drop_line(), numbered()
```

**Shared-helper rule:** `system()` and `display_name()` are used by three or more submodules, so they stay in `mod.rs` as `pub(super) fn`. `direction_name()`/`DIRECTIONS` live in `movement.rs` but are also used by `look::exit_names`, so they are `pub(super)` and `look.rs` imports them via `use super::movement::{DIRECTIONS, direction_name};`.

---

### Task 1: Convert the flat file into a `mod.rs` (no-op move)

**Files:**
- Move: `crates/mud-engine/src/builtins.rs` → `crates/mud-engine/src/builtins/mod.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `builtins/mod.rs` containing the entire current contents of `builtins.rs`, unchanged. `mod builtins;` in `lib.rs` continues to resolve (Rust treats `builtins/mod.rs` identically to `builtins.rs`).

- [ ] **Step 1: Move the file**

```bash
cd /Users/davidepetilli/dev/ferrodun
mkdir -p crates/mud-engine/src/builtins
jj file track crates/mud-engine/src/builtins/mod.rs 2>/dev/null || true
git mv crates/mud-engine/src/builtins.rs crates/mud-engine/src/builtins/mod.rs 2>/dev/null \
  || mv crates/mud-engine/src/builtins.rs crates/mud-engine/src/builtins/mod.rs
```

(Under `jj`, a plain `mv` is fine — `jj` tracks content, not renames. The `git mv` fallback is only for a git-first checkout.)

- [ ] **Step 2: Verify nothing else changed**

Run: `cargo test -p mud-engine`
Expected: PASS, same test count as Step 0a.

- [ ] **Step 3: Commit**

```bash
jj commit -m "refactor(mud-engine): move builtins.rs to builtins/mod.rs"
```

---

### Task 2: Extract `movement.rs` (`Move` + direction helpers)

**Files:**
- Create: `crates/mud-engine/src/builtins/movement.rs`
- Modify: `crates/mud-engine/src/builtins/mod.rs`

**Interfaces:**
- Consumes from `mod.rs`: `system()` (via `use super::system;`).
- Produces for siblings: `pub(super) struct Move(pub(super) Direction);`, `pub(super) fn direction_name(dir: Direction) -> &'static str`, `pub(super) const DIRECTIONS: [Direction; 6]`.

- [ ] **Step 1: Create `movement.rs` with the moved code**

Move `Move` (currently `builtins/mod.rs:96-156`), `direction_name` (`:446-457`), and `DIRECTIONS` (`:459-467`) here verbatim, adding the module imports and `pub(super)` visibility:

```rust
//! The six movement commands and the canonical direction names they render
//! (§3.2.2, §3.14.5.1).

use mud_core::{Direction, Effect, RoleName, StyledText, World};
use mud_i18n::t;

use super::look::render_room;
use super::system;
use crate::dispatch::{Broadcast, CommandContext, CommandHandler, CommandReply};

/// One of the six movement commands, carrying the direction it travels (§3.2.2).
pub(super) struct Move(pub(super) Direction);

impl CommandHandler for Move {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        // ... move the EXISTING body verbatim (mod.rs:100-155) ...
    }
}

/// The canonical English name of a direction (§3.14.5.1: built-in command names
/// are invariant across locales).
pub(super) fn direction_name(dir: Direction) -> &'static str {
    match dir {
        Direction::North => "north",
        Direction::East => "east",
        Direction::South => "south",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// The six directions in display order.
pub(super) const DIRECTIONS: [Direction; 6] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
    Direction::Up,
    Direction::Down,
];
```

Note: `Move`'s body calls `render_room(...)`, `direction_name(...)`, `system(...)`. `render_room` will live in `look.rs` (Task 3); until then it is still in `mod.rs`, so for this task import it via `use super::render_room;` instead of `use super::look::render_room;`. Switch the import to `super::look::render_room` in Task 3.

- [ ] **Step 2: Register the module and remove the moved items from `mod.rs`**

In `builtins/mod.rs`: add `mod movement;` near the top, add `use movement::Move;` to the imports used by `table()`, and delete the `Move` struct+impl, `direction_name`, and `DIRECTIONS` from `mod.rs`. Keep `exit_names` (still in `mod.rs` for now) working by importing: `use movement::{DIRECTIONS, direction_name};`. Remove `Direction` from `mod.rs`'s `use mud_core::{...}` line **only if** it is no longer referenced there (it is still referenced by `table()` via `Move(Direction::North)` — so keep it).

- [ ] **Step 3: Verify**

Run: `cargo test -p mud-engine`
Expected: PASS, same count. Then `cargo clippy -p mud-engine --all-targets` — clean.

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(mud-engine): extract movement builtins into builtins/movement.rs"
```

---

### Task 3: Extract `look.rs` (`Look` + room rendering helpers)

**Files:**
- Create: `crates/mud-engine/src/builtins/look.rs`
- Modify: `crates/mud-engine/src/builtins/mod.rs`, `crates/mud-engine/src/builtins/movement.rs`

**Interfaces:**
- Consumes: `super::system`, `super::display_name`, `super::movement::{DIRECTIONS, direction_name}`.
- Produces for siblings: `pub(super) struct Look;`, `pub(super) fn render_room(place: &Place, world: &World, viewer: EntityId, locale: &Locale) -> StyledText`.

- [ ] **Step 1: Create `look.rs` with the moved code**

Move `Look` (`mod.rs:81-94`), `render_room` (`:363-392`), `append` (`:394-399`), `exit_names` (`:401-408`), `occupant_names` (`:410-418`) here verbatim:

```rust
//! `look` and the room-rendering helpers that show a place to a viewer (§3.2).

use mud_core::{EntityId, Place, PlaceId, RoleName, Span, StyledText, World};
use mud_i18n::{Locale, t};

use super::movement::{DIRECTIONS, direction_name};
use super::{display_name, system};
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};

/// `look`: render the caller's current room (§3.2).
pub(super) struct Look;

impl CommandHandler for Look {
    // ... EXISTING body verbatim (mod.rs:85-93) ...
}

/// Renders a room as the caller sees it ... (move render_room verbatim; make it `pub(super)`)
pub(super) fn render_room(place: &Place, world: &World, viewer: EntityId, locale: &Locale) -> StyledText {
    // ... EXISTING body verbatim ...
}

// append, exit_names, occupant_names: move verbatim as private `fn` (only look.rs uses them).
```

- [ ] **Step 2: Update `mod.rs` and `movement.rs`**

In `mod.rs`: add `mod look;`, add `use look::Look;` for `table()`, delete `Look`/`render_room`/`append`/`exit_names`/`occupant_names` and the now-unused `use movement::{DIRECTIONS, direction_name};` line. In `movement.rs`: change the `render_room` import from `use super::render_room;` to `use super::look::render_room;`.

- [ ] **Step 3: Verify**

Run: `cargo test -p mud-engine` — PASS, same count. `cargo clippy -p mud-engine --all-targets` — clean.

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(mud-engine): extract look builtins into builtins/look.rs"
```

---

### Task 4: Extract `items.rs` (`Get`, `Drop`, `ShowInventory` + helpers)

**Files:**
- Create: `crates/mud-engine/src/builtins/items.rs`
- Modify: `crates/mud-engine/src/builtins/mod.rs`

**Interfaces:**
- Consumes: `super::{display_name, system}`, `crate::objects::{Resolution, resolve_among}`.
- Produces for siblings: `pub(super) struct Get;`, `pub(super) struct Drop;`, `pub(super) struct ShowInventory;`.

- [ ] **Step 1: Create `items.rs` with the moved code**

Move `ShowInventory` (`mod.rs:222-243`), `Get` (`:245-270`), `Drop` (`:272-295`), `take_all` (`:297-322`), `drop_all` (`:324-352`), `drop_line` (`:354-361`), `numbered` (`:420-431`) here verbatim:

```rust
//! Item commands: `inventory`, `get`, `drop`, and their effect/reply helpers
//! (§2.7 step 5, step 7).

use mud_core::{Effect, EntityId, PlaceId, RoleName, StyledText, World};
use mud_i18n::{Locale, t};

use super::{display_name, system};
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};
use crate::objects::{Resolution, resolve_among};

pub(super) struct ShowInventory;
// ... verbatim impl ...

pub(super) struct Get;
// ... verbatim impl ...

pub(super) struct Drop;
// ... verbatim impl ...

// take_all, drop_all, drop_line, numbered: move verbatim as private `fn`.
```

- [ ] **Step 2: Update `mod.rs`**

Add `mod items;`, add `use items::{Drop, Get, ShowInventory};` for `table()`, delete the seven moved items. Remove now-unused imports from `mod.rs` (`Resolution`, `resolve_among`, `Effect`, `PlaceId` if no longer referenced there — verify with the compiler).

- [ ] **Step 3: Verify**

Run: `cargo test -p mud-engine` — PASS, same count. `cargo clippy -p mud-engine --all-targets` — clean (this catches any now-unused import).

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(mud-engine): extract item builtins into builtins/items.rs"
```

---

### Task 5: Extract `say.rs` and `session.rs`; leave shared helpers in `mod.rs`

**Files:**
- Create: `crates/mud-engine/src/builtins/say.rs`, `crates/mud-engine/src/builtins/session.rs`
- Modify: `crates/mud-engine/src/builtins/mod.rs`

**Interfaces:**
- `say.rs` produces `pub(super) struct Say;` (consumes `super::system`, `crate::text::sanitize`).
- `session.rs` produces `pub(super) struct Who;`, `pub(super) struct Quit;` (consumes `super::system`).
- After this task, `mod.rs` retains only: `register`, `table`, and the shared `pub(super) fn system` + `pub(super) fn display_name`.

- [ ] **Step 1: Create `say.rs`**

Move `Say` (`mod.rs:158-190`) verbatim:

```rust
//! `say`: speak to the room (§3.6.3, M1-19a).

use mud_core::{RoleName, StyledText};
use mud_i18n::t;

use super::system;
use crate::dispatch::{Broadcast, CommandContext, CommandHandler, CommandReply};
use crate::text::sanitize;

pub(super) struct Say;
// ... verbatim impl ...
```

- [ ] **Step 2: Create `session.rs`**

Move `Who` (`mod.rs:192-209`) and `Quit` (`:211-220`) verbatim:

```rust
//! Session-facing builtins: `who` (§3.19) and `quit` (§3.19).

use mud_core::{RoleName, StyledText};
use mud_i18n::t;

use super::system;
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};

pub(super) struct Who;
// ... verbatim impl ...

pub(super) struct Quit;
// ... verbatim impl ...
```

Note: this filename (`builtins/session.rs`) is distinct from the crate's top-level `session/` module; no collision.

- [ ] **Step 3: Update `mod.rs` to the final shape**

Add `mod say;` and `mod session;`; extend `table()`'s imports with `use say::Say;` and `use session::{Quit, Who};`. Delete `Say`, `Who`, `Quit` from `mod.rs`. Make `system` and `display_name` `pub(super) fn`. Prune `mod.rs`'s top-level `use` list down to only what `register`/`table`/`system`/`display_name` still reference (let the compiler/clippy tell you what is unused). The final `mod.rs` header comment stays the same module doc it has today.

- [ ] **Step 4: Verify**

Run: `cargo test -p mud-engine` — PASS, same count as Step 0a. `cargo clippy -p mud-engine --all-targets` — clean. `cargo test --workspace` — PASS (confirms no downstream crate depended on `builtins` internals; none should, `register` is the only export).

- [ ] **Step 5: Commit**

```bash
jj commit -m "refactor(mud-engine): extract say/session builtins; builtins/mod.rs is now wiring + shared helpers"
```

---

## Self-review checklist (run after Task 5)

- [ ] `crates/mud-engine/src/builtins.rs` no longer exists; `builtins/` has `mod.rs`, `look.rs`, `movement.rs`, `say.rs`, `session.rs`, `items.rs`.
- [ ] `lib.rs` is unchanged except nothing (it still says `mod builtins;` / `pub use builtins::register;`).
- [ ] `direction_name`/`DIRECTIONS` were moved verbatim into `movement.rs` and NOT deduplicated (that's the separate Direction issue).
- [ ] Test count identical to the Step 0a baseline; clippy clean; `cargo test --workspace` green.
- [ ] No function body changed — `jj diff` shows only moves, `mod`/`use`/visibility edits.
