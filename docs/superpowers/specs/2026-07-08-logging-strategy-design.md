# Logging & Observability Strategy — Design

**Date:** 2026-07-08
**Status:** Approved (design); implementation pending (see PR sequence §7)
**Spec basis:** SPEC §1.6, §3.9, §3.11.2, §3.14.4.3, §3.20.2.2
**Plan basis:** PLAN.md → Cross-cutting tracks → Observability

## 1. Motivation and scope

An audit of all 13 workspace crates found that logging is **nearly absent**
(~31 `tracing` call sites total, concentrated in `mudd` and `mud-engine`),
not inconsistent. The *sensitive-data posture is already strong*: no
credential or PII leak exists anywhere — passwords live in
`secrecy::SecretString`, `Credential` has a redacted `Debug`, the backend
logs infra errors rather than domain auth results, and raw bytes/payloads are
never logged.

This document defines the **workspace-wide logging strategy** — level
semantics, structured-field vocabulary, span taxonomy, never-log rules, and
subscriber configuration — and specifies the instrumentation work to bring the
already-landed M1 boundary code up to that standard.

`tracing` is the sole sanctioned output path (the workspace denies
`print_stdout`/`print_stderr`).

### In scope

- Conventions (levels, fields, spans, redaction) applying to every crate.
- Subscriber configuration in `mudd` (text + JSON, span-field emission).
- `tracing` span/event instrumentation for code that exists **today**.

### Out of scope — deferred to their milestones

Observability is a *cross-cutting track* in PLAN.md: "each PR rides with the
milestone that first needs it." The following ride later milestones and are
only referenced here so their conventions are reserved:

- Prometheus `/metrics` and `world_up` — **M7-A/B**.
- Live admin dashboard, command-log stream, LLM call inspector — **M7-C**.
- Input **replay journal** (§3.9.4) — **M7**.
- LLM call spans (§3.1.10) — **M6**.
- The **security audit trail** (admin auth §3.15.x, journaled moderation
  §3.15.5) — **M7**. This is a *distinct, dedicated concern*, not the
  operational log; it must not be conflated with the `info` level below.

## 2. Architectural principle: log at boundaries, stay silent in the core

The workspace sorts into two layers, and this is the governing rule:

- **Domain / pure / sans-IO crates emit no logs.** They surface outcomes as
  typed values (`Result`, `TickEvent`, `EffectResult`, `ParseOutcome`) and
  take **no `tracing` dependency**. These are: `mud-core`, `mud-cmd`,
  `mud-account`, `mud-session`, `mud-schema`.
- **Boundary / orchestration crates instrument.** They own the task /
  session / request boundaries where context exists: `mudd`, `mud-gateway`,
  `mud-ipc`, `mud-db`, `mud-engine`, `mud-i18n`.

`mud-net` and `mud-world` are exceptions to the "no `tracing`" rule above:
each already carries the SPEC-mandated builder `warn` for broken content
(§3.20.2.2 — an unknown style role in `mud-net`'s renderer, degraded markup in
`mud-world`), which fits the `warn` category in §3. That builder warning is
their *only* log; operational outcomes still surface as typed values, and
`mud-world`'s load *outcome* (the "world loaded, listening" readiness line) is
emitted by `mudd`, which owns tenant boot. So neither gains lifecycle
instrumentation of its own or a dedicated PR below.

**No new `mud-obs` crate** (YAGNI). Conventions live in this doc and a
`docs/` operator page; subscriber setup stays in `mudd`. Secrets are already
handled by `secrecy` and redacted `Debug` impls — no shared redaction helper
is warranted.

Rationale: because callers open spans (§4), a pure crate emits nothing yet any
event it *did* emit would inherit full context ambiently. Logging never has to
be threaded through domain signatures.

## 3. Level semantics

Two razors govern level choice:

- **`info` is the minimum "system is working" heartbeat** — not per-event.
- **`warn` is only for something a human should act on that is not fatal.**
  Under this razor, operational conditions are binary: `error` if broken,
  `debug`/`trace` if not. What remains at `warn` is one coherent category —
  **builder/content faults that degrade gracefully** — which is exactly the
  set SPEC/PLAN already mandate as `tracing` warnings.

| Level | Meaning | The complete list of what belongs here |
|---|---|---|
| `error` | **Operator** must act; infra/system broken | DB/IPC failure; fatal tenant-task exit; corrupt stored credential; migration failure |
| `warn` | **Builder/content** is broken but degrades gracefully | missing i18n key (§3.14.4.3); degraded markup tag (§3.20.2.2); unknown style role — *and essentially nothing else* |
| `info` | System-health heartbeat, boot/shutdown only | server started (tenant count + config summary); per-tenant "world loaded, listening" readiness line; migrations actually applied; graceful shutdown complete |
| `debug` | Per-request operational diagnostics | auth success/failure; session connect/disconnect; command dispatched; routing decision; peer IP at accept; IPC handshake outcome |
| `trace` | Hot-loop firehose | per-tick span; per-effect precondition-failed / rejected; per-frame |

**Key rule:** `warn` means "the game *builder* has a content bug," never "the
*operator* has an ops problem." At `info` a healthy server is nearly silent —
a few lines per tenant at boot, then nothing until something is genuinely
wrong.

### Reclassifications required in existing code

- `mudd` `tick precondition failed` / `tick effect rejected`: **warn → trace**
  (routine gameplay; currently an unbounded warn flood vector).
- `mudd` `entity created`: **info → debug**.
- Anything currently tentatively warn-worthy (oversized frame, unknown-session
  frame, rate-limit drop): **debug**. Sustained-abuse escalation does not
  exist yet, so no warn today.

## 4. Span taxonomy

Span-at-boundary, events-inherit. Three nested spans, each opened by the crate
that owns that boundary:

```
tenant span    (mudd: world_loop::run, per tenant)   fields: tenant, world_id
└─ session span   (gateway conn task / mudd)          fields: session_id [+ account_id once bound]
   └─ command span   (mud-engine — already exists)     fields: command_id, command
```

Because events inherit ancestor span fields, opening these three spans means
**every** log line carries `tenant` / `world_id` / `session_id` automatically,
with no per-call-site plumbing. This closes the "no tenant context anywhere"
gap (SPEC §3.11.2: labels must be tenant-aware) and is the cleanest fix for the
i18n violation (§6).

Spans are opened via `#[tracing::instrument]` or a manual `span!` at the task
boundary; prefer `#[instrument(skip(...))]` with explicit `fields(...)` over
capturing whole arguments (avoids accidentally Debug-dumping secrets).

## 5. Structured fields and formatting convention

Canonical snake_case field names, used identically across crates (stops the
observed `session` vs `session_id` drift):

`tenant`, `world_id`, `session_id`, `command_id`, `command` (canonical verb),
`account_id`, `entity`, `place`, `error`.

**Interpolation rule:** `%` (Display) for IDs and errors; `?` (Debug)
**reserved** for opaque diagnostic structs. Never `?` a domain enum that can
embed player text (e.g. `?effect`, `?frame`) — see §6.

**Message style:** lowercase, no trailing punctuation, terse noun/verb phrase
(`"tenant listening"`, `"authenticate failed"`). This matches the existing
`mudd`/`mud-engine` baseline.

## 6. Never-log rules (redaction)

Hard constraints. These make the existing good posture explicit and durable:

- **Never log:** passwords, password hashes, email, session tokens, raw input
  lines, raw payload bytes.
- **Never `?`-dump `#[non_exhaustive]` frames/effects** (`?frame`, `?effect`).
  Today their variants are payload-free, but a future variant *will* silently
  start leaking player chat. Log a discriminant/variant name instead.
