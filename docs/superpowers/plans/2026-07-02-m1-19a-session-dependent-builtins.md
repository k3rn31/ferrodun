# M1-19a — Session-dependent built-ins (`who`, `quit`, broadcast) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `say` and movement reach other co-located players, add `who` and in-world `quit`, by giving the command pipeline a broadcast fan-out over a new session `Roster` seam.

**Architecture:** Handlers stay session-ignorant: they emit a domain audience (`place` + `except`) plus a styled message on `CommandReply`. The pipeline resolves each broadcast against the **pre-effect** world (occupants of the place, minus `except`), maps each occupant→session via a new `Roster` port, and emits one `SessionOutput` per recipient — all before applying the reply's world effects. `quit` sets a `SessionDisposition::Close` the pipeline returns for the driver to honor later.

**Tech Stack:** Rust (workspace crates `mud-core`, `mud-session`, `mud-engine`, `mud-i18n`), `mud-i18n` `t!` seam, jj VCS, `cargo` + `clippy` + `fmt`.

## Global Constraints

- **VCS is jj (Jujutsu), not git.** Commit with `jj commit -m "…"` (there is one working-copy change already described for this PR; each task's commit is `jj commit`). Never run `git commit`.
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unreachable!` in non-test code** (clippy denies `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `cast_possible_truncation`, `print_stdout`, `print_stderr`). `expect` allowed in tests with a message.
- **Newtype / type-driven design; no `_ =>` catch-alls** — match all enum variants explicitly.
- **Errors via `thiserror`; no third-party error in a public API.**
- **Comment *why*, not *how*; English only; doc-comment every public item.**
- **Green gate every commit:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --all --check` all pass.
- **i18n stays per-tenant/world** (a broadcast is rendered once and fanned to all recipients). The per-session→per-world SPEC §3.14 rework is a **separate follow-up task**, not this PR.

---

### Task 1: `Direction::opposite()` (mud-core)

**Files:**
- Modify: `crates/mud-core/src/place/room.rs` (add an `impl Direction` after the enum at line 27; add a test in the `mod tests` block)

**Interfaces:**
- Produces: `Direction::opposite(self) -> Direction` (N↔S, E↔W, U↔D).

- [ ] **Step 1: Write the failing test** — in the `mod tests` block of `crates/mud-core/src/place/room.rs`, above `fn neighbor_returns_wired_exits`:

```rust
    #[test]
    fn opposite_reverses_every_direction() {
        use Direction::{Down, East, North, South, Up, West};
        assert_eq!(North.opposite(), South);
        assert_eq!(South.opposite(), North);
        assert_eq!(East.opposite(), West);
        assert_eq!(West.opposite(), East);
        assert_eq!(Up.opposite(), Down);
        assert_eq!(Down.opposite(), Up);
        // Applying it twice returns the original heading.
        for dir in [North, East, South, West, Up, Down] {
            assert_eq!(dir.opposite().opposite(), dir);
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-core opposite_reverses`
Expected: FAIL — `no method named 'opposite' found for enum 'Direction'`.

- [ ] **Step 3: Write minimal implementation** — insert after the `Direction` enum (immediately after its closing `}` at line 27):

```rust
impl Direction {
    /// The reverse heading: the direction you would face to return the way you
    /// came (N↔S, E↔W, U↔D). Used to phrase an arrival ("arrives from the east")
    /// as the opposite of the traveller's heading.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-core opposite_reverses`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-core): Direction::opposite() (M1-19a)"
```

---

### Task 2: Carry `PuppetName` into the in-world binding

`Terminal::Bound` and `InWorldBinding` gain the puppet's real name so `who` and broadcasts can show it. Changing the `Terminal::Bound` variant breaks the `mud-engine` match on it, so both crates are updated in this one task to keep the tree green.

**Files:**
- Modify: `crates/mud-session/src/fsm.rs` (`Terminal::Bound`, `State::AwaitingEnter`, `enter`, the two `enter` call sites, the `AwaitingEnter → Entered` arm, and the `entering_the_world_is_terminal`/`new_creates_a_puppet_then_enters_it` tests)
- Modify: `crates/mud-engine/src/session/mod.rs` (`InWorldBinding`, `apply_terminal`)
- Modify: `crates/mud-engine/src/session/resolver.rs` (the test that builds `InWorldBinding`)

**Interfaces:**
- Produces: `Terminal::Bound { account: AccountId, puppet: EntityKey, name: PuppetName }`; `InWorldBinding { account: AccountId, puppet: EntityId, name: PuppetName }`.

- [ ] **Step 1: Write the failing test** — in `crates/mud-session/src/fsm.rs` `mod tests`, update `entering_the_world_is_terminal`'s assertion to require the name (this is the red step — it won't compile until `Terminal::Bound` carries `name`):

```rust
        assert_eq!(
            t.terminal,
            Some(Terminal::Bound {
                account: account().id,
                puppet: key(10),
                name: PuppetName::parse("arden").expect("name"),
            })
        );
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-session entering_the_world_is_terminal`
Expected: FAIL to compile — `struct 'Terminal::Bound' has no field named 'name'`.

- [ ] **Step 3: Implement — thread the name through the FSM.** In `crates/mud-session/src/fsm.rs`:

  Add `name` to the terminal variant:

```rust
    /// The session is bound to a puppet and now in-world; the driver routes its
    /// input to the command pipeline.
    Bound {
        account: AccountId,
        puppet: EntityKey,
        name: PuppetName,
    },
```

  Carry the chosen name in `AwaitingEnter`:

```rust
    AwaitingEnter {
        account: Account,
        puppets: Vec<Puppet>,
        chosen: EntityKey,
        chosen_name: PuppetName,
    },
```

  Change `enter` to take the name and store it:

```rust
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
        }
    }
```

  Update `select_puppet` to pass the name (clone before the `enter` call ends the `&self.state` borrow):

```rust
    fn select_puppet(&mut self, arg: &str) -> Transition {
        let State::PuppetSelect { puppets, .. } = &self.state else {
            return Transition::messages(Vec::new());
        };
        let Some(chosen) = match_puppet(puppets, arg) else {
            return Transition::message(SessionMessage::UnknownCommand);
        };
        let key = chosen.key;
        let name = chosen.name.clone();
        self.enter(key, name)
    }
