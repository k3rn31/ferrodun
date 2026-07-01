# M1-19 — Session FSM (login states): Design

**Date:** 2026-07-01
**Spec:** §3.19.1, §3.19.3, §2.7 step 1/step 3, §3.15.1
**Status:** Approved for planning

## Goal

Give a brand-new connection a path from raw socket to in-world play:
pre-login banner → `login`/`register` → puppet selection → in-world. Wire
the M1-18 account domain (`mud-account` + `mud-db` `Accounts`) into a real
`SessionResolver`, replacing the pipeline's `FakeResolver`. Linkdead, ping,
and idle handling stay minimal — M1 needs only clean connect and quit.

## Context & the plan divergence

Two facts about the current codebase shape this design:

1. **All player input reaches World as `SessionInput { session_id, line }`.**
   The Gateway decodes telnet/IAC and forwards raw lines; there is no
   login-specific IPC frame vocabulary. Register/login/puppet-select lines
   arrive at World exactly like commands do.
2. **Accounts live World-side.** The `mud-account` domain and the `mud-db`
   `Accounts` repository, plus the session→account→puppet map that
   `SessionResolver` reads, are all in World. Authentication is async
   (argon2 in `spawn_blocking` + a DB query).

`PLAN.md` §M1-19 places the FSM "In `mud-net`", but `mud-net` is the
transport/rendering edge (gateway-side), while everything the login FSM needs
is World-side. Per the project rule ("if reality diverges from the plan,
update `PLAN.md` rather than silently deviating"), this PR **updates
`PLAN.md` §M1-19** to place the FSM in a new pure `mud-session` crate driven
by World, and records the rationale.

## Architecture

### Crate layout

- **`mud-session` (new, pure, sans-IO).** A login state machine with no I/O,
  no async, no DB, and no `mud-core::World`. Depends only on `mud-account`
  (domain types and error enums) and `secrecy` (in-memory password handling).
  Emits typed messages and effects; the caller performs the I/O. This is the
  unit tested via pure transition tests.
- **`mud-engine` (driver).** Owns the per-session state, renders the FSM's
  typed messages through `mud-i18n`, and supplies the real `SessionResolver`.
  It does **not** depend on `mud-db`: the async I/O the effects need
  (`authenticate`/`register`/`create_puppet`/`puppets_of` + puppet-key
  resolution) is reached through an injected **`LoginBackend` port trait**
  (dependency inversion, mirroring the pipeline's existing `Places` /
  `SessionResolver` seams). The concrete `mud-db`-backed `LoginBackend`
  implementation lives with the caller that owns the DB — the M1-19 integration
  test now, and the `mudd` binary at M1-22.

Single PR: the pure crate and its World wiring land together. Shipping the
crate alone would violate the "don't build before you need it" rule, and the
resolver replacement only makes sense once the FSM exists.

### The FSM (`mud-session`)

States:

```
Anon ──login <name>──────────► LoginPassword{user}
 │   ──register <name>───────► RegisterPassword{user}   (invalid name → msg, stay Anon)
 │   ──who / help / ? ───────► (emits message) stay Anon
 │   ──quit ─────────────────► Closing
 │
LoginPassword{user}   ──<pwd>──► AwaitingAuth        [effect: Authenticate]
RegisterPassword{user}──<pwd>──► RegisterConfirm{user,pwd}
RegisterConfirm       ──<pwd2>─► AwaitingRegister    [effect: Register]
                                 (mismatch → msg, back to Anon)
AwaitingAuth     ──ok(account,puppets)──► PuppetSelect
                 ──fail ────────────────► Anon
AwaitingRegister ──ok(account) ────────► PuppetSelect (empty)
                 ──taken ──────────────► Anon
PuppetSelect{account,puppets} ──play <name|N>──► AwaitingEnter  [effect: Enter]
                              ──new <name> ─────► AwaitingCreate [effect: CreatePuppet]
                              ──quit ───────────► Closing
AwaitingCreate ──ok(puppet)──► AwaitingEnter        [effect: Enter]
AwaitingEnter  ──ok ─────────► Bound{account,puppet} (terminal)
```

- A brand-new account reaches `PuppetSelect` with an empty puppet list and is
  prompted to create its first puppet (`new <name>`).
- Input arriving in an `Awaiting*` state is **dropped** (M1 minimal — no queue).
- `Bound` is terminal: the driver moves the session to `InWorld` and hands
  subsequent lines to the command pipeline.

**Output is a typed `SessionMessage` enum, not strings.** Variants such as
`Banner`, `PasswordPrompt`, `ConfirmPrompt`, `LoginFailed`,
`AccountRejected(policy)`, `PuppetList(Vec<PuppetName>)`, `NameInvalid`,
`PasswordMismatch`, `UsernameTaken`, `PreLoginHelp`, `WhoStub` are rendered by
the World-side driver through `mud-i18n` (`t!`) at the tenant's default locale
(no account locale exists pre-login). The tenant-authored KDL banner (§3.19.1)
is passed into the driver, not baked into the FSM. Transition tests assert on
message *variants*, keeping them locale-independent.

FSM API (three methods):

```rust
fn on_connect(&self) -> Transition;
fn on_input(&mut self, line: &str) -> Transition;
fn on_effect(&mut self, result: EffectResult) -> Transition;

struct Transition {
    messages: Vec<SessionMessage>,
    effect: Option<Effect>,
    terminal: Option<Bound>, // Some ⇒ session is now in-world
}
```

Password entry is a **separate prompted line** (`PasswordPrompt`); echo
suppression / masking is a transport concern deferred to M1-20. Because the
pure FSM cannot hash, `Authenticate`/`Register` are effects, not in-FSM logic.

**Passwords never live in a plain `String`.** The moment the FSM captures a
password line it wraps it in `secrecy::SecretString`, so the states that hold a
password mid-flow (`LoginPassword`, `RegisterPassword`, `RegisterConfirm`) and
the effects that carry it (`Authenticate`, `Register`) store `SecretString`,
not `String`. `SecretString` zeroizes its buffer on drop and redacts itself in
`Debug`, so a password cannot leak through a `Transition` dump, a traced
effect, or a dropped-state remnant. The confirm-password check compares the two
captured secrets via `expose_secret()` (both are attacker-supplied, so
constant-time comparison is unnecessary). The raw bytes are exposed **only** at
the argon2 boundary — see the driver loop below.

### Effects & the driver loop (`mud-engine`)

The FSM emits `Effect`s the World-side driver executes via the injected
`LoginBackend` port (`password` fields are `SecretString`):

- `Authenticate { username, password }` → `backend.authenticate`. On success the
  driver also calls `backend.puppets_of` and feeds `(account, puppets)` back.
- `Register { username, password }` → `backend.register` (which hashes).
- `CreatePuppet { account, name }` → `backend.create_puppet`.
- `Enter { account, puppet }` → `backend.resolve_puppet(key)` → live `EntityId`,
  then bind the session.

The concrete `LoginBackend` maps these onto `Accounts` +
`PersistentWorld::entity_id`.

The password's raw bytes are exposed via `expose_secret()` **only inside the
`spawn_blocking` closure**, immediately before the argon2 `Credential::hash` /
`verify` call, and never held beyond it. `Credential::hash`/`verify` keep their
existing `&str` API (M1-18, unchanged); the momentary `&str` exposure is
confined to that single call on the blocking thread.

`PersistentWorld::load` already hydrates **all** persisted puppets into the
arena at boot (consistent with the linkdead model: a puppet is in-world whether
or not anyone is connected). `Enter` therefore only **binds** a session to an
already-live puppet entity — no runtime hydration.

Driver loop per input line: run the FSM, render messages → `SessionOutput`,
execute any effect asynchronously, feed the result back via `on_effect`, and
repeat until the transition has no effect. On `terminal: Bound`, move the
session to `InWorld` and emit an initial room `look`.

### Session registry & `SessionResolver` wiring

The World holds a registry:

```rust
enum SessionState {
    Login(SessionFsm),
    InWorld { account: AccountId, puppet: EntityId },
}
// SessionRegistry: HashMap<SessionId, SessionState>
```

- `GatewayFrame::Connect` → insert `Login(SessionFsm::new())`; emit the banner
  via `on_connect`.
- `GatewayFrame::Input` → **branch**: `Login` routes the line through the FSM
  driver; `InWorld` routes to the existing command `Pipeline`.
- `GatewayFrame::Disconnect` → remove the session (M1 minimal: no linkdead
  grace; the puppet entity stays hydrated in the arena but unbound).

The real `SessionResolver::resolve` reads `InWorld` entries: `puppet` →
`CallerContext` (location via `World::location_of`, tenant default locale,
`LockContext` from the puppet's perms — none in M1) plus `LayerCommands`. The
`FakeResolver` is removed from production paths and kept only in pipeline unit
tests.

## Error handling & non-leakiness

- Reuses M1-18 domain errors. `LoginError::UnknownUser` and a bad password both
  render the **same** `LoginFailed` message (no user-existence disclosure);
  `Suspended`/`Banned` render `Account::login_rejection()`'s policy message.
- `mud-session` defines its error type with `thiserror`; no third-party error
  leaks across the public API. No `unwrap`/`expect` outside tests.
- `SessionMessage` and `Effect` are `#[non_exhaustive]` at the crate boundary.
- Password confirm-mismatch and invalid-name (`Username::parse` /
  `PuppetName::parse`) failures are in-FSM messages, never effects.
- In-memory passwords use `secrecy::SecretString` (added with `cargo add
  secrecy`): zeroized on drop, redacted in `Debug`, exposed only at the argon2
  call site (see Architecture). A regression test asserts a captured
  password-state's `Debug` output does not contain the plaintext.

## Scope boundaries (deferred)

Explicitly out of scope for M1-19:

- Linkdead reattach & grace timeout (§3.15.2, M7).
- `Core.Ping`/`Core.Pong` liveness and the idle flag (§3.15.3).
- Echo suppression / password masking (M1-20 telnet).
- Real `who` listing, `say`/broadcast fan-out, and clean `quit` fan-out
  (M1-19a). Pre-login `who` renders a stub; `quit` emits `WorldFrame::Close`
  and drops the session.
- Invite-only registration and password recovery (M7). M1 is open-registration
  only.
- `on_first_login` / `on_disconnect` hooks (need scripting).

## Testing

- **`mud-session` unit tests** — pure transition tests: each state × input →
  asserted `(new state, messages, effect)`; effect-result feedback for auth
  success/failure, register/taken, puppet select/create, password mismatch,
  invalid names, `help`/`who`/`quit`. Plus a redaction test: the `Debug` of a
  password-holding state and of an `Authenticate`/`Register` effect must not
  contain the plaintext.
- **`mud-engine` integration test** — drive the registry end-to-end with an
  in-memory `mud-db`: register → create puppet → enter → run a command through
  the real resolver; login → wrong password → retry → succeed; `quit` closes
  the session; an unknown session still yields `UnknownSession`.

## Documentation

Player-observable surface (the pre-login banner, `login`/`register`/`who`/
`quit`/`help`, puppet selection) gets a short onboarding page under
`docs/docs/` with the `nav` entry, in the same PR.
