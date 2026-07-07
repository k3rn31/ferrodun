# mud-session Login-Flow Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `tests/login_flow.rs` integration suite that drives full login/register journeys through the public `SessionFsm` API and covers the currently-untested `BackendError` recovery arms, the no-effect fallback, input-dropped-while-in-flight, and `match_puppet` ordinal boundaries.

**Architecture:** `SessionFsm` is a pure sans-IO state machine: `on_connect`/`on_input(&str)`/`on_effect(EffectResult)` each return a `Transition { messages, effect, terminal }`. Every uncovered branch is reachable from the public API by driving the machine into the right state and feeding the right effect result, so no white-box access is needed — a `tests/` integration file is the right home.

**Tech Stack:** Rust 2024, `secrecy` for passwords, `mud-account` domain types, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`.
- Integration tests live in `tests/`; they see only the crate's public API.
- Must compile clean under `cargo clippy -p mud-session --all-targets`.
- Tests-only change: no production code is modified. If a test cannot be made to pass without a production change, stop — that is a finding, not a test fix.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-session`
Expected: PASS.

---

### Task 1: Create `tests/login_flow.rs` with journey and edge-arm coverage

**Files:**
- Create: `crates/mud-session/tests/login_flow.rs`

**Interfaces consumed (public):** `SessionFsm::{new, on_connect, on_input, on_effect}`; `Transition { messages, effect, terminal }`; `Effect::{Authenticate, Register, CreatePuppet, Enter}`; `EffectResult::{Authenticated, Registered, PuppetCreated, Entered, BackendError}`; `Terminal::Bound`; `SessionMessage` variants; `mud_account::{Account, AccountId, AccountState, Puppet, PuppetName, Username}`; `mud_core::EntityKey`.

- [ ] **Step 1: Write the test file**

Create `crates/mud-session/tests/login_flow.rs`:

```rust
//! End-to-end login-flow journeys and recovery paths driven through the public
//! `SessionFsm` API (§3.19.1).
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::num::NonZeroU64;

use mud_account::{Account, AccountId, AccountState, Puppet, PuppetName, Username};
use mud_core::EntityKey;
use mud_session::{Effect, EffectResult, SessionFsm, SessionMessage, Terminal};

fn account() -> Account {
    Account {
        id: AccountId::new(NonZeroU64::new(1).expect("nonzero")),
        username: Username::parse("alice").expect("valid username"),
        state: AccountState::Active,
    }
}

fn puppet(id: u64, name: &str) -> Puppet {
    Puppet::new(
        EntityKey::new(NonZeroU64::new(id).expect("non-zero key")),
        PuppetName::parse(name).expect("valid puppet name"),
    )
}

/// Drives a fresh machine through login → authenticated with one puppet, leaving
/// it in puppet-select. Returns the machine ready for a `play`.
fn logged_in_with_one_puppet() -> SessionFsm {
    let mut fsm = SessionFsm::new();
    let _ = fsm.on_input("login alice");
    let t = fsm.on_input("hunter2");
    assert!(matches!(t.effect, Some(Effect::Authenticate { .. })), "login emits Authenticate");
    let t = fsm.on_effect(EffectResult::Authenticated {
        account: account(),
        puppets: vec![puppet(10, "hero")],
    });
    assert!(
        matches!(t.messages.as_slice(), [SessionMessage::PuppetList(_)]),
        "authenticated with puppets shows the list: {:?}",
        t.messages
    );
    fsm
}

#[test]
fn a_full_login_journey_binds_the_session_in_world() {
    let mut fsm = logged_in_with_one_puppet();

    let t = fsm.on_input("play 1");
    assert!(matches!(t.effect, Some(Effect::Enter { .. })), "play emits Enter: {:?}", t.effect);

    let t = fsm.on_effect(EffectResult::Entered);
    assert_eq!(t.messages, vec![SessionMessage::EnteredWorld]);
    match t.terminal {
        Some(Terminal::Bound { name, .. }) => assert_eq!(name.as_str(), "hero"),
        other => panic!("expected Bound terminal, got {other:?}"),
    }
}

#[test]
fn a_full_register_journey_creates_a_puppet_and_binds() {
    let mut fsm = SessionFsm::new();
    let _ = fsm.on_input("register alice");
    let _ = fsm.on_input("hunter2"); // password
    let t = fsm.on_input("hunter2"); // confirm
    assert!(matches!(t.effect, Some(Effect::Register { .. })), "confirm emits Register");

    let t = fsm.on_effect(EffectResult::Registered { account: account() });
    assert_eq!(t.messages, vec![SessionMessage::NoPuppetsYet]);

    let t = fsm.on_input("new hero");
    assert!(matches!(t.effect, Some(Effect::CreatePuppet { .. })), "new emits CreatePuppet");

    let t = fsm.on_effect(EffectResult::PuppetCreated(puppet(10, "hero")));
    // Creation both announces the puppet and emits the Enter effect for it.
    assert!(t.messages.contains(&SessionMessage::PuppetCreated(
        PuppetName::parse("hero").expect("name")
    )));
    assert!(matches!(t.effect, Some(Effect::Enter { .. })), "creation auto-enters: {:?}", t.effect);

    let t = fsm.on_effect(EffectResult::Entered);
    assert!(matches!(t.terminal, Some(Terminal::Bound { .. })));
}

#[test]
fn a_backend_error_during_register_shows_server_error() {
    let mut fsm = SessionFsm::new();
    let _ = fsm.on_input("register alice");
    let _ = fsm.on_input("hunter2");
    let _ = fsm.on_input("hunter2"); // -> AwaitingRegister

    let t = fsm.on_effect(EffectResult::BackendError);
    assert_eq!(t.messages, vec![SessionMessage::ServerError]);
}

#[test]
fn a_backend_error_creating_a_puppet_returns_to_select() {
    let mut fsm = logged_in_with_one_puppet();
    let _ = fsm.on_input("new second"); // -> AwaitingCreate

    let t = fsm.on_effect(EffectResult::BackendError);
    assert_eq!(t.messages, vec![SessionMessage::ServerError]);
    // Recovered to puppet-select: a `play` is accepted again.
    let t = fsm.on_input("play 1");
    assert!(matches!(t.effect, Some(Effect::Enter { .. })), "back in select: {:?}", t.effect);
}

#[test]
fn a_backend_error_entering_returns_to_select() {
    let mut fsm = logged_in_with_one_puppet();
    let _ = fsm.on_input("play 1"); // -> AwaitingEnter

    let t = fsm.on_effect(EffectResult::BackendError);
    assert_eq!(t.messages, vec![SessionMessage::ServerError]);
    // Recovered to puppet-select: another `play` is accepted.
    let t = fsm.on_input("play 1");
    assert!(matches!(t.effect, Some(Effect::Enter { .. })), "back in select: {:?}", t.effect);
}

#[test]
fn an_effect_result_with_no_effect_outstanding_is_ignored() {
    let mut fsm = SessionFsm::new(); // Anon: nothing in flight

    let t = fsm.on_effect(EffectResult::Entered);
    assert!(t.messages.is_empty(), "stray result is dropped: {:?}", t.messages);
    assert!(t.terminal.is_none());

    // State was preserved: the machine still handles anon input.
    let t = fsm.on_input("help");
    assert_eq!(t.messages, vec![SessionMessage::PreLoginHelp]);
}

#[test]
fn input_is_dropped_while_an_effect_is_in_flight() {
    let mut fsm = SessionFsm::new();
    let _ = fsm.on_input("login alice");
    let _ = fsm.on_input("hunter2"); // -> AwaitingAuth (Authenticate in flight)

    let t = fsm.on_input("play 1");
    assert!(t.messages.is_empty(), "input during an effect is dropped: {:?}", t.messages);
    assert!(t.effect.is_none());

    // The pending effect still resolves normally afterward.
    let t = fsm.on_effect(EffectResult::Authenticated {
        account: account(),
        puppets: vec![puppet(10, "hero")],
    });
    assert!(matches!(t.messages.as_slice(), [SessionMessage::PuppetList(_)]));
}

#[test]
fn play_with_a_zero_or_out_of_range_ordinal_is_rejected() {
    let mut fsm = logged_in_with_one_puppet();
    assert_eq!(fsm.on_input("play 0").messages, vec![SessionMessage::UnknownCommand]);

    let mut fsm = logged_in_with_one_puppet();
    assert_eq!(fsm.on_input("play 99").messages, vec![SessionMessage::UnknownCommand]);
}
```

- [ ] **Step 2: Run the new suite**

Run: `cargo test -p mud-session --test login_flow`
Expected: PASS. If a `SessionMessage` variant name or `Account` field visibility differs from what's assumed here, adjust the test to the real name — the fsm.rs in-file test module (`crates/mud-session/src/fsm.rs`) is the reference for exact variant names and the `Account { id, username, state }` literal. Do **not** change production code.

- [ ] **Step 3: Full crate + clippy**

Run: `cargo test -p mud-session && cargo clippy -p mud-session --all-targets`
Expected: PASS, clippy clean. (Note: `panic!` is used in the two `match … { other => panic!(...) }` arms — allowed in tests; if the workspace `clippy::panic` lint fires on tests, replace those with `assert!(matches!(...))` plus a follow-up field assertion.)

- [ ] **Step 4: Commit**

```bash
jj commit -m "test(mud-session): add login-flow integration suite covering error and boundary arms"
```

---

## Self-review checklist

- [ ] `tests/login_flow.rs` covers: full login journey, full register journey, BackendError in register/create/enter (with recovery), no-effect fallback (state preserved), input-dropped-in-flight, and `play` ordinal 0 / out-of-range.
- [ ] No production code touched.
- [ ] Variant names and `Account` construction match `src/fsm.rs`'s test module.
- [ ] `cargo test --workspace` green; clippy clean.
