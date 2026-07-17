# Single `resolve_puppet` per successful Enter — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Carry the `EntityId` resolved during `Effect::Enter` through the FSM types so the engine binds the session without a second, redundant `resolve_puppet` call (issue #33).

**Architecture:** `EffectResult::Entered` gains an `entity: EntityId` field; the FSM copies it opaquely into `Terminal::Bound`, whose `puppet: EntityKey` field it replaces. The engine's `apply_terminal` then binds directly — its second `resolve_puppet`, the "vanishing chance" fallback arm, and its only `.await` all disappear. Spec: `docs/superpowers/specs/2026-07-17-single-resolve-puppet-design.md`.

**Tech Stack:** Rust workspace; crates `mud-session` (pure FSM, no tracing) and `mud-engine`.

## Global Constraints

- VCS is **jj**, not git: commit with `jj commit -m "message" <filesets>`. Never run `git` commands.
- `unwrap()` forbidden everywhere; `expect()` allowed only in tests, always with a descriptive message.
- `mud-session` is a pure domain crate: no `tracing` dependency, no logging.
- Never log `name` (player-authored `PuppetName`); log `account_id` and entity only (logging design §6).
- Workspace must stay clippy-clean: `cargo clippy --workspace --all-targets` denies `unwrap_used`, `expect_used`, `panic`, `print_stdout`, `print_stderr`.
- No observable behavior change: the resolve-failure path (`perform` → `BackendError` → `ServerError` → puppet select) is untouched.
- Code and comments in English; comment *why*, not *how*.

---

### Task 1: Carry the resolved `EntityId` through `mud-session` and bind without re-resolving

Both crates change in one commit: the `mud-session` type change breaks
`mud-engine`'s compile, so splitting them would leave the workspace red
between commits.

**Files:**
- Modify: `crates/mud-session/src/effect.rs` (the `Entered` variant, ~line 53)
- Modify: `crates/mud-session/src/fsm.rs` (imports ~line 6, `Terminal::Bound` ~lines 79–83, the `Entered` match arm ~lines 408–425, unit tests ~lines 480, 492, 756–776)
- Modify: `crates/mud-session/tests/login_flow.rs` (imports, helper, tests at lines 59–64, 98–99, 149)
- Modify: `crates/mud-engine/src/session/mod.rs` (`perform` Enter arm ~lines 315–318, `apply_terminal` ~lines 325–375, its call site ~line 239)

**Interfaces:**
- Consumes: `mud_core::{EntityId, Generation, SlotIndex, TenantTag}` — all exported at the `mud_core` crate root; `EntityId::new(TenantTag, SlotIndex, Generation)`; `TenantTag::new(u16) -> Result<..>`; `SlotIndex::new(u32)` (infallible); `Generation::FIRST`.
- Produces: `EffectResult::Entered { entity: EntityId }` and `Terminal::Bound { account: AccountId, entity: EntityId, name: PuppetName }` — Task 2's regression test relies on these shapes, and on `SessionService::apply_terminal` being sync with no `backend` parameter.

- [ ] **Step 1: Update `mud-session` tests to the new API (the failing test)**

This is a type-shape refactor, so "red" is a compile failure in the tests
that pin the new shape. In `crates/mud-session/src/fsm.rs`, inside
`#[cfg(test)] mod tests`, change the `mud_core` import (currently
`use mud_core::EntityKey;` near line 480) to:

```rust
    use mud_core::{EntityId, EntityKey, Generation, SlotIndex, TenantTag};
```

Add a helper next to the existing `fn key(value: u64) -> EntityKey` helper (~line 492):

```rust
    fn entity(slot: u32) -> EntityId {
        EntityId::new(
            TenantTag::new(1).expect("valid tenant tag"),
            SlotIndex::new(slot),
            Generation::FIRST,
        )
    }
```

Replace the body of the test `entering_the_world_is_terminal` (~lines 756–776) so it asserts the pass-through — the id fed in via `Entered` is the id that comes out in `Bound`:

```rust
    #[test]
    fn entering_the_world_is_terminal() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden")],
        });
        let _ = fsm.on_input("play arden");
        let t = fsm.on_effect(EffectResult::Entered { entity: entity(7) });
        assert_eq!(t.messages, vec![SessionMessage::EnteredWorld]);
        assert_eq!(
            t.terminal,
            Some(Terminal::Bound {
                account: account().id,
                entity: entity(7),
                name: PuppetName::parse("arden").expect("valid puppet name"),
            })
        );
    }
```

In `crates/mud-session/tests/login_flow.rs`, change the `mud_core` import (line 8) to:

```rust
use mud_core::{EntityId, EntityKey, Generation, SlotIndex, TenantTag};
```

Add the same helper below `fn puppet` (~line 24):

```rust
fn entity(slot: u32) -> EntityId {
    EntityId::new(
        TenantTag::new(1).expect("valid tenant tag"),
        SlotIndex::new(slot),
        Generation::FIRST,
    )
}
```

Update the three `EffectResult::Entered` call sites:

