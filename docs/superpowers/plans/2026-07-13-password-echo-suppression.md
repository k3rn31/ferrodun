# Password Echo Suppression (M1-25) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mask password entry over telnet by having the server claim the ECHO option (RFC 857: `IAC WILL ECHO` before the password prompt, `IAC WONT ECHO` once the secret line is consumed), driven end-to-end by a typed signal originating in the session FSM.

**Architecture:** The FSM (`mud-session`) derives an echo change from password-state membership on every step and exposes it on `Transition.echo`. The engine driver (`mud-engine`) interleaves it, in order, into the pre-login output stream as a new `LoginOutput` enum; `mudd` maps it onto a new `WorldFrame::Echo` IPC frame (`mud-schema`, SCHEMA_VERSION 2); the gateway (`mud-gateway`) routes it to the connection task, which asks the telnet machine (`mud-net`) to emit the RFC 857 negotiation bytes.

**Tech Stack:** Rust workspace; tokio; postcard IPC frames; telnet RFC 857/1143.

**Design doc:** `docs/superpowers/specs/2026-07-11-password-echo-suppression-design.md`

## Global Constraints

- **VCS is jj (Jujutsu), not git.** Commit with `jj commit -m "<message>"` (equivalent to describe + new). Never use `git add`/`git commit`.
- `unwrap()` is strictly forbidden. `expect()` only in tests, with a descriptive message. No `panic!`/`todo!`/`unreachable!` in production code unless guarded by a documented `// INVARIANT:` comment.
- Match all enum variants explicitly; avoid `_ =>` catch-alls in new matches (existing `#[non_exhaustive]` catch-alls stay).
- Workspace must stay green after every task: `cargo test --workspace` and `cargo clippy --workspace --all-targets` (clippy denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`).
- Pure/domain crates (`mud-session`, `mud-schema` core) take no `tracing` dependency; do not add logging anywhere in this feature.
- Never log passwords, usernames, or raw player input.
- Comments explain *why*, not *how*; doc comments on all public items.
- Telnet byte values used throughout: `IAC`=255, `WILL`=251, `WONT`=252, `DO`=253, `DONT`=254, option `ECHO`=1.

---

### Task 1: `mud-session` — echo signal on `Transition`

The FSM is the only component that knows when secret entry starts and ends. Add a typed `InputEcho` signal to `Transition`, **derived** from password-state membership around each step (never hand-set per transition), so a future password-collecting state gets masking automatically.

**Files:**
- Modify: `crates/mud-session/src/fsm.rs`
- Modify: `crates/mud-session/src/message.rs` (stale doc comment)
- Modify: `crates/mud-session/src/lib.rs` (export)
- Test: `crates/mud-session/src/fsm.rs` (`mod tests` at bottom)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub enum InputEcho { Enabled, Suppressed }` (Copy, Eq) and `pub echo: Option<InputEcho>` on `Transition`, both re-exported from `mud_session`. Task 4 consumes them.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` at the bottom of `crates/mud-session/src/fsm.rs` (the module already has `use super::*;` and the helpers shown in nearby tests):

```rust
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
        let t = fsm.on_input("hunter2");
        assert_eq!(t.echo, None, "confirm prompt is still secret entry");
        let t = fsm.on_input("hunter2");
        assert_eq!(t.echo, Some(InputEcho::Enabled));
    }

    #[test]
    fn a_mismatched_confirmation_re_enables_echo() {
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("register alice");
        let _ = fsm.on_input("hunter2");
        // A mismatch aborts to Anon, which is not secret entry.
        let t = fsm.on_input("typo");
        assert_eq!(t.echo, Some(InputEcho::Enabled));
    }

    #[test]
    fn non_password_steps_carry_no_echo_change() {
        let mut fsm = SessionFsm::new();
        assert_eq!(fsm.on_connect().echo, None);
        assert_eq!(fsm.on_input("help").echo, None);
        assert_eq!(fsm.on_input("who").echo, None);
    }

    #[test]
    fn auth_results_carry_no_echo_change() {
        // Echo was already re-enabled when the password line was consumed;
        // the auth outcome (success or failure) must not signal again.
        let mut fsm = SessionFsm::new();
        let _ = fsm.on_input("login alice");
        let _ = fsm.on_input("wrong");
        let t = fsm.on_effect(EffectResult::LoginRejected(LoginError::BadPassword));
        assert_eq!(t.echo, None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-session`
Expected: compile error — `InputEcho` not found / no field `echo` on `Transition`.

- [ ] **Step 3: Implement**

In `crates/mud-session/src/fsm.rs`:

3a. Add the enum right above `Transition` (after the imports):

```rust
/// Whether the client should locally echo the next input. Derived from
/// password-state membership on every FSM step; the driver relays it to the
/// transport (design 2026-07-11), which maps it onto telnet RFC 857.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEcho {
    /// Normal input: the client echoes what the player types.
    Enabled,
    /// Secret entry: the client must not echo (password masking).
    Suppressed,
}
```

3b. Add the field to `Transition` and its constructors:

```rust
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
```

Add `echo: None,` to the `Transition::messages` and `Transition::closing` constructor bodies, and to **every** `Transition { ... }` struct literal in the file (the compiler lists them: `capture_login_password`, `confirm_register_password`, and any literal inside `on_effect`/puppet handling). The value is always `None` — the wrappers in 3d overwrite it.

3c. Add the membership predicate and the derivation helper. Place the `impl State` block directly under the `State` enum, and `echo_change` as a free function next to `split_command`:

```rust
impl State {
    /// True while the next input line is a secret (a password or its
    /// confirmation); the client's local echo must be off.
    fn collects_secret(&self) -> bool {
        matches!(
            self,
            State::LoginPassword { .. }
                | State::RegisterPassword { .. }
                | State::RegisterConfirm { .. }
        )
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
```

3d. Wrap the two step functions. Rename the existing body of `on_input` to a private `dispatch_input` (identical match, unchanged) and the existing body of `on_effect` to a private `dispatch_effect`, then:

```rust
    /// Feeds one input line to the machine.
    pub fn on_input(&mut self, line: &str) -> Transition {
        let was_secret = self.state.collects_secret();
        let mut transition = self.dispatch_input(line);
        transition.echo = echo_change(was_secret, self.state.collects_secret());
        transition
    }
```

```rust
    /// Feeds an [`EffectResult`] back into the machine.
    pub fn on_effect(&mut self, result: EffectResult) -> Transition {
        let was_secret = self.state.collects_secret();
        let mut transition = self.dispatch_effect(result);
        transition.echo = echo_change(was_secret, self.state.collects_secret());
        transition
    }
```

Keep the original doc comments on the public functions; `dispatch_input`/`dispatch_effect` need none beyond what exists.

3e. In `crates/mud-session/src/message.rs`, fix the stale doc comment on `PasswordPrompt`:

```rust
    /// Prompt for a password on its own line. Echo suppression rides on
    /// [`Transition::echo`](crate::Transition), not on this message.
    PasswordPrompt,
```

3f. In `crates/mud-session/src/lib.rs`, extend the re-export:

```rust
pub use fsm::{InputEcho, SessionFsm, Terminal, Transition};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-session`
Expected: PASS (all pre-existing tests too — they don't mention `echo`).

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p mud-session --all-targets`
Expected: clean. (Downstream crates are untouched so far; the workspace still builds because `Transition` construction outside this crate doesn't exist.)

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mud-session): derive InputEcho signal on Transition (M1-25)"
```

---

### Task 2: `mud-schema` — `WorldFrame::Echo` and SCHEMA_VERSION 2

**Files:**
- Modify: `crates/mud-schema/src/frame.rs`
- Modify: `crates/mud-schema/src/session.rs` (version bump)
- Modify: `crates/mud-schema/src/lib.rs` (exports)
- Test: `crates/mud-schema/src/frame.rs` (`mod tests`), `crates/mud-schema/src/session.rs` (version test)

**Interfaces:**
- Consumes: `SessionId` (existing).
- Produces: `pub enum EchoMode { Enabled, Suppressed }`, `pub struct SessionEcho { pub session_id: SessionId, pub mode: EchoMode }`, `WorldFrame::Echo(SessionEcho)` — consumed by Tasks 4 and 5. `SCHEMA_VERSION` becomes `SchemaVersion(2)`.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `crates/mud-schema/src/frame.rs` (the module already imports `codec::{decode, encode}` and `NonZeroU64`; mirror the `SessionId` construction used by the neighboring round-trip tests):

```rust
    #[test]
    fn echo_frame_round_trips() {
        let frame = WorldFrame::Echo(SessionEcho {
            session_id: SessionId::new(NonZeroU64::new(7).expect("nonzero id")),
            mode: EchoMode::Suppressed,
        });
        let bytes = encode(&frame).expect("echo frame must encode");
        let decoded: WorldFrame = decode(&bytes).expect("echo frame must decode");
        assert_eq!(decoded, frame);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-schema`
Expected: compile error — `SessionEcho`/`EchoMode` not found.

- [ ] **Step 3: Implement**

3a. In `crates/mud-schema/src/frame.rs`, add below `SessionClose`:

```rust
/// Whether a session's client should locally echo input (RFC 857 masking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum EchoMode {
    /// Normal input: the client echoes what the player types.
    Enabled,
    /// Secret entry (a password): the client must not echo.
    Suppressed,
}

/// Instructs the Gateway to change a session's local-echo mode
/// (World → Gateway; emitted around password entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionEcho {
    /// The session whose echo mode changes.
    pub session_id: SessionId,
    /// The new echo mode.
    pub mode: EchoMode,
}
```

3b. Add the variant to `WorldFrame` **as the last variant** (after `ResumeAck`) so existing variants keep their postcard indices:

```rust
    /// A change to a session's local-echo mode (password masking, §2.8.2).
    Echo(SessionEcho),
```

3c. In `crates/mud-schema/src/session.rs`, bump the constant:

```rust
/// The IPC schema version this build speaks (§2.1.3.1).
pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);
```

Then run `grep -n "get(), 1" crates/mud-schema/src/session.rs` and update every assertion that pins the old version — the unit test currently named `schema_version_is_one` becomes:

```rust
    #[test]
    fn schema_version_is_two() {
        assert_eq!(SCHEMA_VERSION.get(), 2);
    }
```

(Leave `SchemaVersion::new(42)`-style assertions alone; only the `SCHEMA_VERSION` pin changes.)

3d. In `crates/mud-schema/src/lib.rs`, extend the `frame` re-export list with `EchoMode` and `SessionEcho`:

```rust
pub use frame::{
    EchoMode, GatewayFrame, HandshakeAck, InputLine, OutputText, ResumeHandshake, SessionClose,
    SessionConnect, SessionDisconnect, SessionEcho, SessionInput, SessionOutput, WorldFrame,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-schema`
Expected: PASS. If a `ResumeHandshake` test embeds `SCHEMA_VERSION` and asserts a serialized byte length, re-check it — it uses the constant symbolically, so it should pass unchanged.

- [ ] **Step 5: Workspace still green**

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets`
Expected: PASS — `WorldFrame` is `#[non_exhaustive]`; the gateway router's existing catch-all warns on the unknown `Echo` frame but compiles, and `mudd`'s world loop doesn't receive `WorldFrame`s.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mud-schema): WorldFrame::Echo for password masking; SCHEMA_VERSION 2 (M1-25)"
```

---

### Task 3: `mud-net` — negotiator claims ECHO; CRLF after masked lines

**Files:**
- Modify: `crates/mud-net/src/telnet/negotiation.rs`
- Modify: `crates/mud-net/src/telnet/mod.rs`
- Modify: `crates/mud-net/src/lib.rs` (export)
- Test: both files' `mod tests`

**Interfaces:**
- Consumes: nothing new (`mud-net` stays below the domain crates — no `mud-schema` dependency).
- Produces: `pub enum LocalEcho { Enabled, Suppressed }` and `TelnetMachine::set_echo(&mut self, echo: LocalEcho)`, re-exported from `mud_net`. Task 5 consumes them. Negotiation bytes are drained via the existing `take_output()`.

- [ ] **Step 1: Write the failing negotiator tests**

Append to `mod tests` in `crates/mud-net/src/telnet/negotiation.rs`:

```rust
    #[test]
    fn suppress_echo_sends_will_echo() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        assert_eq!(out, vec![IAC, WILL, OPT_ECHO]);
    }

    #[test]
    fn suppress_echo_twice_sends_one_will() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.suppress_echo(&mut out);
        assert!(out.is_empty(), "a pending offer must not repeat");
    }

    #[test]
    fn do_echo_after_our_will_enables_without_reply() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        assert!(negotiator.echo_suppressed());
        assert!(out.is_empty(), "DO answering our WILL must not be re-acknowledged");
    }

    #[test]
    fn dont_echo_after_our_will_is_a_refusal() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(!negotiator.echo_suppressed());
        assert!(out.is_empty(), "refusal of a pending offer needs no reply");
    }

    #[test]
    fn restore_echo_after_agreement_sends_wont_and_dont_acks_it() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        out.clear();
        negotiator.restore_echo(&mut out);
        assert_eq!(out, vec![IAC, WONT, OPT_ECHO]);
        assert!(!negotiator.echo_suppressed());
        out.clear();
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(out.is_empty(), "DONT answering our WONT must not be re-acknowledged");
    }

    #[test]
    fn restore_echo_before_the_client_replied_still_sends_wont() {
        // The password line can be consumed before the client's DO arrives.
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.restore_echo(&mut out);
        assert_eq!(out, vec![IAC, WONT, OPT_ECHO]);
    }

    #[test]
    fn stale_do_echo_after_our_wont_is_ignored() {
        // WILL sent, WONT sent, then the client's DO (answering the WILL)
        // arrives: our WONT is already in flight and wins.
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        negotiator.restore_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        assert!(out.is_empty());
        assert!(!negotiator.echo_suppressed());
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(out.is_empty(), "the DONT lands us back in No, silently");
    }
