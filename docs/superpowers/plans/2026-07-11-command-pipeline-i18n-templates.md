# Command-Pipeline `en` Templates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `en` templates for the five `command.*` keys the command pipeline emits, so players see real messages instead of raw i18n keys (spec: `docs/superpowers/specs/2026-07-11-command-pipeline-i18n-templates-design.md`).

**Architecture:** Pure data + test change. Five rows are added to the static `ENTRIES` table in `mud-i18n`'s builtin catalog; the eight integration-test assertions in `mud-engine` that currently pin the literal keys are updated to the real strings. No production logic changes — `translate()`'s fallback and `warn_missing_once` are untouched, and the misclassified WARN disappears because the lookups now hit.

**Tech Stack:** Rust workspace; crates `mud-i18n` and `mud-engine`; `cargo test` / `cargo clippy`.

## Global Constraints

- **VCS is jj (Jujutsu), NOT git.** Commit with `jj commit -m "message" <files>`. Never run git mutation commands.
- Workspace clippy denies `unwrap_used`, `expect_used` (allowed in tests only, with descriptive message), `print_stdout`, `print_stderr`. Code must be clippy-clean.
- Message wording is fixed by the approved spec — copy the five templates below **verbatim**, including the trailing periods and the `{ $name }` placeholder spacing used throughout `ENTRIES`.
- After the task, append a journal entry to `.claude/JOURNAL.md` (project rule; format is given in the final steps).
- No doc-site changes: no page under `docs/docs/` documents these strings.

---

### Task 1: Add the five `command.*` `en` templates and update the tests that pin the literal keys

**Files:**
- Modify: `crates/mud-engine/tests/command_pipeline.rs` (assertions at lines 262, 282, 291–294, 300, 313, 340, 359, 422–423)
- Modify: `crates/mud-i18n/src/catalog.rs` (`ENTRIES` table, `Catalog::builtin` doc comment, two unit tests)
- Modify: `.claude/JOURNAL.md` (append entry)

**Interfaces:**
- Consumes: `Catalog::builtin()` lookup and the `t!` macro (`mud-i18n`), which renders args via `Display` into `{ $name }` placeholders — both unchanged.
- Produces: five new builtin catalog keys — `command.not-found`, `command.ambiguous` (arg `$options`), `command.bad-switch` (arg `$reason`), `command.unbound`, `command.denied` — resolvable via `Catalog::builtin().lookup(&Locale::EN, …)`.

**Context you need before editing:**

The pipeline (`crates/mud-engine/src/pipeline.rs:118–194`) already emits all five keys via `t!`; only the templates are missing. The integration-test fixture (`FakeResolver` in `command_pipeline.rs`) merges a puppet layer (`look` + alias `p`, `smite`) with a location layer (`look` + alias `q`, `say`, `score`). `CmdSet::merge` lists surviving commands in **canonical-name order** (BTreeSet), and the trie returns prefix candidates in sorted table order — so the input `s` deterministically yields the candidates `say, score, smite`. The `bad-switch` reason comes from `SwitchError`'s `thiserror` Display; input `look/` produces `SwitchError::Empty` = `switch must not be empty`. The in-crate test at `crates/mud-engine/src/pipeline.rs:433` compares against `t!(Locale::EN, "command.not-found")` and therefore self-adjusts — do not touch it.

- [ ] **Step 1: Update the integration-test assertions to the real player-facing strings (this is the failing test)**

In `crates/mud-engine/tests/command_pipeline.rs`, make these exact replacements.

Line 262 (in `the_puppet_alias_survives_the_merge_but_the_locations_does_not`):

```rust
// OLD
    assert_eq!(only_line(&dropped), "command.not-found");
// NEW
    assert_eq!(only_line(&dropped), "Unrecognized command. Type 'help'.");
```

Line 282 (in `an_unknown_command_reports_not_found_and_runs_nothing`):

```rust
// OLD
    assert_eq!(only_line(&outputs), "command.not-found");
// NEW
    assert_eq!(only_line(&outputs), "Unrecognized command. Type 'help'.");
```

