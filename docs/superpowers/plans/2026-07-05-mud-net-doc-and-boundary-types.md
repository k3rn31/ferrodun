# mud-net Crate Doc & Boundary-Type Rationale Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reframe `mud-net`'s crate-level doc around its three current pillars (telnet transport core, per-session ANSI rendering, rate limiting) instead of just rendering, and record the deliberate decision to keep raw primitives in `TelnetEvent` at the sans-IO transport seam.

**Architecture:** `mud-net`'s module doc still opens "Per-session rendering of styled text to a terminal," a framing from M1-13 when rendering was the crate's only job. Since M1-20 the crate is the session/transport edge with three coequal responsibilities. This plan is documentation-only: it rewrites the crate summary and adds a short rationale note on `TelnetEvent`. No behavior changes.

**Tech Stack:** Rust 2024, `anstyle` / `anstyle-lossy` (rendering), sans-IO telnet, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- Doc comments describe behavior/purpose for callers, not internal implementation.
- Newtype pattern is mandatory *where a domain meaning crosses a public API* — but see the decision below for why the transport seam is deliberately excepted.
- Must compile clean under `cargo clippy -p mud-net --all-targets` and `cargo doc -p mud-net`.
- VCS is `jj`. Commit with `jj commit -m "..."`.

## Design decision (confirm at review)

- **`TelnetEvent` keeps `String`/`u16` primitives — no newtypes.** `TelnetEvent` is a sans-IO transport DTO; its consumer (the M1-21 gateway) is the boundary that parses these into domain values — a `Line(String)` becomes a `mud_schema::InputLine`, a `TerminalType` drives tier resolution, `WindowSize` feeds rendering width. Wrapping them *inside* `mud-net` would either duplicate domain types that live in higher crates (`mud-net` sits **below** `mud-schema`/`mud-cmd` in the dependency graph — it cannot import them without an inversion) or create single-use wrappers with no invariant to enforce (`width`/`height` are already named `u16` fields). The correct place to parse raw transport bytes into typed domain values is the boundary above, not the sans-IO core. We record this as a comment so a future reader doesn't "fix" it.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-net && cargo doc -p mud-net`
Expected: PASS; docs build.

---

### Task 1: Rewrite the crate doc and add the `TelnetEvent` rationale

**Files:**
- Modify: `crates/mud-net/src/lib.rs`, `crates/mud-net/src/telnet/mod.rs`

- [ ] **Step 1: Rewrite the crate-level doc in `lib.rs`**

Replace the module doc comment at the top of `crates/mud-net/src/lib.rs` (the current lines 1–12) with a three-pillar framing:

```rust
//! The session/transport edge: telnet protocol, per-session rendering, and rate
//! limiting (§2.8.2, §3.20.5, §2.1.1).
//!
//! `mud-net` sits between the raw socket and the engine, and does three things,
//! all sans-IO (it never owns a socket — the M1-21 gateway drives it):
//!
//! - **Telnet core** — [`TelnetMachine`] turns raw client bytes into typed
//!   [`TelnetEvent`]s and negotiation replies (the §2.8.2 M1 subset: NAWS,
//!   TTYPE, EOR, CHARSET).
//! - **Rendering** — [`render`] compiles transport-neutral
//!   [`StyledText`](mud_core::StyledText) against a [`Palette`](mud_core::Palette)
//!   into ANSI escapes for a session's color [`Tier`]. This is the one place
//!   escape sequences are generated (§3.20.1.2); downsampling and SGR emission
//!   reuse `anstyle` / `anstyle-lossy`, confined to the conversion adapter.
//! - **Rate limiting** — [`RateLimiter`] enforces the §2.1.1 per-session command
//!   rate limit.
```

- [ ] **Step 2: Add the boundary-type rationale to `TelnetEvent`**

In `crates/mud-net/src/telnet/mod.rs`, extend the `TelnetEvent` doc comment (currently "A validated, decoded event from the client.") to record the decision:

```rust
/// A validated, decoded event from the client.
///
/// A transport DTO: fields stay raw (`String`, `u16`) because parsing them into
/// domain values (`InputLine`, tier from terminal type, render width) belongs to
/// the gateway above, which owns those types. `mud-net` sits below the domain
/// crates, so wrapping here would invert the dependency or add empty newtypes.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum TelnetEvent {
```
(Keep the existing per-variant doc comments unchanged.)

- [ ] **Step 3: Build docs and tests**

Run:
```bash
cargo doc -p mud-net
cargo test -p mud-net
cargo clippy -p mud-net --all-targets
```
Expected: docs build with no broken intra-doc links (`[`TelnetMachine`]`, `[`render`]`, `[`RateLimiter`]`, `[`Tier`]`, `[`StyledText`]`, `[`Palette`]` all resolve — they are all re-exported or reachable); tests pass; clippy clean.

- [ ] **Step 4: Commit**

```bash
jj commit -m "docs(mud-net): reframe crate doc around three pillars, record TelnetEvent boundary-type decision"
```

---

## Self-review checklist

- [ ] `lib.rs` doc leads with the three-pillar framing, not rendering alone; all intra-doc links resolve.
- [ ] `TelnetEvent` carries the boundary-type rationale; variant docs untouched.
- [ ] Decision (no newtypes) is stated above for review.
- [ ] `cargo doc -p mud-net` clean; `cargo test --workspace` green; clippy clean.