```

The existing `do_echo_is_refused_with_wont` test stays as-is (spontaneous `DO ECHO` with no offer outstanding is still refused) — update only its trailing comment: `// ECHO: the server never echoes normal input`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-net`
Expected: compile error — `OPT_ECHO`, `suppress_echo` not found.

- [ ] **Step 3: Implement the negotiator**

In `crates/mud-net/src/telnet/negotiation.rs`:

3a. Add the constant next to the other options, and update the module doc header's "Everything else — including ECHO and SGA — is refused" sentence to: "ECHO is server-claimed around password entry (RFC 857); SGA and everything else is refused; unknown options are never silently ignored."

```rust
pub(crate) const OPT_ECHO: u8 = 1;
```

3b. Extend `QState` (this is the first option we actively disable):

```rust
enum QState {
    No,
    WantYes,
    Yes,
    /// We sent a disabling verb and await the acknowledgement.
    WantNo,
}
```

Fix the two helper matches the compiler now flags:

- in `enable`: extend the `QState::No` arm to `QState::No | QState::WantNo` (a remote enable while our disable is in flight is acknowledged fresh — only reachable for options we also disable);
- in `disable`: extend the `QState::WantYes | QState::No` arm to `QState::WantYes | QState::WantNo | QState::No`.

3c. Add the field and initialize it in `new()`:

