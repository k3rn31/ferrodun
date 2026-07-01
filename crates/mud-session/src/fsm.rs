//! The login state machine and its transitions.

use crate::effect::{Effect, EffectResult};
use crate::message::SessionMessage;
use mud_account::{Account, LoginError, Puppet, Username};
use secrecy::SecretString;

/// The result of one FSM step: what to show, what I/O to perform, whether the
/// session has left the login flow.
#[derive(Debug)]
#[must_use]
pub struct Transition {
    /// Messages to render to the session, in order.
    pub messages: Vec<SessionMessage>,
    /// An effect the driver must perform, if any. Feed its result back via
    /// [`SessionFsm::on_effect`].
    pub effect: Option<Effect>,
    /// Set once the session leaves the login flow.
    pub terminal: Option<Terminal>,
}

impl Transition {
    fn messages(messages: Vec<SessionMessage>) -> Self {
        Self { messages, effect: None, terminal: None }
    }

    fn message(message: SessionMessage) -> Self {
        Self::messages(vec![message])
    }

    fn closing(message: SessionMessage) -> Self {
        Self { messages: vec![message], effect: None, terminal: Some(Terminal::Closed) }
    }
}

/// Splits a line into a lowercased command word and the untrimmed remainder.
fn split_command(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (word, rest) = trimmed.split_at(end);
    if word.is_empty() {
        None
    } else {
        Some((word.to_ascii_lowercase(), rest.trim()))
    }
}

/// How a login flow ended.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Terminal {
    /// The session's connection should be closed (`quit`).
    Closed,
}

/// A single session's login state machine.
#[derive(Debug)]
#[must_use]
pub struct SessionFsm {
    state: State,
}

#[derive(Debug)]
enum State {
    Anon,
    LoginPassword { username: Username },
    AwaitingAuth,
    // Fields carried across turns; read by `puppet_select_input` (Task 5). The
    // scoped allow is removed in Task 5 when the reads land.
    #[allow(dead_code)] // LINT: consumed by puppet selection added in Task 5 (M1-19)
    PuppetSelect { account: Account, puppets: Vec<Puppet> },
}

impl Default for SessionFsm {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionFsm {
    /// A fresh machine for a just-connected session, before any input.
    pub fn new() -> Self {
        Self { state: State::Anon }
    }

    /// The banner and prompt to present the moment a session connects (§3.19.1).
    pub fn on_connect(&self) -> Transition {
        Transition::messages(vec![SessionMessage::Banner, SessionMessage::Prompt])
    }

    /// Feeds one input line to the machine.
    pub fn on_input(&mut self, line: &str) -> Transition {
        match &self.state {
            State::Anon => self.anon_input(line),
            State::LoginPassword { .. } => self.capture_login_password(line),
            // Input arriving while an effect is in flight is dropped (M1 minimal).
            State::AwaitingAuth => Transition::messages(Vec::new()),
            State::PuppetSelect { .. } => self.puppet_select_input(line),
        }
    }

    fn anon_input(&mut self, line: &str) -> Transition {
        let Some((command, rest)) = split_command(line) else {
            return Transition::messages(Vec::new());
        };
        match command.as_str() {
            "help" | "?" => Transition::message(SessionMessage::PreLoginHelp),
            "who" => Transition::message(SessionMessage::WhoStub),
            "quit" => Transition::closing(SessionMessage::Goodbye),
            "login" => match parse_name(rest) {
                Ok(username) => {
                    self.state = State::LoginPassword { username };
                    Transition::message(SessionMessage::PasswordPrompt)
                }
                Err(_) => Transition::message(SessionMessage::UnknownCommand),
            },
            _ => Transition::message(SessionMessage::UnknownCommand),
        }
    }

    fn capture_login_password(&mut self, line: &str) -> Transition {
        // Take ownership of the username by swapping the state to the next one.
        let State::LoginPassword { username } =
            std::mem::replace(&mut self.state, State::Anon)
        else {
            // INVARIANT: only reached from `on_input`'s LoginPassword arm.
            return Transition::messages(Vec::new());
        };
        let password = SecretString::from(line.to_owned());
        self.state = State::AwaitingAuth;
        Transition {
            messages: Vec::new(),
            effect: Some(Effect::Authenticate { username, password }),
            terminal: None,
        }
    }

    fn enter_puppet_select(&mut self, account: Account, puppets: Vec<Puppet>) -> Transition {
        let message = if puppets.is_empty() {
            SessionMessage::NoPuppetsYet
        } else {
            SessionMessage::PuppetList(puppets.iter().map(|p| p.name.clone()).collect())
        };
        self.state = State::PuppetSelect { account, puppets };
        Transition::message(message)
    }

    fn puppet_select_input(&mut self, _line: &str) -> Transition {
        Transition::messages(Vec::new())
    }

