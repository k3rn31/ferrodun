# mud-cmd Merge Consolidation & TokenKind Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the §2.7-step-4 token-ownership settling out of `parser.rs` into `cmdset.rs` (where the rest of the merge lives), making the module dependency one-directional, and replace the settling's bare `bool` rank field with a named `TokenKind` enum.

**Architecture:** Today the §2.7 merge is split: `cmdset.rs::merge` resolves each canonical name, then hands a `Vec<(Priority, Command)>` to `parser.rs::CommandTable::from_resolved`, which does the *token-ownership settling* (deciding which command owns each colliding name/alias) and builds the trie. That split forces a module cycle — `parser.rs` imports `cmdset::Priority`, `cmdset.rs` imports `parser::CommandTable`. This plan moves the settling into `cmdset.rs` so the whole merge (step 4) lives in one module; `parser.rs` keeps only line parsing (step 5) and a thin `CommandTable::new(commands, trie)` constructor. With settling gone, `parser.rs` no longer needs `Priority`, so the cycle collapses to a single edge `cmdset → parser`. While moving the ranking logic, the `(Priority, bool, Reverse<usize>)` rank tuple's `bool` (canonical-vs-alias) becomes an ordered `enum TokenKind { Alias, Canonical }`.

**Tech Stack:** Rust 2024, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- No `unwrap()`/`expect()`/`panic!()` in production code; `expect()` in tests must carry a message.
- Prefer enums over booleans for state (project rule; this is the direct motivation for `TokenKind`).
- Must compile clean under `cargo clippy -p mud-cmd --all-targets` and `cargo clippy --workspace --all-targets`.
- **Public API must not change:** `CommandTable::from_resolved` is `pub(crate)`, so it may be renamed/replaced freely; no external caller references it. `CmdSet::merge`'s signature and behavior are unchanged.
- This is a behavior-preserving refactor: the existing `cmdset.rs` and `parser.rs` test suites (which cover alias collisions, canonical-beats-alias, priority wins) are the oracle. Do not weaken any assertion.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green and observe the cycle**

Run:
```bash
cargo test -p mud-cmd
grep -n "use crate::cmdset::Priority" crates/mud-cmd/src/parser.rs
grep -n "use crate::parser::CommandTable" crates/mud-cmd/src/cmdset.rs
```
Expected: tests PASS; both `grep`s match — the two-way edge being removed.

---

### Task 1: Move token settling into `cmdset.rs`, introduce `TokenKind`, thin the `CommandTable` constructor

One atomic refactor: the settling logic cannot live in both modules at once, so the `parser.rs` deletion, the `cmdset.rs` addition, and the constructor swap land together.

**Files:**
- Modify: `crates/mud-cmd/src/parser.rs`, `crates/mud-cmd/src/cmdset.rs`

**Interfaces:**
- Produces: `CommandTable::new(commands: Vec<Command>, trie: PrefixTrie) -> Self` (`pub(crate)`), replacing `from_resolved`.
- `cmdset.rs` gains private `enum TokenKind { Alias, Canonical }` and private `fn settle_token_ownership(resolved: &[(Priority, Command)]) -> PrefixTrie`.

- [ ] **Step 1: Replace `from_resolved` with `new` in `parser.rs`**

In `crates/mud-cmd/src/parser.rs`, delete the entire `from_resolved` method (lines 49–93, the doc comment through the closing brace and `Self { commands, trie }` return) and replace it with:

```rust
    /// Builds a table from the merge's already-resolved commands (canonical-name
    /// order) and the prefix trie that maps every settled token to its owning
    /// command's index.
    ///
    /// Token ownership is settled by the caller — [`CmdSet::merge`](crate::CmdSet::merge),
    /// the last step of the §2.7 merge — so the trie never indexes a colliding
    /// key. This constructor only stores the two halves.
    pub(crate) fn new(commands: Vec<Command>, trie: PrefixTrie) -> Self {
        Self { commands, trie }
    }
```

- [ ] **Step 2: Drop the now-unused imports in `parser.rs`**

At the top of `parser.rs`, delete these three lines (the settling was their only production use):

```rust
use std::cmp::Reverse;
use std::collections::BTreeMap;
```
and
```rust
use crate::cmdset::Priority;
```