```rust
    us_echo: QState,
```

with `us_echo: QState::No,` in the `Self { ... }` literal (we only claim ECHO on demand, never in the opening offers).

3d. Add the server-initiated toggles and the query:

```rust
    /// Claims the ECHO option (RFC 857): asks the client to stop local echo
    /// for password entry. Idempotent while an offer is pending or active.
    pub(crate) fn suppress_echo(&mut self, out: &mut Vec<u8>) {
        match self.us_echo {
            QState::No | QState::WantNo => {
                self.us_echo = QState::WantYes;
                out.extend_from_slice(&[IAC, WILL, OPT_ECHO]);
            }
            QState::WantYes | QState::Yes => {}
        }
    }

    /// Releases the ECHO option: the client resumes local echo. Also sent
    /// from `WantYes` — the password line can be consumed before the
    /// client's DO arrives, and the retraction must still go out.
    pub(crate) fn restore_echo(&mut self, out: &mut Vec<u8>) {
        match self.us_echo {
            QState::Yes | QState::WantYes => {
                self.us_echo = QState::WantNo;
                out.extend_from_slice(&[IAC, WONT, OPT_ECHO]);
            }
            QState::WantNo | QState::No => {}
        }
    }

    /// True when the client has agreed to suppress its local echo.
    pub(crate) fn echo_suppressed(&self) -> bool {
        self.us_echo == QState::Yes
    }
```