Lines 59–64 (`a_full_login_journey_binds_the_session_in_world`) become:

```rust
    let t = fsm.on_effect(EffectResult::Entered { entity: entity(7) });
    assert_eq!(t.messages, vec![SessionMessage::EnteredWorld]);
    assert!(matches!(&t.terminal, Some(Terminal::Bound { .. })));
    if let Some(Terminal::Bound { entity: bound, name, .. }) = t.terminal {
        assert_eq!(bound, entity(7), "Bound carries the resolved entity through");
        assert_eq!(name.as_str(), "hero");
    }
```

Lines 98–99 (`a_full_register_journey_creates_a_puppet_and_binds`) become:

```rust
    let t = fsm.on_effect(EffectResult::Entered { entity: entity(7) });
    assert!(matches!(t.terminal, Some(Terminal::Bound { .. })));
```

Line 149 (`an_effect_result_with_no_effect_outstanding_is_ignored`) becomes:

```rust
    let t = fsm.on_effect(EffectResult::Entered { entity: entity(1) });
```

- [ ] **Step 2: Run the `mud-session` tests to verify they fail**

Run: `cargo test -p mud-session`
Expected: COMPILE ERROR — `EffectResult::Entered` takes no fields and `Terminal::Bound` has no field `entity`.

- [ ] **Step 3: Change the `mud-session` types**

In `crates/mud-session/src/effect.rs`, change the `mud_core` import (line 4) to:

```rust
use mud_core::{EntityId, EntityKey};
```

Replace the `Entered` variant (~line 53):

```rust
    /// The session was bound to its puppet; login is complete. Carries the
    /// entity the driver resolved while performing [`Effect::Enter`] so the
    /// bind reuses it instead of re-resolving the key.
    Entered { entity: EntityId },
```

In `crates/mud-session/src/fsm.rs`, change the import (line 6) to:

```rust
use mud_core::{EntityId, EntityKey};
```

Replace the `Bound` variant (~lines 77–83):

```rust
    /// The session is bound to a puppet and now in-world; the driver routes its
    /// input to the command pipeline.
    Bound {
        account: AccountId,
        entity: EntityId,
        name: PuppetName,
    },
```

Replace the `(AwaitingEnter, Entered)` match arm (~lines 408–425). `chosen`
(the `EntityKey`) is no longer destructured — the terminal now carries the
resolved `EntityId` instead:

```rust
            (
                State::AwaitingEnter {
                    account,
                    chosen_name,
                    ..
                },
                EffectResult::Entered { entity },
            ) => Transition {
                messages: vec![SessionMessage::EnteredWorld],
                effect: None,
                terminal: Some(Terminal::Bound {
                    account: account.id,
                    entity,
                    name: chosen_name,
                }),
                echo: None,
            },
```

- [ ] **Step 4: Run the `mud-session` tests to verify they pass**

Run: `cargo test -p mud-session`
Expected: PASS (all unit + integration tests). `cargo test --workspace` would still fail — `mud-engine` doesn't compile yet; that is the next step.

- [ ] **Step 5: Adapt `mud-engine` to the new shapes**

In `crates/mud-engine/src/session/mod.rs`, replace the `Effect::Enter` arm of `perform` (~lines 315–318) so the resolved id is no longer discarded:

```rust
            Effect::Enter { account: _, puppet } => match backend.resolve_puppet(puppet).await {
                Some(entity) => EffectResult::Entered { entity },
                None => EffectResult::BackendError,
            },
```

Replace `apply_terminal` entirely (~lines 322–375). The second resolve, the
"vanishing chance" comment, and the `None => drop` arm are deleted; with no
`.await` left the method becomes sync and drops its `backend` parameter:

```rust
    /// Applies a terminal transition. `Bound` moves the session in-world;
    /// `Closed` drops it. Returns whether the connection should close and, on a
    /// successful bind, the puppet entity now in-world (the spawn-announcement cue).
    fn apply_terminal(&mut self, session: SessionId, terminal: Terminal) -> TerminalOutcome {
        match terminal {
            Terminal::Bound {
                account,
                entity,
                name,
            } => {
                // account_id and entity only — never `name`, a
                // player-authored PuppetName (design §6).
                tracing::debug!(session_id = %session, account_id = %account, ?entity, "session bound");
                self.sessions.insert(
                    session,
                    SessionState::InWorld(InWorldBinding {
                        account,
                        puppet: entity,
                        name,
                    }),
                );
                TerminalOutcome {
                    close: false,
                    bound: Some(entity),
                }
            }
            Terminal::Closed => {
                tracing::debug!(session_id = %session, "session closed at login");
                self.sessions.remove(&session);
                TerminalOutcome {
                    close: true,
                    bound: None,
                }
            }
        }
    }
```

Update the call site in `drive` (~line 239) from
`let outcome = self.apply_terminal(session, terminal, backend).await;` to:

```rust
                let outcome = self.apply_terminal(session, terminal);
```

- [ ] **Step 6: Run the full workspace gates**

