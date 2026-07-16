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
use mud_schema::{EchoMode, OutputKind, OutputText, SessionEcho, SessionId, SessionOutput};
use mud_session::{Effect, EffectResult, InputEcho, SessionFsm, Terminal, Transition};
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
    ///
    /// Async because implementors reach shared world state behind an async lock.
    fn resolve_puppet(&self, key: EntityKey) -> impl Future<Output = Option<EntityId>> + Send;
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
#[must_use]
pub struct SessionService {
    sessions: HashMap<SessionId, SessionState>,
    banner: String,
    locale: Locale,
}

/// One ordered item of pre-login output.
#[derive(Debug)]
#[must_use]
pub enum LoginOutput {
    /// Rendered text for the session.
    Text(SessionOutput),
    /// A change to the session's local-echo mode (password masking).
    Echo(SessionEcho),
}

/// How an input line was routed.
#[derive(Debug)]
#[must_use]
pub enum Routing {
    /// Handled by the pre-login FSM; here is the output and whether to close.
    Login {
        outputs: Vec<LoginOutput>,
        close: bool,
        /// The puppet entity this line just bound in-world, when it completed
        /// a login — the driver's cue to announce the spawn (§2.7 step 8).
        bound: Option<EntityId>,
    },
    /// The session is in-world; the caller must run the command pipeline.
    InWorld,
    /// No such session.
    Unknown,
}

/// What applying a terminal produced: whether to close the connection, and
/// the puppet entity bound when the terminal moved the session in-world.
struct TerminalOutcome {
    close: bool,
    bound: Option<EntityId>,
}

impl SessionService {
    /// A service greeting new sessions with `banner` and rendering in `locale`.
    pub fn new(banner: impl Into<String>, locale: Locale) -> Self {
        Self {
            sessions: HashMap::new(),
            banner: banner.into(),
            locale,
        }
    }