Leave `use crate::command::{Command, CommandName, Switch, SwitchError};` and `use crate::trie::{PrefixTrie, TrieMatch};` intact. (The test module's `use crate::cmdset::{CmdSet, CmdSetKey, MergeType, Priority};` stays — tests still build sets.)

- [ ] **Step 3: Fix the stale INVARIANT comment in `parser.rs::matched`**

In the `matched` method, the `None` arm's comment currently reads "produced from `self.commands` in `from_commands`". Replace that comment with:

```rust
            // INVARIANT: trie indices are produced alongside `self.commands` when
            // the table is built in `CmdSet::merge`, so a lookup always points at
            // a live command.
```

- [ ] **Step 4: Add `TokenKind` and `settle_token_ownership` to `cmdset.rs`**

In `crates/mud-cmd/src/cmdset.rs`, update the top imports — replace `use std::collections::BTreeSet;` with:

```rust
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
```
and add, next to the other `crate::` imports:
```rust
use crate::trie::PrefixTrie;
```

Then add, just after the `Priority` `impl`/`Default` block (before `MergeType`), the private ordered kind:

```rust
/// Whether a token claim is a command's canonical name or one of its aliases.
///
/// Ordered so a canonical name outranks an alias at equal [`Priority`] when the
/// same token is claimed by two commands (§2.7 step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TokenKind {
    Alias,
    Canonical,
}
```

And add, just above the `resolve_name` free function (after the `impl CmdSet` block), the settling routine moved from `parser.rs`:

```rust
/// Settles which resolved command owns each token and returns the prefix trie
/// mapping every surviving token to its owning command's index (§2.7 step 4).
///
/// For a token claimed by several commands the strongest claim wins: higher
/// [`Priority`], then a canonical name over an alias ([`TokenKind`]), then the
/// earlier command in canonical-name order. A losing command keeps every token
/// it still owns.
fn settle_token_ownership(resolved: &[(Priority, Command)]) -> PrefixTrie {
    // `Reverse(index)` makes the lower index outrank at equal precedence.
    let mut owner: BTreeMap<&str, (Priority, TokenKind, Reverse<usize>)> = BTreeMap::new();
    for (index, (priority, command)) in resolved.iter().enumerate() {
        let claims = std::iter::once((command.name().as_str(), TokenKind::Canonical)).chain(
            command
                .aliases()
                .iter()
                .map(|alias| (alias.as_str(), TokenKind::Alias)),
        );
        for (token, kind) in claims {
            let rank = (*priority, kind, Reverse(index));
            owner
                .entry(token)
                .and_modify(|current| {
                    if rank > *current {
                        *current = rank;
                    }
                })
                .or_insert(rank);
        }
    }

    let mut trie = PrefixTrie::default();
    for (token, &(_, _, Reverse(index))) in &owner {
        trie.insert(token, index);
    }
    trie
}
```

- [ ] **Step 5: Wire `merge` to settle then build the table**

In `CmdSet::merge`, replace the tail (the `let resolved = …collect(); CommandTable::from_resolved(resolved)` block) with:

```rust
        let resolved: Vec<(Priority, Command)> = names
            .into_iter()
            .filter_map(|name| resolve_name(sets, name))
            .map(|(priority, command)| (priority, command.clone()))
            .collect();

        // Settle token ownership before consuming `resolved` for the command list.
        let trie = settle_token_ownership(&resolved);
        let commands = resolved.into_iter().map(|(_, command)| command).collect();
        CommandTable::new(commands, trie)
```

- [ ] **Step 6: Build and test the crate**

Run:
```bash
cargo test -p mud-cmd
cargo clippy -p mud-cmd --all-targets
```
Expected: PASS, clippy clean. The `cmdset.rs` tests (`alias_collision_resolves_to_the_higher_priority_command`, `a_canonical_name_beats_another_commands_alias_at_equal_priority`) and the `parser.rs` test (`an_exact_alias_beats_a_command_it_prefixes`) all still pass — proof the settling behaves identically after the move. Clippy confirms no leftover unused import in `parser.rs`.

- [ ] **Step 7: Confirm the cycle is gone and the workspace builds**

Run:
```bash
grep -n "use crate::cmdset" crates/mud-cmd/src/parser.rs   # only inside the test module now
cargo test --workspace
```
Expected: `parser.rs` no longer imports `Priority` in production (the only remaining `crate::cmdset` reference is the test module's); the workspace suite (including `mud-engine`, which drives `CmdSet::merge` through `Dispatcher`) passes unchanged.

- [ ] **Step 8: Commit**

```bash
jj commit -m "refactor(mud-cmd): move token settling into cmdset, add TokenKind enum, break parser<->cmdset cycle"
```

---

## Self-review checklist

- [ ] `parser.rs` no longer contains `from_resolved`, `BTreeMap`, `Reverse`, or a production `Priority` import; it exposes `CommandTable::new(commands, trie)`.
- [ ] The `matched` INVARIANT comment names `CmdSet::merge`, not `from_commands`.
- [ ] `cmdset.rs` owns `TokenKind` (Alias < Canonical) and `settle_token_ownership`; `merge` calls them.
- [ ] No behavior change: every pre-existing `mud-cmd` test passes with identical assertions.
- [ ] `cargo test --workspace` green; clippy clean.
