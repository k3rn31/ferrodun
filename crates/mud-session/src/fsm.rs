//! The login state machine and its transitions.

use crate::effect::{Effect, EffectResult};
use crate::message::SessionMessage;
use mud_account::{Account, AccountId, LoginError, Puppet, PuppetName, RegisterError, Username};
use mud_core::EntityKey;
use secrecy::{ExposeSecret, SecretString};

/// Whether the client should locally echo the next input. Derived from
/// password-state membership on each FSM step; the driver relays it to the
/// transport (design 2026-07-11), which maps it onto telnet RFC 857.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEcho {
    /// Normal input: the client echoes what the player types.
    Enabled,
    /// Secret entry: the client must not echo (password masking).
    Suppressed,
}

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
    /// A change to the client's local echo, applied before `messages`.
    pub echo: Option<InputEcho>,
}

impl Transition {
    fn messages(messages: Vec<SessionMessage>) -> Self {
        Self {
            messages,
            effect: None,
            terminal: None,
            echo: None,
        }
    }

    fn message(message: SessionMessage) -> Self {
        Self::messages(vec![message])
    }

    fn closing(message: SessionMessage) -> Self {
        Self {
            messages: vec![message],
            effect: None,
            terminal: Some(Terminal::Closed),
            echo: None,
        }
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
pub enum Terminal {
    /// The session's connection should be closed (`quit`).
    Closed,
    /// The session is bound to a puppet and now in-world; the driver routes its
    /// input to the command pipeline.
    Bound {
        account: AccountId,
        puppet: EntityKey,
        name: PuppetName,
    },
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
    LoginPassword {
        username: Username,
    },
    AwaitingAuth,
    RegisterPassword {
        username: Username,
    },
    RegisterConfirm {
        username: Username,
        password: SecretString,
    },
    // Carries no data for the same reason as `AwaitingAuth`.
    AwaitingRegister,
    PuppetSelect {
        account: Account,
        puppets: Vec<Puppet>,
    },
    AwaitingCreate {
        account: Account,
        puppets: Vec<Puppet>,
    },
    AwaitingEnter {
        account: Account,
        puppets: Vec<Puppet>,
        chosen: EntityKey,
        chosen_name: PuppetName,
    },
}

impl State {
    /// Whether this state is mid-collection of a secret (a password), which
    /// requires the client's local echo to stay suppressed.
    fn collects_secret(&self) -> bool {
        // Exhaustive on purpose: a new secret-collecting State variant must
        // force a decision here rather than silently default to echo-on.
        match self {
            State::LoginPassword { .. }
            | State::RegisterPassword { .. }
            | State::RegisterConfirm { .. } => true,
            State::Anon
            | State::AwaitingAuth
            | State::AwaitingRegister
            | State::PuppetSelect { .. }
            | State::AwaitingCreate { .. }
            | State::AwaitingEnter { .. } => false,
        }
    }
}

/// The echo change implied by entering or leaving secret entry, if any.
fn echo_change(was_secret: bool, is_secret: bool) -> Option<InputEcho> {
    match (was_secret, is_secret) {
        (false, true) => Some(InputEcho::Suppressed),
        (true, false) => Some(InputEcho::Enabled),
        (true, true) | (false, false) => None,
    }
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
        Transition::messages(vec![
            SessionMessage::Banner,
            SessionMessage::LoginInstructions,
        ])
    }

    /// Feeds one input line to the machine.
    pub fn on_input(&mut self, line: &str) -> Transition {
        let was_secret = self.state.collects_secret();
        let mut transition = self.dispatch_input(line);
        transition.echo = echo_change(was_secret, self.state.collects_secret());
        transition
    }

    fn dispatch_input(&mut self, line: &str) -> Transition {
        match &self.state {
            State::Anon => self.anon_input(line),
            State::LoginPassword { .. } => self.capture_login_password(line),
            // Input arriving while an effect is in flight is dropped (M1 minimal).
            State::AwaitingAuth => Transition::messages(Vec::new()),
            State::RegisterPassword { .. } => self.capture_register_password(line),
            State::RegisterConfirm { .. } => self.confirm_register_password(line),
            State::AwaitingRegister => Transition::messages(Vec::new()),
            State::PuppetSelect { .. } => self.puppet_select_input(line),
            State::AwaitingCreate { .. } | State::AwaitingEnter { .. } => {
                Transition::messages(Vec::new())
            }
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
            "register" => match parse_name(rest) {
                Ok(username) => {
                    self.state = State::RegisterPassword { username };
                    Transition::message(SessionMessage::PasswordPrompt)
                }
                Err(_) => Transition::message(SessionMessage::NameInvalid),
            },
            _ => Transition::message(SessionMessage::UnknownCommand),
        }
    }

    fn capture_login_password(&mut self, line: &str) -> Transition {
        // Take ownership of the username by swapping the state to the next one.
        let State::LoginPassword { username } = std::mem::replace(&mut self.state, State::Anon)
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
            echo: None,
        }
    }

