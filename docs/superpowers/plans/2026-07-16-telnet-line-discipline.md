# Telnet Line Discipline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every output block reaches the telnet client as a properly framed unit — blank line before, CRLF termination for messages, no termination for input prompts — per the approved design `docs/superpowers/specs/2026-07-16-telnet-line-discipline-design.md` (M1-28).

**Architecture:** A typed `OutputKind` (`Line` / `Prompt`) rides `SessionOutput` across the IPC wire. The engine classifies at the source (only `PasswordPrompt`/`ConfirmPrompt` are prompts) and coalesces all text produced by one input line into a single block. The gateway is the single owner of line discipline: per block it writes `\r\n` (breathing room) + body + `\r\n` for Lines (nothing for Prompts) + the existing GA/EOR prompt frame.

**Tech Stack:** Rust workspace (crates `mud-session`, `mud-schema`, `mud-engine`, `mud-gateway`, `mudd`), postcard/serde wire encoding, tokio, jj (Jujutsu) for VCS.

## Global Constraints

- **VCS is jj, not git.** Commit with `jj commit -m "<message>"` (commits the working copy; no staging step). Never run `git commit`.
- **`unwrap()` is strictly forbidden; `expect()` only in tests, always with a descriptive message.**
- **Never suppress lints.** Workspace clippy denies `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `print_stdout`, `print_stderr`. Integration-test crates carry a file-level `#![allow(clippy::expect_used, clippy::panic)]` with the standard comment — keep it where it already exists, don't add it to non-test code.
- **Match enums exhaustively** — no `_ =>` catch-alls; new variants must surface as compile errors.
- **No new dependencies.** If you think you need one, stop — you don't.
- **Comments say why, not how.** Doc comments on every new public item.
- **TDD:** failing test → minimal code → refactor. Run the named test before implementing to see it fail.
- **Verification commands:** `cargo test --workspace` and `cargo clippy --workspace --all-targets` must be green at the end of every task.

---

### Task 1: Rename `SessionMessage::Prompt` → `SessionMessage::LoginInstructions`

The variant carries the "Type 'login <name>'…" instruction line — a complete message, not an input prompt. The rename removes the collision with the `OutputKind::Prompt` concept Task 2 introduces. Purely mechanical, compiler-driven; the i18n catalog key `session.prompt` is intentionally untouched (catalog keys are builder-facing and stable).

**Files:**
- Modify: `crates/mud-session/src/message.rs:9-10`
- Modify: `crates/mud-session/src/fsm.rs:168` and `crates/mud-session/src/fsm.rs:601`
- Modify: `crates/mud-engine/src/session/render.rs:15` and `crates/mud-engine/src/session/render.rs:60`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `mud_session::SessionMessage::LoginInstructions` (unit variant, replaces `SessionMessage::Prompt`). Task 3's classification function matches on this name.

- [ ] **Step 1: Rename the variant in `mud-session`**

In `crates/mud-session/src/message.rs`, change:

```rust
    /// The pre-login prompt: how to register and how to log in.
    Prompt,
```

to:

```rust
    /// The pre-login instruction line: how to register and how to log in.
    LoginInstructions,
```

In `crates/mud-session/src/fsm.rs:168` change:

```rust
        Transition::messages(vec![SessionMessage::Banner, SessionMessage::Prompt])
```

to:

```rust
        Transition::messages(vec![
            SessionMessage::Banner,
            SessionMessage::LoginInstructions,
        ])
```

In `crates/mud-session/src/fsm.rs:601` (test `on_connect_presents_banner_then_prompt`) change:

```rust
            vec![SessionMessage::Banner, SessionMessage::Prompt]
```

to:

```rust
            vec![SessionMessage::Banner, SessionMessage::LoginInstructions]
```

- [ ] **Step 2: Follow the compile error into `mud-engine`**

In `crates/mud-engine/src/session/render.rs:15` change:

```rust
        SessionMessage::Prompt => t!(*locale, "session.prompt"),
```

to:

```rust
        SessionMessage::LoginInstructions => t!(*locale, "session.prompt"),
```

In `crates/mud-engine/src/session/render.rs:60` (test) change:

```rust
        let text = render(&SessionMessage::Prompt, "", &Locale::EN);
```

to:

```rust
        let text = render(&SessionMessage::LoginInstructions, "", &Locale::EN);
```

- [ ] **Step 3: Verify workspace is green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no warnings. Also confirm no stragglers: `rg -n "SessionMessage::Prompt\b" crates/` must return nothing.

- [ ] **Step 4: Commit**

```bash
jj commit -m "refactor(session): rename SessionMessage::Prompt to LoginInstructions"
```

---

### Task 2: `OutputKind` on the wire (`mud-schema`) + mechanical construction-site updates

Adds the typed Line/Prompt distinction to `SessionOutput`. Every existing construction site states `kind: OutputKind::Line` in this task so the workspace stays green with behavior unchanged (the gateway ignores `kind` until Task 4; the engine classifies prompts in Task 3).

**Files:**
- Modify: `crates/mud-schema/src/frame.rs` (enum + field + 4 test sites + 1 stable-encoding test)
- Modify: `crates/mud-schema/src/lib.rs:23-26` (export)
- Modify: `crates/mud-ipc/tests/transport.rs:58,235`
- Modify: `crates/mud-engine/src/session/mod.rs:363-366`
- Modify: `crates/mud-engine/src/pipeline.rs:248-251`
- Modify: `crates/mud-engine/src/presence.rs:31-34`
- Modify: `crates/mud-gateway/src/router.rs` (6 test construction sites at lines 170, 195, 272, 323, 331, 392)
- Modify: `crates/mud-gateway/tests/loopback.rs:126,154`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `mud_schema::OutputKind` (`enum OutputKind { Line, Prompt }`, derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`) and the new field `pub kind: OutputKind` on `mud_schema::SessionOutput`, declared **after** `text` (postcard field order is the wire format). Tasks 3–5 rely on both names exactly.

- [ ] **Step 1: Write the failing round-trip test**

In `crates/mud-schema/src/frame.rs`, inside `mod tests`, next to `world_output_round_trips`:

```rust
    #[test]
    fn world_output_round_trips_a_prompt_block() {
        let frame = WorldFrame::Output(SessionOutput {
            session_id: session(4),
            text: OutputText::new("Password:"),
            kind: OutputKind::Prompt,
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<WorldFrame>(&bytes).expect("decode"), frame);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p mud-schema world_output_round_trips_a_prompt_block`
Expected: FAIL to compile — `cannot find OutputKind` / `struct SessionOutput has no field named kind`.

- [ ] **Step 3: Add `OutputKind` and the `kind` field**

In `crates/mud-schema/src/frame.rs`, directly above `SessionOutput`:

```rust
/// How the Gateway terminates an output block (§2.8.2 line discipline).
///
/// A [`Line`](OutputKind::Line) is a completed message: the Gateway ends the
/// line after it. A [`Prompt`](OutputKind::Prompt) awaits input on the same
/// line: the Gateway leaves it unterminated and the EOR/GA prompt frame tells
/// prompt-aware clients where the prompt ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum OutputKind {
    /// A completed message; the Gateway terminates it with a line ending.
    Line,
    /// An input prompt; left unterminated so the cursor rests on the line.
    Prompt,
}
```

Extend `SessionOutput` (the `kind` field goes **last** — wire order):

```rust
pub struct SessionOutput {
    /// The session this output is destined for.
    pub session_id: SessionId,
    /// The text to present to the client.
    pub text: OutputText,
    /// How the Gateway terminates the block (§2.8.2 line discipline).
    pub kind: OutputKind,
}
```

In `crates/mud-schema/src/lib.rs`, add `OutputKind` to the `frame` re-export list (alphabetical position, after `InputLine`):

```rust
pub use frame::{
    EchoMode, GatewayFrame, HandshakeAck, InputLine, OutputKind, OutputText, ResumeHandshake,
    SessionClose, SessionConnect, SessionDisconnect, SessionEcho, SessionInput, SessionOutput,
    WorldFrame,
};
```

- [ ] **Step 4: Fix every construction site (mechanically `kind: OutputKind::Line`)**

Add `kind: OutputKind::Line,` after the `text:` field in each `SessionOutput { … }` literal below (import `OutputKind` where the file's `mud_schema` import list doesn't already have it):

- `crates/mud-schema/src/frame.rs` tests: `world_output_round_trips_a_styled_payload` (line ~246), `world_output_round_trips` (~256), `output_frame_has_a_stable_encoding` (~349), `output_text_round_trips_multibyte_unicode` (~414).
- `crates/mud-ipc/tests/transport.rs`: lines ~58 and ~235.
- `crates/mud-engine/src/session/mod.rs` `render_outputs` (~363):

```rust
            .map(|message| SessionOutput {
                session_id: session,
                text: OutputText::new(render(&message, &self.banner, &self.locale)),
                kind: OutputKind::Line,
            })
```

- `crates/mud-engine/src/pipeline.rs` `message()` (~248):

```rust
    vec![SessionOutput {
        session_id,
        text: OutputText::new(text),
        kind: OutputKind::Line,
    }]
```

- `crates/mud-engine/src/presence.rs` `announce` (~31):

```rust
        .map(|session_id| SessionOutput {
            session_id,
            text: OutputText::new(message.clone()),
            kind: OutputKind::Line,
        })
```

- `crates/mud-gateway/src/router.rs` tests: all six `SessionOutput { … }` literals.
- `crates/mud-gateway/tests/loopback.rs`: both `SessionOutput { … }` literals.

- [ ] **Step 5: Update the stable-encoding expectation**

`kind` appends one byte (enum variant index; `Line` = `0x00`) to the postcard encoding. In `crates/mud-schema/src/frame.rs`, `output_frame_has_a_stable_encoding`, change the expected bytes and extend the layout comment:

```rust
    // Output = variant 0; session_id = 4; text = StyledText with one plain span:
    // spans len 1, span.text "hi" (len 2, 0x68 0x69), span.style = SpanStyle::Plain
    // (variant 0); kind = OutputKind::Line (variant 0).
    #[test]
    fn output_frame_has_a_stable_encoding() {
        let frame = WorldFrame::Output(SessionOutput {
            session_id: session(4),
            text: OutputText::new("hi"),
            kind: OutputKind::Line,
        });
        assert_eq!(
            encode(&frame).expect("encode"),
            vec![0x00, 0x04, 0x01, 0x02, 0x68, 0x69, 0x00, 0x00]
        );
    }
```

- [ ] **Step 6: Verify workspace is green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS (including the Step 1 test), no warnings.

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(schema): typed OutputKind (Line/Prompt) on SessionOutput"
```

---

### Task 3: Engine classification and one-block-per-input coalescing

The engine states which blocks are prompts and merges everything one input line produces into a single block. Two mechanisms (design §Architecture 2): `render_batch` joins one FSM transition's messages with `\n` and takes the kind of the **last** message; `drive` coalesces text across the effect chain of one input line, flushing at echo changes and returns, so `Created Hero.` and `Welcome. You are now in the world.` share one block.

**Files:**
- Modify: `crates/mud-engine/src/session/render.rs` (add `kind` classifier + tests)
- Modify: `crates/mud-engine/src/session/mod.rs` (`connect`, `drive`, replace `render_outputs` with `render_batch`, add `block`/`flush_pending` helpers, tests)
- Modify: `docs/superpowers/specs/2026-07-16-telnet-line-discipline-design.md` (one clarifying sentence, see Step 7)

**Interfaces:**
- Consumes: `SessionMessage::LoginInstructions` (Task 1); `mud_schema::OutputKind`, `SessionOutput.kind` (Task 2).
- Produces: behavior only — `SessionService::connect` and `SessionService::on_input` now emit at most one `SessionOutput` per contiguous text run, with `kind` set. `pipeline.rs` and `presence.rs` already emit `OutputKind::Line` (Task 2) and are final as-is.

- [ ] **Step 1: Write the failing classifier tests**

In `crates/mud-engine/src/session/render.rs`, inside the existing `mod tests`:

```rust
    #[test]
    fn password_prompts_classify_as_prompt_blocks() {
        assert_eq!(kind(&SessionMessage::PasswordPrompt), OutputKind::Prompt);
        assert_eq!(kind(&SessionMessage::ConfirmPrompt), OutputKind::Prompt);
    }

    #[test]
    fn ordinary_messages_classify_as_line_blocks() {
        for message in [
            SessionMessage::Banner,
            SessionMessage::LoginInstructions,
            SessionMessage::LoginFailed,
            SessionMessage::EnteredWorld,
        ] {
            assert_eq!(kind(&message), OutputKind::Line, "for {message:?}");
        }
    }
```

Add `use mud_schema::OutputKind;` to the test module's imports if `super::*` doesn't already surface it.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p mud-engine password_prompts_classify_as_prompt_blocks`
Expected: FAIL to compile — `cannot find function kind`.

- [ ] **Step 3: Implement the classifier**

In `crates/mud-engine/src/session/render.rs` (add `use mud_schema::OutputKind;` at the top):

```rust
/// Classifies a message's output block (§2.8.2 line discipline): the two
/// password prompts leave the cursor on their line; every other message is a
/// completed line the gateway terminates. Exhaustive so a new variant forces
/// a classification decision here.
pub(crate) fn kind(message: &SessionMessage) -> OutputKind {
    match message {
        SessionMessage::PasswordPrompt | SessionMessage::ConfirmPrompt => OutputKind::Prompt,
        SessionMessage::Banner
        | SessionMessage::LoginInstructions
        | SessionMessage::PreLoginHelp
        | SessionMessage::WhoStub
        | SessionMessage::UnknownCommand
        | SessionMessage::Goodbye
        | SessionMessage::LoginFailed
        | SessionMessage::AccountSuspended
        | SessionMessage::AccountBanned
        | SessionMessage::ServerError
        | SessionMessage::PuppetList(_)
        | SessionMessage::NoPuppetsYet
        | SessionMessage::PasswordMismatch
        | SessionMessage::NameInvalid
        | SessionMessage::UsernameTaken
        | SessionMessage::PuppetCreated(_)
        | SessionMessage::EnteredWorld => OutputKind::Line,
    }
}
```

Run: `cargo test -p mud-engine password_prompts ordinary_messages` — Expected: PASS.

- [ ] **Step 4: Write the failing merge/coalesce tests**

In `crates/mud-engine/src/session/mod.rs`, inside `mod tests`:

```rust
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
```

- [ ] **Step 5: Run them to verify they fail**

Run: `cargo test -p mud-engine connect_merges a_password_prompt_block puppet_creation_coalesces`
Expected: FAIL — `connect` currently returns two outputs; blocks are unmerged.

- [ ] **Step 6: Implement `render_batch` + coalescing `drive`**

In `crates/mud-engine/src/session/mod.rs`, add `OutputKind` to the `mud_schema` import:

```rust
use mud_schema::{EchoMode, OutputKind, OutputText, SessionEcho, SessionId, SessionOutput};
```

Replace `render_outputs` (lines 356–368) with:

```rust
    /// Renders one FSM message batch as block text plus its kind: messages
    /// joined with `\n`, kind taken from the last message (a batch ending in
    /// a password prompt is a prompt block). `None` for an empty batch.
    fn render_batch(&self, messages: &[mud_session::SessionMessage]) -> Option<(String, OutputKind)> {
        let kind = messages.last().map(render::kind)?;
        let text = messages
            .iter()
            .map(|message| render(message, &self.banner, &self.locale))
            .collect::<Vec<_>>()
            .join("\n");
        Some((text, kind))
    }
```

(No import change needed: `use render::render;` imports only the *function* (value namespace), so the path `render::kind` still resolves through the *module* `render` (type/module namespace). Write the call exactly as `messages.last().map(render::kind)?`.)

Below `SessionService`'s `impl` block (module level, next to `echo_mode`), add:

```rust
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
```

Rewrite `connect` (lines 145–150):

```rust
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
```

Rewrite `drive` (lines 198–246) — same control flow, with a `pending` accumulator coalescing consecutive text and flushing before every `Echo` push and every return:

```rust
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
```

Note `render_batch` now takes `&[SessionMessage]`, so the old `std::mem::take` still works (`&Vec<_>` derefs to a slice); pass `&std::mem::take(&mut transition.messages)` as shown.

- [ ] **Step 7: Run the engine tests; reconcile the design doc**

Run: `cargo test -p mud-engine`
Expected: PASS — the three new tests plus all pre-existing ones (they assert with `contains` over joined text, which survives merging). If a pre-existing test fails on an output *count*, update its expectation to the merged shape and say so in the commit message.

In `docs/superpowers/specs/2026-07-16-telnet-line-discipline-design.md`, Architecture item 2, first bullet, append one clarifying sentence after "…every other batch is a `Line` block.":

```
     Batches produced by one input line's effect chain (e.g. puppet
     creation then world entry) coalesce further in the session driver,
     flushing at echo-change boundaries, so one input yields one block.
```

- [ ] **Step 8: Verify workspace is green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 9: Commit**

```bash
jj commit -m "feat(engine): classify prompt blocks and coalesce one input's output into one block"
```

---

### Task 4: Gateway framing — blank line, CRLF termination, prompt passthrough

The gateway becomes the single owner of line discipline. `ToConnection::Output` carries `kind` from the router; the connection task writes `\r\n` + body + (`\r\n` for Lines) + prompt frame.

**Files:**
- Modify: `crates/mud-gateway/src/router.rs:24-31` (enum), `:62` (route site), tests at `:178,345,400`
- Modify: `crates/mud-gateway/src/connection.rs:147-158` (Output arm), tests at `:316-339` (+ one new test)
- Modify: `crates/mud-gateway/tests/loopback.rs:98-169`

**Interfaces:**
- Consumes: `mud_schema::OutputKind`, `SessionOutput.kind` (Task 2).
- Produces: `ToConnection::Output { text: OutputText, kind: OutputKind }` (struct variant, replaces `Output(OutputText)`); wire behavior `b"\r\n" + body + (b"\r\n" iff Line) + prompt_frame` that Task 5's e2e assertions rely on.

- [ ] **Step 1: Write the failing connection tests**

In `crates/mud-gateway/src/connection.rs`, rewrite the existing `output_is_encoded_and_prompt_framed` test and add a prompt-block test (both inside `mod tests`; add `OutputKind` to the `mud_schema` imports of the test module):

```rust
    #[tokio::test]
    async fn a_line_block_is_framed_with_blank_line_and_terminator() {
        let (mut client, mut router_rx, _task) = spawn_connection(default_limiter());

        let tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        let mut offers = [0u8; 12];
        client
            .read_exact(&mut offers)
            .await
            .expect("opening offers");

        tx.send(ToConnection::Output {
            text: OutputText::new("hello"),
            kind: OutputKind::Line,
        })
        .await
        .expect("connection must accept output");

        // §2.8.2 line discipline: blank line, body, CRLF terminator, then the
        // IAC GA prompt frame (no EOR negotiated).
        let mut buf = [0u8; 11];
        client
            .read_exact(&mut buf)
            .await
            .expect("output must be written");
        assert_eq!(&buf, b"\r\nhello\r\n\xff\xf9");
    }

    #[tokio::test]
    async fn a_prompt_block_stays_unterminated_before_the_prompt_frame() {
        let (mut client, mut router_rx, _task) = spawn_connection(default_limiter());

        let tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        let mut offers = [0u8; 12];
        client
            .read_exact(&mut offers)
            .await
            .expect("opening offers");

        tx.send(ToConnection::Output {
            text: OutputText::new("Password:"),
            kind: OutputKind::Prompt,
        })
        .await
        .expect("connection must accept output");

        // Blank line, body, NO terminator: the cursor rests on the prompt line
        // and IAC GA marks the prompt end.
        let mut buf = [0u8; 13];
        client
            .read_exact(&mut buf)
            .await
            .expect("output must be written");
        assert_eq!(&buf, b"\r\nPassword:\xff\xf9");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p mud-gateway a_line_block a_prompt_block`
Expected: FAIL to compile — `ToConnection::Output` is still a tuple variant.

- [ ] **Step 3: Plumb `kind` through the router**

In `crates/mud-gateway/src/router.rs`, add `OutputKind` to the `mud_schema` import, then change the variant (lines 24–27):

```rust
pub(crate) enum ToConnection {
    /// A rendered output block and how to terminate it (§2.8.2 line
    /// discipline), written to the client followed by a prompt frame.
    Output {
        text: OutputText,
        kind: OutputKind,
    },
```

and the route site (line ~62):

```rust
                Some(WorldFrame::Output(output)) => {
                    route(
                        &registry,
                        output.session_id,
                        ToConnection::Output {
                            text: output.text,
                            kind: output.kind,
                        },
                    );
                }
```

Update the three router test matchers (lines ~178, ~345, ~400) from the tuple pattern to the struct pattern, e.g.:

```rust
        assert!(matches!(routed, ToConnection::Output { text, .. } if text.to_plain_string() == "hello"));
```

(same shape for `"b-marker"` and `"for-a"`).

- [ ] **Step 4: Implement the framing in the connection task**

In `crates/mud-gateway/src/connection.rs`, add `OutputKind` to the `mud_schema` import and replace the Output arm (lines 147–158):

```rust
                Some(ToConnection::Output { text, kind }) => {
                    // The one place escapes are generated (§3.20.1.2): render the
                    // styled payload for this session, then encode per its charset.
                    let ansi = mud_net::render(text.styled(), ctx.palette, ctx.tier);
                    // §2.8.2 line discipline: a blank line precedes every block; a
                    // Line block ends its line, a Prompt block leaves the cursor
                    // resting on it.
                    let mut bytes = b"\r\n".to_vec();
                    bytes.extend_from_slice(&machine.encode_output(&ansi));
                    match kind {
                        OutputKind::Line => bytes.extend_from_slice(b"\r\n"),
                        OutputKind::Prompt => {}
                    }
                    // One rendered block = one prompt frame (§2.8.2 EOR/GA).
                    bytes.extend_from_slice(&machine.prompt_frame());
                    if writer.write_all(&bytes).await.is_err() {
                        return ExitCause::ClientGone;
                    }
                }
```

- [ ] **Step 5: Run the new tests**

Run: `cargo test -p mud-gateway a_line_block a_prompt_block`
Expected: PASS.

- [ ] **Step 6: Update the loopback integration tests**

In `crates/mud-gateway/tests/loopback.rs`:

In `echo_round_trip_with_negotiation_and_prompt_frame` (~line 125), drop the body's trailing `\n` (bodies carry no terminators now) and assert the framed shape:

```rust
    world_end
        .send(WorldFrame::Output(SessionOutput {
            session_id,
            text: OutputText::new("echo: look"),
            kind: OutputKind::Line,
        }))
        .await
        .expect("world must send output");
    // IAC GA prompt frame follows the block (client offered no EOR).
    let output = read_until(&mut client, &[255, 249]).await;
    let framed = b"\r\necho: look\r\n";
    assert!(
        output.windows(framed.len()).any(|w| w == framed),
        "block must arrive blank-line-prefixed and CRLF-terminated, got {output:?}"
    );
```

In `styled_output_renders_ansi16_sgr_to_the_client` (~line 150), change `.plain(" waves\n")` to `.plain(" waves")` and add `kind: OutputKind::Line,` to the `SessionOutput` literal; the SGR assertion is unchanged. Add `OutputKind` to the `mud_schema` import list at the top of the file.

- [ ] **Step 7: Verify workspace is green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit**

```bash
jj commit -m "feat(gateway): own telnet line discipline — blank line, CRLF for lines, bare prompts"
```

---

### Task 5: `mudd` e2e — transcript shape and bold title

End-to-end proof over a real socket: greeting merged and terminated, creation/entry coalesced, look reply blank-line-prefixed with a bold title. Also strengthens the M1-26 ANSI assertion (any-escape → bold-wrapped title), making the documented bold-title behavior test-backed.

**Files:**
- Modify: `crates/mudd/tests/telnet_login.rs:18-72`

**Interfaces:**
- Consumes: the wire behavior from Task 4 and coalescing from Task 3. Fixture facts (from `tests/common/mod.rs`): banner `Welcome to Testville.`, room title `Town Square`, description `A test square.`.
- Produces: nothing further; terminal task for code.

- [ ] **Step 1: Extend the e2e assertions (they fail only if Tasks 3–4 misbehave, so write-and-run)**

In `crates/mudd/tests/telnet_login.rs`, `login_and_enter_world`, replace the first read (line 19):

```rust
    let greeting = client.read_until(b"lists commands.").await;
    // §2.8.2 line discipline: banner and instructions arrive as one
    // CRLF-terminated block, no longer glued into a single line.
    let merged_greeting = b"Welcome to Testville.\r\nType 'login";
    assert!(
        greeting
            .windows(merged_greeting.len())
            .any(|w| w == merged_greeting),
        "banner and instructions must share one terminated block, got {greeting:?}"
    );
```

Replace the creation/entry reads (lines 42–47):

```rust
    client.write_line("new Hero").await;
    let entered = client
        .read_until(b"Welcome. You are now in the world.")
        .await;
    // One input line, one block: creation confirmation and world entry
    // coalesce (design §Architecture 2).
    let merged_entry = b"Created Hero.\r\nWelcome. You are now in the world.\r\n";
    assert!(
        entered.windows(merged_entry.len()).any(|w| w == merged_entry),
        "creation and entry must share one block, got {entered:?}"
    );
```

In `a_full_register_create_enter_flow_over_telnet`, replace the look-reply assertion (lines 64–71):

```rust
    client.write_line("look").await;
    let look_reply = client.read_until(b"A test square.").await;
    // Blank line before the block, bold SGR around the title (M1-26 render +
    // §2.8.2 framing), then the description on its own line.
    let framed_title = b"\r\n\x1b[1mTown Square\x1b[0m\r\nA test square.";
    assert!(
        look_reply
            .windows(framed_title.len())
            .any(|w| w == framed_title),
        "look must open with a blank line and a bold title, got {look_reply:?}"
    );
```

- [ ] **Step 2: Run the mudd e2e suite**

Run: `cargo test -p mudd`
Expected: PASS — including `presence.rs` and `login_masks_the_password_like_registration`, whose needle-based reads are framing-agnostic. If a needle no longer matches, the fix is in Tasks 3–4, not in loosening the assertion.

- [ ] **Step 3: Verify workspace is green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
jj commit -m "test(mudd): e2e transcript shape — merged blocks, CRLF framing, bold title"
```

---

### Task 6: SPEC, PLAN, docs site, journal

Bookkeeping in the governing documents, in the same PR per project rules.

**Files:**
- Modify: `SPEC.md:987` (the `EOR / GA` bullet in §2.8.2)
- Modify: `PLAN.md` (append M1-28 after the M1-27 entry, ~line 713)
- Modify: `docs/docs/architecture/rendering.md` (new short section)
- Modify: `.claude/JOURNAL.md` (append entry)

**Interfaces:**
- Consumes: the shipped behavior from Tasks 1–5 (documents current state only).
- Produces: nothing; final task.

- [ ] **Step 1: SPEC.md — make line discipline normative**

Replace the single bullet line in §2.8.2:

```markdown
  - **EOR / GA** — prompt framing.
```

with:

```markdown
  - **EOR / GA** — prompt framing. Every output block MUST be followed
    by one EOR/GA prompt frame. The Gateway MUST precede every output
    block with a blank line, MUST terminate a completed message block
    with CRLF, and MUST leave an input-prompt block (e.g. `Password:`)
    unterminated so the cursor rests on the prompt line.
```

- [ ] **Step 2: PLAN.md — record M1-28**

Append after the M1-27 entry (before the `---` that closes the M1 section):

```markdown
- **M1-28 — Telnet line discipline: block termination and prompt framing.**
  Typed `OutputKind` (`Line`/`Prompt`) on `SessionOutput`; the engine
  classifies (only `PasswordPrompt`/`ConfirmPrompt` are prompts) and
  coalesces one input line's outputs into one block; the gateway owns
  framing — blank line before every block, CRLF termination for lines,
  prompts left unterminated before EOR/GA. `SessionMessage::Prompt`
  renamed `LoginInstructions`.
  - *Spec:* §2.8.2 (EOR/GA line discipline); design
    `docs/superpowers/specs/2026-07-16-telnet-line-discipline-design.md`.
    *Verify:* gateway framing unit tests; `mudd` e2e transcript-shape and
    bold-title assertions; workspace tests and clippy green.
```

- [ ] **Step 3: docs site — document the framing**

In `docs/docs/architecture/rendering.md`, after the "Delivery to the terminal" section's closing paragraph (the one about ANSI escapes surviving the legacy-charset path) and before the mermaid flowchart, insert:

```markdown
## Line discipline — live

The gateway owns how a block meets the socket: every output block is
preceded by a blank line, a completed message block is terminated with
CRLF, and a prompt block (`Password:`) is left unterminated so the cursor
rests on the prompt line. Every block is followed by one EOR/GA prompt
frame. Which treatment a block gets rides the wire as `OutputKind`
(`Line` / `Prompt`) on `SessionOutput` in `mud-schema`; the engine
classifies at the source (only the password and confirm prompts are
prompts) and coalesces the output of one input line into a single block.
```

Verify: from `docs/`, run `uv run mkdocs build --strict`
Expected: build succeeds with no warnings.

- [ ] **Step 4: Journal entry**

Append to `.claude/JOURNAL.md`:

```markdown
## 2026-07-16 — Telnet line discipline (M1-28)

- **Spec:** §2.8.2 (EOR/GA line discipline, made normative this PR) — blocks
  blank-line-prefixed; messages CRLF-terminated; prompts unterminated.
- **Done:** `OutputKind` (`Line`/`Prompt`) on `SessionOutput`; engine
  classifies + coalesces one input's outputs into one block; gateway owns
  framing; `SessionMessage::Prompt` → `LoginInstructions`; bold-title e2e
  assertion added (behavior already worked, now pinned).
- **Verify:** gateway framing unit tests; `mudd` e2e transcript-shape +
  bold-title assertions; `cargo test --workspace` + clippy green;
  `uv run mkdocs build --strict` green.
- **Next:** in-world command prompt (`> `) has no emitter yet; `OutputKind::
  Prompt` is ready for it. Tier negotiation (TTYPE) still M3.
```

- [ ] **Step 5: Final verification and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no warnings.

```bash
jj commit -m "docs: normative line discipline in SPEC §2.8.2, PLAN M1-28, rendering page, journal"
```