    /// Feeds an [`EffectResult`] back after the driver performed an [`Effect`].
    pub fn on_effect(&mut self, result: EffectResult) -> Transition {
        match (std::mem::replace(&mut self.state, State::Anon), result) {
            (State::AwaitingAuth, EffectResult::Authenticated { account, puppets }) => {
                self.enter_puppet_select(account, puppets)
            }
            (State::AwaitingAuth, EffectResult::LoginRejected(reason)) => {
                Transition::message(login_rejection_message(reason))
            }
            (State::AwaitingAuth, EffectResult::BackendError) => {
                Transition::message(SessionMessage::ServerError)
            }
            // No effect was outstanding for this state: ignore.
            (state, _) => {
                self.state = state;
                Transition::messages(Vec::new())
            }
        }
    }
}

/// Parses a `login` argument into a [`Username`] (username and puppet names
/// share the same name alphabet).
fn parse_name(raw: &str) -> Result<Username, mud_account::NameError> {
    Username::parse(raw)
}

/// Maps a [`LoginError`] to its player-facing message. `UnknownUser` and
/// `BadPassword` deliberately share [`SessionMessage::LoginFailed`] so neither
/// confirms a username exists (§3.15.1.5).
fn login_rejection_message(reason: LoginError) -> SessionMessage {
    match reason {
        LoginError::UnknownUser | LoginError::BadPassword => SessionMessage::LoginFailed,
        LoginError::Suspended => SessionMessage::AccountSuspended,
        LoginError::Banned => SessionMessage::AccountBanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::{Account, AccountId, AccountState, LoginError, Puppet, PuppetName, Username};
    use mud_core::EntityKey;
    use secrecy::ExposeSecret;
    use std::num::NonZeroU64;

    fn account() -> Account {
        Account {
            id: AccountId::new(NonZeroU64::new(1).expect("nonzero")),
            username: Username::parse("alice").expect("valid username"),
            state: AccountState::Active,
        }
    }

    fn key(value: u64) -> EntityKey {
        EntityKey::new(NonZeroU64::new(value).expect("non-zero key"))
    }

    fn puppet(id: u64, name: &str) -> Puppet {
        Puppet::new(key(id), PuppetName::parse(name).expect("valid puppet name"))
    }

    #[test]
    fn login_prompts_for_a_password_then_emits_an_authenticate_effect() {
        let mut fsm = SessionFsm::new();
        assert_eq!(fsm.on_input("login alice").messages, vec![SessionMessage::PasswordPrompt]);

        let t = fsm.on_input("hunter2");
        assert!(t.messages.is_empty());
        match t.effect {
            Some(Effect::Authenticate { username, password }) => {
                assert_eq!(username.as_str(), "alice");
                assert_eq!(password.expose_secret(), "hunter2");
            }
            // INVARIANT: capture_login_password always emits Authenticate.
            other => unreachable!("expected Authenticate, got {other:?}"),
        }
    }

    #[test]
    fn a_captured_password_is_redacted_in_debug_output() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let t = fsm.on_input("hunter2");
        let dumped = format!("{:?}", t.effect);
        assert!(!dumped.contains("hunter2"), "password leaked in Debug: {dumped}");
    }

    #[test]
    fn successful_auth_lists_the_account_puppets() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden"), puppet(11, "borel")],
        });
        assert_eq!(
            t.messages,
            vec![SessionMessage::PuppetList(vec![
                PuppetName::parse("arden").expect("name"),
                PuppetName::parse("borel").expect("name"),
            ])]
        );
    }

    #[test]
    fn an_account_with_no_puppets_is_prompted_to_create_one() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_effect(EffectResult::Authenticated { account: account(), puppets: Vec::new() });
        assert_eq!(t.messages, vec![SessionMessage::NoPuppetsYet]);
    }

    #[test]
    fn a_bad_login_is_non_leaky_and_returns_to_anon() {
        for rejection in [LoginError::UnknownUser, LoginError::BadPassword] {
            let mut fsm = SessionFsm::new();
            let _ = fsm.on_input("login alice");
            let _ = fsm.on_input("wrong");
            let t = fsm.on_effect(EffectResult::LoginRejected(rejection));
            assert_eq!(t.messages, vec![SessionMessage::LoginFailed]);
            // Back at the prompt: a following command parses as Anon again.
            assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
        }
    }

    #[test]
    fn suspended_and_banned_render_distinct_policy_messages() {
        let cases = [
            (LoginError::Suspended, SessionMessage::AccountSuspended),
            (LoginError::Banned, SessionMessage::AccountBanned),
        ];
        for (rejection, expected) in cases {
            let mut fsm = SessionFsm::new();
            let _ = fsm.on_input("login alice");
            let _ = fsm.on_input("pw");
            assert_eq!(fsm.on_effect(EffectResult::LoginRejected(rejection)).messages, vec![expected]);
        }
    }

    #[test]
    fn a_backend_fault_during_auth_reports_and_returns_to_anon() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let t = fsm.on_effect(EffectResult::BackendError);
        assert_eq!(t.messages, vec![SessionMessage::ServerError]);
        assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
    }

    #[test]
    fn on_connect_presents_banner_then_prompt() {
        let fsm = SessionFsm::new();
        let t = fsm.on_connect();
        assert_eq!(t.messages, vec![SessionMessage::Banner, SessionMessage::Prompt]);
        assert!(t.effect.is_none());
        assert!(t.terminal.is_none());
    }

    #[test]
    fn help_and_question_mark_list_pre_login_commands() {
        for line in ["help", "?", "  HELP  "] {
            let mut fsm = SessionFsm::new();
            let t = fsm.on_input(line);
            assert_eq!(t.messages, vec![SessionMessage::PreLoginHelp], "line: {line:?}");
            assert!(t.terminal.is_none());
        }
    }

    #[test]
    fn who_returns_the_stub() {
        let mut fsm = SessionFsm::new();
        assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
    }

    #[test]
    fn quit_says_goodbye_and_closes() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("quit");
        assert_eq!(t.messages, vec![SessionMessage::Goodbye]);
        assert_eq!(t.terminal, Some(Terminal::Closed));
    }

    #[test]
    fn an_unknown_command_reports_and_stays_anon() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("frobnicate");
        assert_eq!(t.messages, vec![SessionMessage::UnknownCommand]);
        assert!(t.terminal.is_none());
    }

    #[test]
    fn blank_input_is_silently_ignored() {
        let mut fsm = SessionFsm::new();
        assert!(fsm.on_input("   ").messages.is_empty());
    }
}
