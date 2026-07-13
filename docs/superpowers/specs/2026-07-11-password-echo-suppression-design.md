# Password Echo Suppression — Design

**Date:** 2026-07-11
**Status:** Approved (design); implementation pending (PLAN.md → M1-25)
**Spec basis:** SPEC §2.8.2 (telnet option subset), §2.1.3 (IPC frames), §3.19.1 (login flow)
**Plan basis:** PLAN.md → Networking and integration → M1-25 (new)

## 1. Motivation and scope

Passwords are visible as typed during login and registration. The telnet
`Negotiator` (`crates/mud-net/src/telnet/negotiation.rs`) refuses `DO ECHO`
with `WONT` and never offers `WILL ECHO`, so the client's local echo prints
the password at the password prompt. Code comments deferred masking to
"M1-20", but M1-20 shipped without it and no later PLAN PR schedules it — the
gap fell through. It is documented as a current limitation in
`docs/docs/playing/getting-started.md`.

The fix is the standard telnet mechanism (RFC 857): the server sends
`IAC WILL ECHO` before the password prompt — a compliant client stops local
echo — and `IAC WONT ECHO` once the password line is received.

**Scope (decided):** minimal, telnet-only. One typed echo-on/off signal flows
FSM → IPC → telnet `WILL`/`WONT ECHO`. No general "input mode" abstraction
until a second transport exists (M3). Clients that refuse (`DONT ECHO`) or
ignore the offer keep visible passwords: no warning message, no waiting for
the reply; the docs limitation note shrinks to cover only them.

## 2. Architecture and data flow

One new concept, an echo mode with two states (*enabled* / *suppressed*),
flows down the existing ordered pipeline. The FSM is the sole authority on
when echo changes; every layer below relays it without interpretation:

```
mud-session   FSM derives the echo change from (state before, state after)
     │        Transition { messages, effect, terminal, echo: Option<InputEcho> }
     ▼
mud-engine    SessionService::drive places the echo change BEFORE the same
     │        transition's rendered messages in the ordered output vec
     │        Routing::Login { outputs: Vec<LoginOutput>, close }
     ▼
mud-schema    WorldFrame::Echo(SessionEcho { session_id, mode }) — new frame
     │        variant; SCHEMA_VERSION bump (version-locked, both sides
     │        rebuilt together, §2.8.5.7)
     ▼
mud-gateway   router: WorldFrame::Echo → ToConnection::Echo(mode) → the
     │        session's connection task
     ▼
mud-net       TelnetMachine/Negotiator: Suppressed → IAC WILL ECHO,
              Enabled → IAC WONT ECHO (RFC 857, RFC 1143 Q-method)
```

Ordering needs no new mechanism: the IPC endpoint and each per-connection
mpsc are FIFO, so `WILL ECHO` reaches the socket before the `Password:`
prompt bytes of the same transition.

**Derive, don't hand-set.** The FSM computes the echo change once per step
from the state pair instead of setting it on each individual transition:
entering the password-state set (`LoginPassword`, `RegisterPassword`,
`RegisterConfirm`) ⇒ `Some(Suppressed)`; leaving it ⇒ `Some(Enabled)`;
otherwise `None` (no change). A future password-collecting state gets masking
automatically, and a forgotten transition is unrepresentable.

### Alternatives considered

- **Engine infers from `SessionMessage`** (`PasswordPrompt`/`ConfirmPrompt`
  ⇒ off): smaller diff, but duplicates FSM knowledge in the renderer — a new
  password-ish message would silently ship unmasked. Rejected.
- **Gateway-side prompt sniffing** (pattern-match rendered text): couples the
  gateway to localized, tenant-authored strings. Rejected outright.

## 3. Per-crate changes