    fn capture_register_password(&mut self, line: &str) -> Transition {
        let State::RegisterPassword { username } = std::mem::replace(&mut self.state, State::Anon)
        else {
            // INVARIANT: only reached from `on_input`'s RegisterPassword arm.
            return Transition::messages(Vec::new());
        };
        self.state = State::RegisterConfirm {
            username,
            password: SecretString::from(line.to_owned()),
        };
        Transition::message(SessionMessage::ConfirmPrompt)
    }

    fn confirm_register_password(&mut self, line: &str) -> Transition {
        let State::RegisterConfirm { username, password } =
            std::mem::replace(&mut self.state, State::Anon)
        else {
            // INVARIANT: only reached from `on_input`'s RegisterConfirm arm.
            return Transition::messages(Vec::new());
        };
        // Both secrets are attacker-supplied here, so a plain comparison is fine.
        if password.expose_secret() != line {
            return Transition::message(SessionMessage::PasswordMismatch);
        }
        self.state = State::AwaitingRegister;
        Transition {
            messages: Vec::new(),
            effect: Some(Effect::Register { username, password }),
            terminal: None,
            echo: None,
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

    fn puppet_select_input(&mut self, line: &str) -> Transition {
        let Some((command, rest)) = split_command(line) else {
            return Transition::messages(Vec::new());
        };
        match command.as_str() {
            "help" | "?" => Transition::message(SessionMessage::PreLoginHelp),
            "quit" => Transition::closing(SessionMessage::Goodbye),
            "play" => self.select_puppet(rest),
            "new" => self.create_puppet(rest),
            _ => Transition::message(SessionMessage::UnknownCommand),
        }
    }

    fn select_puppet(&mut self, arg: &str) -> Transition {
        let State::PuppetSelect { puppets, .. } = &self.state else {
            return Transition::messages(Vec::new());
        };
        let Some(chosen) = match_puppet(puppets, arg) else {
            return Transition::message(SessionMessage::NoSuchPuppet);
        };
        let key = chosen.key;
        let name = chosen.name.clone();
        self.enter(key, name)
    }

    fn create_puppet(&mut self, arg: &str) -> Transition {
        let State::PuppetSelect { account, .. } = &self.state else {
            return Transition::messages(Vec::new());
        };
        let account_id = account.id;
        match PuppetName::parse(arg) {
            Ok(name) => {
                let State::PuppetSelect { account, puppets } =
                    std::mem::replace(&mut self.state, State::Anon)
                else {
                    // INVARIANT: only reached from the `State::PuppetSelect` match above.
                    return Transition::messages(Vec::new());
                };
                self.state = State::AwaitingCreate { account, puppets };
                Transition {
                    messages: Vec::new(),
                    effect: Some(Effect::CreatePuppet {
                        account: account_id,
                        name,
                    }),
                    terminal: None,
                    echo: None,
                }
            }
            Err(_) => Transition::message(SessionMessage::NameInvalid),
        }
    }

    /// Moves to `AwaitingEnter` for `chosen` and emits the `Enter` effect.
    fn enter(&mut self, chosen: EntityKey, chosen_name: PuppetName) -> Transition {
        let State::PuppetSelect { account, puppets } =
            std::mem::replace(&mut self.state, State::Anon)
        else {
            // INVARIANT: only reached from `select_puppet`/`on_effect` while in `PuppetSelect`.
            return Transition::messages(Vec::new());
        };
        let account_id = account.id;
        self.state = State::AwaitingEnter {
            account,
            puppets,
            chosen,
            chosen_name,
        };
        Transition {
            messages: Vec::new(),
            effect: Some(Effect::Enter {
                account: account_id,
                puppet: chosen,
            }),
            terminal: None,
            echo: None,
        }
    }

    /// Feeds an [`EffectResult`] back after the driver performed an [`Effect`].
    pub fn on_effect(&mut self, result: EffectResult) -> Transition {
        let was_secret = self.state.collects_secret();
        let mut transition = self.dispatch_effect(result);
        transition.echo = echo_change(was_secret, self.state.collects_secret());
        transition
    }

    fn dispatch_effect(&mut self, result: EffectResult) -> Transition {
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
            (State::AwaitingRegister, EffectResult::Registered { account }) => {
                self.enter_puppet_select(account, Vec::new())
            }
            (
                State::AwaitingRegister,
                EffectResult::RegisterRejected(RegisterError::UsernameTaken),
            ) => Transition::message(SessionMessage::UsernameTaken),
            (State::AwaitingRegister, EffectResult::BackendError) => {
                Transition::message(SessionMessage::ServerError)
            }
            (State::AwaitingCreate { account, puppets }, EffectResult::PuppetCreated(created)) => {
                let name = created.name.clone();
                let key = created.key;
                let mut puppets = puppets;
                puppets.push(created);
                self.state = State::PuppetSelect { account, puppets };
                let mut transition = self.enter(key, name.clone());
                transition
                    .messages
                    .insert(0, SessionMessage::PuppetCreated(name));
                transition
            }
            (State::AwaitingCreate { account, puppets }, EffectResult::BackendError) => {
                self.state = State::PuppetSelect { account, puppets };
                Transition::message(SessionMessage::ServerError)
            }
            (
                State::AwaitingEnter {
                    account,
                    chosen,
                    chosen_name,
                    ..
                },
                EffectResult::Entered,
            ) => Transition {
                messages: vec![SessionMessage::EnteredWorld],
                effect: None,
                terminal: Some(Terminal::Bound {
                    account: account.id,
                    puppet: chosen,
                    name: chosen_name,
                }),
                echo: None,
            },
            (
                State::AwaitingEnter {
                    account, puppets, ..
                },
                EffectResult::BackendError,
            ) => {
                self.state = State::PuppetSelect { account, puppets };
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

/// Resolves a `play` argument to a puppet: a 1-based ordinal into the list as
/// displayed, or a case-insensitive name match.
///
/// Ordinal-first is safe: names can never be all digits ([`PuppetName`]
/// rejects them, §3.15.1.4), so a pure-digit argument is always an ordinal
/// and can never shadow a name (issue #32).
fn match_puppet<'a>(puppets: &'a [Puppet], arg: &str) -> Option<&'a Puppet> {
    if let Ok(ordinal) = arg.parse::<usize>() {
        return ordinal.checked_sub(1).and_then(|index| puppets.get(index));
    }
    puppets
        .iter()
        .find(|p| p.name.as_str().eq_ignore_ascii_case(arg))
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
        assert_eq!(
            fsm.on_input("login alice").messages,
            vec![SessionMessage::PasswordPrompt]
        );

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
        assert!(
            !dumped.contains("hunter2"),
            "password leaked in Debug: {dumped}"
        );
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
        let t = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: Vec::new(),
        });
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
            assert_eq!(
                fsm.on_effect(EffectResult::LoginRejected(rejection))
                    .messages,
                vec![expected]
            );
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
        assert_eq!(
            t.messages,
            vec![SessionMessage::Banner, SessionMessage::LoginInstructions]
        );
        assert!(t.effect.is_none());
        assert!(t.terminal.is_none());
    }

