# M1-20 — `mud-net` telnet core — Design

**Date:** 2026-07-02
**Spec:** SPEC §2.8.2 (M1 protocol subset), §2.1.1 (per-session command rate limit)
**Plan:** PLAN.md M1-20

## Goal

Add the telnet protocol core to the existing `mud-net` crate: IAC parsing and
option negotiation for the M1 subset (NAWS, TTYPE, CHARSET/UTF-8 with legacy
transliteration fallback, EOR/GA prompt framing), an incremental line decoder,
and the per-session command rate limiter (leaky bucket, 10/s sustained, burst
20). No sockets, no listener, no async — that is M1-21 (`mud-gateway`).

## Decisions

1. **Hand-rolled IAC machinery.** No third-party telnet crate
   (`libtelnet-rs`, `nectar`): they are thin, unevenly maintained, would leak
   third-party types through the public API, and M3 (MCCP2/GMCP/MSDP/MXP/MSSP)
   would outgrow them.
2. **Sans-IO.** `mud-net` stays free of tokio. The telnet core is a pure
   state machine — bytes in, typed events plus reply bytes out. M1-21's
   gateway drives it from a tokio stream. This matches `mud-net`'s existing
   character (the M1-13 renderer is a pure styled-text→ANSI compiler) and
   makes every negotiation edge case unit-testable by feeding byte slices.
3. **`deunicode` for transliteration.** When the client does not accept
   UTF-8 via CHARSET, server output is transliterated to ASCII with the
   `deunicode` crate (pure Rust, maintained, no third-party types exposed).

### Rejected alternatives

- **tokio-util `Decoder`/`Encoder` codec:** drags tokio into `mud-net`,
  entangles protocol logic with framing, forces async plumbing into
  negotiation tests.
- **Existing telnet crate:** see decision 1.

## Public surface

All in `mud-net`; raw primitives do not cross the API where a domain meaning
exists.

- **`TelnetMachine`** — per-connection protocol state machine.
  - `fn receive(&mut self, bytes: &[u8]) -> Vec<TelnetEvent>` — consume raw
    socket bytes; negotiation replies the server must send are queued
    internally and drained via `take_output`. State survives across calls
    (split packets are fine).
  - `fn take_output(&mut self) -> Vec<u8>` — bytes to write to the client
    (negotiation replies).
  - `fn encode_output(&mut self, text: &str) -> Vec<u8>` — encode
    server→client text: UTF-8 passthrough or ASCII transliteration
    (per negotiated CHARSET), IAC escaping, LF→CRLF normalization.
  - `fn prompt_frame(&mut self) -> Vec<u8>` — `IAC EOR` if EOR was
    negotiated, else `IAC GA`.
- **`TelnetEvent`** — `#[non_exhaustive]` enum of validated, decoded values:
  `Line(String)`, `WindowSize { width, height }` (NAWS),
  `TerminalType(String)` (TTYPE). Additional variants only as needed.
- **`RateLimiter`** — leaky bucket. Constructor takes tenant-configurable
  newtyped params (`SustainedRate`, default 10 commands/s; `Burst`,
  default 20). Time is injected: `fn check(&mut self, now: Instant) ->
  Decision` where `Decision` is `Forward | Drop`. On `Drop` the gateway
  (M1-21) sends the structured `rate_limited` event and does not forward to
  World (§2.1.1). This PR ships the component; enforcement wiring is M1-21.

## Internal structure

Modules (indicative): `telnet/parser.rs`, `telnet/negotiation.rs`,
`telnet/line.rs`, `ratelimit.rs`.

1. **IAC parser** — incremental framing of data bytes / commands / option
   negotiation (`IAC WILL/WONT/DO/DONT opt`) / subnegotiation
   (`IAC SB … IAC SE`), with `IAC IAC` unescaping. Tolerant of arbitrary
   packet splits. Subnegotiation buffers are size-capped (untrusted input).
2. **Option negotiation** — RFC 1143 Q-method state per option to prevent
   negotiation loops. Options handled:
   - **NAWS** (RFC 1073): server sends `DO NAWS`; subnegotiation delivers
     `WindowSize`.
   - **TTYPE** (RFC 1091): server sends `DO TTYPE` and the `SEND`
     subnegotiation; delivers `TerminalType`.
   - **CHARSET** (RFC 2066): server offers UTF-8; acceptance switches output
     encoding to UTF-8 passthrough, refusal keeps ASCII transliteration.
   - **EOR** (RFC 885): server sends `WILL EOR`; if refused, prompt framing
     falls back to `IAC GA`.
   - **SGA/ECHO**: answered with correct refusals (no server echo in M1).
   - Unknown options are refused per RFC (`DONT`/`WONT`), never ignored.
3. **Line decoder** — accumulates data bytes into lines, handling both
   `CR LF` and `CR NUL` line endings; decodes UTF-8 lossily (invalid
   sequences become U+FFFD); enforces a maximum line length: once the cap
   is exceeded the rest of the line is discarded and no `Line` event is
   emitted for it (dropping beats truncating — a half command must not be
   executed).
4. **Output encoder** — see `encode_output` above.

## Error handling

A `thiserror` error type for genuinely fatal protocol violations only.
Malformed-but-survivable input degrades gracefully — a telnet server must
tolerate garbage. No `unwrap`/`expect`/`panic` outside tests.

## Out of scope

MCCP2, GMCP, MSDP, MXP, MSSP (M3); TLS/SSH/WebSocket (M3); sockets and the
telnet listener (M1-21); `Core.Hello`/`Core.Welcome` handshake (needs GMCP,
M3); linkdead/idle/ping (M7).

## Testing (TDD)

Byte-level unit tests, no sockets:

- Negotiation: NAWS delivers `WindowSize`; TTYPE round-trip; CHARSET accept
  and refuse paths; EOR refusal → GA fallback; unknown option refusal;
  Q-method loop prevention.
- Parser: `IAC IAC` escaping both directions; split-packet resumption
  mid-command and mid-subnegotiation; oversized subnegotiation capped.
- Line decoder: `CR LF`, `CR NUL`, oversize line dropped (no `Line` event),
  lossy UTF-8.
- Output: transliteration fallback; LF→CRLF; IAC escaping; prompt framing
  under EOR vs GA.
- Rate limiter: sustained-rate and burst behavior with an injected clock;
  the drop test required by PLAN's Definition of Done.

## Documentation

No player/builder/operator-visible surface lands in this PR (the telnet
listener arrives in M1-21), so no `docs/docs/` page changes are required.