- **`mud-session`** — new `InputEcho { Enabled, Suppressed }` enum;
  `Transition.echo: Option<InputEcho>` derived from the state pair as above.
  Fix the stale doc comment on `SessionMessage::PasswordPrompt` ("echo
  masking is M1-20" → reference this design / M1-25).
- **`mud-schema`** — wire types `EchoMode { Enabled, Suppressed }`,
  `SessionEcho { session_id: SessionId, mode: EchoMode }`, and
  `WorldFrame::Echo(SessionEcho)`; bump `SCHEMA_VERSION`; postcard codec
  round-trip test. (`mud-session` stays free of `mud-schema`; the engine
  converts `InputEcho` → `EchoMode` at its boundary, mirroring the existing
  `SessionMessage` → `OutputText` split.)
- **`mud-engine`** — `Routing::Login.outputs` becomes `Vec<LoginOutput>`
  where `enum LoginOutput { Text(SessionOutput), Echo(SessionEcho) }`,
  preserving interleaving order through `drive()`'s effect loop (the echo
  item is pushed before the same transition's rendered messages).
- **`mudd`** (`world_loop.rs`) — map `LoginOutput::Text` →
  `WorldFrame::Output` (as today) and `LoginOutput::Echo` →
  `WorldFrame::Echo`.
- **`mud-gateway`** — router routes `WorldFrame::Echo` to a new
  `ToConnection::Echo(EchoMode)`; the connection task feeds it to the telnet
  machine and writes the resulting bytes. No prompt frame is appended for
  echo-only writes.
- **`mud-net`** — `Negotiator` gains a `us_echo` Q-method state; the `QState`
  enum gains `WantNo` (this is the first option we actively disable).
  `TelnetMachine::set_echo(mode)` queues `IAC WILL ECHO` / `IAC WONT ECHO`;
  client replies `DO ECHO` / `DONT ECHO` are handled per RFC 1143 instead of
  the current blanket-`WONT` refusal. A spontaneous client `DO ECHO` while
  `us_echo` is `No` is still refused with `WONT` — the server never echoes
  normal input.

## 4. Behavior details and edge cases

- **CRLF after the masked line.** While suppressed, a compliant client echoes
  nothing — not even Enter — so the next output would glue onto the
  `Password:` line. `TelnetMachine` queues `\r\n` when a line completes while
  echo is suppressed. Contained entirely in `mud-net`.
- **Registration.** Echo goes off at `register <name>`, stays off across the
  confirm prompt (password-state → password-state derives `None`), and comes
  back on when the confirm line is consumed — including the mismatch path.
- **Login failure.** Echo re-enables when the password line is consumed
  (transition into `AwaitingAuth`), before the auth result is known; the
  retry prompt is normal-echo.
- **Refusing/ignoring clients.** The prompt is sent immediately after
  `WILL ECHO` without waiting for the reply. Refusers keep visible passwords;
  no warning message.
- **Disconnect mid-password.** The connection is gone; nothing to restore.
  Fresh connections start `Enabled` (the telnet default) and no frame is sent
  until the first suppression.
- **Never-log discipline** is unchanged: the masked line is raw player input
  and is already never logged; the echo frames carry no payload beyond the
  session id and mode.

## 5. Testing

TDD per crate, then one end-to-end proof:

- **`mud-session`** — transition tests asserting the `echo` field across the
  full login, registration, mismatch-abort (a mismatch returns to the anon
  prompt, so echo must re-enable), and failed-login flows.
- **`mud-schema`** — postcard round-trip for `WorldFrame::Echo`.
- **`mud-net`** — negotiator unit tests: `WILL ECHO` emitted on suppress;
  `DO`/`DONT` replies per Q-method; `WONT ECHO` on re-enable; client refusal;
  spontaneous `DO ECHO` still refused; CRLF queued after a masked line.
- **`mud-gateway`** — connection test: `ToConnection::Echo` writes the IAC
  bytes, with no prompt frame appended.
- **End-to-end** — extend `crates/mudd/tests/telnet_login.rs`: assert
  `IAC WILL ECHO` appears before the password-prompt bytes and
  `IAC WONT ECHO` after the password line, for both login and registration.

## 6. Docs and PLAN

- Update `docs/docs/playing/getting-started.md` in the same PR: the
  limitation note shrinks to "clients that refuse echo suppression still
  display the password" (observable-behavior rule).
- Add **M1-25 — password echo suppression** to PLAN.md under Networking and
  integration, after M1-24, with this document as its design basis.
- Journal entry on completion.
