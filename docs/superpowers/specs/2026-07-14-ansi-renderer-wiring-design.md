# ANSI Renderer Wiring — Design

**Date:** 2026-07-14
**Status:** Approved (design); implementation pending (PLAN.md → M1-26, new)
**Spec basis:** SPEC §3.20.1 (styled text), §3.20.1.2 (escape generation only in
the per-session telnet renderer), §3.20.5 (capability tiers), §2.8.5.7
(version-locked IPC schema)
**Plan basis:** PLAN.md → Networking and integration → M1-26 (new, before the
M1-23 acceptance gate)

## 1. Motivation and scope

The full color stack exists but is severed at the last hop. Handlers emit
semantic roles (`say` → `SAY`, movement/look → `SYSTEM`), room titles and
descriptions compile builder markup through the tenant palette at load, and
`mud-net` ships a snapshot-tested per-session ANSI renderer
(`render(&StyledText, &Palette, Tier)`) with deterministic downsampling. But
the engine pipeline flattens every reply and broadcast via
`StyledText::to_plain_string()` (`crates/mud-engine/src/pipeline.rs:215,217`),
so `render()` and `resolve_tier()` have no non-test caller and players see
plain text.

The IPC schema anticipated this exact change: `mud-schema`'s `OutputText` doc
reserves "swapping this payload to carry styled text across the IPC boundary —
rendered to ANSI per session on the Gateway side", deferred to the PR that
wires the renderer into the session pipeline. That deferral pointed at
M1-21/M1-22, which shipped without it — this design closes the gap as a new
PLAN entry, **M1-26**.

**In scope:** styled text across IPC; gateway-side per-session rendering at a
fixed `ansi16` tier; the ASCII-charset transliteration fix so escapes survive
for legacy clients; PLAN/docs bookkeeping.

**Out of scope (tracked elsewhere in PLAN.md):** the M1-23 acceptance
integration test (separate follow-up PR, depends on this one); TTYPE/MTTS tier
detection and xterm256/truecolor sessions (§3.20.5.2 step 3 → M3, cf.
PLAN.md M1-13 deferral note and M3-C); webclient semantic spans (→ M3);
per-account color preferences (§3.20.6.1 → M7); colorblind palette
(§3.20.6.3 → 1.0); palette hot-reload (§3.20.3.3 → M2-H); role-styling the
login/session-FSM prompts (emission-site styling policy stays as M1-17 left
it; login output crosses as unstyled spans).

## 2. Where rendering happens

Gateway-side, in the connection actor. SPEC §3.20.1.2 mandates that internal
pipelines carry no raw escape sequences and that escape generation happens
only in the per-session telnet renderer; the engine→gateway IPC channel is an
internal pipeline. Rendering engine-side would be a smaller diff (no schema
change) but is non-compliant and forecloses M3's per-session tiers and the
webclient's semantic-span path, both of which need styled text to survive to
the edge.

```mermaid
flowchart LR
    H[Handlers / room content<br/>StyledText with roles] --> P[Pipeline<br/>passes StyledText through]
    P -->|"WorldFrame::Output<br/>(styled, schema v3)"| G[Gateway router]
    G --> C[Connection actor<br/>mud_net::render + palette + tier]
    C -->|SGR bytes| T[Telnet client]
```

## 3. Component changes

### 3.1 `mud-core` — serde on the text model

Plain serde derives (no feature gate) on `StyledText`, `Span`, `SpanStyle`,
`Style`, `Color`, `Attributes`, `RoleName`. Every current consumer sits in one
workspace that already compiles serde via `mud-schema`, so a feature gate buys
nothing today and can be introduced later without breaking anyone. A
schema-local mirror of the text model was rejected as speculative decoupling
(~6 duplicated types plus two conversion layers); revisit if M3-D wire-protocol
codegen forces a wire-owned representation.

### 3.2 `mud-schema` — payload swap, version bump

- Gains a `mud-core` dependency (no cycle: `mud-core` depends on nothing
  internal).
