//! The session driver: the pre-login FSM's World-side home (§3.19.1).
//!
//! `mud-engine` owns the per-session state and renders the pure `mud-session`
//! FSM's messages, but reaches account persistence only through the injected
//! [`LoginBackend`] port — it never depends on `mud-db`, mirroring the
//! pipeline's `Places` / `SessionResolver` seams.

mod render;
mod resolver;

use std::collections::HashMap;
use std::future::Future;

use mud_account::{Account, AccountId, LoginError, Puppet, PuppetName, RegisterError, Username};
use mud_core::{EntityId, EntityKey};
use mud_i18n::Locale;
use mud_schema::{OutputText, SessionId, SessionOutput};
use mud_session::{Effect, EffectResult, SessionFsm, Terminal, Transition};
use secrecy::SecretString;

use render::render;
pub use resolver::RegistryResolver;

/// An opaque server-side fault performing account/world I/O on the FSM's behalf.
/// Carries no detail — no DB or task-join error leaks across the port boundary;
/// `LoginBackend` implementors return it to signal "the operation failed, the
/// player may retry."
#[derive(Debug)]
pub struct BackendError;

/// The account/world I/O the session driver performs on the FSM's behalf.
///
/// The concrete implementation lives with the caller that owns the database
/// (the M1-19 integration test; the `mudd` binary at M1-22).
pub trait LoginBackend {
    /// Verifies credentials, returning the account or a login rejection.
    ///
    /// Declared as a return-position `impl Future` rather than `async fn` so
    /// the trait stays free of the `async_fn_in_trait` lint (no `Send` bound
    /// on the returned future otherwise); callers still invoke it exactly
    /// like an `async fn` via static dispatch (`&impl LoginBackend`), with no
    /// `async-trait` boxing and no `dyn` dispatch.
    fn authenticate(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> impl Future<Output = Result<Result<Account, LoginError>, BackendError>> + Send;

    /// Creates a new account, or reports the username taken.
    fn register(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> impl Future<Output = Result<Result<Account, RegisterError>, BackendError>> + Send;

    /// Lists an account's puppets, oldest first.
    fn puppets_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Puppet>, BackendError>> + Send;

    /// Creates a puppet for `account` in the tenant's starting room.
    fn create_puppet(
        &self,
        account: AccountId,
        name: &PuppetName,
    ) -> impl Future<Output = Result<Puppet, BackendError>> + Send;

    /// Resolves a puppet's durable key to its live entity, if resident.
    fn resolve_puppet(&self, key: EntityKey) -> Option<EntityId>;
}

/// A session bound to a puppet and playing in-world.
#[derive(Debug, Clone)]
pub struct InWorldBinding {
    /// The owning account.
    pub account: AccountId,
    /// The puppet entity the session controls.
    pub puppet: EntityId,
    /// The puppet's authored display name (for `who` and broadcasts).
    pub name: PuppetName,
}

pub(crate) enum SessionState {
    Login(SessionFsm),
    InWorld(InWorldBinding),
}

/// Owns every connected session's state and drives the pre-login FSM.
#[derive(Default)]
#[must_use]
pub struct SessionService {
    sessions: HashMap<SessionId, SessionState>,
    banner: String,
}

/// How an input line was routed.
#[derive(Debug)]
#[must_use]
pub enum Routing {
    /// Handled by the pre-login FSM; here is the output and whether to close.
    Login {
        outputs: Vec<SessionOutput>,
        close: bool,
    },
    /// The session is in-world; the caller must run the command pipeline.
    InWorld,
    /// No such session.
    Unknown,
}

impl SessionService {
    /// A service greeting new sessions with `banner`.
    pub fn new(banner: impl Into<String>) -> Self {
        Self {
            sessions: HashMap::new(),
            banner: banner.into(),
        }
    }

    /// Registers a new session and returns its banner + prompt.
    pub fn connect(&mut self, session: SessionId) -> Vec<SessionOutput> {
        let fsm = SessionFsm::new();
        let transition = fsm.on_connect();
        self.sessions.insert(session, SessionState::Login(fsm));
        self.render_outputs(session, transition.messages)
    }

    /// A resolver over the current in-world bindings, contributing `builtins`.
    pub fn resolver<'a>(&'a self, builtins: &'a [mud_cmd::Command]) -> RegistryResolver<'a> {
        RegistryResolver::new(&self.sessions, builtins)
    }

    /// Test seam: binds `session` directly to an in-world `binding`, skipping
    /// the login FSM so resolver tests can seed state without a full login.
    #[cfg(test)]
    pub(crate) fn bind_for_test(&mut self, session: SessionId, binding: InWorldBinding) {
        self.sessions
            .insert(session, SessionState::InWorld(binding));
    }

    /// Drops a session (M1 minimal: no linkdead grace; §3.15.2 is M7).
    pub fn disconnect(&mut self, session: SessionId) {
        self.sessions.remove(&session);
    }

    /// Feeds one input line, routing it to the FSM or signaling the pipeline.
    pub async fn on_input(
        &mut self,
        session: SessionId,
        line: &str,
        backend: &impl LoginBackend,
    ) -> Routing {
        match self.sessions.get_mut(&session) {
            None => Routing::Unknown,
            Some(SessionState::InWorld(_)) => Routing::InWorld,
            Some(SessionState::Login(fsm)) => {
                let transition = fsm.on_input(line);
                self.drive(session, transition, backend).await
            }
        }
    }

    /// Runs a transition to completion: renders messages, performs effects, and
    /// feeds each result back until no effect remains, then applies any terminal.
    async fn drive(
        &mut self,
        session: SessionId,
        first: Transition,
        backend: &impl LoginBackend,
    ) -> Routing {
        let mut outputs = Vec::new();
        let mut transition = first;
        loop {
            outputs.extend(self.render_outputs(session, std::mem::take(&mut transition.messages)));

            if let Some(terminal) = transition.terminal {
                let close = self.apply_terminal(session, terminal, backend);
                return Routing::Login { outputs, close };
            }

            let Some(effect) = transition.effect.take() else {
                return Routing::Login {
                    outputs,
                    close: false,
                };
            };

            let result = self.perform(effect, backend).await;
            let Some(SessionState::Login(fsm)) = self.sessions.get_mut(&session) else {
                return Routing::Login {
                    outputs,
                    close: false,
                };
            };
            transition = fsm.on_effect(result);
        }
    }

    /// Performs one effect against the backend, mapping faults to `BackendError`.
    async fn perform(&self, effect: Effect, backend: &impl LoginBackend) -> EffectResult {
        match effect {
            Effect::Authenticate { username, password } => {
                match backend.authenticate(&username, &password).await {
                    Ok(Ok(account)) => match backend.puppets_of(account.id).await {
                        Ok(puppets) => EffectResult::Authenticated { account, puppets },
                        Err(BackendError) => EffectResult::BackendError,
                    },
                    Ok(Err(reason)) => EffectResult::LoginRejected(reason),
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::Register { username, password } => {
                match backend.register(&username, &password).await {
                    Ok(Ok(account)) => EffectResult::Registered { account },
                    Ok(Err(reason)) => EffectResult::RegisterRejected(reason),
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::CreatePuppet { account, name } => {
                match backend.create_puppet(account, &name).await {
                    Ok(puppet) => EffectResult::PuppetCreated(puppet),
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::Enter { account: _, puppet } => match backend.resolve_puppet(puppet) {
                Some(_) => EffectResult::Entered,
                None => EffectResult::BackendError,
            },
        }
    }

    /// Applies a terminal transition. `Bound` moves the session in-world;
    /// `Closed` drops it. Returns whether the connection should close.
    fn apply_terminal(
        &mut self,
        session: SessionId,
        terminal: Terminal,
        backend: &impl LoginBackend,
    ) -> bool {
        match terminal {
            Terminal::Bound {
                account,
                puppet,
                name,
            } => {
                // The FSM already emitted Enter and saw it succeed, so the key
                // resolves; on the vanishing chance it does not, drop cleanly.
                match backend.resolve_puppet(puppet) {
                    Some(entity) => {
                        self.sessions.insert(
                            session,
                            SessionState::InWorld(InWorldBinding {
                                account,
                                puppet: entity,
                                name,
                            }),
                        );
                        false
                    }
                    None => {
                        self.sessions.remove(&session);
                        true
                    }
                }
            }
            Terminal::Closed => {
                self.sessions.remove(&session);
                true
            }
        }
    }

    fn render_outputs(
        &self,
        session: SessionId,
        messages: Vec<mud_session::SessionMessage>,
    ) -> Vec<SessionOutput> {
        messages
            .into_iter()
            .map(|message| SessionOutput {
                session_id: session,
                text: OutputText::new(render(&message, &self.banner, &Locale::EN)),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::{AccountState, Puppet};
    use mud_core::{Generation, SlotIndex, TenantTag};
    use std::num::NonZeroU64;

    fn sid(n: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    fn account(name: &str) -> Account {
        Account {
            id: AccountId::new(NonZeroU64::new(1).expect("nonzero")),
            username: Username::parse(name).expect("username"),
            state: AccountState::Active,
        }
    }

    /// A backend where `alice`/`hunter2` authenticates to one puppet.
    struct FakeBackend;

    impl LoginBackend for FakeBackend {
        async fn authenticate(
            &self,
            username: &Username,
            password: &SecretString,
        ) -> Result<Result<Account, LoginError>, BackendError> {
            use secrecy::ExposeSecret;
            if username.as_str() == "alice" && password.expose_secret() == "hunter2" {
                Ok(Ok(account("alice")))
            } else {
                Ok(Err(LoginError::BadPassword))
            }
        }
        async fn register(
            &self,
            username: &Username,
            _password: &SecretString,
        ) -> Result<Result<Account, RegisterError>, BackendError> {
            Ok(Ok(account(username.as_str())))
        }
        async fn puppets_of(&self, _account: AccountId) -> Result<Vec<Puppet>, BackendError> {
            Ok(vec![Puppet::new(
                EntityKey::new(NonZeroU64::new(10).expect("nonzero")),
                PuppetName::parse("arden").expect("name"),
            )])
        }
        async fn create_puppet(
            &self,
            _account: AccountId,
            name: &PuppetName,
        ) -> Result<Puppet, BackendError> {
            Ok(Puppet::new(
                EntityKey::new(NonZeroU64::new(10).expect("nonzero")),
                name.clone(),
            ))
        }
        fn resolve_puppet(&self, _key: EntityKey) -> Option<EntityId> {
            // Any well-formed id: these driver tests assert on routing, not identity.
            Some(EntityId::new(
                TenantTag::new(1).expect("tenant"),
                SlotIndex::new(0),
                Generation::FIRST,
            ))
        }
    }

    fn text_of(outputs: &[mud_schema::SessionOutput]) -> String {
        outputs
            .iter()
            .map(|o| o.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn connect_greets_with_a_banner_and_prompt() {
        let mut svc = SessionService::new("WELCOME");
        let outputs = svc.connect(sid(1));
        let text = text_of(&outputs);
        assert!(
            text.contains("WELCOME") && text.contains("login"),
            "got: {text}"
        );
    }

    #[tokio::test]
    async fn a_full_login_reaches_in_world() {
        let mut svc = SessionService::new("WELCOME");
        svc.connect(sid(1));
        for line in ["login alice", "hunter2", "play arden"] {
            let routing = svc.on_input(sid(1), line, &FakeBackend).await;
            assert!(
                matches!(routing, Routing::Login { close: false, .. }),
                "expected an open Login routing on {line:?}, got {routing:?}"
            );
        }
        // Now in-world: further input routes to the pipeline.
        assert!(matches!(
            svc.on_input(sid(1), "look", &FakeBackend).await,
            Routing::InWorld
        ));
    }

    #[tokio::test]
    async fn a_wrong_password_stays_pre_login() {
        let mut svc = SessionService::new("W");
        svc.connect(sid(1));
        let _ = svc.on_input(sid(1), "login alice", &FakeBackend).await;
        let routing = svc.on_input(sid(1), "wrong", &FakeBackend).await;
        assert!(
            matches!(routing, Routing::Login { close: false, .. }),
            "expected an open Login routing, got {routing:?}"
        );
        // INVARIANT: the assertion above already confirmed `routing` is an
        // open `Routing::Login`.
        let Routing::Login { outputs, .. } = routing else {
            unreachable!()
        };
        assert!(
            text_of(&outputs).contains("Login failed"),
            "got: {}",
            text_of(&outputs)
        );
        // Still pre-login: not routed to the pipeline.
        assert!(matches!(
            svc.on_input(sid(1), "look", &FakeBackend).await,
            Routing::Login { .. }
        ));
    }

    #[tokio::test]
    async fn quit_closes_the_session() {
        let mut svc = SessionService::new("W");
        svc.connect(sid(1));
        let routing = svc.on_input(sid(1), "quit", &FakeBackend).await;
        assert!(matches!(routing, Routing::Login { close: true, .. }));
        // The session is gone; a later input is Unknown.
        assert!(matches!(
            svc.on_input(sid(1), "hi", &FakeBackend).await,
            Routing::Unknown
        ));
    }

    #[tokio::test]
    async fn input_for_an_unknown_session_is_unknown() {
        let mut svc = SessionService::new("W");
        assert!(matches!(
            svc.on_input(sid(7), "hi", &FakeBackend).await,
            Routing::Unknown
        ));
    }
}
