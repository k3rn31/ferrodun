# Unambiguous `play` Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix issue #32 — make `play <arg>` resolution unambiguous by forbidding all-digit names, numbering the character menu, and adding a dedicated no-such-puppet message.

**Architecture:** The ambiguity is killed at the type level: `Username`/`PuppetName` validation gains a "not all digits" rule, so a pure-digit `play` argument is provably an ordinal and `match_puppet`'s ordinal-first order can never shadow a name. The character menu becomes a numbered multi-line list so ordinals are discoverable, and a failed `play` gets its own message instead of the generic `UnknownCommand`.

**Tech Stack:** Rust workspace (`mud-account`, `mud-session`, `mud-engine`, `mud-i18n`), MkDocs docs site, SPEC.md.

**Design doc:** `docs/superpowers/specs/2026-07-17-play-name-resolution-design.md`

## Global Constraints

- **VCS is jj (Jujutsu), not git.** Commit with `jj commit -m "message" <filesets>`. Never run git mutation commands.
- `unwrap()` is strictly forbidden; `expect()` only in tests, always with a descriptive message.
- Workspace clippy denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`; code must be clippy-clean: `cargo clippy --workspace --all-targets`.
- TDD: write the failing test, watch it fail, then implement.
- Pure/domain crates (`mud-account`, `mud-session`) take no `tracing` dependency — no logging anywhere in this plan.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they cover.
- Code and comments in English; comment *why*, not *how*.

---

### Task 1: Forbid all-digit names in `mud-account`

**Files:**
- Modify: `crates/mud-account/src/name.rs`
- Modify: `crates/mud-i18n/src/catalog.rs:123-126` (the `session.name-invalid` template)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `NameError::AllDigits` (unit variant on the existing `pub enum NameError`); `Username::parse` / `PuppetName::parse` now reject all-digit input. Every consumer matches `NameError` with `Err(_)`, so no call sites break. Task 2's FSM test relies on `PuppetName::parse("42")` returning `Err(NameError::AllDigits)`.

- [ ] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` at the bottom of `crates/mud-account/src/name.rs`:

```rust
    #[test]
    fn rejects_an_all_digit_name() {
        // An all-digit name would be indistinguishable from a `play` ordinal
        // (issue #32); `007` also covers the leading-zero parse (`007` → 7).
        for raw in ["42", "007", "1"] {
            assert_eq!(
                Username::parse(raw),
                Err(NameError::AllDigits),
                "{raw:?} must be rejected"
            );
            assert_eq!(
                PuppetName::parse(raw),
                Err(NameError::AllDigits),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn accepts_a_name_containing_digits_but_not_only_digits() {
        for raw in ["4rden", "42_", "x42"] {
            assert!(Username::parse(raw).is_ok(), "{raw:?} should be valid");
            assert!(PuppetName::parse(raw).is_ok(), "{raw:?} should be valid");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-account rejects_an_all_digit_name`
Expected: FAIL — `Username::parse("42")` currently returns `Ok(..)`, and `NameError::AllDigits` does not compile yet (compile error first; that counts as the failing state).

- [ ] **Step 3: Implement the rule**

In `crates/mud-account/src/name.rs`, add the variant to `NameError` (after `InvalidChar`):

```rust
    /// The name consisted entirely of ASCII digits.
    ///
    /// Forbidden so a pure-digit `play` argument can only ever be a list
    /// ordinal, never a name (issue #32).
    #[error("name cannot be all digits")]
    AllDigits,
```

Extend `validate()` — the new check goes **last**, so the more specific `Length`/`InvalidChar` diagnoses still win for input that breaks several rules (note: `MIN_LEN` is 1, so the empty string is caught by the length check before `all()` could vacuously pass):

```rust
fn validate(raw: &str) -> Result<(), NameError> {
    let len = raw.chars().count();
    if !(MIN_LEN..=MAX_LEN).contains(&len) {
        return Err(NameError::Length { got: len });
    }
    if let Some(ch) = raw.chars().find(|ch| !is_allowed(*ch)) {
        return Err(NameError::InvalidChar { ch });
    }
    if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(NameError::AllDigits);
    }
    Ok(())
}
```

Update the module doc (line 4-6) so the shared-rule summary stays accurate — change:

```rust
//! names an in-world character. Both share one validation rule (length bounds +
//! a restricted character set) so a name is parsed once, at the boundary, and
//! never re-validated downstream.
```

to:

```rust
//! names an in-world character. Both share one validation rule (length bounds,
//! a restricted character set, and never all digits — so a digit-only `play`
//! argument is always an ordinal, cf. §3.15.1.4) so a name is parsed once, at
//! the boundary, and never re-validated downstream.
```