3e. Wire the client replies into `on_negotiate`. In the `Verb::Do` match, add an `OPT_ECHO` arm **before** the `unsupported` catch-all:

```rust
                OPT_ECHO => match self.us_echo {
                    QState::WantYes => self.us_echo = QState::Yes,
                    // No offer outstanding: the server never echoes normal
                    // input, so a spontaneous DO is refused.
                    QState::No => out.extend_from_slice(&[IAC, WONT, OPT_ECHO]),
                    // Stale agreement to a WILL we have since retracted; our
                    // WONT is in flight and the client's DONT lands us in No.
                    QState::WantNo => {}
                    QState::Yes => {}
                },
```

In the `Verb::Dont` match, add before the `_ => {}` arm:

```rust
                OPT_ECHO => Self::disable(&mut self.us_echo, WONT, option, out),
```

- [ ] **Step 4: Run negotiator tests to verify they pass**

Run: `cargo test -p mud-net`
Expected: PASS.

- [ ] **Step 5: Write the failing `TelnetMachine` tests**

Append to `mod tests` in `crates/mud-net/src/telnet/mod.rs`:

```rust
    #[test]
    fn set_echo_queues_the_negotiation_bytes() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output(); // discard opening offers
        machine.set_echo(LocalEcho::Suppressed);
        assert_eq!(machine.take_output(), vec![255, 251, 1], "IAC WILL ECHO");
        machine.set_echo(LocalEcho::Enabled);
        assert_eq!(machine.take_output(), vec![255, 252, 1], "IAC WONT ECHO");
    }

    #[test]
    fn a_masked_line_is_answered_with_a_crlf() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        machine.set_echo(LocalEcho::Suppressed);
        let _ = machine.receive(&[255, 253, 1]); // client agrees: IAC DO ECHO
        let _ = machine.take_output();
        let events = machine.receive(b"hunter2\r\n");
        assert_eq!(events, vec![TelnetEvent::Line("hunter2".into())]);
        assert_eq!(
            machine.take_output(),
            b"\r\n".to_vec(),
            "the client echoes nothing, so the server advances the line"
        );
    }

    #[test]
    fn an_unmasked_line_gets_no_crlf() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        let events = machine.receive(b"look\r\n");
        assert_eq!(events, vec![TelnetEvent::Line("look".into())]);
        assert!(machine.take_output().is_empty());
    }

    #[test]
    fn a_refusing_client_gets_no_crlf_compensation() {
        // The client refused (or ignored) WILL ECHO: it is still echoing
        // locally, including the newline, so no compensation is owed.
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        machine.set_echo(LocalEcho::Suppressed);
        let _ = machine.receive(&[255, 254, 1]); // IAC DONT ECHO
        let _ = machine.take_output();
        let _ = machine.receive(b"visible\r\n");
        assert!(machine.take_output().is_empty());
    }
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test -p mud-net`
Expected: compile error — `LocalEcho`, `set_echo` not found.