    #[test]
    fn help_and_question_mark_list_pre_login_commands() {
        for line in ["help", "?", "  HELP  "] {
            let mut fsm = SessionFsm::new();
            let t = fsm.on_input(line);
            assert_eq!(
                t.messages,
                vec![SessionMessage::PreLoginHelp],
                "line: {line:?}"
            );
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

    #[test]
    fn register_confirms_the_password_then_emits_a_register_effect() {
        let mut fsm = SessionFsm::new();
        assert_eq!(
            fsm.on_input("register alice").messages,
            vec![SessionMessage::PasswordPrompt]
        );
        assert_eq!(
            fsm.on_input("hunter2").messages,
            vec![SessionMessage::ConfirmPrompt]
        );
        let t = fsm.on_input("hunter2");
        match t.effect {
            Some(Effect::Register { username, password }) => {
                assert_eq!(username.as_str(), "alice");
                assert_eq!(password.expose_secret(), "hunter2");
            }
            // INVARIANT: confirm_register_password always emits Register.
            other => unreachable!("expected Register, got {other:?}"),
        }
    }

    #[test]
    fn a_mismatched_confirmation_reports_and_returns_to_anon() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("register alice");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_input("typo");
        assert_eq!(t.messages, vec![SessionMessage::PasswordMismatch]);
        assert!(t.effect.is_none());
        assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
    }

    #[test]
    fn an_invalid_register_name_is_rejected_before_prompting() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("register bad name!");
        assert_eq!(t.messages, vec![SessionMessage::NameInvalid]);
    }