- `OutputText` stays as the marker newtype ("text destined for presentation")
  but wraps `StyledText` instead of `String`. Producers of plain text (login
  prompts, banner) wrap via `StyledText::from(...)`.
- `SCHEMA_VERSION` bumps 2 → 3 — free, because Gateway and World are
  version-locked and rebuild together (§2.8.5.7), exactly the escape hatch the
  schema comment planned for.
- The stale "deferred to M1-21/M1-22" doc comments update to point at M1-26.

### 3.3 `mud-engine` — stop flattening

`pipeline.rs` passes `reply.output()` and each broadcast's `StyledText`
through unchanged; the two `to_plain_string()` calls disappear. The
login/session-FSM path wraps its plain strings as unstyled `StyledText`.

### 3.4 `mud-gateway` — render per session

- `GatewayConfig` gains the tenant palette (`Arc<Palette>`, cloned per
  connection) and the session `Tier`, resolved once at boot via the existing
  `resolve_tier(false, DEFAULT_TENANT_TIER)` → `Ansi16`. M3 replaces that
  single call site with real TTYPE/MTTS negotiation; until then every session
  renders at the spec's maximum-compatibility default (§3.20.5.2 step 4).
- The connection actor's `ToConnection::Output` arm becomes:
  `mud_net::render(&styled, &palette, tier)` → `machine.encode_output(...)` →
  socket. Prompt framing (EOR/GA) is unchanged.

### 3.5 `mudd` — boot plumbing

`boot()` hands `loaded.palette().clone()` into `GatewayConfig`; palette and
gateway already meet there, no new plumbing. Palette hot-reload stays M2-H.

### 3.6 `mud-net` — shield escapes from ASCII transliteration

Charset and color capability are orthogonal: a legacy non-UTF-8 MUD client
very likely supports ANSI, so escapes must not be mangled for it. Today
`encode_output`'s `CharsetMode::Ascii` arm runs `deunicode` over the whole
line, and deunicode drops control bytes — it would eat ESC and leave `[97m`
as visible garbage. Fix inside that arm: split the rendered string into ANSI
escape sequences (ESC `[` … final byte `@`–`~`) and plain-text segments,
transliterate only the plain segments, pass escape segments through untouched.
The UTF-8 arm and the byte-level loop (LF→CRLF, IAC doubling) already pass ESC
through correctly and are unchanged.

## 4. Error handling

No new error surface. Unknown palette roles already degrade to unstyled with a
`tracing::warn` inside `render()` (§3.20.2.2); the renderer now runs in the
gateway, a boundary crate where that logging is legitimate under the logging
strategy (`2026-07-08-logging-strategy-design.md`). Serde derives introduce no
fallible paths; a frame that fails to decode remains a fatal IPC error under
the version-locked schema, unchanged.

## 5. Testing

- **`mud-net`:** unit tests for escape shielding under ASCII transliteration —
  ANSI survives, accents transliterate (`café` + styled span fixture).
- **`mud-schema`:** round-trip encode/decode of a styled
  `WorldFrame::Output`, mirroring the existing frame tests.
- **`mud-gateway` loopback:** a styled output frame reaches the fake client
  carrying the expected ansi16 SGR bytes (e.g. `\x1b[97m` from the `say`
  role) — the wiring's own end-to-end assertion, and the piece the M1-23
  acceptance test later leans on for its "assert ANSI" clause.
- **`mud-engine`:** pipeline tests asserting plain strings update to assert
  styled payloads (or their plain projection).
- Full workspace tests and clippy green.

## 6. PLAN and docs bookkeeping

- **PLAN.md:** add **M1-26 — ANSI renderer wiring** before the still-open
  M1-23 gate entry, noting M1-23 depends on it; fix the stale
  "M1-21/M1-22" wiring pointer in the M1-13 deferral note (PLAN.md:426–427).
- **Docs site:** three pages document the flattened state and need updating in
  the same PR: `architecture/rendering.md` ("sessions receive plain text"),
  `architecture/engine.md` ("today's render step flattens styled output"), and
  the `building/styling.md` admonition telling builders styling is not yet
  visible.
- **Journal:** one entry per house rules.