Run: `cargo test --workspace`
Expected: PASS — the existing engine test `binding_a_puppet_reports_the_bound_entity` now exercises the single-resolve path end to end.

Run: `cargo clippy --workspace --all-targets`
Expected: clean, no warnings. In particular no unused-variable warning for `chosen` (it stays in the `AwaitingEnter` state, still used to emit `Effect::Enter`) and no unused-parameter warning in `drive` (`backend` is still used by `perform`).

- [ ] **Step 7: Commit**

```bash
jj commit -m "refactor(session): carry resolved EntityId through Enter (#33)" crates/mud-session crates/mud-engine
```

---

### Task 2: Regression test pinning "exactly one resolve per successful Enter" + journal

**Files:**
- Modify: `crates/mud-engine/src/session/mod.rs` (test module, after `FakeBackend` ~line 496 and after `binding_a_puppet_reports_the_bound_entity` ~line 848)
- Modify: `.claude/JOURNAL.md` (append entry)

**Interfaces:**
- Consumes: `LoginBackend` (exact signatures at `crates/mud-engine/src/session/mod.rs:35-73`), the existing test helpers `sid`, `bound_of`, `FakeBackend`, and the sync `apply_terminal` shape from Task 1.
- Produces: nothing consumed later — this is the terminal task.

- [ ] **Step 1: Write the counting-backend regression test**

In the `#[cfg(test)] mod tests` of `crates/mud-engine/src/session/mod.rs`, add below the `FakeBackend` impl (~line 496):

```rust
    /// Delegates to `FakeBackend`, counting `resolve_puppet` calls — pins the
    /// issue #33 property that a successful Enter resolves exactly once.
    #[derive(Default)]
    struct CountingBackend {
        resolves: std::sync::atomic::AtomicUsize,
    }

    impl LoginBackend for CountingBackend {
        async fn authenticate(
            &self,
            username: &Username,
            password: &SecretString,
        ) -> Result<Result<Account, LoginError>, BackendError> {
            FakeBackend.authenticate(username, password).await
        }
        async fn register(
            &self,
            username: &Username,
            password: &SecretString,
        ) -> Result<Result<Account, RegisterError>, BackendError> {
            FakeBackend.register(username, password).await
        }
        async fn puppets_of(&self, account: AccountId) -> Result<Vec<Puppet>, BackendError> {
            FakeBackend.puppets_of(account).await
        }
        async fn create_puppet(
            &self,
            account: AccountId,
            name: &PuppetName,
        ) -> Result<Puppet, BackendError> {
            FakeBackend.create_puppet(account, name).await
        }
        fn resolve_puppet(
            &self,
            key: EntityKey,
        ) -> impl std::future::Future<Output = Option<EntityId>> + Send {
            self.resolves
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            FakeBackend.resolve_puppet(key)
        }
    }
```

Add the test after `binding_a_puppet_reports_the_bound_entity` (~line 848):

```rust
    #[tokio::test]
    async fn a_successful_enter_resolves_the_puppet_exactly_once() {
        let backend = CountingBackend::default();
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let _ = svc.on_input(sid(1), "login alice", &backend).await;
        let _ = svc.on_input(sid(1), "hunter2", &backend).await;
        let routing = svc.on_input(sid(1), "play arden", &backend).await;
        assert!(bound_of(&routing).is_some(), "the enter must bind");
        assert_eq!(
            backend.resolves.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a successful Enter must resolve the puppet exactly once"
        );
    }
```

- [ ] **Step 2: Run the new test to verify it passes**

Run: `cargo test -p mud-engine a_successful_enter_resolves_the_puppet_exactly_once`
Expected: PASS with count 1. (This is a regression pin, not red-green TDD: the old double-resolve code cannot compile against the Task 1 types, so the test cannot be run against it. If it reports 2, `apply_terminal` still re-resolves — Task 1 was applied incorrectly.)

- [ ] **Step 3: Run the full workspace gates**

Run: `cargo test --workspace`
Expected: PASS

Run: `cargo clippy --workspace --all-targets`
Expected: clean, no warnings

- [ ] **Step 4: Append the journal entry**

Append to `.claude/JOURNAL.md` (newest at the bottom):

```markdown
## 2026-07-17 — Single resolve_puppet per successful Enter (#33)

- **Spec:** docs/superpowers/specs/2026-07-17-single-resolve-puppet-design.md — carry the resolved EntityId through the FSM instead of re-resolving at bind
- **Done:** `EffectResult::Entered { entity }`; `Terminal::Bound` carries `entity: EntityId` (replacing `puppet: EntityKey`); engine `apply_terminal` binds directly, now sync, second resolve and its impossible-failure arm deleted
- **Verify:** `cargo test --workspace`; new pin `a_successful_enter_resolves_the_puppet_exactly_once` (counting fake backend); `cargo clippy --workspace --all-targets` clean
- **Next:** none — no observable behavior change, no docs-site impact
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "test(engine): pin single resolve_puppet per Enter (#33)" crates/mud-engine .claude/JOURNAL.md
```