    #[test]
    fn successful_registration_enters_puppet_select_empty() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("register alice");
        let _ = fsm.on_input("hunter2");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_effect(EffectResult::Registered { account: account() });
        assert_eq!(t.messages, vec![SessionMessage::NoPuppetsYet]);
    }

    #[test]
    fn a_taken_username_reports_and_returns_to_anon() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("register alice");
        let _ = fsm.on_input("hunter2");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_effect(EffectResult::RegisterRejected(
            mud_account::RegisterError::UsernameTaken,
        ));
        assert_eq!(t.messages, vec![SessionMessage::UsernameTaken]);
        assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
    }

    #[test]
    fn play_by_ordinal_enters_the_selected_puppet() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden"), puppet(11, "borel")],
        });
        let t = fsm.on_input("play 2");
        match t.effect {
            Some(Effect::Enter {
                account: acct,
                puppet,
            }) => {
                assert_eq!(acct, account().id);
                assert_eq!(puppet, key(11));
            }
            // INVARIANT: `select_puppet` on an ordinal match always emits Enter.
            other => unreachable!("expected Enter, got {other:?}"),
        }
    }

    #[test]
    fn play_by_name_enters_the_matching_puppet() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden")],
        });
        let t = fsm.on_input("play arden");
        assert!(matches!(t.effect, Some(Effect::Enter { .. })));
    }

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
        let t = fsm.on_effect(EffectResult::Entered);
        assert_eq!(t.messages, vec![SessionMessage::EnteredWorld]);
        assert_eq!(
            t.terminal,
            Some(Terminal::Bound {
                account: account().id,
                puppet: key(10),
                name: PuppetName::parse("arden").expect("name"),
            })
        );
    }

    #[test]
    fn new_creates_a_puppet_then_enters_it() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: Vec::new(),
        });
        let t = fsm.on_input("new arden");
        match t.effect {
            Some(Effect::CreatePuppet {
                account: acct,
                name,
            }) => {
                assert_eq!(acct, account().id);
                assert_eq!(name.as_str(), "arden");
            }
            // INVARIANT: `create_puppet` on a valid name always emits CreatePuppet.
            other => unreachable!("expected CreatePuppet, got {other:?}"),
        }
        // The created puppet is echoed and immediately entered.
        let t = fsm.on_effect(EffectResult::PuppetCreated(puppet(12, "arden")));
        assert_eq!(
            t.messages,
            vec![SessionMessage::PuppetCreated(
                PuppetName::parse("arden").expect("name")
            )]
        );
        assert!(matches!(t.effect, Some(Effect::Enter { .. })));
    }

    #[test]
    fn play_with_no_match_reports_and_stays_in_select() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden")],
        });
        let t = fsm.on_input("play ghost");
        assert_eq!(t.messages, vec![SessionMessage::NoSuchPuppet]);
        assert!(t.effect.is_none());
        // Still selectable.
        assert!(matches!(
            fsm.on_input("play arden").effect,
            Some(Effect::Enter { .. })
        ));
    }

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

    #[test]
    fn quit_from_puppet_select_closes() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("pw");
        let _ = fsm.on_effect(EffectResult::Authenticated {
            account: account(),
            puppets: vec![puppet(10, "arden")],
        });
        assert_eq!(fsm.on_input("quit").terminal, Some(Terminal::Closed));
    }

    #[test]
    fn login_flow_suppresses_echo_for_the_password_line_only() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("login alice");
        assert_eq!(t.echo, Some(InputEcho::Suppressed));
        let t = fsm.on_input("hunter2");
        assert_eq!(t.echo, Some(InputEcho::Enabled));
    }

    #[test]
    fn register_flow_keeps_echo_suppressed_across_the_confirm_prompt() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("register alice");
        assert_eq!(t.echo, Some(InputEcho::Suppressed));
        // Password line moves RegisterPassword -> RegisterConfirm: still secret,
        // so no echo change is signaled (client stays suppressed).
        let t = fsm.on_input("hunter2");
        assert_eq!(t.echo, None);
        // Confirm line matches: leaves secret entry, echo re-enables.
        let t = fsm.on_input("hunter2");
        assert_eq!(t.echo, Some(InputEcho::Enabled));
    }

    #[test]
    fn a_mismatched_confirmation_re_enables_echo() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("register alice");
        let _ = fsm.on_input("hunter2");
        let t = fsm.on_input("typo");
        assert_eq!(t.echo, Some(InputEcho::Enabled));
    }

    #[test]
    fn echo_is_none_outside_secret_state_transitions() {
        let mut fsm = SessionFsm::new();
        assert_eq!(fsm.on_connect().echo, None);
        assert_eq!(fsm.on_input("help").echo, None);
        assert_eq!(fsm.on_input("who").echo, None);
    }

    #[test]
    fn login_rejection_does_not_change_echo() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("wrong");
        let t = fsm.on_effect(EffectResult::LoginRejected(LoginError::BadPassword));
        assert_eq!(t.echo, None);
    }
}