```

  Update the `AwaitingCreate → PuppetCreated` arm's `enter` call (in `on_effect`) to pass the created name:

```rust
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
```

  Update the `AwaitingEnter → Entered` arm to emit the name:

```rust
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
            },
```

  In `crates/mud-engine/src/session/mod.rs`, add `name` to the binding and store it in `apply_terminal`:

```rust
pub struct InWorldBinding {
    /// The owning account.
    pub account: AccountId,
    /// The puppet entity the session controls.
    pub puppet: EntityId,
    /// The puppet's authored display name (for `who` and broadcasts).
    pub name: PuppetName,
}
```

```rust
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
```

  In `crates/mud-engine/src/session/resolver.rs`, the test that builds `InWorldBinding` (around line 74) adds the name:

```rust
        let binding = InWorldBinding {
            account: mud_account::AccountId::new(NonZeroU64::new(1).expect("nonzero")),
            puppet,
            name: mud_account::PuppetName::parse("arden").expect("name"),
        };
```

  Update the FSM `new_creates_a_puppet_then_enters_it` test if it asserts on `Terminal::Bound` (it asserts only on `Effect::Enter`, so no change needed — verify).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-session && cargo test -p mud-engine`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-session): carry PuppetName into the in-world binding (M1-19a)"
```

---

### Task 3: `Roster` port + `CallerContext` name + `RegistryResolver` impls

Adds the session-registry read seam. `CallerContext::new` grows a `name` parameter, so **every** construction site is updated here to stay green (the pipeline signature is unchanged until Task 4).

**Files:**
- Create: `crates/mud-engine/src/roster.rs`
- Modify: `crates/mud-engine/src/lib.rs` (module + re-exports)
- Modify: `crates/mud-engine/src/caller.rs` (`CallerContext.name`, `caller_name()`, test)
- Modify: `crates/mud-engine/src/session/resolver.rs` (fill `name`; `impl Roster`; add a Roster test)
- Modify: `crates/mud-engine/src/pipeline.rs` (`FakeResolver` CallerContext name)
- Modify: `crates/mud-engine/tests/builtins.rs` (`FakeResolver` CallerContext name)
- Modify: `crates/mud-engine/tests/command_pipeline.rs` (`FakeResolver` CallerContext name)

**Interfaces:**
- Produces: `pub trait Roster { fn session_of(&self, entity: EntityId) -> Option<SessionId>; fn connected(&self) -> Vec<Presence>; }`; `pub struct Presence { pub name: PuppetName }`; `CallerContext::new(session_id, caller, location, name, locale, access)` (name added as the 4th arg); `CallerContext::caller_name(&self) -> &PuppetName`.

- [ ] **Step 1: Write the failing test** — in `crates/mud-engine/src/session/resolver.rs` `mod tests`, add:

```rust
    #[test]
    fn roster_reports_sessions_and_connected_players() {
        use crate::roster::Roster;
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let arden = world.create().expect("create arden");
        let borel = world.create().expect("create borel");
        world.move_to(arden, place(10)).expect("seat arden");
        world.move_to(borel, place(10)).expect("seat borel");

        let mut svc = SessionService::new("W");
        svc.bind_for_test(
            sid(1),
            InWorldBinding {
                account: mud_account::AccountId::new(NonZeroU64::new(1).expect("nonzero")),
                puppet: arden,
                name: mud_account::PuppetName::parse("arden").expect("name"),
            },
        );
        svc.bind_for_test(
            sid(2),
            InWorldBinding {
                account: mud_account::AccountId::new(NonZeroU64::new(2).expect("nonzero")),
                puppet: borel,
                name: mud_account::PuppetName::parse("borel").expect("name"),
            },
        );

        let builtins = Vec::new();
        let resolver = svc.resolver(&builtins);

        assert_eq!(resolver.session_of(arden), Some(sid(1)));
        assert_eq!(resolver.session_of(borel), Some(sid(2)));

        let mut names: Vec<String> = resolver
            .connected()
            .into_iter()
            .map(|p| p.name.as_str().to_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["arden".to_owned(), "borel".to_owned()]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine roster_reports`
Expected: FAIL to compile — `Roster` / `session_of` / `connected` do not exist.

- [ ] **Step 3: Implement.** Create `crates/mud-engine/src/roster.rs`:

```rust
//! The session-registry read seam.
//!
//! [`Roster::session_of`] is the entity→session reverse map the pipeline uses to
//! fan a broadcast out to an audience's sessions; [`Roster::connected`] backs the
//! `who` command. The `SessionService`'s resolver implements it over the live
//! in-world bindings, so `mud-engine`'s command layer never touches the registry
//! storage directly (mirroring the `Places` / `SessionResolver` seams).

use mud_account::PuppetName;
use mud_core::EntityId;
use mud_schema::SessionId;

/// A connected in-world player, for the `who` listing.
#[derive(Debug, Clone)]
#[must_use]
pub struct Presence {
    /// The player's puppet display name.
    pub name: PuppetName,
}

/// Reads the in-world session registry without exposing its storage.
pub trait Roster {
    /// The session controlling `entity`, or `None` if no in-world session does
    /// (an NPC, or an entity nobody is puppeting).
    fn session_of(&self, entity: EntityId) -> Option<SessionId>;

    /// Every connected in-world player, in no guaranteed order.
    fn connected(&self) -> Vec<Presence>;
}
```

  In `crates/mud-engine/src/lib.rs`, register the module and re-export (place with the existing `mod`/`pub use` lines):

```rust
mod roster;
pub use roster::{Presence, Roster};
```

  In `crates/mud-engine/src/caller.rs`: add `use mud_account::PuppetName;`, a `name: PuppetName` field to `CallerContext`, the `name` parameter to `new` (as the 4th arg, after `location`), and an accessor:

```rust
    /// The caller's display name, used when a command names the actor to other
    /// players (`say`, movement). M1: always a player's puppet name.
    pub fn caller_name(&self) -> &PuppetName {
        &self.name
    }
```

  Update `caller.rs`'s own test (`caller_context_exposes_its_parts`) to pass a name, e.g. `PuppetName::parse("hero").expect("name")` as the 4th arg, and assert `ctx.caller_name().as_str() == "hero"`.

  In `crates/mud-engine/src/session/resolver.rs`: add `use mud_core::EntityId;` and `use crate::roster::{Presence, Roster};`; fill the name in `resolve`:

```rust
        Some(ResolvedSession {
            caller: CallerContext::new(
                session,
                binding.puppet,
                location,
                binding.name.clone(),
                Locale::EN,
                LockContext::new(),
            ),
            layers: LayerCommands {
                builtins: self.builtins.to_vec(),
                ..LayerCommands::default()
            },
        })
```

  And implement `Roster` (explicit variant matching, no `_` catch-all):

```rust
impl Roster for RegistryResolver<'_> {
    fn session_of(&self, entity: EntityId) -> Option<SessionId> {
        self.sessions.iter().find_map(|(session, state)| match state {
            SessionState::InWorld(binding) if binding.puppet == entity => Some(*session),
            SessionState::InWorld(_) | SessionState::Login(_) => None,
        })
    }

    fn connected(&self) -> Vec<Presence> {
        self.sessions
            .values()
            .filter_map(|state| match state {
                SessionState::InWorld(binding) => Some(Presence {
                    name: binding.name.clone(),
                }),
                SessionState::Login(_) => None,
            })
            .collect()
    }
}
```

  Update the three test `FakeResolver`s' `CallerContext::new` calls to pass a name as the 4th arg (`Locale::EN` and access shift right one position):
  - `crates/mud-engine/src/pipeline.rs` (~line 237): `PuppetName::parse("hero").expect("name")` (add `use mud_account::PuppetName;` to the test module).
  - `crates/mud-engine/tests/builtins.rs` (~line 104): add `mud_account::PuppetName::parse("hero").expect("name")` and `use mud_account` as needed.
  - `crates/mud-engine/tests/command_pipeline.rs` (~line 139): same.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-engine`
Expected: PASS (the new roster test and all existing tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): Roster port + caller display name (M1-19a)"
```

---

### Task 4: Broadcast substrate — reply slot, `Roster`-driven fan-out, close disposition

Adds `Broadcast`/`SessionDisposition` to the reply, threads the `Roster` through the context, and changes `Pipeline::dispatch` to return `DispatchOutcome { outputs, disposition }` with pre-effect broadcast fan-out. All `dispatch`/`CommandContext::new` call sites and test resolvers' `Roster` impls are updated here.

**Files:**
- Modify: `crates/mud-engine/src/dispatch.rs` (`Broadcast`, `SessionDisposition`, `CommandReply` slots, `CommandContext` roster + `caller_name`)
- Modify: `crates/mud-engine/src/pipeline.rs` (`DispatchOutcome`, `dispatch`/`run_matched` signatures + fan-out; `FakeResolver` `impl Roster`; new substrate test)
- Modify: `crates/mud-engine/src/lib.rs` (re-export `Broadcast`, `SessionDisposition`, `DispatchOutcome`)
- Modify: `crates/mud-engine/tests/builtins.rs` (`FakeResolver` `impl Roster`; `Harness::run` → `.outputs`)
- Modify: `crates/mud-engine/tests/command_pipeline.rs` (`FakeResolver` `impl Roster`; each `dispatch(...).expect(...)` → `.outputs`)

**Interfaces:**
- Consumes: `Roster`, `Presence` (Task 3); `World::occupants_of(PlaceId) -> impl Iterator<Item = EntityId>`; `CommandContext::caller_name`.
- Produces: `Broadcast::to_place(place: PlaceId, except: EntityId, message: StyledText) -> Broadcast`; `CommandReply::with_broadcast(self, Broadcast) -> Self`; `CommandReply::closing(self) -> Self`; `CommandContext::roster(&self) -> &dyn Roster`; `CommandContext::caller_name(&self) -> &PuppetName`; `enum SessionDisposition { Remain, Close }`; `struct DispatchOutcome { pub outputs: Vec<SessionOutput>, pub disposition: SessionDisposition }`; `Pipeline::dispatch(&mut self, &mut World, &dyn Places, &(impl SessionResolver + Roster), &SessionInput) -> Result<DispatchOutcome, PipelineError>`.

- [ ] **Step 1: Write the failing test** — in `crates/mud-engine/src/pipeline.rs` `mod tests`, add a broadcasting handler, a two-session roster, and the fan-out assertion:

```rust
    struct Announcing;

    impl crate::dispatch::CommandHandler for Announcing {
        fn run(&self, ctx: &CommandContext<'_>) -> crate::dispatch::CommandReply {
            crate::dispatch::CommandReply::to_caller(
                mud_core::StyledText::new().plain("you shout"),
            )
            .with_broadcast(crate::dispatch::Broadcast::to_place(
                ctx.location(),
                ctx.caller(),
                mud_core::StyledText::new().plain("someone shouts"),
            ))
        }
    }

    struct TwoSessionResolver {
        speaker_session: mud_schema::SessionId,
        speaker: EntityId,
        listener_session: mud_schema::SessionId,
        listener: EntityId,
        place: PlaceId,
    }

    impl SessionResolver for TwoSessionResolver {
        fn resolve(&self, session: mud_schema::SessionId, _world: &World) -> Option<ResolvedSession> {
            (session == self.speaker_session).then(|| ResolvedSession {
                caller: CallerContext::new(
                    session,
                    self.speaker,
                    self.place,
                    mud_account::PuppetName::parse("arden").expect("name"),
                    Locale::EN,
                    LockContext::new(),
                ),
                layers: {
                    let mut layers = LayerCommands::default();
                    layers.builtins = vec![mud_cmd::Command::new(
                        mud_cmd::CommandName::parse("shout").expect("name"),
                    )];
                    layers
                },
            })
        }
    }

    impl crate::roster::Roster for TwoSessionResolver {
        fn session_of(&self, entity: EntityId) -> Option<mud_schema::SessionId> {
            if entity == self.speaker {
                Some(self.speaker_session)
            } else if entity == self.listener {
                Some(self.listener_session)
            } else {
                None
            }
        }
        fn connected(&self) -> Vec<crate::roster::Presence> {
            Vec::new()
        }
    }

    #[test]
    fn a_broadcast_reaches_other_sessions_and_excludes_the_speaker() {
        let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
        let speaker = world.create().expect("speaker");
        let listener = world.create().expect("listener");
        let place = PlaceId::new(NonZeroU64::new(10).expect("non-zero"));
        world.move_to(speaker, place).expect("seat speaker");
        world.move_to(listener, place).expect("seat listener");

        let resolver = TwoSessionResolver {
            speaker_session: mud_schema::SessionId::new(NonZeroU64::new(1).expect("nz")),
            speaker,
            listener_session: mud_schema::SessionId::new(NonZeroU64::new(2).expect("nz")),
            listener,
            place,
        };
        let mut dispatcher = Dispatcher::new();
        dispatcher.bind(
            mud_cmd::CommandName::parse("shout").expect("name"),
            crate::dispatch::CommandBinding::new(std::sync::Arc::new(Announcing)),
        );
        let mut pipeline = Pipeline::new(dispatcher);

        let outcome = pipeline
            .dispatch(&mut world, &NoPlaces, &resolver, &input(1, "shout"))
            .expect("dispatch");

        // The speaker hears the echo; the listener hears the broadcast; the
        // speaker is not in the broadcast audience.
        assert!(outcome.outputs.iter().any(|o| o.session_id
            == mud_schema::SessionId::new(NonZeroU64::new(1).expect("nz"))
            && o.text.as_str() == "you shout"));
        assert!(outcome.outputs.iter().any(|o| o.session_id
            == mud_schema::SessionId::new(NonZeroU64::new(2).expect("nz"))
            && o.text.as_str() == "someone shouts"));
        assert_eq!(
            outcome.outputs.iter().filter(|o| o.text.as_str() == "someone shouts").count(),
            1,
            "the speaker must not hear their own broadcast"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine a_broadcast_reaches`
Expected: FAIL to compile — `with_broadcast`, `Broadcast`, `DispatchOutcome`, `.outputs` do not exist.

- [ ] **Step 3: Implement.** In `crates/mud-engine/src/dispatch.rs`:

  Add imports: `use mud_account::PuppetName;` and `use crate::roster::Roster;`.

  Add the broadcast + disposition types:

```rust
/// A styled message the pipeline delivers to an audience of *other* co-located
/// sessions (§3.6.3): everyone in `place` except `except` (the actor, who gets
/// the caller reply instead). Resolved against the pre-effect world, so a
/// departure still sees the mover in the room they are leaving.
#[derive(Debug, Clone)]
#[must_use]
pub struct Broadcast {
    place: PlaceId,
    except: EntityId,
    message: StyledText,
}

impl Broadcast {
    /// A broadcast to the occupants of `place` other than `except`.
    pub fn to_place(place: PlaceId, except: EntityId, message: StyledText) -> Self {
        Self {
            place,
            except,
            message,
        }
    }

    pub(crate) fn place(&self) -> PlaceId {
        self.place
    }

    pub(crate) fn except(&self) -> EntityId {
        self.except
    }

    pub(crate) fn message(&self) -> &StyledText {
        &self.message
    }
}

/// Whether a command leaves the caller connected or asks the driver to close the
/// session (`quit`). The socket teardown is the driver's job (M1-21/22); the
/// pipeline only reports the intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDisposition {
    Remain,
    Close,
}
```

  Extend `CommandReply` — add fields, init them in `to_caller`, and add the builders + accessors:

```rust
#[must_use]
pub struct CommandReply {
    output: StyledText,
    effects: Vec<Effect>,
    broadcasts: Vec<Broadcast>,
    disposition: SessionDisposition,
}

impl CommandReply {
    /// A reply that sends `output` to the caller and applies no world effects.
    pub fn to_caller(output: StyledText) -> Self {
        Self {
            output,
            effects: Vec::new(),
            broadcasts: Vec::new(),
            disposition: SessionDisposition::Remain,
        }
    }

    // ... keep with_effect as-is ...

    /// Adds a message delivered to other co-located sessions (§3.6.3). Broadcasts
    /// are resolved before this reply's effects apply.
    pub fn with_broadcast(mut self, broadcast: Broadcast) -> Self {
        self.broadcasts.push(broadcast);
        self
    }

    /// Marks this reply as ending the session (`quit`).
    pub fn closing(mut self) -> Self {
        self.disposition = SessionDisposition::Close;
        self
    }

    // ... keep output() and effects() ...

    /// The broadcasts to fan out to other sessions, in order.
    pub(crate) fn broadcasts(&self) -> &[Broadcast] {
        &self.broadcasts
    }

    /// Whether this reply asks the driver to close the caller's session.
    pub(crate) fn disposition(&self) -> SessionDisposition {
        self.disposition
    }
}
```

  Extend `CommandContext` — add a `roster` field, the `new` parameter (last), and accessors:

```rust
#[must_use]
pub struct CommandContext<'a> {
    command_id: CommandId,
    caller: &'a CallerContext,
    switches: &'a [Switch],
    args: &'a str,
    world: &'a World,
    places: &'a dyn Places,
    roster: &'a dyn Roster,
}
```

  Add `roster` as the final parameter of `CommandContext::new`, store it, and add:

```rust
    /// The actor's display name, for naming them to other players.
    pub fn caller_name(&self) -> &PuppetName {
        self.caller.caller_name()
    }

    /// The session roster, for commands that list or address other sessions
    /// (`who`, broadcast delivery is the pipeline's job).
    pub fn roster(&self) -> &dyn Roster {
        self.roster
    }
```

  In `crates/mud-engine/src/pipeline.rs`:

  Add imports: `use crate::dispatch::SessionDisposition;` and `use crate::roster::Roster;` and `use mud_schema::OutputText;` (already imported — verify).

  Add the outcome type:

```rust
/// What one `dispatch` produced: the outputs to route (caller reply plus any
/// broadcast fan-out, each addressed to its own session) and whether the caller's
/// session should be closed.
#[derive(Debug)]
#[must_use]
pub struct DispatchOutcome {
    pub outputs: Vec<SessionOutput>,
    pub disposition: SessionDisposition,
}

impl DispatchOutcome {
    fn remain(outputs: Vec<SessionOutput>) -> Self {
        Self {
            outputs,
            disposition: SessionDisposition::Remain,
        }
    }
}
```

  Change `dispatch`'s signature and wrap each parse arm in a `DispatchOutcome`:

```rust
    pub fn dispatch(
        &mut self,
        world: &mut World,
        places: &dyn Places,
        resolver: &(impl SessionResolver + Roster),
        input: &SessionInput,
    ) -> Result<DispatchOutcome, PipelineError> {
```

  Replace `let outputs = match table.parse(...) { ... }; Ok(outputs)` with a `DispatchOutcome`:

```rust
        let outcome = match table.parse(input.line.as_str()) {
            ParseOutcome::Empty => DispatchOutcome::remain(Vec::new()),
            ParseOutcome::NotFound => {
                DispatchOutcome::remain(message(session_id, t!(locale, "command.not-found")))
            }
            ParseOutcome::Ambiguous(names) => {
                let options = names
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                DispatchOutcome::remain(message(
                    session_id,
                    t!(locale, "command.ambiguous", options = options),
                ))
            }
            ParseOutcome::BadSwitch(error) => DispatchOutcome::remain(message(
                session_id,
                t!(locale, "command.bad-switch", reason = error),
            )),
            ParseOutcome::Matched {
                command,
                switches,
                args,
            } => self.run_matched(
                world,
                places,
                resolver,
                command_id,
                &caller,
                Parsed {
                    command,
                    switches: &switches,
                    args,
                },
            ),
        };

        Ok(outcome)
```

  Change `run_matched` to take the roster, return a `DispatchOutcome`, fan out broadcasts pre-effect, then apply effects:

```rust
    fn run_matched(
        &self,
        world: &mut World,
        places: &dyn Places,
        roster: &dyn Roster,
        command_id: CommandId,
        caller: &CallerContext,
        parsed: Parsed<'_>,
    ) -> DispatchOutcome {
        let Parsed {
            command,
            switches,
            args,
        } = parsed;
        let session_id = caller.session_id();
        let locale = caller.locale();

        let Some(binding) = self.dispatcher.binding(command.name()) else {
            tracing::warn!(command = %command.name().as_str(), "matched command has no bound handler");
            return DispatchOutcome::remain(message(session_id, t!(locale.clone(), "command.unbound")));
        };

        if let Some(lock) = binding.lock()
            && !lock.evaluate(caller.access())
        {
            tracing::warn!(command = %command.name().as_str(), "lock denied command");
            return DispatchOutcome::remain(message(session_id, t!(locale.clone(), "command.denied")));
        }

        let reply = {
            let ctx =
                CommandContext::new(command_id, caller, switches, args, &*world, places, roster);
            binding.handler().run(&ctx)
        };

        // Caller reply first, then fan out each broadcast to the other sessions
        // in its audience — all resolved against the pre-effect world, before the
        // reply's own effects apply.
        let mut outputs = message(session_id, reply.output().to_plain_string());
        for broadcast in reply.broadcasts() {
            let rendered = broadcast.message().to_plain_string();
            for occupant in world.occupants_of(broadcast.place()) {
                if occupant == broadcast.except() {
                    continue;
                }
                if let Some(recipient) = roster.session_of(occupant) {
                    outputs.push(SessionOutput {
                        session_id: recipient,
                        text: OutputText::new(rendered.clone()),
                    });
                }
            }
        }

        for &effect in reply.effects() {
            if let Some(TickEvent::Rejected { effect, error }) = world.apply_effect(effect) {
                tracing::warn!(
                    command = %command.name().as_str(),
                    ?effect,
                    ?error,
                    "command effect rejected",
                );
            }
        }

        DispatchOutcome {
            outputs,
            disposition: reply.disposition(),
        }
    }
```

  Add `impl crate::roster::Roster for FakeResolver` in `pipeline.rs`'s test module (single caller): `session_of` returns `Some(that session)` when `entity == self.caller` else `None`; `connected` returns an empty `Vec`. (The `TwoSessionResolver` from Step 1 already covers fan-out.)

  In `crates/mud-engine/src/lib.rs`, re-export the new public items alongside the existing dispatch/pipeline exports: `Broadcast`, `SessionDisposition`, `DispatchOutcome`.

  Update the test call sites:
  - `crates/mud-engine/tests/builtins.rs`: add `impl mud_engine::Roster for FakeResolver` (`session_of` → `Some(session())` when `entity == self.caller`, else `None`; `connected` → `vec![Presence { name: PuppetName::parse("hero").expect("name") }]`), import `Presence`/`Roster`/`PuppetName`, and change `Harness::run`'s body to `.expect("dispatch succeeds").outputs`.
  - `crates/mud-engine/tests/command_pipeline.rs`: add the analogous `impl Roster for FakeResolver` (`session_of` → `Some(session(1))` when `entity == self.caller` else `None`; `connected` → empty), and append `.outputs` to each `pipeline.dispatch(...).expect("dispatch succeeds")` whose result is used as a slice (leave the `let result = pipeline.dispatch(...)` error-path assertion at line ~372 unchanged — it inspects the `Result`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-engine && cargo clippy -p mud-engine --all-targets -D warnings`
Expected: PASS, clippy clean.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): broadcast fan-out + close disposition in the pipeline (M1-19a)"
```

---

### Task 5: `say` broadcasts to the room

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs` (add `say.broadcast`)
- Modify: `crates/mud-engine/src/builtins.rs` (`Say` emits a broadcast; imports)
- Create: `crates/mud-engine/tests/broadcast.rs` (two-session harness + `say` test)

**Interfaces:**
- Consumes: `Broadcast::to_place`, `CommandReply::with_broadcast`, `CommandContext::caller_name`, `Roster` (Task 4).

- [ ] **Step 1: Write the failing test** — create `crates/mud-engine/tests/broadcast.rs`:

```rust
//! Cross-player broadcast (§3.6.3): `say` and movement reach other co-located
//! players' sessions via the pipeline's `Roster` fan-out.
#![allow(clippy::expect_used)] // integration test

use std::collections::HashMap;
use std::num::NonZeroU64;

use mud_account::{AccountId, PuppetName};
use mud_core::{
    Description, Direction, EntityId, LockContext, Place, PlaceId, RegionId, RoomData, TenantTag,
    Title, World,
};
use mud_engine::{
    CallerContext, Dispatcher, LayerCommands, Pipeline, Places, Presence, ResolvedSession, Roster,
    SessionResolver,
};
use mud_i18n::Locale;
use mud_schema::{InputLine, SessionId, SessionInput, SessionOutput};

const HALL: u64 = 10;
const STUDY: u64 = 11;

fn place(v: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(v).expect("place nz"))
}
fn sid(v: u64) -> SessionId {
    SessionId::new(NonZeroU64::new(v).expect("session nz"))
}
fn region() -> RegionId {
    RegionId::new(NonZeroU64::new(1).expect("region nz"))
}

/// Two rooms: HALL ↔ STUDY (north/south).
struct Rooms(HashMap<PlaceId, Place>);
impl Rooms {
    fn two() -> Self {
        let hall = Place::Room(
            RoomData::new(place(HALL), region(), Description::new("A hall."))
                .with_title(Title::new("The Hall"))
                .with_exit(Direction::North, place(STUDY)),
        );
        let study = Place::Room(
            RoomData::new(place(STUDY), region(), Description::new("A study."))
                .with_title(Title::new("The Study"))
                .with_exit(Direction::South, place(HALL)),
        );
        let mut m = HashMap::new();
        m.insert(place(HALL), hall);
        m.insert(place(STUDY), study);
        Self(m)
    }
}
impl Places for Rooms {
    fn get(&self, id: PlaceId) -> Option<&Place> {
        self.0.get(&id)
    }
}

/// A fixed set of in-world players: (session, entity, name), each with a location
/// read live from the world so movement is reflected on the next command.
struct Players {
    players: Vec<(SessionId, EntityId, PuppetName)>,
    builtins: Vec<mud_cmd::Command>,
}
impl SessionResolver for Players {
    fn resolve(&self, session: SessionId, world: &World) -> Option<ResolvedSession> {
        let (s, entity, name) = self
            .players
            .iter()
            .find(|(s, ..)| *s == session)
            .cloned()?;
        let location = world.location_of(entity)?;
        Some(ResolvedSession {
            caller: CallerContext::new(s, entity, location, name, Locale::EN, LockContext::new()),
            layers: LayerCommands {
                builtins: self.builtins.clone(),
                ..LayerCommands::default()
            },
        })
    }
}
impl Roster for Players {
    fn session_of(&self, entity: EntityId) -> Option<SessionId> {
        self.players
            .iter()
            .find(|(_, e, _)| *e == entity)
            .map(|(s, ..)| *s)
    }
    fn connected(&self) -> Vec<Presence> {
        self.players
            .iter()
            .map(|(_, _, name)| Presence { name: name.clone() })
            .collect()
    }
}

fn text_for(outputs: &[SessionOutput], session: SessionId) -> String {
    outputs
        .iter()
        .filter(|o| o.session_id == session)
        .map(|o| o.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn say_echoes_to_the_speaker_and_broadcasts_to_the_room() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    let borel = world.create().expect("borel");
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("say hello"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    let speaker = text_for(&outcome.outputs, sid(1));
    let listener = text_for(&outcome.outputs, sid(2));
    assert!(speaker.contains("You say") && speaker.contains("hello"), "speaker: {speaker}");
    assert!(listener.contains("arden") && listener.contains("hello"), "listener: {listener}");
    assert!(!listener.contains("You say"), "listener must not see the echo: {listener}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine --test broadcast say_echoes`
Expected: FAIL — the listener receives no broadcast (`say` does not broadcast yet).

- [ ] **Step 3: Implement.** In `crates/mud-i18n/src/catalog.rs`, add to the `ENTRIES` table (under the `say` group):

```rust
        ("say.broadcast", "{ $name } says, \"{ $message }\""),
```

  In `crates/mud-engine/src/builtins.rs`, import `Broadcast` (`use crate::dispatch::{Broadcast, CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher};`) and make `Say` broadcast:

```rust
impl CommandHandler for Say {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let message = match sanitize(ctx.args()) {
            Ok(message) => message,
            Err(_) => return CommandReply::to_caller(system(t!(locale, "content.too-long"))),
        };
        if message.trim().is_empty() {
            return CommandReply::to_caller(system(t!(locale, "say.nothing")));
        }
        let name = ctx.caller_name().as_str().to_owned();
        // The caller hears "You say, …"; everyone else in the room hears
        // "<name> says, …". Sanitized player text is plain, so any markup renders
        // literally (§3.20.7).
        let heard = StyledText::new().role(
            t!(locale, "say.broadcast", name = name, message = message.clone()),
            RoleName::SAY,
        );
        CommandReply::to_caller(
            StyledText::new().role(t!(locale, "say.speech", message = message), RoleName::SAY),
        )
        .with_broadcast(Broadcast::to_place(ctx.location(), ctx.caller(), heard))
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-engine --test broadcast say_echoes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): say broadcasts to the room (M1-19a)"
```

---

### Task 6: movement broadcasts arrival/departure

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs` (add `move.depart`, `move.arrive-from`, `move.arrive`)
- Modify: `crates/mud-engine/src/builtins.rs` (`Move` emits departure + arrival)
- Modify: `crates/mud-engine/tests/broadcast.rs` (movement test)

**Interfaces:**
- Consumes: `Direction::opposite` (Task 1), `direction_name` (existing in `builtins.rs`), `Broadcast::to_place`.

- [ ] **Step 1: Write the failing test** — append to `crates/mud-engine/tests/broadcast.rs`:

```rust
#[test]
fn moving_announces_departure_and_arrival_to_the_two_rooms() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden"); // mover
    let borel = world.create().expect("borel"); // stays in HALL
    let cade = world.create().expect("cade"); // waits in STUDY
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");
    world.move_to(cade, place(STUDY)).expect("seat cade");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
            (sid(3), cade, PuppetName::parse("cade").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("north"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    // Borel (left behind in HALL) sees the departure; Cade (in STUDY) sees the
    // arrival from the south (the opposite of travelling north).
    let left_behind = text_for(&outcome.outputs, sid(2));
    let destination = text_for(&outcome.outputs, sid(3));
    assert!(left_behind.contains("arden") && left_behind.contains("leaves"), "depart: {left_behind}");
    assert!(destination.contains("arden") && destination.contains("arrives") && destination.contains("south"), "arrive: {destination}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine --test broadcast moving_announces`
Expected: FAIL — no departure/arrival broadcasts yet.

- [ ] **Step 3: Implement.** In `crates/mud-i18n/src/catalog.rs`, under the `movement` group:

```rust
        ("move.depart", "{ $name } leaves { $direction }."),
        ("move.arrive-from", "{ $name } arrives from { $direction }."),
        ("move.arrive", "{ $name } arrives."),
```

  In `crates/mud-engine/src/builtins.rs`, replace the tail of `Move::run` (the successful-move branch) so it broadcasts to both rooms before the effect:

```rust
        // Show the destination room as the caller arrives; the MoveTo effect is
        // applied by the pipeline after this handler returns.
        let arrival = render_room(place, ctx.world(), ctx.caller(), &locale);
        let name = ctx.caller_name().as_str().to_owned();
        let depart = StyledText::new().role(
            t!(locale, "move.depart", name = name.clone(), direction = direction_name(self.0)),
            RoleName::SYSTEM,
        );
        let arrive = StyledText::new().role(
            t!(
                locale,
                "move.arrive-from",
                name = name,
                direction = direction_name(self.0.opposite())
            ),
            RoleName::SYSTEM,
        );
        CommandReply::to_caller(arrival)
            .with_broadcast(Broadcast::to_place(ctx.location(), ctx.caller(), depart))
            .with_broadcast(Broadcast::to_place(to, ctx.caller(), arrive))
            .with_effect(Effect::MoveTo {
                entity: ctx.caller(),
                place: to,
            })
```

  (`ctx.location()` is the room being left; `to` is the destination — both resolved against the pre-effect world, so departure sees the mover still present, arrival does not yet include them.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-engine --test broadcast`
Expected: PASS (both broadcast tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): movement announces arrival/departure (M1-19a)"
```

---

### Task 7: `who` lists connected players

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs` (add `who.online`)
- Modify: `crates/mud-engine/src/builtins.rs` (`Who` handler + table entry; import `Roster`)
- Modify: `crates/mud-engine/tests/broadcast.rs` (`who` test)

**Interfaces:**
- Consumes: `CommandContext::roster`, `Roster::connected`, `Presence`.

- [ ] **Step 1: Write the failing test** — append to `crates/mud-engine/tests/broadcast.rs`:

```rust
#[test]
fn who_lists_the_connected_players() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    let borel = world.create().expect("borel");
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("who"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    let listed = text_for(&outcome.outputs, sid(1));
    assert!(listed.contains("arden") && listed.contains("borel"), "who: {listed}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine --test broadcast who_lists`
Expected: FAIL — `who` is not a bound command.

- [ ] **Step 3: Implement.** In `crates/mud-i18n/src/catalog.rs`:

```rust
        ("who.online", "Players online: { $names }"),
```

  In `crates/mud-engine/src/builtins.rs`, import the trait so `ctx.roster()` methods are in scope (`use crate::roster::Roster;`), add the handler, and register it in `table()`:

```rust
/// `who`: list the connected players (§3.19).
struct Who;

impl CommandHandler for Who {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let mut names: Vec<String> = ctx
            .roster()
            .connected()
            .into_iter()
            .map(|presence| presence.name.as_str().to_owned())
            .collect();
        names.sort();
        CommandReply::to_caller(system(t!(locale, "who.online", names = names.join(", "))))
    }
}
```

  Add to the `table()` vector: `("who", &[], Arc::new(Who)),`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-engine --test broadcast who_lists`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): who lists connected players (M1-19a)"
```

---

### Task 8: in-world `quit` signals a close

**Files:**
- Modify: `crates/mud-i18n/src/catalog.rs` (add `quit.goodbye`)
- Modify: `crates/mud-engine/src/builtins.rs` (`Quit` handler + table entry)
- Modify: `crates/mud-engine/tests/broadcast.rs` (`quit` test)

**Interfaces:**
- Consumes: `CommandReply::closing`, `SessionDisposition`, `DispatchOutcome`.

- [ ] **Step 1: Write the failing test** — append to `crates/mud-engine/tests/broadcast.rs` (add `SessionDisposition` to the `mud_engine` import line):

```rust
#[test]
fn quit_signals_a_close_with_a_goodbye() {
    use mud_engine::SessionDisposition;
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    world.move_to(arden, place(HALL)).expect("seat arden");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![(sid(1), arden, PuppetName::parse("arden").expect("name"))],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("quit"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    assert_eq!(outcome.disposition, SessionDisposition::Close);
    assert!(text_for(&outcome.outputs, sid(1)).contains("Goodbye"), "goodbye shown");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine --test broadcast quit_signals`
Expected: FAIL — `quit` is not a bound command (disposition stays `Remain`).

- [ ] **Step 3: Implement.** In `crates/mud-i18n/src/catalog.rs`:

```rust
        ("quit.goodbye", "Goodbye!"),
```

  In `crates/mud-engine/src/builtins.rs`, add the handler and register it:

```rust
/// `quit`: leave the game. Signals the driver to close the session (§3.19); the
/// socket teardown is the gateway's job (M1-21/22).
struct Quit;

impl CommandHandler for Quit {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        CommandReply::to_caller(system(t!(locale, "quit.goodbye"))).closing()
    }
}
```

  Add to `table()`: `("quit", &[], Arc::new(Quit)),`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-engine --test broadcast quit_signals`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mud-engine): in-world quit signals a session close (M1-19a)"
```

---

### Task 9: end-to-end broadcast through the real `RegistryResolver`

Proves the fan-out works through the production `SessionService`/`RegistryResolver` (Roster) — not just the test fakes — using the crate-internal `bind_for_test` seam.

**Files:**
- Modify: `crates/mud-engine/src/session/mod.rs` (`mod tests`: one unit test)

**Interfaces:**
- Consumes: `SessionService::bind_for_test`, `SessionService::resolver`, `Pipeline`, `mud_engine::register`, `Broadcast` fan-out.

- [ ] **Step 1: Write the failing test** — add to `crates/mud-engine/src/session/mod.rs` `mod tests`:

```rust
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
        let mut svc = SessionService::new("W");
        svc.bind_for_test(
            sid(1),
            InWorldBinding { account: acct(1), puppet: arden, name: PuppetName::parse("arden").expect("name") },
        );
        svc.bind_for_test(
            sid(2),
            InWorldBinding { account: acct(2), puppet: borel, name: PuppetName::parse("borel").expect("name") },
        );

        let mut dispatcher = Dispatcher::new();
        let builtins = crate::register(&mut dispatcher);
        let resolver = svc.resolver(&builtins);
        let mut pipeline = Pipeline::new(dispatcher);

        let outcome = pipeline
            .dispatch(
                &mut world,
                &room,
                &resolver,
                &SessionInput { session_id: sid(1), line: InputLine::new("say hi") },
            )
            .expect("dispatch");

        assert!(
            outcome.outputs.iter().any(|o| o.session_id == sid(2) && o.text.as_str().contains("arden") && o.text.as_str().contains("hi")),
            "the second session must receive the broadcast",
        );
    }
```

  (Add `use mud_core::TenantTag;` / `use mud_schema::SessionId;` etc. to the test module if not already present — the module's existing `mod tests` already imports `TenantTag`, `AccountId`, `PuppetName`, `sid`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine say_broadcasts_through`
Expected: FAIL to compile first if imports are missing, then — with imports — PASS only once Tasks 4–5 are in (they are). If run in order this passes immediately; the value is the regression guard over the real resolver. If it does not compile, fix the imports named above.

- [ ] **Step 3: (Covered by Tasks 4–5.)** No new production code — this task is the end-to-end regression test. If the assertion fails, the defect is in the real `RegistryResolver::session_of` (Task 3) or the fan-out (Task 4); fix there.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mud-engine say_broadcasts_through`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "test(mud-engine): broadcast through the real session resolver (M1-19a)"
```

---

### Task 10: docs, journal, and full workspace gate

**Files:**
- Modify: `docs/docs/playing/commands.md` (document `who`, `quit`, and room-heard `say`/movement)
- Modify: `.claude/JOURNAL.md` (append the M1-19a entry)

- [ ] **Step 1: Update the player docs.** In `docs/docs/playing/commands.md`, add entries for `who` (lists online players) and `quit` (leaves the game), and note under `say`/movement that other players in the room now hear them. Match the page's existing wording/format.

- [ ] **Step 2: Verify docs build**

Run: `cd docs && uv run mkdocs build --strict`
Expected: clean build (no warnings-as-errors).

- [ ] **Step 3: Append the journal entry.** Add to `.claude/JOURNAL.md` (newest at the bottom), following the `CLAUDE.md` format: Spec (§2.7 step 8, §3.6.3, §3.19), Done (broadcast fan-out via `Roster`; `PuppetName` in the binding; `say`/movement broadcasts; `who`; `quit` close disposition; `Direction::opposite`), Verify (the test list below), Next (M1-20; the deferred i18n per-world locale spec/impl rework; gateway close teardown at M1-21/22). Note the §3.6.3 rationale (audience is entity-based; session-less NPCs are skipped at fan-out; NPC perception reuses occupants at M5) and that the i18n per-world locale rework is a tracked separate task.

- [ ] **Step 4: Full workspace gate**

Run:
```bash
cargo test --workspace \
  && cargo clippy --workspace --all-targets -D warnings \
  && cargo fmt --all --check
```
Expected: all green; no `unwrap`/`expect`/`panic` outside tests.

- [ ] **Step 5: Commit**

```bash
jj commit -m "docs(m1-19a): document who/quit/broadcast + journal entry"
```

---

## Self-Review

**Spec coverage** (against `2026-07-02-m1-19a-session-dependent-builtins-design.md`):
- Broadcast (`say`) → Tasks 4–5. Movement arrival/departure → Task 6. `who` → Task 7. `quit` close disposition → Tasks 4 + 8. `PuppetName` into binding → Task 2. `Direction::opposite` → Task 1. `Roster` port → Task 3. Pre-effect fan-out → Task 4. Real-resolver end-to-end → Task 9. Docs/journal → Task 10.
- §3.6.3 NPC-hearing: design note only (no code); recorded in Task 10's journal entry — no task needed (session-less entities are skipped at fan-out by construction).
- i18n per-world locale rework: intentionally **out of scope** (separate follow-up task per the design); Task 10's journal notes it.

**Placeholder scan:** none — every code step shows the code; every run step shows the command + expected result.

**Type consistency:** `Broadcast::to_place`, `CommandReply::{with_broadcast,closing}`, `CommandContext::{roster,caller_name}`, `Roster::{session_of,connected}`, `Presence.name`, `SessionDisposition::{Remain,Close}`, `DispatchOutcome.{outputs,disposition}`, `Terminal::Bound{account,puppet,name}`, `InWorldBinding{account,puppet,name}`, `Direction::opposite` — used consistently across Tasks 1–9. `CallerContext::new` gains `name` as its 4th argument (before `locale`) uniformly in Task 3.