- [ ] **Step 7: Implement `TelnetMachine`**

In `crates/mud-net/src/telnet/mod.rs`:

7a. Add above `TelnetMachine`:

```rust
/// Client-side local echo, controlled by the server via RFC 857.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEcho {
    /// Normal input: the client echoes what the player types.
    Enabled,
    /// Secret entry: the client is asked to stop echoing (IAC WILL ECHO).
    Suppressed,
}
```

7b. Add the method to `impl TelnetMachine`:

```rust
    /// Asks the client to change its local echo (RFC 857, password masking).
    /// The negotiation bytes accumulate internally; drain them with
    /// [`take_output`](Self::take_output) and write them to the client
    /// before the prompt they guard.
    pub fn set_echo(&mut self, echo: LocalEcho) {
        match echo {
            LocalEcho::Suppressed => self.negotiator.suppress_echo(&mut self.output),
            LocalEcho::Enabled => self.negotiator.restore_echo(&mut self.output),
        }
    }
```

7c. In `receive`, compensate for the swallowed newline. Replace the `ParsedItem::Data` arm with:

```rust
                ParsedItem::Data(byte) => {
                    if let Some(text) = self.line.push(byte) {
                        // A client that agreed to suppress echo shows nothing
                        // — not even the Enter — so advance its display past
                        // the prompt line (design 2026-07-11 §4).
                        if self.negotiator.echo_suppressed() {
                            self.output.extend_from_slice(b"\r\n");
                        }
                        events.push(TelnetEvent::Line(text));
                    }
                }
```

7d. In `crates/mud-net/src/lib.rs`, extend the telnet re-export:

```rust
pub use telnet::{LocalEcho, TelnetEvent, TelnetMachine};
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p mud-net` then `cargo clippy -p mud-net --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 9: Commit**

```bash
jj commit -m "feat(mud-net): server-claimed ECHO for password masking (RFC 857) (M1-25)"
```

---

### Task 4: `mud-engine` + `mudd` — echo flows through `Routing::Login` onto the IPC channel

Changing `Routing::Login.outputs` breaks `mudd`'s compile, so both crates change in this task and the workspace stays green.

**Files:**
- Modify: `crates/mud-engine/src/session/mod.rs`
- Modify: `crates/mud-engine/src/lib.rs` (export)
- Modify: `crates/mud-engine/tests/session_login.rs` (text-collection sites)
- Modify: `crates/mudd/src/world_loop.rs`
- Test: `crates/mud-engine/src/session/mod.rs` (`mod tests`)

**Interfaces:**
- Consumes: `mud_session::InputEcho` (Task 1), `mud_schema::{EchoMode, SessionEcho}` (Task 2).
- Produces: `pub enum LoginOutput { Text(SessionOutput), Echo(SessionEcho) }` exported from `mud_engine`; `Routing::Login { outputs: Vec<LoginOutput>, close: bool }`. Task 5 does not consume these (the gateway sees only IPC frames); Task 6's e2e test exercises the whole chain.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `crates/mud-engine/src/session/mod.rs` (uses the existing `FakeBackend`, `sid`, and `Routing` items):

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mud-engine`
Expected: compile error — `LoginOutput` not found.

- [ ] **Step 3: Implement `mud-engine`**

In `crates/mud-engine/src/session/mod.rs`:

3a. Extend the schema import (line ~17):

```rust
use mud_schema::{EchoMode, OutputText, SessionEcho, SessionId, SessionOutput};
```

and add `InputEcho` to whatever `mud_session` items the file already imports.

3b. Add the enum next to `Routing`:

```rust
/// One ordered item of pre-login output.
#[derive(Debug)]
#[must_use]
pub enum LoginOutput {
    /// Rendered text for the session.
    Text(SessionOutput),
    /// A change to the session's local-echo mode (password masking).
    Echo(SessionEcho),
}
```

3c. Change the `Routing::Login` variant field:

```rust
    /// Handled by the pre-login FSM; here is the output and whether to close.
    Login {
        outputs: Vec<LoginOutput>,
        close: bool,
    },
```

3d. In `drive()`, emit the echo item **before** the same transition's messages. The top of the loop becomes:

```rust
        loop {
            if let Some(echo) = transition.echo {
                outputs.push(LoginOutput::Echo(SessionEcho {
                    session_id: session,
                    mode: echo_mode(echo),
                }));
            }
            outputs.extend(
                self.render_outputs(session, std::mem::take(&mut transition.messages))
                    .into_iter()
                    .map(LoginOutput::Text),
            );
```

(The rest of the loop body — terminal, effect, `on_effect` — is unchanged.)

3e. Add the boundary conversion as a free function near `render` usage:

```rust
/// Maps the FSM's echo signal onto the IPC wire type at the engine boundary.
fn echo_mode(echo: InputEcho) -> EchoMode {
    match echo {
        InputEcho::Enabled => EchoMode::Enabled,
        InputEcho::Suppressed => EchoMode::Suppressed,
    }
}
```

3f. `connect()` is untouched — it still returns `Vec<SessionOutput>` (a fresh connection is echo-on by telnet default; no signal exists to carry).

3g. In `crates/mud-engine/src/lib.rs`, add `LoginOutput` to the session re-export list (line ~42):

```rust
    BackendError, InWorldBinding, LoginBackend, LoginOutput, RegistryResolver, Routing,
    SessionService,
```

3h. Update the two internal tests that read `outputs` as text. Keep `text_of` (used by `connect_greets_with_a_banner_and_prompt`) and add beside it:

```rust
    fn login_text_of(outputs: &[LoginOutput]) -> String {
        outputs
            .iter()
            .filter_map(|output| match output {
                LoginOutput::Text(text) => Some(text.text.as_str()),
                LoginOutput::Echo(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
```

In `a_wrong_password_stays_pre_login`, replace both `text_of(&outputs)` calls with `login_text_of(&outputs)`.

3i. In `crates/mud-engine/tests/session_login.rs`, the test `a_wrong_password_then_retry_succeeds` collects text with `outputs.iter().map(|o| o.text.as_str()).collect::<String>()`. Replace that line with:

```rust
    let text = outputs
        .iter()
        .filter_map(|output| match output {
            mud_engine::LoginOutput::Text(text) => Some(text.text.as_str()),
            mud_engine::LoginOutput::Echo(_) => None,
        })
        .collect::<String>();
```

- [ ] **Step 4: Implement `mudd`**

In `crates/mudd/src/world_loop.rs`:

4a. Add `LoginOutput` to the `mud_engine` import (line 10):

```rust
use mud_engine::{LoginOutput, Pipeline, PipelineError, Routing, SessionDisposition, SessionService};
```

4b. Add a mapping helper next to `log_tick_event`:

```rust
/// Maps one pre-login output item onto its IPC frame.
fn frame_of(output: LoginOutput) -> WorldFrame {
    match output {
        LoginOutput::Text(output) => WorldFrame::Output(output),
        LoginOutput::Echo(echo) => WorldFrame::Echo(echo),
    }
}
```

4c. In `handle_input`, the `Routing::Login` arm's send loop becomes:

```rust
        Routing::Login { outputs, close } => {
            for output in outputs {
                endpoint
                    .send(frame_of(output))
                    .await
                    .context("send output")?;
            }
```