Also update the `# Errors` doc on `Username::parse` (line 66-69) to mention the new rejection — change "contains a character outside `[A-Za-z0-9_'-]`" to "contains a character outside `[A-Za-z0-9_'-]`, or consists entirely of digits".

- [ ] **Step 4: Update the player-facing rejection text**

In `crates/mud-i18n/src/catalog.rs` (line 123-126) change:

```rust
    (
        "session.name-invalid",
        "That name isn't allowed. Use letters, digits, _ ' - (1-32 chars).",
    ),
```

to:

```rust
    (
        "session.name-invalid",
        "That name isn't allowed. Use letters, digits, _ ' - (1-32 chars, not all digits).",
    ),
```

- [ ] **Step 5: Run the tests and clippy**

Run: `cargo test -p mud-account && cargo test -p mud-i18n`
Expected: PASS (all tests, including the two new ones).

Run: `cargo clippy --workspace --all-targets`
Expected: clean, no warnings.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(account): forbid all-digit names (#32)" crates/mud-account/src/name.rs crates/mud-i18n/src/catalog.rs
```

---

### Task 2: `NoSuchPuppet` message for a failed `play`

**Files:**
- Modify: `crates/mud-session/src/message.rs` (add variant)
- Modify: `crates/mud-session/src/fsm.rs:301-303` (`select_puppet` failure arm) and its `#[cfg(test)]` tests
- Modify: `crates/mud-engine/src/session/render.rs` (`kind()` and `render()` match arms)
- Modify: `crates/mud-i18n/src/catalog.rs` (new entry + key-list test)

**Interfaces:**
- Consumes: `NameError::AllDigits` from Task 1 (for the `new 42` FSM test).
- Produces: `SessionMessage::NoSuchPuppet` (unit variant on `pub enum SessionMessage`); catalog key `"session.no-such-puppet"` with template `"No such character."`. Task 5 documents the behavior.

- [ ] **Step 1: Write the failing FSM tests**

In `crates/mud-session/src/fsm.rs`, **modify** the existing test `play_with_no_match_reports_and_stays_in_select` (line 805-822): replace both `SessionMessage::UnknownCommand` expectations — the assert becomes:

```rust
        let t = fsm.on_input("play ghost");
        assert_eq!(t.messages, vec![SessionMessage::NoSuchPuppet]);
```

(keep the rest of the test unchanged), and add two new tests next to it:

```rust
    #[test]
    fn play_with_an_out_of_range_ordinal_reports_no_such_puppet() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden")],
        });
        let t = fsm.on_input("play 9");
        assert_eq!(t.messages, vec![SessionMessage::NoSuchPuppet]);
        assert!(t.effect.is_none());
    }

    #[test]
    fn an_all_digit_name_is_rejected_at_create() {
        // Guards the Task 1 rule at the FSM boundary: `new 42` must fail as an
        // invalid name, otherwise the puppet could never be selected by name.
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: Vec::new(),
        });
        let t = fsm.on_input("new 42");
        assert_eq!(t.messages, vec![SessionMessage::NameInvalid]);
        assert!(t.effect.is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-session play_with`
Expected: compile error — `SessionMessage::NoSuchPuppet` does not exist yet. (`an_all_digit_name_is_rejected_at_create` already passes once it compiles, thanks to Task 1 — that is fine; it is a regression guard.)

- [ ] **Step 3: Add the variant and return it from `select_puppet`**

In `crates/mud-session/src/message.rs`, after the `PuppetList` variant (line 31):

```rust
    /// The `play` argument matched no owned puppet (unknown name or
    /// out-of-range ordinal).
    NoSuchPuppet,
```

In `crates/mud-session/src/fsm.rs` `select_puppet` (line 301-303) change:

```rust
        let Some(chosen) = match_puppet(puppets, arg) else {
            return Transition::message(SessionMessage::UnknownCommand);
        };
```

to:

```rust
        let Some(chosen) = match_puppet(puppets, arg) else {
            return Transition::message(SessionMessage::NoSuchPuppet);
        };
```

- [ ] **Step 4: Classify and render the new variant**

`kind()` in `crates/mud-engine/src/session/render.rs` is deliberately exhaustive, so the workspace will not compile until this is done. Add `NoSuchPuppet` to the `OutputKind::Line` arm (after `| SessionMessage::NoPuppetsYet`):

```rust
        | SessionMessage::NoPuppetsYet
        | SessionMessage::NoSuchPuppet
```

Add the render arm in `render()` (after the `NoPuppetsYet` line):

```rust
        SessionMessage::NoSuchPuppet => t!(*locale, "session.no-such-puppet"),
```

In `crates/mud-i18n/src/catalog.rs` add the entry to `ENTRIES` right after `("session.puppet-created", "Created { $name }.")` (line 134):

```rust
    ("session.no-such-puppet", "No such character."),
```

and add `"session.no-such-puppet",` to the key array in the test `the_builtin_catalog_holds_the_session_keys` (after `"session.puppet-created",`, line 213).

- [ ] **Step 5: Add a render test**

In the `#[cfg(test)] mod tests` of `crates/mud-engine/src/session/render.rs`:

```rust
    #[test]
    fn no_such_puppet_renders_from_the_catalog() {
        assert_eq!(kind(&SessionMessage::NoSuchPuppet), OutputKind::Line);
        assert_eq!(
            render(&SessionMessage::NoSuchPuppet, "", &Locale::EN),
            "No such character."
        );
    }
```

- [ ] **Step 6: Run the tests and clippy**

Run: `cargo test -p mud-session && cargo test -p mud-engine && cargo test -p mud-i18n`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(session): dedicated message for a failed play (#32)" crates/mud-session/src/message.rs crates/mud-session/src/fsm.rs crates/mud-engine/src/session/render.rs crates/mud-i18n/src/catalog.rs
```

---

### Task 3: Numbered multi-line character menu

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs:130-133` (the `session.puppet-list` template)
- Modify: `crates/mud-engine/src/session/render.rs:59-66` (`PuppetList` arm) and its test `puppet_list_names_every_character`

**Interfaces:**
- Consumes: nothing new — `SessionMessage::PuppetList(Vec<PuppetName>)` is unchanged; the FSM already emits puppets in list order, and ordinals index that same `Vec` (both live in `State::PuppetSelect`), so displayed numbers and `play <number>` always agree.
- Produces: the exact menu text Task 5 quotes in the docs.

- [ ] **Step 1: Write the failing test**

In `crates/mud-engine/src/session/render.rs`, **replace** the existing test `puppet_list_names_every_character` (line 111-122) with:

```rust
    #[test]
    fn puppet_list_renders_a_numbered_menu() {
        let names = vec![
            PuppetName::parse("arden").expect("name"),
            PuppetName::parse("borel").expect("name"),
        ];
        let text = render(&SessionMessage::PuppetList(names), "", &Locale::EN);
        assert_eq!(
            text,
            "Your characters:\n  1) arden\n  2) borel\nType 'play <name or number>' or 'new <name>'."
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine puppet_list_renders_a_numbered_menu`
Expected: FAIL — the current output is the single-line comma-joined form.

- [ ] **Step 3: Implement the numbered menu**

In `crates/mud-i18n/src/catalog.rs` (line 130-133) change:

```rust
    (
        "session.puppet-list",
        "Your characters: { $names }. Type 'play <name>' or 'new <name>'.",
    ),
```

to:

```rust
    (
        "session.puppet-list",
        "Your characters:\n{ $names }\nType 'play <name or number>' or 'new <name>'.",
    ),
```

In `crates/mud-engine/src/session/render.rs` change the `PuppetList` arm (line 59-66) to number the entries — 1-based, matching what `match_puppet` accepts:

```rust
        SessionMessage::PuppetList(names) => {
            let names = names
                .iter()
                .enumerate()
                .map(|(i, n)| format!("  {}) {}", i + 1, n.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            t!(*locale, "session.puppet-list", names = names)
        }
```

- [ ] **Step 4: Run the tests and clippy**

Run: `cargo test -p mud-engine && cargo test -p mud-i18n && cargo test --workspace`
Expected: PASS. The workspace run catches any other test asserting on the old single-line menu text.

Run: `cargo clippy --workspace --all-targets`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(engine): numbered multi-line character menu (#32)" crates/mud-engine/src/session/render.rs crates/mud-i18n/src/catalog.rs
```

---

### Task 4: Invariant doc on `match_puppet` and SPEC.md amendment

**Files:**
- Modify: `crates/mud-session/src/fsm.rs:450-458` (`match_puppet` doc comment)
- Modify: `SPEC.md` §3.15.1.4 (the paragraph ending "…the same puppet name in two tenants is unrelated.")

**Interfaces:**
- Consumes: the Task 1 name rule (the doc comment states it as an invariant).
- Produces: normative spec text; no code surface.

- [ ] **Step 1: Update the `match_puppet` doc comment**

In `crates/mud-session/src/fsm.rs` change:

```rust
/// Resolves a `play` argument to a puppet: a 1-based ordinal, or a name match.
fn match_puppet<'a>(puppets: &'a [Puppet], arg: &str) -> Option<&'a Puppet> {
```

to:

```rust
/// Resolves a `play` argument to a puppet: a 1-based ordinal into the list as
/// displayed, or a case-insensitive name match.
///
/// Ordinal-first is safe: names can never be all digits ([`PuppetName`]
/// rejects them, §3.15.1.4), so a pure-digit argument is always an ordinal
/// and can never shadow a name (issue #32).
fn match_puppet<'a>(puppets: &'a [Puppet], arg: &str) -> Option<&'a Puppet> {
```

- [ ] **Step 2: Amend SPEC.md §3.15.1.4**

In `SPEC.md`, the paragraph currently ends (around line 1967):

```
MUST be rejected. Uniqueness is tenant-scoped (§3.11.4): the same
puppet name in two tenants is unrelated.
```

Append two sentences so the paragraph ends:

```
MUST be rejected. Uniqueness is tenant-scoped (§3.11.4): the same
puppet name in two tenants is unrelated. Account and puppet names
MUST NOT consist entirely of digits. Puppet selection MUST accept
the puppet's name (compared case-insensitively) and MUST accept the
1-based ordinal at which the puppet appears in the displayed
character list; because no valid name is all digits, a digit-only
argument always denotes an ordinal.
```

Match the file's existing hard-wrap width (wrap as shown).

- [ ] **Step 3: Verify the build is unaffected**

Run: `cargo test -p mud-session && cargo clippy --workspace --all-targets`
Expected: PASS / clean (comment-only code change).

- [ ] **Step 4: Commit**

```bash
jj commit -m "docs(spec): all-digit name ban and play selection rules (#32)" crates/mud-session/src/fsm.rs SPEC.md
```

---

### Task 5: Documentation site and journal

**Files:**
- Modify: `docs/docs/playing/getting-started.md:40-70`
- Verify (no change expected): `docs/docs/architecture/sessions.md:21`
- Modify: `.claude/JOURNAL.md` (append entry)

**Interfaces:**
- Consumes: the exact menu text from Task 3 and the name rule from Task 1.
- Produces: player-facing documentation of the new behavior.

- [ ] **Step 1: Update the name rules in `getting-started.md`**

Line 42-44 currently reads:

```markdown
Type `register <name>` to create a new account. Names may use letters,
digits, and `_ ' -`, and must be between 1 and 32 characters. You'll be
prompted twice, to catch typos:
```

Change to:

```markdown
Type `register <name>` to create a new account. Names may use letters,
digits, and `_ ' -`, must be between 1 and 32 characters, and cannot
consist entirely of digits. You'll be prompted twice, to catch typos:
```

Line 68-70 currently reads:

```markdown
A brand-new account has no characters yet, so it's prompted straight to
`new <name>` to create its first one. Character names follow the same rules
as account names (letters, digits, `_ ' -`, 1–32 characters).
```

Change to:

```markdown
A brand-new account has no characters yet, so it's prompted straight to
`new <name>` to create its first one. Character names follow the same rules
as account names (letters, digits, `_ ' -`, 1–32 characters, not all
digits).
```

- [ ] **Step 2: Show the numbered menu**

In the "Choosing your character" section, insert an example block between the intro sentence (ending "…picking which character you want to play:") and the command table:

````markdown
```
Your characters:
  1) aria
  2) borin