- **Client IP policy:** never at `info`; log the peer IP **once at
  connection-accept at `debug`**, keyed by `session_id`. Ops can correlate on
  demand; default (`info`) deployments never persist PII.
- Passwords remain wrapped in `SecretString`; secret-bearing structs keep a
  redacted `Debug` (the `Credential` model).

### Known landmines (documented, fixed opportunistically — not in this track)

- `mud-account` `Credential::verify` returns bare `bool`, collapsing "wrong
  password" and "corrupt stored hash," so credential-row corruption is
  unobservable end-to-end. Surfacing it needs an API change (a `Mismatch` vs
  `Corrupt` result) — noted for a future account-hardening PR, not here.

## 7. Subscriber configuration (`mudd/main.rs`)

Current: `fmt()` with `RUST_LOG`/`info` default, text-only, no span-field
emission. Changes:

- **Add JSON opt-in:** process-level env var `FERRODUN_LOG_FORMAT=text|json`
  (default `text`). Must be a process-level env/CLI knob, not per-tenant
  config — the subscriber is process-global and initializes before any tenant
  loads. Text for dev, JSON for aggregators in prod.
- **Emit span fields:** enable current-span context in the formatter
  (`.with_current_span(true)` for JSON; span context in the fmt layer).
  Without this, the inherited tenant/session fields never reach output and the
  span taxonomy (§4) is invisible.
- Keep `RUST_LOG` override and the `info` default.

## 8. Instrumentation work — PR sequence

Per PLAN's "small, independently reviewable PR" rule, this is a sequence, not
one change. Conventions land first, then the span backbone, then leaf crates.

| PR | Scope | Depends on |
|---|---|---|
| **L0** | Conventions doc under `docs/docs/` + subscriber config (JSON, span fields) in `mudd` | — |
| **L1** | Tenant + session spans in `mudd`/`mud-gateway`; reclassify tick events → trace, entity-created → debug; standardize `session` → `session_id` | L0 |
| **L2** | `mud-i18n` missing-key inherits `tenant` from ambient span (verify §3.14.4.3 compliance); add a dedup/rate guard for the hot-path flood | L1 |
| **L3** | `mud-db` — add `tracing` dep; migrations-applied (info), connect/boot-load lifecycle, query errors (error) with operation name and non-sensitive ids only — never bound values | L1 |
| **L4** | `mud-ipc` handshake success/failure + framing/decode errors (frame type + length, never payload); `mud-gateway` connect/close (info→debug per §3), peer-IP-at-debug | L1 |
| **L5** | `mud-engine` — session span + auth outcomes at `debug` (keyed by `session_id`, no credentials) | L1 |

Each PR is independently testable: assert emitted events with `tracing-test`
(already a dev-dependency in `mud-engine`, `mud-i18n`). Every PR that changes
operator-observable behavior updates the `docs/` operator page in the same PR
(CLAUDE.md → Documentation site).

### i18n compliance note (§3.14.4.3)

The missing-key warning currently records `key` + `locale` but omits the
mandated `tenant`. The fix is **not** a `translate` signature change: because
`translate` runs inside the ambient tenant span (§4), the warn event inherits
`tenant`, and the subscriber (§7) emits span fields on the event. This
satisfies "record key, locale, and tenant" without touching the i18n call
sites. L2 must include a test asserting all three fields are present on the
emitted event.

## 9. Verification

- `cargo clippy --workspace --all-targets` clean (no new suppressions).
- `cargo test --workspace` green; each L-PR adds `tracing-test` assertions for
  the events it introduces.
- Manual: run `mudd` at default `info` and confirm a healthy server is nearly
  silent (boot/readiness lines only); run at `RUST_LOG=debug` and confirm
  per-session/per-command diagnostics carry `tenant`/`session_id`; run with
  `FERRODUN_LOG_FORMAT=json` and confirm span fields appear in the JSON.