(The `close` block below is unchanged. The `GatewayFrame::Connect` arm in `run` is also unchanged — `connect()` still yields `SessionOutput`s.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mud-engine -p mudd`
Expected: PASS, including the pre-existing FSM-driver and world-loop tests.

- [ ] **Step 6: Workspace check and commit**

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets`
Expected: green.

```bash
jj commit -m "feat(mud-engine,mudd): carry echo changes through Routing::Login onto WorldFrame::Echo (M1-25)"
```

---

### Task 5: `mud-gateway` — route `WorldFrame::Echo` to the telnet connection

**Files:**
- Modify: `crates/mud-gateway/src/router.rs`
- Modify: `crates/mud-gateway/src/connection.rs`
- Test: both files' `mod tests`

**Interfaces:**
- Consumes: `mud_schema::EchoMode` (Task 2), `mud_net::LocalEcho` + `TelnetMachine::set_echo` (Task 3).
- Produces: `ToConnection::Echo(EchoMode)` (crate-internal); the connection task writes the RFC 857 bytes.

- [ ] **Step 1: Write the failing router test**

Append to `mod tests` in `crates/mud-gateway/src/router.rs` (uses the existing `session`, `drain_barrier`, and channel setup; add `EchoMode, SessionEcho` to the test module's `mud_schema` import list):

```rust
    #[tokio::test]
    async fn echo_frame_routes_to_the_registered_session() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        let (tx, mut output_rx) = mpsc::channel(OUTPUT_CAPACITY);
        let id = session(1);
        commands_tx
            .send(ToRouter::Register { session_id: id, tx })
            .await
            .expect("router must accept registration");

        drain_barrier(&commands_tx, &mut world_end, session(2)).await;

        world_end
            .send(WorldFrame::Echo(SessionEcho {
                session_id: id,
                mode: EchoMode::Suppressed,
            }))
            .await
            .expect("world endpoint must send");

        let routed = output_rx.recv().await.expect("echo must be routed");
        assert!(matches!(routed, ToConnection::Echo(EchoMode::Suppressed)));

        drop(world_end);
        router
            .await
            .expect("router task must not panic")
            .expect("closed peer is a clean shutdown");
    }
```

- [ ] **Step 2: Write the failing connection test**

Append to `mod tests` in `crates/mud-gateway/src/connection.rs` (uses the existing `spawn_connection`, `expect_register`, `default_limiter`; add `EchoMode` to the test module's imports):

```rust
    #[tokio::test]
    async fn echo_change_writes_will_echo_without_a_prompt_frame() {
        let (mut client, mut router_rx, _task) = spawn_connection(default_limiter());

        let tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        let mut offers = [0u8; 12];
        client
            .read_exact(&mut offers)
            .await
            .expect("opening offers");

        tx.send(ToConnection::Echo(EchoMode::Suppressed))
            .await
            .expect("connection must accept the echo change");

        // Exactly IAC WILL ECHO — no prompt frame rides along.
        let mut buf = [0u8; 3];
        client
            .read_exact(&mut buf)
            .await
            .expect("negotiation bytes must be written");
        assert_eq!(buf, [255, 251, 1]);

        tx.send(ToConnection::Echo(EchoMode::Enabled))
            .await
            .expect("connection must accept the echo change");
        client
            .read_exact(&mut buf)
            .await
            .expect("negotiation bytes must be written");
        assert_eq!(buf, [255, 252, 1]);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p mud-gateway`
Expected: compile error — no variant `Echo` on `ToConnection`.

- [ ] **Step 4: Implement**

4a. In `crates/mud-gateway/src/router.rs`, extend the schema import:

```rust
use mud_schema::{EchoMode, GatewayFrame, OutputText, SessionId, WorldFrame};
```

Add the variant to `ToConnection` (between `Output` and `Close`):

```rust
    /// Ask the client to change its local echo (password masking, RFC 857).
    Echo(EchoMode),
```

Add the router arm in `run_router`'s `endpoint.recv()` match, after the `Close` arm and before the catch-all:

```rust
                Some(WorldFrame::Echo(echo)) => {
                    route(&registry, echo.session_id, ToConnection::Echo(echo.mode));
                }
```

4b. In `crates/mud-gateway/src/connection.rs`, extend the `mud_net` import:

```rust
use mud_net::{Decision, LocalEcho, RateLimiter, TelnetEvent, TelnetMachine};
```

and import `EchoMode` from `mud_schema` (extend the existing `mud_schema` import line).

In `connection_loop`'s `output_rx.recv()` match, add between the `Output` and `Close` arms:

```rust
                Some(ToConnection::Echo(mode)) => {
                    machine.set_echo(match mode {
                        EchoMode::Enabled => LocalEcho::Enabled,
                        EchoMode::Suppressed => LocalEcho::Suppressed,
                    });
                    // Negotiation only — no prompt frame rides along.
                    let bytes = machine.take_output();
                    if !bytes.is_empty() && writer.write_all(&bytes).await.is_err() {
                        return ExitCause::ClientGone;
                    }
                }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mud-gateway` then `cargo clippy -p mud-gateway --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mud-gateway): route WorldFrame::Echo to the telnet connection (M1-25)"
```

---

### Task 6: End-to-end proof, docs, SPEC matrix line, PLAN.md, journal

**Files:**
- Modify: `crates/mudd/tests/telnet_login.rs`
- Modify: `docs/docs/playing/getting-started.md`
- Modify: `SPEC.md` (§2.8.2 support matrix, one bullet)
- Modify: `PLAN.md` (M1-25 entry)
- Modify: `.claude/JOURNAL.md`

**Interfaces:**
- Consumes: the full chain from Tasks 1–5.
- Produces: nothing for later tasks — this closes M1-25.

- [ ] **Step 1: Write the failing e2e assertions**

In `crates/mudd/tests/telnet_login.rs`, extend the shared helper `login_and_enter_world` (both tests then verify masking framing). Replace its register/password section:

```rust
    client.write_line("register alice").await;
    let to_password = client.read_until(b"Password:").await;
    assert!(
        to_password.windows(3).any(|w| w == [255, 251, 1]),
        "IAC WILL ECHO must precede the password prompt, got {to_password:?}"
    );

    client.write_line("hunter2!").await;
    let to_confirm = client.read_until(b"Confirm password:").await;
    assert!(
        !to_confirm.windows(3).any(|w| w == [255, 252, 1]),
        "echo must stay suppressed across the confirm prompt, got {to_confirm:?}"
    );

    client.write_line("hunter2!").await;
    let after_secret = client.read_until(b"You have no characters yet.").await;
    assert!(
        after_secret.windows(3).any(|w| w == [255, 252, 1]),
        "IAC WONT ECHO must follow the final password line, got {after_secret:?}"
    );
```

(The trailing `new Hero` / entered-world steps of the helper are unchanged. The test client never replies `DO ECHO`, which exercises the restore-from-`WantYes` path — masking framing must not depend on the client's cooperation.)

Then add a new test covering the **login** path (the design's §5 requires both flows). It reuses the account and puppet ("Hero") that `login_and_enter_world` creates on a first connection to the same booted server:

```rust
#[tokio::test]
async fn login_masks_the_password_like_registration() {
    let tenant_dir = TempDir::new().expect("temp dir");
    write_tenant(tenant_dir.path());

    let (addrs, _tasks) = mudd::boot(single_tenant_config(tenant_dir.path()))
        .await
        .expect("boot must succeed");
    let addr = *addrs.first().expect("one bound address");

    // First connection registers alice and creates the puppet Hero.
    let stream = TcpStream::connect(addr).await.expect("client must connect");
    let mut client = ClientReader::new(stream);
    login_and_enter_world(&mut client).await;
    client.write_line("quit").await;
    drop(client);

    // Second connection logs in; the password prompt must be masked too.
    let stream = TcpStream::connect(addr).await.expect("client must reconnect");
    let mut client = ClientReader::new(stream);
    client.read_until(b"Welcome to Testville.").await;

    client.write_line("login alice").await;
    let to_password = client.read_until(b"Password:").await;
    assert!(
        to_password.windows(3).any(|w| w == [255, 251, 1]),
        "IAC WILL ECHO must precede the login password prompt, got {to_password:?}"
    );

    client.write_line("hunter2!").await;
    // The first post-auth output is the puppet list naming Hero; the echo
    // release must have been written by then.
    let after_secret = client.read_until(b"Hero").await;
    assert!(
        after_secret.windows(3).any(|w| w == [255, 252, 1]),
        "IAC WONT ECHO must follow the password line, got {after_secret:?}"
    );
}
```

- [ ] **Step 2: Run the e2e tests**

Run: `cargo test -p mudd --test telnet_login`
Expected: **PASS already** — Tasks 1–5 are in place; this step verifies the assertions are wired correctly rather than red. If any assertion fails, debug the chain before proceeding (the failure text prints the raw bytes).

To confirm the assertions actually bite, temporarily flip `[255, 251, 1]` to `[255, 251, 2]` in the first assertion, watch it fail, and flip it back.

- [ ] **Step 3: Update the player docs**

In `docs/docs/playing/getting-started.md`, replace the `!!! note` block under "Logging in" with:

```markdown
!!! note
    The server asks your client to stop echoing while you type a password
    (telnet echo suppression, RFC 857). Most MUD clients and plain `telnet`
    honor it; if yours refuses, it will still display the password as you
    type — be mindful of who can see your screen.
```

Verify from `docs/`: `uv run mkdocs build --strict`
Expected: clean build.

- [ ] **Step 4: Add ECHO to the SPEC §2.8.2 matrix**

In `SPEC.md` §2.8.2, the telnet bullet list ("full IAC negotiation, including:") — add after the **EOR / GA** bullet:

```markdown
  - **ECHO** (RFC 857) — server-claimed around password entry to suppress
    the client's local echo; released as soon as the secret line is
    consumed. The server never echoes normal input.
```

- [ ] **Step 5: Add the M1-25 entry to PLAN.md**

Insert after the **M1-24** block (before the `---` that closes the M1 section):

```markdown
- **M1-25 — Password echo suppression.** The session FSM signals secret
  entry (`Transition.echo`, derived from password-state membership so a
  future secret-collecting state is masked automatically); a new
  `WorldFrame::Echo` carries it to the gateway (SCHEMA_VERSION 2); the
  telnet negotiator claims ECHO (RFC 857): IAC WILL ECHO before the
  password prompt, IAC WONT ECHO once the secret line is consumed, plus a
  CRLF echo for masked lines. Clients that refuse keep visible passwords
  (documented limitation, no warning message).
  - *Spec:* §2.8.2; design doc
    `docs/superpowers/specs/2026-07-11-password-echo-suppression-design.md`.
    *Verify:* telnet e2e asserts WILL/WONT ECHO framing around the password
    prompt; workspace tests and clippy green.
```

- [ ] **Step 6: Journal entry**

Append to `.claude/JOURNAL.md`:

```markdown
## 2026-07-13 — M1-25 password echo suppression

- **Spec:** §2.8.2 (ECHO row added), §2.1.3 — mask password entry over telnet.
- **Done:** `Transition.echo` derived in the FSM; `WorldFrame::Echo`
  (SCHEMA_VERSION 2); `LoginOutput` through `Routing::Login`; gateway routes
  to `TelnetMachine::set_echo` (RFC 857 WILL/WONT ECHO + CRLF after masked
  lines). Docs note reduced to refusing clients; PLAN M1-25 added.
- **Verify:** per-crate unit tests; `telnet_login` e2e asserts WILL/WONT
  ECHO framing; workspace tests + clippy green.
- **Next:** none — refusing clients intentionally get no warning (design §1).
```

- [ ] **Step 7: Full verification and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: green.

```bash
jj commit -m "test(mudd),docs: e2e echo-masking assertions; docs/SPEC/PLAN for M1-25"
```