Type 'play <name or number>' or 'new <name>'.
```
````

(The fenced block is literal terminal output, matching the Task 3 renderer exactly — with the numbers, `play 2` and `play borin` are interchangeable.)

- [ ] **Step 3: Skim `sessions.md` for accuracy**

`docs/docs/architecture/sessions.md:21` already says the select state "lists the account's puppets and accepts `play <name>`, `play <number>`" — confirm the surrounding sentence needs no change (it describes behavior, not the menu format). Change nothing unless it now contradicts the implementation.

- [ ] **Step 4: Build the docs strictly**

Run (from `docs/`): `uv run mkdocs build --strict`
Expected: build succeeds with no warnings.

- [ ] **Step 5: Append the journal entry**

Append to `.claude/JOURNAL.md` (newest at the bottom):

```markdown
## 2026-07-17 — Unambiguous play resolution (#32)

- **Spec:** §3.15.1.4 — names MUST NOT be all digits; play MUST accept name (case-insensitive) or displayed 1-based ordinal (amended this PR)
- **Done:** `NameError::AllDigits` on the shared name rule; `SessionMessage::NoSuchPuppet` for a failed play; numbered multi-line character menu; SPEC + player docs updated
- **Verify:** `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `uv run mkdocs build --strict`
- **Next:** none
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "docs: name rule and numbered character menu (#32)" docs/docs/playing/getting-started.md .claude/JOURNAL.md
```
