# mud-world KDL Helper Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the generic KDL helper `arg` (and its only-caller test helper `first_node`) out of `rooms.rs` into a new `kdl.rs` module, so `regions.rs` and `palette.rs` no longer reach into the room loader for a utility that isn't about rooms.

**Architecture:** `arg(node, index) -> Option<&str>` is a generic "read the Nth positional string of a KDL node" utility with nothing room-specific about it, yet it lives in `rooms.rs` and is imported by `regions.rs` and `palette.rs` via `use crate::rooms::arg`. That makes the sibling loaders structurally depend on the room module for a shared primitive. Extracting `arg` into a neutral `kdl.rs` gives all three loaders a common, correctly-named home to import from, and removes the "everyone depends on rooms" smell. Pure move; no behavior change.

**Tech Stack:** Rust 2024, `kdl` crate, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- No `unwrap()`/`expect()`/`panic!()` in production code; `expect()` in tests must carry a message.
- Must compile clean under `cargo clippy -p mud-world --all-targets`.
- `arg` stays `pub(crate)` — it is an internal helper, not part of the crate's public API.
- Pure move-refactor: the three `arg` unit tests move verbatim with their `first_node` helper; assertions unchanged.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green and observe the coupling**

Run:
```bash
cargo test -p mud-world
grep -rn "use crate::rooms::arg" crates/mud-world/src
```
Expected: tests PASS; both `regions.rs` and `palette.rs` show `use crate::rooms::arg;` — the imports being repointed.

---

### Task 1: Create `kdl.rs`, move `arg` + its tests, repoint importers

One atomic move: `arg` cannot be defined in two modules, so the new file, the `rooms.rs` deletion, and the three import updates land together.

**Files:**
- Create: `crates/mud-world/src/kdl.rs`
- Modify: `crates/mud-world/src/lib.rs`, `crates/mud-world/src/rooms.rs`, `crates/mud-world/src/regions.rs`, `crates/mud-world/src/palette.rs`

**Interfaces:**
- Produces: `pub(crate) fn arg(node: &KdlNode, index: usize) -> Option<&str>` at `crate::kdl::arg`.

- [ ] **Step 1: Create `kdl.rs` with `arg` and its moved tests**

Create `crates/mud-world/src/kdl.rs`:

```rust
//! Small, room-agnostic helpers over the `kdl` crate's node model.
//!
//! Every loader (`rooms`, `regions`, `palette`) reads positional string
//! arguments off KDL nodes; this is that shared primitive, kept out of any one
//! loader so none has to depend on another for it.

use kdl::{KdlNode, KdlValue};

/// The first positional string argument of `node` at `index`, if present.
///
/// Returns `None` when the index is past the last argument or the value at that
/// position is not a string.
pub(crate) fn arg(node: &KdlNode, index: usize) -> Option<&str> {
    node.get(index).and_then(KdlValue::as_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdl::KdlDocument;

    fn first_node(text: &str) -> KdlNode {
        let document = KdlDocument::parse(text).expect("valid kdl");
        document.nodes().first().expect("at least one node").clone()
    }

    #[test]
    fn arg_reads_a_positional_string() {
        let node = first_node("room \"town\" \"extra\"");
        assert_eq!(arg(&node, 0), Some("town"));
        assert_eq!(arg(&node, 1), Some("extra"));
    }

    #[test]
    fn arg_is_none_past_the_last_argument() {
        let node = first_node("room \"town\"");
        assert_eq!(arg(&node, 5), None);
    }

    #[test]
    fn arg_is_none_for_a_non_string_value() {
        let node = first_node("room 42");
        assert_eq!(arg(&node, 0), None);
    }
}
```

- [ ] **Step 2: Declare the module in `lib.rs`**

In `crates/mud-world/src/lib.rs`, add `mod kdl;` to the module list (alphabetical: between `mod error;` and `mod palette;`). No re-export — it is `pub(crate)`.

- [ ] **Step 3: Remove `arg`, its tests, and the now-unused `KdlValue` import from `rooms.rs`**

In `crates/mud-world/src/rooms.rs`:
- Delete the `arg` definition (the doc comment + `pub(crate) fn arg(...)` at lines ~308–311).
- Delete the three `arg_*` tests (`arg_reads_a_positional_string`, `arg_is_none_past_the_last_argument`, `arg_is_none_for_a_non_string_value`) and the `first_node` test helper (it now has no callers in `rooms.rs` — its only users were those three tests).
- Change the import `use kdl::{KdlDocument, KdlNode, KdlValue};` to `use kdl::{KdlDocument, KdlNode};` (`KdlValue` was used only by `arg`; `KdlDocument`/`KdlNode` are still used by the loaders and other tests).
- Add `use crate::kdl::arg;` alongside the other `use crate::…` imports (`rooms.rs` still calls `arg` at lines ~169, 186, 197, 237, 241).

- [ ] **Step 4: Repoint `regions.rs` and `palette.rs`**

In both `crates/mud-world/src/regions.rs` and `crates/mud-world/src/palette.rs`, change:
```rust
use crate::rooms::arg;
```
to:
```rust
use crate::kdl::arg;
```

- [ ] **Step 5: Build and test**

Run:
```bash
cargo test -p mud-world
cargo clippy -p mud-world --all-targets
```
Expected: PASS (same test count — the three `arg` tests now run from `kdl.rs`), clippy clean. Clippy is the guard that `KdlValue` and `first_node` are gone from `rooms.rs` without leaving an unused import or dead helper.

- [ ] **Step 6: Confirm no importer still points at `rooms::arg`, and the workspace builds**

Run:
```bash
grep -rn "rooms::arg" crates/mud-world/src   # expect no matches
cargo test --workspace
```
Expected: no `rooms::arg` references remain; workspace suite green.

- [ ] **Step 7: Commit**

```bash
jj commit -m "refactor(mud-world): extract generic KDL arg helper into kdl module"
```

---

## Self-review checklist

- [ ] `kdl.rs` holds `arg` + the three `arg_*` tests + a local `first_node`; `lib.rs` declares `mod kdl;`.
- [ ] `rooms.rs` no longer defines `arg`/`first_node`, imports `arg` from `crate::kdl`, and dropped `KdlValue`.
- [ ] `regions.rs` and `palette.rs` import `arg` from `crate::kdl`.
- [ ] `grep -rn "rooms::arg"` returns nothing; `cargo test --workspace` green; clippy clean.
