# Telnet line discipline: block termination and prompt framing — design

**Date:** 2026-07-16
**Status:** approved

## Problem

The engine renders every message without a line terminator, and the gateway
writes each output block as-is (charset-encoded, GA/EOR-framed) without adding
one. Consecutive blocks therefore glue together on the client, and the
player's typed input lands on the same line as the last output. Captured
bytes from a fresh session:

```
Welcome to probe.\xff\xf9Type 'login <name>' or 'register <name>'. ...\xff\xf9
...
Created Mimmo.\xff\xf9Welcome. You are now in the world.\xff\xf9
```

(`\xff\xf9` is the per-block IAC GA prompt frame.) Two distinct gaps:

1. **No message termination.** A finished message must end the line; today
   nothing does.
2. **No visual separation.** Even with termination, responses read densely
   against the player's echoed commands.

The classic MUD lineages (Diku/ROM/Circle; Evennia is identical in shape)
solved this decades ago: every *message* is a complete line the server
terminates with CRLF; the only unterminated output is a *prompt*
(`Password:`, `> `), sent without a newline and followed by GA/EOR so
prompt-aware clients know the cursor stays put. This design adopts that
model rather than inventing one.

## Decisions

- **Typed Line/Prompt distinction, not a string convention.** Whether a block
  is a complete message or an input prompt is encoded as a type on the wire
  (`OutputKind` on `SessionOutput`), per the type-driven-design mandate
  (SPEC §1.7). No trailing-`\n`-means-line conventions buried in strings;
  i18n catalog entries stay free of terminators.
- **The gateway owns line discipline.** It already owns CRLF encoding and
  the GA/EOR prompt frame; it now also owns block termination and spacing.
  Exactly one place decides how a block meets the socket.
- **Blank line before every block.** Uniform breathing room: each block is
  preceded by one blank line, separating it from the player's echoed command
  or the previous block. No solicited/unsolicited distinction.
- **One engine step, one block.** A batch of `SessionMessage`s produced by
  one engine step merges into a single `SessionOutput` (messages joined with
  `\n`), so the blank line separates *responses*, not individual sentences —
  and prompt-aware clients get one GA per step instead of several.
- **Prompts are exactly `PasswordPrompt` and `ConfirmPrompt`.** Everything
  else — banner, session FSM replies, command replies, presence
  announcements — is a `Line`.
- **`SessionMessage::Prompt` is renamed to `LoginInstructions`.** The
  variant carries the "Type 'login <name>'…" instruction line, which is a
  `Line`, not an `OutputKind::Prompt`; keeping the old name next to the new
  enum is a naming trap.
- **SPEC.md gains a normative paragraph** (§2.8.2 area): output blocks are
  preceded by a blank line; Line blocks are CRLF-terminated; Prompt blocks
  are unterminated and followed by GA/EOR.
- **Bold-title e2e assertion rides along (test-only).** Room titles already
  render bold end-to-end (verified on the wire: `\x1b[1mStart\x1b[0m`), but
  the existing e2e only asserts *some* ANSI escape in a `look` reply — the
  colored exits line alone satisfies it. One added assertion pins SGR bold
  on the title so the documented behavior is test-backed. No production
  change.

## Player-visible behavior

```
Welcome to test-tenant.
Type 'login <name>' or 'register <name>'. 'help' lists commands.

register Test

Password:████
Confirm password:████

You have no characters yet. Type 'new <name>' to create one.

new Mimmo

Created Mimmo.
Welcome. You are now in the world.

look

Start
A quiet starting room. Edit world/start/start.kdl to begin building.

Tromak says, "hello"
```

Password prompts keep the cursor on the same line (classic style); every
other message is a complete line; every block is preceded by a blank line.
`Created Mimmo.` and `Welcome. You are now in the world.` share one block
because one engine step produced them.

## Architecture

Changes span `mud-schema`, `mud-engine`, `mud-session` (rename only), and
`mud-gateway`. `mud-net` is untouched.

