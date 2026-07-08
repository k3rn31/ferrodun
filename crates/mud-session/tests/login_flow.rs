//! End-to-end login-flow journeys and recovery paths driven through the public
//! `SessionFsm` API (§3.19.1).
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

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
    assert!(
        matches!(t.effect, Some(Effect::Authenticate { .. })),
        "login emits Authenticate"
    );
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
    assert!(
        matches!(t.effect, Some(Effect::Enter { .. })),
        "play emits Enter: {:?}",
        t.effect
    );

    let t = fsm.on_effect(EffectResult::Entered);
    assert_eq!(t.messages, vec![SessionMessage::EnteredWorld]);
    assert!(matches!(&t.terminal, Some(Terminal::Bound { .. })));
    if let Some(Terminal::Bound { name, .. }) = t.terminal {
        assert_eq!(name.as_str(), "hero");
    }
}

#[test]
fn a_full_register_journey_creates_a_puppet_and_binds() {
    let mut fsm = SessionFsm::new();
    let _ = fsm.on_input("register alice");
    let _ = fsm.on_input("hunter2"); // password
    let t = fsm.on_input("hunter2"); // confirm
    assert!(
        matches!(t.effect, Some(Effect::Register { .. })),
        "confirm emits Register"
    );

    let t = fsm.on_effect(EffectResult::Registered { account: account() });
    assert_eq!(t.messages, vec![SessionMessage::NoPuppetsYet]);

    let t = fsm.on_input("new hero");
    assert!(
        matches!(t.effect, Some(Effect::CreatePuppet { .. })),
        "new emits CreatePuppet"
    );

    let t = fsm.on_effect(EffectResult::PuppetCreated(puppet(10, "hero")));
    // Creation both announces the puppet and emits the Enter effect for it.
    assert!(t.messages.contains(&SessionMessage::PuppetCreated(
        PuppetName::parse("hero").expect("name")
    )));
    assert!(
        matches!(t.effect, Some(Effect::Enter { .. })),
        "creation auto-enters: {:?}",
        t.effect
    );

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
    assert!(
        matches!(t.effect, Some(Effect::Enter { .. })),
        "back in select: {:?}",
        t.effect
    );
}

#[test]
fn a_backend_error_entering_returns_to_select() {
    let mut fsm = logged_in_with_one_puppet();
    let _ = fsm.on_input("play 1"); // -> AwaitingEnter

    let t = fsm.on_effect(EffectResult::BackendError);
    assert_eq!(t.messages, vec![SessionMessage::ServerError]);
    // Recovered to puppet-select: another `play` is accepted.
    let t = fsm.on_input("play 1");
    assert!(
        matches!(t.effect, Some(Effect::Enter { .. })),
        "back in select: {:?}",
        t.effect
    );
}

#[test]
fn an_effect_result_with_no_effect_outstanding_is_ignored() {
    let mut fsm = SessionFsm::new(); // Anon: nothing in flight

    let t = fsm.on_effect(EffectResult::Entered);
    assert!(
        t.messages.is_empty(),
        "stray result is dropped: {:?}",
        t.messages
    );
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
    assert!(
        t.messages.is_empty(),
        "input during an effect is dropped: {:?}",
        t.messages
    );
    assert!(t.effect.is_none());

    // The pending effect still resolves normally afterward.
    let t = fsm.on_effect(EffectResult::Authenticated {
        account: account(),
        puppets: vec![puppet(10, "hero")],
    });
    assert!(matches!(
        t.messages.as_slice(),
        [SessionMessage::PuppetList(_)]
    ));
}

#[test]
fn play_with_a_zero_or_out_of_range_ordinal_is_rejected() {
    let mut fsm = logged_in_with_one_puppet();
    assert_eq!(
        fsm.on_input("play 0").messages,
        vec![SessionMessage::UnknownCommand]
    );

    let mut fsm = logged_in_with_one_puppet();
    assert_eq!(
        fsm.on_input("play 99").messages,
        vec![SessionMessage::UnknownCommand]
    );
}
