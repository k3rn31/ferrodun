# Room Presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Announce a player's spawn (login) and departure (quit/socket drop) to their room, and list connected players in `look` — per `docs/superpowers/specs/2026-07-15-room-presence-design.md`.

**Architecture:** A new `Roster::name_of` primitive answers "is this occupant a connected player, and what's their name?" A new `mud-engine::presence` module owns the single audience fan-out implementation (extracted from `Pipeline`) plus the enter/leave messages; `mudd::world_loop` calls it at three lifecycle sites (spawn, quit-Close, gateway Disconnect). `look` partitions occupants into a Diku-voice players sentence and the existing keyword list. `mud-core` and `mud-session` are untouched.

**Tech Stack:** Rust workspace (`mud-engine`, `mudd`, `mud-i18n`), tokio, existing `t!` i18n macro, jj (Jujutsu) for VCS.

## Global Constraints

- VCS is **jj**, not git: commit with `jj commit -m "message" <filesets>`. Never use `git` commands.
- `unwrap()` forbidden everywhere; `expect()` only in tests, with a descriptive message. No `panic!`/`todo!`/`unreachable!` in production code (only under a documented `// INVARIANT:`).
- Integration-test files (in `tests/`) need the crate-level header `#![allow(clippy::expect_used, clippy::panic)]` with the standard justification comment (copy from an existing `tests/*.rs`).
- Workspace must stay green: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`.
- Message copy (exact, from the spec): `{ $name } appears from nowhere.` / `{ $name } disappears.` / `{ $name } is here.` / `{ $names } are here.`
- New i18n keys: `presence.enter`, `presence.leave`, `look.player-here`, `look.players-here`. Every `t!`-referenced key MUST exist in the `en` catalog (SPEC §3.14.6.2).
- Comments explain *why*, never *how*; doc comments on all public items.
- Newtypes cross public APIs (`PuppetName`, `EntityId`, `PlaceId`, `SessionId`) — never raw primitives.

---

### Task 1: `Roster::name_of`

**Files:**
- Modify: `crates/mud-engine/src/roster.rs`
- Modify: `crates/mud-engine/src/session/resolver.rs`
- Modify: `crates/mud-engine/tests/builtins.rs:120-129` (FakeResolver's `impl Roster`)
- Modify: `crates/mud-engine/tests/command_pipeline.rs:151-159` (FakeResolver's `impl Roster`)
- Modify: `crates/mud-engine/tests/broadcast.rs:77-93` (Players' `impl Roster`)

**Interfaces:**
- Consumes: existing `trait Roster { session_of, connected }`, `SessionState::InWorld(InWorldBinding { puppet, name, .. })`.
- Produces: `fn name_of(&self, entity: EntityId) -> Option<PuppetName>` on `trait Roster` — Tasks 2, 3 and 5 rely on it.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module at the bottom of `crates/mud-engine/src/session/resolver.rs` (inside `mod tests`):

```rust
    #[test]
    fn name_of_names_connected_puppets_only() {
        use crate::roster::Roster;
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let arden = world.create().expect("create arden");
        let stray = world.create().expect("create stray");
        world.move_to(arden, place(10)).expect("seat arden");
        world.move_to(stray, place(10)).expect("seat stray");

        let mut svc = SessionService::new("W", Locale::EN);
        svc.bind_for_test(
            sid(1),
            InWorldBinding {
                account: mud_account::AccountId::new(NonZeroU64::new(1).expect("nonzero")),
                puppet: arden,
                name: mud_account::PuppetName::parse("arden").expect("name"),
            },
        );

        let builtins = Vec::new();
        let resolver = svc.resolver(&builtins);

        assert_eq!(
            resolver.name_of(arden).map(|n| n.as_str().to_owned()),
            Some("arden".to_owned())
        );
        assert!(
            resolver.name_of(stray).is_none(),
            "a session-less entity has no roster name"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine name_of_names_connected_puppets_only`
Expected: FAIL to compile — `no method named 'name_of' found for struct 'RegistryResolver'`.

- [ ] **Step 3: Add the trait method and production impl**

In `crates/mud-engine/src/roster.rs`, add to `trait Roster` (after `connected`):

```rust
    /// The display name of `entity` when a connected in-world session puppets
    /// it, or `None` for anything session-less (an item, an NPC, a
    /// disconnected puppet's body).
    fn name_of(&self, entity: EntityId) -> Option<PuppetName>;
```

In `crates/mud-engine/src/session/resolver.rs`, add to `impl Roster for RegistryResolver<'_>`:

```rust
    fn name_of(&self, entity: EntityId) -> Option<PuppetName> {
        self.sessions.values().find_map(|state| match state {
            SessionState::InWorld(binding) if binding.puppet == entity => {
                Some(binding.name.clone())
            }
            SessionState::InWorld(_) | SessionState::Login(_) => None,
        })
    }
```

- [ ] **Step 4: Update the three test fakes (compile errors point at them)**

`crates/mud-engine/tests/builtins.rs`, inside `impl Roster for FakeResolver`:

```rust
    fn name_of(&self, entity: EntityId) -> Option<PuppetName> {
        (entity == self.caller).then(|| PuppetName::parse("hero").expect("name"))
    }
```

`crates/mud-engine/tests/command_pipeline.rs`, inside `impl Roster for FakeResolver`:

```rust
    fn name_of(&self, entity: EntityId) -> Option<mud_account::PuppetName> {
        (entity == self.caller)
            .then(|| mud_account::PuppetName::parse("hero").expect("name"))
    }
```

`crates/mud-engine/tests/broadcast.rs`, inside `impl Roster for Players`:

```rust
    fn name_of(&self, entity: EntityId) -> Option<PuppetName> {
        self.players
            .iter()
            .find(|(_, e, _)| *e == entity)
            .map(|(.., name)| name.clone())
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mud-engine`
Expected: PASS, including `name_of_names_connected_puppets_only`.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(engine): Roster::name_of names connected puppets" crates/mud-engine
```

---

### Task 2: `presence` module + pipeline refactor

**Files:**
- Create: `crates/mud-engine/src/presence.rs`
- Modify: `crates/mud-engine/src/lib.rs` (add `pub mod presence;` alongside the existing `mod` declarations)
- Modify: `crates/mud-engine/src/pipeline.rs:212-230` (fan-out loop)
- Modify: `crates/mud-i18n/src/catalog.rs` (`ENTRIES` table)

**Interfaces:**
- Consumes: `Roster::session_of` (Task 1's trait, method pre-existing), `World::occupants_of(place) -> impl Iterator<Item = EntityId>`, `t!` macro, `StyledText::new().role(String, RoleName)`.
- Produces (Task 5 relies on these exact signatures):
  - `pub fn presence::announce(world: &World, roster: &dyn Roster, place: PlaceId, except: EntityId, message: &StyledText) -> Vec<SessionOutput>`
  - `pub fn presence::entered(locale: Locale, name: &str) -> StyledText`
  - `pub fn presence::left(locale: Locale, name: &str) -> StyledText`

- [ ] **Step 1: Add the i18n keys**

In `crates/mud-i18n/src/catalog.rs`, add to the `ENTRIES` table (after the `move.*` rows):

```rust
    // presence lifecycle (§2.7 step 8): spawn and quit/disconnect
    ("presence.enter", "{ $name } appears from nowhere."),
    ("presence.leave", "{ $name } disappears."),
```

- [ ] **Step 2: Write the module with failing tests**

Create `crates/mud-engine/src/presence.rs`:

```rust
//! Room-presence announcements (§2.7 step 8).
//!
//! One audience-resolution implementation for every room broadcast: the
//! pipeline's `say`/movement fan-out and the session-lifecycle announcements
//! (spawn, quit, socket drop) all resolve through [`announce`], so the
//! "occupants minus the actor, mapped to sessions" semantics cannot drift.

use mud_core::{EntityId, PlaceId, RoleName, StyledText, World};
use mud_i18n::{Locale, t};
use mud_schema::{OutputText, SessionOutput};

use crate::roster::Roster;

/// The outputs delivering `message` to every occupant of `place` except
/// `except`, one per occupant with a connected session. Session-less
/// occupants (items, NPCs, disconnected bodies) are skipped; an empty room
/// yields no outputs.
pub fn announce(
    world: &World,
    roster: &dyn Roster,
    place: PlaceId,
    except: EntityId,
    message: &StyledText,
) -> Vec<SessionOutput> {
    world
        .occupants_of(place)
        .filter(|&occupant| occupant != except)
        .filter_map(|occupant| roster.session_of(occupant))
        .map(|session_id| SessionOutput {
            session_id,
            text: OutputText::new(message.clone()),
        })
        .collect()
}

/// The room message announcing `name` entering the world (`presence.enter`).
pub fn entered(locale: Locale, name: &str) -> StyledText {
    StyledText::new().role(t!(locale, "presence.enter", name = name), RoleName::SYSTEM)
}

/// The room message announcing `name` leaving the world (`presence.leave`).
pub fn left(locale: Locale, name: &str) -> StyledText {
    StyledText::new().role(t!(locale, "presence.leave", name = name), RoleName::SYSTEM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_core::TenantTag;
    use std::collections::HashMap;
    use std::num::NonZeroU64;

    use crate::roster::Presence;
    use mud_schema::SessionId;

    /// A roster over a fixed entity→session map.
    struct FakeRoster(HashMap<EntityId, SessionId>);

    impl Roster for FakeRoster {
        fn session_of(&self, entity: EntityId) -> Option<SessionId> {
            self.0.get(&entity).copied()
        }
        fn connected(&self) -> Vec<Presence> {
            Vec::new()
        }
        fn name_of(&self, _entity: EntityId) -> Option<mud_account::PuppetName> {
            None
        }
    }

    fn sid(n: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    fn place(n: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    #[test]
    fn announce_reaches_co_located_sessions_except_the_actor() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let actor = world.create().expect("actor");
        let witness = world.create().expect("witness");
        let prop = world.create().expect("prop"); // session-less: skipped
        for entity in [actor, witness, prop] {
            world.move_to(entity, place(10)).expect("seat entity");
        }
        let roster = FakeRoster(HashMap::from([(actor, sid(1)), (witness, sid(2))]));

        let message = StyledText::new().plain("Bob appears from nowhere.");
        let outputs = announce(&world, &roster, place(10), actor, &message);

        assert_eq!(outputs.len(), 1, "only the witness hears it");
        let output = outputs.first().expect("one output");
        assert_eq!(output.session_id, sid(2));
        assert_eq!(output.text.to_plain_string(), "Bob appears from nowhere.");
    }

    #[test]
    fn announce_to_an_empty_audience_yields_nothing() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let actor = world.create().expect("actor");
        world.move_to(actor, place(10)).expect("seat actor");
        let roster = FakeRoster(HashMap::from([(actor, sid(1))]));

        let message = StyledText::new().plain("gone");
        assert!(announce(&world, &roster, place(10), actor, &message).is_empty());
    }

    #[test]
    fn lifecycle_messages_render_the_spec_copy() {
        assert_eq!(
            entered(Locale::EN, "Bob").to_plain_string(),
            "Bob appears from nowhere."
        );
        assert_eq!(left(Locale::EN, "Bob").to_plain_string(), "Bob disappears.");
    }
}
```

In `crates/mud-engine/src/lib.rs`, add alongside the other module declarations:

```rust
pub mod presence;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p mud-engine presence`
Expected: PASS (3 tests). If `t!` rejects `name = name` for a `&str` argument, match the existing call style in `builtins/say.rs`/`movement.rs` (`name = name.to_owned()`).

- [ ] **Step 4: Refactor the pipeline fan-out onto `announce`**

In `crates/mud-engine/src/pipeline.rs`, replace the broadcast loop in `run_matched` (the `for broadcast in reply.broadcasts()` block, currently lines 217–230):

```rust
        let mut outputs = message(session_id, reply.output().clone());
        for broadcast in reply.broadcasts() {
            outputs.extend(crate::presence::announce(
                world,
                roster,
                broadcast.place(),
                broadcast.except(),
                broadcast.message(),
            ));
        }
```

(The `use mud_schema::{OutputText, ...}` import may become partially unused — remove only what YOUR change orphaned.)

- [ ] **Step 5: Run the regression gate**

Run: `cargo test -p mud-engine`
Expected: PASS — in particular every test in `tests/broadcast.rs`, unmodified. Then `cargo clippy -p mud-engine --all-targets` — clean.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(engine): presence module owns the room-broadcast fan-out" crates/mud-engine crates/mud-i18n
```

---

### Task 3: `look` lists connected players

**Files:**
- Modify: `crates/mud-engine/src/builtins/look.rs`
- Modify: `crates/mud-engine/src/builtins/movement.rs:40` (the `render_room` call)
- Modify: `crates/mud-i18n/src/catalog.rs` (`ENTRIES` table)
- Test: `crates/mud-engine/src/builtins/look.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Roster::name_of` (Task 1), `CommandContext::roster() -> &dyn Roster` (pre-existing), `display_name(world, entity) -> Option<String>` (pre-existing in `builtins/mod.rs`).
- Produces: `pub(super) fn render_room(place: &Place, world: &World, roster: &dyn Roster, viewer: EntityId, locale: &Locale) -> StyledText` — the signature Task 5's e2e exercises via `look`.

- [ ] **Step 1: Add the i18n keys**

In `crates/mud-i18n/src/catalog.rs`, add to `ENTRIES` (next to the other `look.*` rows):

```rust
    ("look.player-here", "{ $name } is here."),
    ("look.players-here", "{ $names } are here."),
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/mud-engine/src/builtins/look.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::PuppetName;
    use mud_core::{Description, Keyword, RegionId, RoomData, TenantTag, Title};
    use mud_i18n::Locale;
    use std::collections::HashMap;
    use std::num::NonZeroU64;

    use crate::roster::{Presence, Roster};
    use mud_schema::SessionId;

    /// A roster naming a fixed set of player entities.
    struct FakeRoster(HashMap<EntityId, PuppetName>);

    impl Roster for FakeRoster {
        fn session_of(&self, entity: EntityId) -> Option<SessionId> {
            self.0
                .contains_key(&entity)
                .then(|| SessionId::new(NonZeroU64::new(1).expect("nonzero")))
        }
        fn connected(&self) -> Vec<Presence> {
            Vec::new()
        }
        fn name_of(&self, entity: EntityId) -> Option<PuppetName> {
            self.0.get(&entity).cloned()
        }
    }

    fn room(id: u64) -> Place {
        Place::Room(
            RoomData::new(
                PlaceId::new(NonZeroU64::new(id).expect("nonzero")),
                RegionId::new(NonZeroU64::new(1).expect("nonzero")),
                Description::new("A taproom."),
            )
            .with_title(Title::new("The Tavern")),
        )
    }

    fn puppet_name(name: &str) -> PuppetName {
        PuppetName::parse(name).expect("valid puppet name")
    }

    /// Seats `count` entities in the room, returning them in creation order.
    fn seated(world: &mut World, place: PlaceId, count: usize) -> Vec<EntityId> {
        (0..count)
            .map(|_| {
                let entity = world.create().expect("create entity");
                world.move_to(entity, place).expect("seat entity");
                entity
            })
            .collect()
    }

    /// The nth seated entity (clippy denies slice indexing workspace-wide).
    fn nth(entities: &[EntityId], index: usize) -> EntityId {
        *entities.get(index).expect("seated entity")
    }

    #[test]
    fn one_player_renders_the_singular_sentence() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 2);
        let (viewer, alice) = (nth(&entities, 0), nth(&entities, 1));
        let roster = FakeRoster(HashMap::from([(alice, puppet_name("Alice"))]));

        let text = render_room(&place, &world, &roster, viewer, &Locale::EN).to_plain_string();
        assert!(text.contains("Alice is here."), "got: {text}");
        assert!(!text.contains("are here"), "got: {text}");
    }

    #[test]
    fn many_players_collapse_into_one_and_joined_sentence() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 4);
        let roster = FakeRoster(HashMap::from([
            (nth(&entities, 1), puppet_name("Carol")),
            (nth(&entities, 2), puppet_name("Alice")),
            (nth(&entities, 3), puppet_name("Bob")),
        ]));

        let text = render_room(&place, &world, &roster, nth(&entities, 0), &Locale::EN)
            .to_plain_string();
        // Sorted, comma-joined with a final "and" (design: Diku voice, one line).
        assert!(text.contains("Alice, Bob and Carol are here."), "got: {text}");
    }

    #[test]
    fn objects_stay_in_the_also_here_list_and_players_leave_it() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 3);
        let (viewer, alice, sword) = (nth(&entities, 0), nth(&entities, 1), nth(&entities, 2));
        world
            .name_entity(sword, vec![Keyword::new("sword")])
            .expect("name the sword");
        let roster = FakeRoster(HashMap::from([(alice, puppet_name("Alice"))]));

        let text = render_room(&place, &world, &roster, viewer, &Locale::EN).to_plain_string();
        assert!(text.contains("Alice is here."), "got: {text}");
        assert!(text.contains("Also here: sword"), "got: {text}");
        assert!(!text.contains("Also here: Alice"), "got: {text}");
    }

    #[test]
    fn a_nameless_session_less_occupant_is_still_skipped() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 2);
        let roster = FakeRoster(HashMap::new());

        let text = render_room(&place, &world, &roster, nth(&entities, 0), &Locale::EN)
            .to_plain_string();
        assert!(!text.contains("here"), "got: {text}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p mud-engine --lib look`
Expected: FAIL to compile — `render_room` takes 4 arguments but 5 were supplied.

- [ ] **Step 4: Implement the partition**

Rewrite the occupant section of `crates/mud-engine/src/builtins/look.rs`:

Change the signature and body of `render_room`:

```rust
/// Renders a room as the caller sees it: title, description, exits, the
/// connected players present, and the other entities present (§3.2).
pub(super) fn render_room(
    place: &Place,
    world: &World,
    roster: &dyn Roster,
    viewer: EntityId,
    locale: &Locale,
) -> StyledText {
    let mut out = StyledText::new();
    if let Some(title) = place.title() {
        append(&mut out, title.styled());
        out.push(Span::plain("\n"));
    }
    append(&mut out, place.describe(viewer).styled());

    let exits = exit_names(place);
    if !exits.is_empty() {
        out.push(Span::role(
            format!("\n{}", t!(locale, "look.exits", exits = exits.join(", "))),
            RoleName::SYSTEM,
        ));
    }

    let (players, things) = occupants(world, roster, place.id(), viewer);
    if let Some(line) = players_line(&players, locale) {
        out.push(Span::role(format!("\n{line}"), RoleName::SYSTEM));
    }
    if !things.is_empty() {
        out.push(Span::role(
            format!(
                "\n{}",
                t!(locale, "look.also-here", names = things.join(", "))
            ),
            RoleName::SYSTEM,
        ));
    }
    out
}
```

Replace `occupant_names` with:

```rust
/// Splits the occupants of `place` other than `viewer` into connected players
/// (roster names, sorted for a stable sentence) and keyword-named things,
/// skipping entities with neither name source.
fn occupants(
    world: &World,
    roster: &dyn Roster,
    place: PlaceId,
    viewer: EntityId,
) -> (Vec<String>, Vec<String>) {
    let mut players = Vec::new();
    let mut things = Vec::new();
    for entity in world.occupants_of(place).filter(|&entity| entity != viewer) {
        if let Some(name) = roster.name_of(entity) {
            players.push(name.as_str().to_owned());
        } else if let Some(name) = display_name(world, entity) {
            things.push(name);
        }
    }
    players.sort();
    (players, things)
}

/// One Diku-voice sentence for the players present, or `None` for an empty
/// list: singular `look.player-here`, plural `look.players-here` over an
/// English and-join (locale-aware list formatting is the M2-I rework's job).
fn players_line(players: &[String], locale: &Locale) -> Option<String> {
    match players {
        [] => None,
        [name] => Some(t!(locale, "look.player-here", name = name.clone())),
        many => Some(t!(locale, "look.players-here", names = and_join(many))),
    }
}

/// Joins names as `a`, `a and b`, or `a, b and c`.
fn and_join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => format!("{} and {}", head.join(", "), last),
    }
}
```

Update the imports at the top of `look.rs` to add `use crate::roster::Roster;` (keep the rest as-is; `display_name` stays imported from `super`).

Update the two callers:

`crates/mud-engine/src/builtins/look.rs` (the `Look` handler):

```rust
            Some(place) => CommandReply::to_caller(render_room(
                place,
                ctx.world(),
                ctx.roster(),
                ctx.caller(),
                &locale,
            )),
```

`crates/mud-engine/src/builtins/movement.rs:40`:

```rust
        let arrival = render_room(place, ctx.world(), ctx.roster(), ctx.caller(), &locale);
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mud-engine`
Expected: PASS — the 4 new look tests plus all pre-existing tests (the `builtins.rs` look tests still pass because their `FakeResolver::name_of` only names the viewer, who is excluded).

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(engine): look lists connected players in a Diku-voice sentence" crates/mud-engine crates/mud-i18n
```

---

### Task 4: login routing reports the bound puppet

**Files:**
- Modify: `crates/mud-engine/src/session/mod.rs`
- Test: `crates/mud-engine/src/session/mod.rs` (existing `mod tests`)

**Interfaces:**
- Consumes: `Terminal::Bound { account, puppet, name }`, `LoginBackend::resolve_puppet`.
- Produces (Task 5 relies on these):
  - `Routing::Login { outputs: Vec<LoginOutput>, close: bool, bound: Option<EntityId> }`
  - `pub fn SessionService::binding_of(&self, session: SessionId) -> Option<&InWorldBinding>`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/mud-engine/src/session/mod.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-engine --lib session`
Expected: FAIL to compile — no field `bound` on `Routing::Login`, no method `binding_of`.

- [ ] **Step 3: Implement**

In `crates/mud-engine/src/session/mod.rs`:

Extend `Routing::Login`:

```rust
    /// Handled by the pre-login FSM; here is the output and whether to close.
    Login {
        outputs: Vec<LoginOutput>,
        close: bool,
        /// The puppet entity this line just bound in-world, when it completed
        /// a login — the driver's cue to announce the spawn (§2.7 step 8).
        bound: Option<EntityId>,
    },
```

Add a private outcome type above `apply_terminal`:

```rust
/// What applying a terminal produced: whether to close the connection, and
/// the puppet entity bound when the terminal moved the session in-world.
struct TerminalOutcome {
    close: bool,
    bound: Option<EntityId>,
}
```

Change `apply_terminal` to return it (`-> TerminalOutcome`), with these arm results:
- `Terminal::Bound` resolve success: `TerminalOutcome { close: false, bound: Some(entity) }` (keep the existing `sessions.insert`).
- `Terminal::Bound` resolve failure: `TerminalOutcome { close: true, bound: None }` (keep the existing `sessions.remove`).
- `Terminal::Closed`: `TerminalOutcome { close: true, bound: None }`.

Update `drive`'s three `Routing::Login` returns:

```rust
            if let Some(terminal) = transition.terminal {
                let outcome = self.apply_terminal(session, terminal, backend).await;
                return Routing::Login {
                    outputs,
                    close: outcome.close,
                    bound: outcome.bound,
                };
            }
```

The other two returns (`effect exhausted` and `session vanished`) gain `bound: None`.

Add the accessor (near `disconnect`):

```rust
    /// The in-world binding of `session`, or `None` for a pre-login or
    /// unknown session.
    pub fn binding_of(&self, session: SessionId) -> Option<&InWorldBinding> {
        match self.sessions.get(&session)? {
            SessionState::InWorld(binding) => Some(binding),
            SessionState::Login(_) => None,
        }
    }
```

- [ ] **Step 4: Keep `mudd` compiling**

`crates/mudd/src/world_loop.rs:131` destructures `Routing::Login { outputs, close }` and now misses the new field. Change it to:

```rust
        Routing::Login {
            outputs,
            close,
            bound: _,
        } => {
```

(Task 5 replaces `bound: _` with the real spawn announcement.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS. (Existing `Routing::Login { close: false, .. }` matches — including `tests/session_login.rs` — absorb the new field via `..`.)

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(engine): login routing reports the bound puppet entity" crates/mud-engine/src/session crates/mudd/src/world_loop.rs
```

---

### Task 5: `world_loop` announces spawn/quit/drop + e2e test

**Files:**
- Modify: `crates/mudd/src/world_loop.rs`
- Modify: `crates/mudd/src/boot.rs` (TenantRuntime construction)
- Create: `crates/mudd/tests/common/mod.rs` (harness extracted from `telnet_login.rs`)
- Modify: `crates/mudd/tests/telnet_login.rs` (use the extracted harness)
- Create: `crates/mudd/tests/presence.rs`

**Interfaces:**
- Consumes: `presence::{announce, entered, left}` (Task 2), `Routing::Login { bound }` and `SessionService::binding_of` (Task 4), `PersistentWorld`'s `guard.world() -> &World`, `World::location_of(entity) -> Option<PlaceId>`.
- Produces: player-visible behavior only (the spec's e2e contract); no new APIs.

- [ ] **Step 1: Extract the shared telnet test harness**

Create `crates/mudd/tests/common/mod.rs` by moving — verbatim, unmodified — from `crates/mudd/tests/telnet_login.rs`: the `TICK` const, `write_tenant`, `ClientReader` (struct + impl), and `single_tenant_config`. Mark each moved item `pub` and keep their doc comments. Add the needed `use` lines (copy from `telnet_login.rs`).

In `crates/mudd/tests/telnet_login.rs`: delete the moved items, add

```rust
mod common;

use common::{ClientReader, single_tenant_config, write_tenant};
```

and remove now-unused imports. Run: `cargo test -p mudd --test telnet_login` — Expected: PASS (pure extraction; `TICK` stays internal to `common`).

- [ ] **Step 2: Write the failing e2e test**

Create `crates/mudd/tests/presence.rs`:

```rust
//! Room-presence e2e (design 2026-07-15): spawn and quit/drop are announced
//! to co-located players, and `look` lists connected players only.
#![allow(clippy::expect_used, clippy::panic)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-{expect,panic}-in-tests do not cover their helpers; both are permitted in tests per policy

mod common;

use common::{ClientReader, single_tenant_config, write_tenant};
use tempfile::TempDir;
use tokio::net::TcpStream;

/// Registers `username`, creates puppet `puppet`, and enters the world.
async fn register_and_enter(client: &mut ClientReader, username: &str, puppet: &str) {
    client.read_until(b"Welcome to Testville.").await;
    client.write_line(&format!("register {username}")).await;
    client.read_until(b"Password:").await;
    client.write_line("hunter2!").await;
    client.read_until(b"Confirm password:").await;
    client.write_line("hunter2!").await;
    client.read_until(b"You have no characters yet.").await;
    client.write_line(&format!("new {puppet}")).await;
    client
        .read_until(b"Welcome. You are now in the world.")
        .await;
}

/// Logs an existing account back in and plays `puppet`.
async fn login_and_play(client: &mut ClientReader, username: &str, puppet: &str) {
    client.read_until(b"Welcome to Testville.").await;
    client.write_line(&format!("login {username}")).await;
    client.read_until(b"Password:").await;
    client.write_line("hunter2!").await;
    client.read_until(puppet.as_bytes()).await;
    client.write_line(&format!("play {puppet}")).await;
    client
        .read_until(b"Welcome. You are now in the world.")
        .await;
}

#[tokio::test]
async fn presence_is_announced_and_listed() {
    let tenant_dir = TempDir::new().expect("temp dir");
    write_tenant(tenant_dir.path());
    let (addrs, _tasks) = mudd::boot(single_tenant_config(tenant_dir.path()))
        .await
        .expect("boot must succeed");
    let addr = *addrs.first().expect("one bound address");

    let stream = TcpStream::connect(addr).await.expect("alice connects");
    let mut alice = ClientReader::new(stream);
    register_and_enter(&mut alice, "alice", "Hero").await;

    // Spawn: Bob's arrival is announced to Alice.
    let stream = TcpStream::connect(addr).await.expect("bob connects");
    let mut bob = ClientReader::new(stream);
    register_and_enter(&mut bob, "bob", "Sidekick").await;
    alice.read_until(b"Sidekick appears from nowhere.").await;

    // look lists the connected player.
    alice.write_line("look").await;
    alice.read_until(b"Sidekick is here.").await;

    // A clean quit is announced.
    bob.write_line("quit").await;
    alice.read_until(b"Sidekick disappears.").await;

    // After the leave, look no longer lists the body: bound the look reply
    // with a say echo and assert the absence inside the captured bytes.
    alice.write_line("look").await;
    alice.write_line("say done").await;
    let look_reply = alice.read_until(b"You say").await;
    let needle = b"is here";
    assert!(
        !look_reply.windows(needle.len()).any(|w| w == needle),
        "a disconnected puppet must not be listed, got {look_reply:?}"
    );

    // A dropped socket is announced identically to a quit.
    let stream = TcpStream::connect(addr).await.expect("bob reconnects");
    let mut bob = ClientReader::new(stream);
    login_and_play(&mut bob, "bob", "Sidekick").await;
    alice.read_until(b"Sidekick appears from nowhere.").await;
    drop(bob);
    alice.read_until(b"Sidekick disappears.").await;
}
```

Run: `cargo test -p mudd --test presence` — Expected: FAIL — the test times out at `alice.read_until(b"Sidekick appears from nowhere.")` with `client read must not time out` (no announcement is wired yet).

- [ ] **Step 3: Wire the three lifecycle sites**

In `crates/mudd/src/boot.rs`: add `locale: tenant_config.locale(),` to the `TenantRuntime` construction (next to the existing `sessions`/`pipeline` fields).

In `crates/mudd/src/world_loop.rs`:

Add to imports: `use mud_core::{StyledText, World};` `use mud_i18n::Locale;` `use mud_schema::SessionOutput;` (merge with existing `use` lines; keep what is already imported).

Add the field to `TenantRuntime`:

```rust
pub struct TenantRuntime {
    pub world: Arc<Mutex<PersistentWorld>>,
    pub backend: DbBackend,
    pub sessions: SessionService,
    pub pipeline: Pipeline,
    pub builtins: Vec<Command>,
    pub places: WorldPlaces,
    /// The tenant locale presence announcements render in (§3.14.6).
    pub locale: Locale,
}
```

Add the helper (below `handle_input`):

```rust
/// The outputs announcing `session_id`'s puppet entering or leaving its room.
/// Empty when the session has no in-world binding (pre-login) or the puppet
/// has no location — both mean there is no room audience to tell.
fn presence_announcement(
    rt: &TenantRuntime,
    world: &World,
    session_id: SessionId,
    message: fn(Locale, &str) -> StyledText,
) -> Vec<SessionOutput> {
    let Some(binding) = rt.sessions.binding_of(session_id) else {
        return Vec::new();
    };
    let Some(place) = world.location_of(binding.puppet) else {
        return Vec::new();
    };
    let text = message(rt.locale.clone(), binding.name.as_str());
    mud_engine::presence::announce(
        world,
        &rt.sessions.resolver(&rt.builtins),
        place,
        binding.puppet,
        &text,
    )
}
```

(`SessionId` is already in scope via the `mud_schema` imports used by `handle_input`; add it if not.)

**Site 1 — spawn.** In `handle_input`, replace the `Routing::Login` arm (dropping Task 4's `bound: _`):

```rust
        Routing::Login {
            outputs,
            close,
            bound,
        } => {
            for output in outputs {
                endpoint
                    .send(frame_of(output))
                    .await
                    .context("send output")?;
            }
            if bound.is_some() {
                let guard = rt.world.lock().await;
                let announcements = presence_announcement(
                    rt,
                    guard.world(),
                    session_id,
                    mud_engine::presence::entered,
                );
                drop(guard);
                for output in announcements {
                    endpoint
                        .send(WorldFrame::Output(output))
                        .await
                        .context("send output")?;
                }
            }
            if close {
                endpoint
                    .send(WorldFrame::Close(SessionClose { session_id }))
                    .await
                    .context("send close")?;
                rt.sessions.disconnect(session_id);
            }
        }
```

**Site 2 — quit.** In the `Routing::InWorld` arm's `Ok(outcome)` branch, compute the farewell while the world guard is still held (after the effects are submitted, before `drop(guard)`), and send it after the caller's outputs:

```rust
                Ok(outcome) => {
                    for effect in outcome.effects {
                        guard.submit(MutationCommand::new(effect));
                    }
                    // Resolved before the unbind below so the roster still
                    // maps the audience; the quitter is excluded by entity.
                    let farewells = if matches!(outcome.disposition, SessionDisposition::Close) {
                        presence_announcement(
                            rt,
                            guard.world(),
                            session_id,
                            mud_engine::presence::left,
                        )
                    } else {
                        Vec::new()
                    };
                    drop(guard);
                    for output in outcome.outputs {
                        endpoint
                            .send(WorldFrame::Output(output))
                            .await
                            .context("send output")?;
                    }
                    for output in farewells {
                        endpoint
                            .send(WorldFrame::Output(output))
                            .await
                            .context("send output")?;
                    }
                    if matches!(outcome.disposition, SessionDisposition::Close) {
                        endpoint
                            .send(WorldFrame::Close(SessionClose { session_id }))
                            .await
                            .context("send close")?;
                        rt.sessions.disconnect(session_id);
                    }
                }
```

**Site 3 — socket drop.** In `run`, replace the Disconnect arm with a call to a new handler:

```rust
                Some(GatewayFrame::Disconnect(disconnect)) => {
                    handle_disconnect(&mut endpoint, &mut rt, disconnect.session_id).await?;
                }
```

and add (next to `handle_input`):

```rust
/// A gateway-reported socket drop: announce the departure to the puppet's
/// room, then unbind. Order matters — the roster must still hold the binding
/// while the audience is resolved.
async fn handle_disconnect(
    endpoint: &mut InMemoryEndpoint<WorldFrame, GatewayFrame>,
    rt: &mut TenantRuntime,
    session_id: SessionId,
) -> anyhow::Result<()> {
    let guard = rt.world.lock().await;
    let farewells =
        presence_announcement(rt, guard.world(), session_id, mud_engine::presence::left);
    drop(guard);
    for output in farewells {
        endpoint
            .send(WorldFrame::Output(output))
            .await
            .context("send output")?;
    }
    rt.sessions.disconnect(session_id);
    Ok(())
}
```

- [ ] **Step 4: Run the e2e to verify it passes**

Run: `cargo test -p mudd`
Expected: PASS — `presence_is_announced_and_listed` plus the pre-existing `telnet_login` tests.

- [ ] **Step 5: Full workspace gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mudd): announce spawn and quit/drop to the room" crates/mudd
```

---

### Task 6: docs, PLAN, journal

**Files:**
- Modify: `docs/docs/playing/commands.md`
- Modify: `PLAN.md` (append the completed PR entry after M1-26)
- Modify: `.claude/JOURNAL.md` (append entry)

- [ ] **Step 1: Update the player docs**

In `docs/docs/playing/commands.md`:

In the "Looking around" table, change the `look` row description to:

```markdown
| `look` | `l` | Show the current room: title, description, obvious exits, the players here, and anything else here. |
```

After the "Looking around" section, add:

```markdown
## Seeing other players

Players in your room appear in `look` on their own line — `Alice is here.`,
or `Alice, Bob and Carol are here.` when several are present. Objects are
listed separately under "Also here:".

When a player logs in you'll see `Alice appears from nowhere.`; when they
quit or lose their connection you'll see `Alice disappears.`. Walking
between rooms is announced too (`Alice leaves north.` / `Alice arrives from
the south.`).
```

Run from `docs/`: `uv run mkdocs build --strict`
Expected: build succeeds.

- [ ] **Step 2: Record the PR in PLAN.md**

Append after the M1-26 entry, following the existing entry style:

```markdown
- **M1-27 — Room presence: spawn/leave announcements, players in `look`.**
  `Roster::name_of`; `presence` module owning the single audience fan-out
  (pipeline refactored onto it); spawn (`Routing::Login.bound`), quit-Close
  and gateway-Disconnect announce `presence.enter`/`presence.leave` from
  `world_loop`; `look` renders connected players as a Diku-voice sentence
  (`look.player-here`/`look.players-here`) separate from the keyword
  "also here" list. Disconnect leaves the body in place but hidden
  (presence is session-based; linkdead proper is M7).
  - *Spec:* §2.7 step 8, §3.6.3; design
    `docs/superpowers/specs/2026-07-15-room-presence-design.md`. *Verify:*
    two-session e2e (`crates/mudd/tests/presence.rs`); look-partition and
    `presence::announce` unit tests.
```

- [ ] **Step 3: Append the journal entry**

Append to `.claude/JOURNAL.md` (newest at the bottom), following the house format:

```markdown
## 2026-07-15 — M1-27 room presence (spawn/leave announcements, players in look)

- **Spec:** §2.7 step 8, §3.6.3; docs/superpowers/specs/2026-07-15-room-presence-design.md
- **Done:** `Roster::name_of`; `mud-engine::presence` (shared `announce` fan-out
  + `entered`/`left` messages; pipeline refactored onto it); `Routing::Login`
  gained `bound`; `SessionService::binding_of`; `world_loop` announces
  spawn/quit/drop (`TenantRuntime.locale`); `look` partitions players
  (sorted, and-joined sentence) from keyword things; i18n keys
  `presence.enter`/`presence.leave`/`look.player-here`/`look.players-here`;
  mudd test harness extracted to `tests/common`; playing docs updated.
- **Verify:** `cargo test --workspace`, `cargo clippy --workspace
  --all-targets`, `cargo fmt --all --check`, `uv run mkdocs build --strict`;
  e2e `crates/mudd/tests/presence.rs`.
- **Next:** M2-F archetypes must gate room listings/object resolution on
  session presence too, or disconnected bodies resurface (design "hidden
  body" note). English and-join is absorbed by the M2-I i18n rework.
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "docs: room presence — player docs, PLAN M1-27, journal" docs/docs/playing/commands.md PLAN.md .claude/JOURNAL.md
```