    /// Registers a new session and returns its greeting: banner and login
    /// instructions merged into one block (§2.8.2 line discipline).
    pub fn connect(&mut self, session: SessionId) -> Vec<SessionOutput> {
        let fsm = SessionFsm::new();
        let transition = fsm.on_connect();
        self.sessions.insert(session, SessionState::Login(fsm));
        self.render_batch(&transition.messages)
            .map(|(text, kind)| block(session, text, kind))
            .into_iter()
            .collect()
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

    /// The in-world binding of `session`, or `None` for a pre-login or
    /// unknown session.
    pub fn binding_of(&self, session: SessionId) -> Option<&InWorldBinding> {
        match self.sessions.get(&session)? {
            SessionState::InWorld(binding) => Some(binding),
            SessionState::Login(_) => None,
        }
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
    ///
    /// Consecutive message batches coalesce into one output block per input
    /// line (§2.8.2 line discipline); an echo change flushes the pending block
    /// first so masking still lands before the prompt it protects.
    async fn drive(
        &mut self,
        session: SessionId,
        first: Transition,
        backend: &impl LoginBackend,
    ) -> Routing {
        let mut outputs = Vec::new();
        let mut pending: Option<(String, OutputKind)> = None;
        let mut transition = first;
        loop {
            if let Some(echo) = transition.echo {
                flush_pending(session, &mut outputs, &mut pending);
                outputs.push(LoginOutput::Echo(SessionEcho {
                    session_id: session,
                    mode: echo_mode(echo),
                }));
            }
            if let Some((text, kind)) = self.render_batch(&std::mem::take(&mut transition.messages))
            {
                // INVARIANT: a coalesced block inherits the latest batch's kind.
                // Safe because a `Prompt` message is always the terminal message
                // of an input cycle (it coincides with the echo-suppress boundary,
                // which flushes first). An FSM that emitted `Prompt` followed by
                // `Line` messages in one cycle without an echo change would bury
                // the prompt's unterminated framing under the trailing `Line` kind.
                pending = Some(match pending.take() {
                    Some((previous, _)) => (format!("{previous}\n{text}"), kind),
                    None => (text, kind),
                });
            }

            if let Some(terminal) = transition.terminal {
                flush_pending(session, &mut outputs, &mut pending);
                let outcome = self.apply_terminal(session, terminal, backend).await;
                return Routing::Login {
                    outputs,
                    close: outcome.close,
                    bound: outcome.bound,
                };
            }

            let Some(effect) = transition.effect.take() else {
                flush_pending(session, &mut outputs, &mut pending);
                return Routing::Login {
                    outputs,
                    close: false,
                    bound: None,
                };
            };

            let result = self.perform(effect, backend).await;
            let Some(SessionState::Login(fsm)) = self.sessions.get_mut(&session) else {
                flush_pending(session, &mut outputs, &mut pending);
                return Routing::Login {
                    outputs,
                    close: false,
                    bound: None,
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
                    Ok(Ok(account)) => {
                        // account_id only — never the username (design §6).
                        tracing::debug!(account_id = %account.id, "login authenticated");
                        match backend.puppets_of(account.id).await {
                            Ok(puppets) => EffectResult::Authenticated { account, puppets },
                            Err(BackendError) => EffectResult::BackendError,
                        }
                    }
                    Ok(Err(reason)) => {
                        // LoginError variants are data-free; Debug is safe.
                        tracing::debug!(reason = ?reason, "login rejected");
                        EffectResult::LoginRejected(reason)
                    }
                    // The backend impl already error!-logs the fault; a second
                    // event here would double-log it.
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::Register { username, password } => {
                match backend.register(&username, &password).await {
                    Ok(Ok(account)) => {
                        // account_id only — never the username (design §6).
                        tracing::debug!(account_id = %account.id, "account registered");
                        EffectResult::Registered { account }
                    }
                    Ok(Err(reason)) => {
                        // RegisterError variants are data-free; Debug is safe.
                        tracing::debug!(reason = ?reason, "registration rejected");
                        EffectResult::RegisterRejected(reason)
                    }
                    // The backend impl already error!-logs the fault; a second
                    // event here would double-log it.
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::CreatePuppet { account, name } => {
                match backend.create_puppet(account, &name).await {
                    Ok(puppet) => EffectResult::PuppetCreated(puppet),
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::Enter { account: _, puppet } => match backend.resolve_puppet(puppet).await {
                Some(_) => EffectResult::Entered,
                None => EffectResult::BackendError,
            },
        }
    }

    /// Applies a terminal transition. `Bound` moves the session in-world;
    /// `Closed` drops it. Returns whether the connection should close and, on a
    /// successful bind, the puppet entity now in-world (the spawn-announcement cue).
    async fn apply_terminal(
        &mut self,
        session: SessionId,
        terminal: Terminal,
        backend: &impl LoginBackend,
    ) -> TerminalOutcome {
        match terminal {
            Terminal::Bound {
                account,
                puppet,
                name,
            } => {
                // The FSM already emitted Enter and saw it succeed, so the key
                // resolves; on the vanishing chance it does not, drop cleanly.
                match backend.resolve_puppet(puppet).await {
                    Some(entity) => {
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
                    None => {
                        self.sessions.remove(&session);
                        TerminalOutcome {
                            close: true,
                            bound: None,
                        }
                    }
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

    /// Renders one FSM message batch as block text plus its kind: messages
    /// joined with `\n`, kind taken from the last message (a batch ending in
    /// a password prompt is a prompt block). `None` for an empty batch.
    fn render_batch(
        &self,
        messages: &[mud_session::SessionMessage],
    ) -> Option<(String, OutputKind)> {
        let kind = messages.last().map(render::kind)?;
        let text = messages
            .iter()
            .map(|message| render(message, &self.banner, &self.locale))
            .collect::<Vec<_>>()
            .join("\n");
        Some((text, kind))
    }
}

/// Wraps one coalesced block as the wire output for `session`.
fn block(session: SessionId, text: String, kind: OutputKind) -> SessionOutput {
    SessionOutput {
        session_id: session,
        text: OutputText::new(text),
        kind,
    }
}

/// Flushes the pending coalesced text, if any, onto `outputs`.
///
/// Called at every echo boundary and every return so text order relative to
/// echo-mode changes is preserved exactly.
fn flush_pending(
    session: SessionId,
    outputs: &mut Vec<LoginOutput>,
    pending: &mut Option<(String, OutputKind)>,
) {
    if let Some((text, kind)) = pending.take() {
        outputs.push(LoginOutput::Text(block(session, text, kind)));
    }
}

/// Maps the FSM's echo signal onto the IPC wire type at the engine boundary.
fn echo_mode(echo: InputEcho) -> EchoMode {
    match echo {
        InputEcho::Enabled => EchoMode::Enabled,
        InputEcho::Suppressed => EchoMode::Suppressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::{AccountState, Puppet};
    use mud_core::{Generation, SlotIndex, TenantTag};
    use std::num::NonZeroU64;
    use tracing_test::traced_test;

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
        fn resolve_puppet(
            &self,
            _key: EntityKey,
        ) -> impl std::future::Future<Output = Option<EntityId>> + Send {
            // Any well-formed id: these driver tests assert on routing, not identity.
            let result = Some(EntityId::new(
                TenantTag::new(1).expect("tenant"),
                SlotIndex::new(0),
                Generation::FIRST,
            ));
            async move { result }
        }
    }

    fn text_of(outputs: &[mud_schema::SessionOutput]) -> String {
        outputs
            .iter()
            .map(|o| o.text.to_plain_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn login_text_of(outputs: &[LoginOutput]) -> String {
        outputs
            .iter()
            .filter_map(|output| match output {
                LoginOutput::Text(text) => Some(text.text.to_plain_string()),
                LoginOutput::Echo(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn connect_greets_with_a_banner_and_prompt() {
        let mut svc = SessionService::new("WELCOME", Locale::EN);
        let outputs = svc.connect(sid(1));
        let text = text_of(&outputs);
        assert!(
            text.contains("WELCOME") && text.contains("login"),
            "got: {text}"
        );
    }

    #[tokio::test]
    async fn connect_merges_banner_and_instructions_into_one_line_block() {
        let mut svc = SessionService::new("WELCOME", Locale::EN);
        let outputs = svc.connect(sid(1));
        assert_eq!(outputs.len(), 1, "one connect step, one block");
        let output = outputs.first().expect("one output");
        assert_eq!(output.kind, mud_schema::OutputKind::Line);
        assert_eq!(
            output.text.to_plain_string(),
            "WELCOME\nType 'login <name>' or 'register <name>'. 'help' lists commands."
        );
    }

    #[tokio::test]
    async fn a_password_prompt_block_has_kind_prompt() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let routing = svc.on_input(sid(1), "login alice", &FakeBackend).await;
        let Routing::Login { outputs, .. } = routing else {
            panic!("expected Login routing");
        };
        let texts: Vec<_> = outputs
            .iter()
            .filter_map(|output| match output {
                LoginOutput::Text(text) => Some(text),
                LoginOutput::Echo(_) => None,
            })
            .collect();
        assert_eq!(texts.len(), 1, "one input, one block");
        let block = texts.first().expect("one text block");
        assert_eq!(block.kind, mud_schema::OutputKind::Prompt);
        assert_eq!(block.text.to_plain_string(), "Password:");
    }

    #[tokio::test]
    async fn puppet_creation_coalesces_created_and_entered_into_one_block() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let _ = svc.on_input(sid(1), "login alice", &FakeBackend).await;
        let _ = svc.on_input(sid(1), "hunter2", &FakeBackend).await;
        let routing = svc.on_input(sid(1), "new Hero", &FakeBackend).await;
        let Routing::Login { outputs, .. } = routing else {
            panic!("expected Login routing");
        };
        let texts: Vec<_> = outputs
            .iter()
            .filter_map(|output| match output {
                LoginOutput::Text(text) => Some(text.text.to_plain_string()),
                LoginOutput::Echo(_) => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["Created Hero.\nWelcome. You are now in the world.".to_owned()],
            "creation and entry must coalesce into one Line block"
        );
    }

    #[tokio::test]
    async fn a_full_login_reaches_in_world() {
        let mut svc = SessionService::new("WELCOME", Locale::EN);
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
        let mut svc = SessionService::new("W", Locale::EN);
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
            login_text_of(&outputs).contains("Login failed"),
            "got: {}",
            login_text_of(&outputs)
        );
        // Still pre-login: not routed to the pipeline.
        assert!(matches!(
            svc.on_input(sid(1), "look", &FakeBackend).await,
            Routing::Login { .. }
        ));
    }

    #[tokio::test]
    #[traced_test]
    async fn a_successful_login_logs_the_account_id_and_no_credentials() {
        let mut service = SessionService::new("welcome", Locale::EN);
        let session = sid(9);
        service.connect(session);
        let _ = service.on_input(session, "login alice", &FakeBackend).await;
        let _ = service.on_input(session, "hunter2", &FakeBackend).await;

        assert!(logs_contain("login authenticated"));
        // The never-log rule (design §6): credentials and usernames stay out.
        assert!(!logs_contain("hunter2"));
        assert!(!logs_contain("alice"));
    }

    #[tokio::test]
    #[traced_test]
    async fn a_failed_login_logs_the_rejection_without_the_password() {
        let mut service = SessionService::new("welcome", Locale::EN);
        let session = sid(10);
        service.connect(session);
        let _ = service.on_input(session, "login alice", &FakeBackend).await;
        let _ = service
            .on_input(session, "wrong-password", &FakeBackend)
            .await;

        assert!(logs_contain("login rejected"));
        assert!(!logs_contain("wrong-password"));
    }

    #[tokio::test]
    async fn quit_closes_the_session() {
        let mut svc = SessionService::new("W", Locale::EN);
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
        let mut svc = SessionService::new("W", Locale::EN);
        assert!(matches!(
            svc.on_input(sid(7), "hi", &FakeBackend).await,
            Routing::Unknown
        ));
    }

    /// Collects the echo items of a login routing, in order.
    fn echoes_of(routing: &Routing) -> Vec<EchoMode> {
        let Routing::Login { outputs, .. } = routing else {
            return Vec::new();
        };
        outputs
            .iter()
            .filter_map(|output| match output {
                LoginOutput::Echo(echo) => Some(echo.mode),
                LoginOutput::Text(_) => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn login_flow_emits_echo_changes_around_the_password() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let routing = svc.on_input(sid(1), "login alice", &FakeBackend).await;
        assert_eq!(echoes_of(&routing), vec![EchoMode::Suppressed]);
        // The suppression must precede the rendered password prompt.
        let Routing::Login { outputs, .. } = &routing else {
            unreachable!("asserted Login above")
        };
        assert!(
            matches!(outputs.first(), Some(LoginOutput::Echo(_))),
            "echo change must come before the prompt, got {outputs:?}"
        );

        let routing = svc.on_input(sid(1), "hunter2", &FakeBackend).await;
        assert_eq!(echoes_of(&routing), vec![EchoMode::Enabled]);
    }

    #[tokio::test]
    async fn non_password_input_emits_no_echo_changes() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let routing = svc.on_input(sid(1), "help", &FakeBackend).await;
        assert_eq!(echoes_of(&routing), Vec::new());
    }

    #[tokio::test]
    async fn say_broadcasts_through_the_real_resolver() {
        use crate::{Dispatcher, Pipeline, Places};
        use mud_core::{Description, PlaceId, RegionId, RoomData, Title};
        use mud_schema::{InputLine, SessionInput};

        struct OneRoom(mud_core::Place);
        impl Places for OneRoom {
            fn get(&self, id: PlaceId) -> Option<&mud_core::Place> {
                (id == self.0.id()).then_some(&self.0)
            }
        }

        let mut world = mud_core::World::new(TenantTag::new(1).expect("tenant"));
        let arden = world.create().expect("arden");
        let borel = world.create().expect("borel");
        let room_id = PlaceId::new(NonZeroU64::new(10).expect("nz"));
        world.move_to(arden, room_id).expect("seat arden");
        world.move_to(borel, room_id).expect("seat borel");
        let room = OneRoom(mud_core::Place::Room(
            RoomData::new(
                room_id,
                RegionId::new(NonZeroU64::new(1).expect("nz")),
                Description::new("A room."),
            )
            .with_title(Title::new("A Room")),
        ));

        let acct = |n| AccountId::new(NonZeroU64::new(n).expect("nz"));
        let mut svc = SessionService::new("W", Locale::EN);
        svc.bind_for_test(
            sid(1),
            InWorldBinding {
                account: acct(1),
                puppet: arden,
                name: PuppetName::parse("arden").expect("name"),
            },
        );
        svc.bind_for_test(
            sid(2),
            InWorldBinding {
                account: acct(2),
                puppet: borel,
                name: PuppetName::parse("borel").expect("name"),
            },
        );

        let mut dispatcher = Dispatcher::new();
        let builtins = crate::register(&mut dispatcher);
        let resolver = svc.resolver(&builtins);
        let mut pipeline = Pipeline::new(dispatcher);

        let outcome = pipeline
            .dispatch(
                &world,
                &room,
                &resolver,
                &SessionInput {
                    session_id: sid(1),
                    line: InputLine::new("say hi"),
                },
            )
            .expect("dispatch");

        assert!(
            outcome.outputs.iter().any(|o| o.session_id == sid(2)
                && o.text.to_plain_string().contains("arden")
                && o.text.to_plain_string().contains("hi")),
            "the second session must receive the broadcast",
        );

        // styled payload survive frame: renderer (gateway side) resolves SAY
        // role against palette (§3.20.4.2).
        let has_say_role = |o: &mud_schema::SessionOutput| {
            o.text.styled().spans().iter().any(
                |s| matches!(s.style(), mud_core::SpanStyle::Role(r) if *r == mud_core::RoleName::SAY),
            )
        };
        assert!(
            outcome
                .outputs
                .iter()
                .any(|o| o.session_id == sid(2) && has_say_role(o)),
            "the broadcast must carry say role span",
        );
        assert!(
            outcome
                .outputs
                .iter()
                .any(|o| o.session_id == sid(1) && has_say_role(o)),
            "the caller reply must carry say role span",
        );
    }

    /// The `bound` field of a Login routing (`None` for the other variants),
    /// so assertions need no `panic!` (denied outside documented invariants).
    fn bound_of(routing: &Routing) -> Option<EntityId> {
        match routing {
            Routing::Login { bound, .. } => *bound,
            Routing::InWorld | Routing::Unknown => None,
        }
    }

    #[tokio::test]
    async fn binding_a_puppet_reports_the_bound_entity() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        let pre = svc.on_input(sid(1), "login alice", &FakeBackend).await;
        assert!(
            matches!(pre, Routing::Login { .. }) && bound_of(&pre).is_none(),
            "a pre-bind line must not report a binding, got {pre:?}"
        );
        let _ = svc.on_input(sid(1), "hunter2", &FakeBackend).await;
        let routing = svc.on_input(sid(1), "play arden", &FakeBackend).await;
        assert!(
            matches!(routing, Routing::Login { close: false, .. }),
            "expected an open Login routing, got {routing:?}"
        );
        let entity = bound_of(&routing).expect("binding must report the puppet entity");
        assert_eq!(
            svc.binding_of(sid(1)).map(|binding| binding.puppet),
            Some(entity),
            "binding_of must expose the same entity"
        );
    }

    #[tokio::test]
    async fn binding_of_is_none_pre_login() {
        let mut svc = SessionService::new("W", Locale::EN);
        svc.connect(sid(1));
        assert!(svc.binding_of(sid(1)).is_none());
        assert!(svc.binding_of(sid(2)).is_none());
    }
}