Lines 291–294 (stale comment in `an_ambiguous_prefix_reports_ambiguity`) and line 300 — replace comment and assertion:

```rust
// OLD
    // `s` prefixes both `say` and `score`. The candidate list is threaded to the
    // `t!` seam as `options`, but `command.ambiguous` is not in the builtin
    // catalog (only the M1-17 command bodies' keys are), so the message renders
    // as its literal key; surfacing the candidates is a M2 catalog concern.
// NEW
    // `s` prefixes `say` and `score` (location layer) and `smite` (puppet
    // layer). The merged table lists commands in canonical-name order, so the
    // candidate list rendered through `command.ambiguous` is deterministic.
```

```rust
// OLD
    assert_eq!(only_line(&outputs), "command.ambiguous");
// NEW
    assert_eq!(only_line(&outputs), "Which do you mean? say, score, smite");
```

Line 313 (in `a_malformed_switch_reports_a_bad_switch`):

```rust
// OLD
    assert_eq!(only_line(&outputs), "command.bad-switch");
// NEW
    assert_eq!(only_line(&outputs), "Invalid switch: switch must not be empty.");
```

Line 340 (in `a_matched_but_unbound_command_reports_generically`):

```rust
// OLD
    assert_eq!(only_line(&outputs), "command.unbound");
// NEW
    assert_eq!(only_line(&outputs), "That command isn't available right now.");
```

Line 359 (in `a_lock_denies_a_caller_without_permission`):

```rust
// OLD
    assert_eq!(only_line(&outputs), "command.denied");
// NEW
    assert_eq!(only_line(&outputs), "You can't do that.");
```

Lines 422–423 (in `each_run_mints_a_distinct_command_id`):

```rust
// OLD
    assert_eq!(only_line(&first), "command.unbound");
    assert_eq!(only_line(&second), "command.unbound");
// NEW
    assert_eq!(only_line(&first), "That command isn't available right now.");
    assert_eq!(only_line(&second), "That command isn't available right now.");
```

- [ ] **Step 2: Run the integration tests to verify they fail**

Run: `cargo test -p mud-engine --test command_pipeline`

Expected: FAIL — exactly 7 failing tests (`the_puppet_alias_survives_the_merge_but_the_locations_does_not`, `an_unknown_command_reports_not_found_and_runs_nothing`, `an_ambiguous_prefix_reports_ambiguity`, `a_malformed_switch_reports_a_bad_switch`, `a_matched_but_unbound_command_reports_generically`, `a_lock_denies_a_caller_without_permission`, `each_run_mints_a_distinct_command_id`), each with `assertion failed` output showing the left side is still the literal key (e.g. `command.not-found`). All other tests in the file pass.

If `an_ambiguous_prefix_reports_ambiguity`'s failure output shows a candidate order other than `say, score, smite`, stop and re-check — order is deterministic and the assertion in Step 1 should match what the failure prints once templates exist; do not weaken the assertion to `contains`.

- [ ] **Step 3: Add the five templates to the builtin catalog**

In `crates/mud-i18n/src/catalog.rs`, inside `const ENTRIES`, insert a new group between the `content.too-long` row and the `// session FSM (§3.19.1)` comment:

```rust
    ("content.too-long", "Your message is too long."),
    // command pipeline outcomes (§2.7 steps 5–6)
    ("command.not-found", "Unrecognized command. Type 'help'."),
    ("command.ambiguous", "Which do you mean? { $options }"),
    ("command.bad-switch", "Invalid switch: { $reason }."),
    ("command.unbound", "That command isn't available right now."),
    ("command.denied", "You can't do that."),
    // session FSM (§3.19.1)
```

In the same file, fix the now-stale `Catalog::builtin` doc comment:

```rust
// OLD
    /// Holds the engine-emitted `en` strings for the M1-17 built-in commands
    /// (§3.14.6.2 requires every `t!`-referenced `en` key to exist). Keys not
    /// listed here still fall through to the literal key (§3.14.4.3) — the
    /// M1-16 pipeline `command.*` outcomes remain literal for now. M2-I replaces
    /// this hand-built table with Fluent bundles without changing the contract.
// NEW
    /// Holds the engine-emitted `en` strings for the M1-17 built-in commands
    /// and the M1-16 pipeline `command.*` outcomes (§3.14.6.2 requires every
    /// `t!`-referenced `en` key to exist). Keys not listed here still fall
    /// through to the literal key (§3.14.4.3). M2-I replaces this hand-built
    /// table with Fluent bundles without changing the contract.
```

- [ ] **Step 4: Update the catalog unit tests**

Still in `crates/mud-i18n/src/catalog.rs`, in the `tests` module. The test `the_builtin_catalog_holds_the_m1_17_keys` uses `command.not-found` as its example of an *absent* key — that example is now populated, so swap in `engine.boot` (the same deliberately-unlisted key `lib.rs`'s macro test uses):

```rust
// OLD
        // ...while an unlisted key still misses, falling through to the literal
        // key at the translate boundary (§3.14.4.3).
        assert_eq!(
            Catalog::builtin().lookup(&Locale::EN, &MessageKey::from_static("command.not-found")),
            None
        );
// NEW
        // ...while an unlisted key still misses, falling through to the literal
        // key at the translate boundary (§3.14.4.3).
        assert_eq!(
            Catalog::builtin().lookup(&Locale::EN, &MessageKey::from_static("engine.boot")),
            None
        );
```

Then add a new test directly after `the_builtin_catalog_holds_the_session_keys`, in the same style:

```rust
    #[test]
    fn the_builtin_catalog_holds_the_command_pipeline_keys() {
        let catalog = Catalog::builtin();
        for key in [
            "command.not-found",
            "command.ambiguous",
            "command.bad-switch",
            "command.unbound",
            "command.denied",
        ] {
            assert!(
                catalog
                    .lookup(&Locale::EN, &MessageKey::from_static(key))
                    .is_some(),
                "missing command pipeline key: {key}"
            );
        }
    }
```

- [ ] **Step 5: Run the mud-i18n unit tests**

Run: `cargo test -p mud-i18n`

Expected: PASS — all tests green, including `the_builtin_catalog_holds_the_command_pipeline_keys` and the duplicate-key guard `the_builtin_entries_have_no_duplicate_keys`.

- [ ] **Step 6: Run the mud-engine tests**

Run: `cargo test -p mud-engine`

Expected: PASS — the 7 tests from Step 2 now green (templates resolve, `$options`/`$reason` interpolate), and the in-crate `pipeline.rs` unit tests still pass because line 433 computes its expectation through `t!`.

- [ ] **Step 7: Full workspace verification**

Run: `cargo clippy --workspace --all-targets` then `cargo test --workspace`

Expected: clippy clean (no warnings), all workspace tests PASS.

- [ ] **Step 8: Journal entry and commit**

Append to `.claude/JOURNAL.md` (newest at the bottom):

```markdown
## 2026-07-11 — command.* pipeline keys get en templates

- **Spec:** §3.14.6.2 — every `t!`-referenced key MUST exist in `en`; §3.14.4.3 fallback was leaking literal keys to players
- **Done:** added `command.not-found` / `.ambiguous` / `.bad-switch` / `.unbound` / `.denied` en templates to the builtin catalog; updated the 8 integration-test assertions that pinned the literal keys; misclassified missing-key WARN gone as a side effect
- **Verify:** `cargo test --workspace`, `cargo clippy --workspace --all-targets` clean
- **Next:** load-time §3.14.6.2 verification lands with M2-I (design: docs/superpowers/specs/2026-07-11-command-pipeline-i18n-templates-design.md)
```

Then commit (jj, not git):

```bash
jj commit -m "fix(i18n): add en templates for command.* pipeline outcomes" crates/mud-i18n/src/catalog.rs crates/mud-engine/tests/command_pipeline.rs .claude/JOURNAL.md
```

Expected: `jj st --no-pager` afterwards shows a clean working copy (no remaining changes from this task).