1. **`mud-schema`: `OutputKind` on the wire.**

   ```rust
   pub enum OutputKind {
       /// A complete message: the gateway terminates it with a line ending.
       Line,
       /// An input prompt: left unterminated so the cursor stays on the
       /// line; the GA/EOR frame tells clients where the prompt ends.
       Prompt,
   }

   pub struct SessionOutput {
       pub session_id: SessionId,
       pub text: OutputText,
       pub kind: OutputKind,
   }
   ```

   Plain two-variant enum, serde-derived, exhaustively matched. No default:
   both construction sites state the kind explicitly. Gateway and World are
   version-locked and rebuild together (§2.8.5.7), so the wire change is
   free.

2. **`mud-engine`: classification and batch merging.**
   - `render_outputs` merges a `Vec<SessionMessage>` batch into at most one
     `SessionOutput`: rendered strings joined with `\n`, `kind` taken from
     the **last** message in the batch. A batch ending in `PasswordPrompt` /
     `ConfirmPrompt` is a `Prompt` block (e.g. a hypothetical
     `[LoginFailed, PasswordPrompt]` becomes `"Login failed.\nPassword:"`,
     unterminated); every other batch is a `Line` block.
   - The command pipeline and `presence::announce` already emit one block
     per recipient per step; they state `kind: OutputKind::Line`.

3. **`mud-session`: rename `SessionMessage::Prompt` →
   `SessionMessage::LoginInstructions`.** Mechanical; callers in
   `mud-engine` follow.

4. **`mud-gateway`: framing in the `ToConnection::Output` arm.** The router
   passes `kind` through alongside the text. Per block the connection writes,
   in order:
   1. `\r\n` — the breathing-room blank line (with the client's echoed
      Enter this renders as exactly one blank line);
   2. the rendered, charset-encoded body (unchanged path:
      `mud_net::render` → `encode_output`, which already maps `\n`→`\r\n`);
   3. `\r\n` if `kind == Line`; nothing for `Prompt`;
   4. the existing GA/EOR prompt frame.

## Edge cases

- **First block on connect** is preceded by a blank line. Accepted: classic
  servers behave the same and it reads fine.
- **Echo negotiation frames** (`ToConnection::Echo`) remain bare telnet
  negotiation: no blank line, no terminator, no prompt frame — unchanged.
- **Charset safety:** the injected CRLF bytes are plain ASCII, valid under
  any negotiated charset; they bypass no `encode_output` invariant because
  the body is encoded separately.
- **Empty batch:** `render_outputs` of an empty batch produces no output at
  all (no empty block, no stray blank line).
- **Multi-line bodies** (room render, merged batches) keep internal `\n`
  separators; only the block boundary gets the blank line.

## Testing

TDD throughout (red → green → refactor):

- **`mud-schema`:** serde round-trip covering `OutputKind` on
  `SessionOutput`.
- **`mud-engine`:** `render_outputs` merge — connect batch
  `[Banner, LoginInstructions]` yields one `Line` block joined with `\n`; a
  batch ending in `PasswordPrompt` yields one `Prompt` block; classification
  test pinning exactly `PasswordPrompt`/`ConfirmPrompt` as prompts.
- **`mud-gateway`:** a `Line` block writes `\r\nhello\r\n` + GA; a `Prompt`
  block writes `\r\nPassword:` + GA with no trailing newline; echo frames
  stay bare.
- **`mudd` e2e:** existing `read_until` assertions stay green; add a
  transcript-shape assertion (banner and instructions no longer glued —
  `Welcome to Testville.\r\n` appears); add the bold-title assertion
  (`\x1b[1m` adjacent to the title text in the `look` reply).
- **SPEC.md:** normative line-discipline paragraph added under §2.8.2.
- **PLAN.md:** appended as **M1-28 — telnet line discipline**, with spec
  refs and verify clause.
- **Docs site:** update the Architecture page describing the gateway output
  path (`architecture/rendering.md`) where it documents framing; journal
  entry on completion.

## Out of scope

- An in-world command prompt line (`> `, vitals prompts) — nothing emits one
  in M1; the Prompt kind is ready for it when one arrives.
- NAWS-driven pagination, TTYPE tier detection, GMCP (M3).
- Any change to `mud-net` rendering, tiers, or the styled-text model.
- Solicited/unsolicited spacing distinctions — spacing is uniform by
  decision.
