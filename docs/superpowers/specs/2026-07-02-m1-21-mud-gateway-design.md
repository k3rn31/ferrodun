# M1-21 — `mud-gateway` (library) design

**Date:** 2026-07-02
**Spec:** §2.1.1 (Gateway responsibilities), §2.1.3 (IPC contract), §2.8.2 (telnet M1 subset)
**Plan item:** M1-21

## Summary

M1-21 delivers the tokio glue that owns the telnet listener and bridges each
client connection to a World over the existing session-multiplexed IPC
`Endpoint`. It consumes the sans-IO pieces already built: `mud-net`'s
`TelnetMachine` and `RateLimiter`, `mud-schema`'s `GatewayFrame`/`WorldFrame`
and `SessionId`, and `mud-ipc`'s `Endpoint` trait + resume handshake.

The gateway is shipped as a **library crate** (`crates/mud-gateway`), generic
over `Endpoint`. There is **no separate gateway executable**: `mudd` (M1-22) is
the sole binary and embeds this library — in-proc over `in_memory_pair`
(single-process mode, §2.1.3.3) or, in a later milestone, driven with a
`SocketEndpoint` in a split-mode gateway-role process (§2.1.3.1/.4). Being
generic over `Endpoint` is what lets the same code serve both transports
unchanged.

## Scope

**In scope (M1):** a telnet-only TCP listener, per-session IPC wiring
(connect / input / output / disconnect / world-initiated close), the resume
handshake at startup, and per-session command rate-limit enforcement.

**Deferred (with rationale):**

- **Standalone binary / CLI / config loading** → M1-22 (`mudd`). No runnable
  `main.rs` this PR — nothing in M1 serves a World for it to talk to yet, so a
  binary would be dead code (YAGNI). PLAN's M1-21 wording is updated to
  "library, generic over `Endpoint`."
- **`rate_limited` structured event to the session** (§2.1.1). A bare M1 telnet
  client has no structured channel (GMCP is M3). M1 **drops throttled commands
  silently**; the structured-event obligation is annotated in PLAN at the
  M3/GMCP stage so it is not forgotten.
- **World-down handling** — §2.1.1's "hold all TCP connections open, show a
  reconnecting… banner, re-handshake on return" is **M7**. M1 assumes the World
  is up; on IPC loss the gateway tears down cleanly.
- **HTTP listener, `/metrics`, TLS/SSH/WebSocket, MCCP2/GMCP/MSDP/MXP/MSSP,
  admin reverse-proxy** → M3 and later.

## Public API

New lib crate `crates/mud-gateway` (deps: `tokio`, `tracing`, `thiserror`;
plus workspace crates `mud-net`, `mud-schema`, `mud-ipc`).

```rust
pub struct GatewayConfig {
    pub world_id: WorldId,
    pub rate: SustainedRate,
    pub burst: Burst,
}

/// Serve telnet clients on `listener`, bridging them to a World over `endpoint`.
///
/// Drives the resume handshake, then runs until the endpoint closes cleanly
/// (`Ok(())`) or an error occurs. `listener` is passed in already bound so
/// callers control the address (tests bind `127.0.0.1:0`; `mudd` binds the
/// configured address).
pub async fn serve<E>(listener: TcpListener, endpoint: E, config: GatewayConfig)
    -> Result<(), GatewayError>
where
    E: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame> + Send + 'static;
```

`SchemaVersion` for the handshake comes from `mud-schema`'s build constant, not
from config. Internal modules: `router`, `connection`, `session` (id minting),
`config`, `error`.

## Runtime & task topology (actor-style router + per-connection tasks)

The gateway owns a single multiplexed IPC `Endpoint` (both `send` and `recv`
take `&mut self`, so exactly one task may own it) plus N telnet connections that
each own a `TcpStream` + `TelnetMachine` + `RateLimiter`.

**`serve` (entry).** Runs the resume handshake on the raw endpoint first —
`announce_sessions(world_id, schema_version, live_sessions = [])` → await
`ResumeAck` — then moves the endpoint into the **router task** and runs the
**accept loop**. The accept loop `select!`s between `listener.accept()` (mint a
`SessionId`, spawn a connection task) and the router's `JoinHandle`; if the
router finishes (endpoint closed), the accept loop stops and returns the
router's result.

**Router task** — owns the `endpoint` and a
`HashMap<SessionId, mpsc::Sender<ToConnection>>` registry. Two-branch `select!`:

- `endpoint.recv()` → `WorldFrame::Output` / `Close` routed to
  `registry[session_id]` (silently dropped if the session already departed);
  `Ok(None)` → clean shutdown; `Err` → return `GatewayError`.
- a **single ordered command channel** `ToRouter` fed by every connection:
  `Register { session_id, tx }` | `Frame(GatewayFrame)` | `Deregister { session_id }`.

Folding registration and outbound frames into **one FIFO channel per
connection** is the correctness crux: a connection sends `Register` immediately
before `Frame(Connect(..))`, and an mpsc preserves per-sender order, so the
router always inserts the registry entry before forwarding that `Connect` — no
race where a `WorldFrame::Output` could arrive before the session is
registered. `serve`/the accept loop retains one `ToRouter` sender so the channel
stays open while serving even with zero connections.

**Connection task** (one per socket) — owns the split `TcpStream`, a
`TelnetMachine`, and a `RateLimiter`. Sends `Register` then `Connect`, flushes
the machine's opening negotiation offers (`take_output`), then `select!`s
between:

- **socket read** → `machine.receive(bytes)` → per `TelnetEvent`:
  - `Line` → `rate.check(now)`: `Allow` ⇒ `Frame(Input(..))`; `Throttle` ⇒
    silent drop (M1).
  - `WindowSize` / `TerminalType` ⇒ ignored in M1 (no consumer yet — YAGNI).
  - Then flush `take_output()` to the socket. Read of 0 bytes / error ⇒ exit.
- **`output_rx`** → `ToConnection::Output(text)` ⇒ write
  `machine.encode_output(text)` followed by `machine.prompt_frame()` (an EOR/GA
  prompt after every rendered output block, §2.8.2); `ToConnection::Close` or a
  closed channel ⇒ exit.

`ToConnection { Output(OutputText), Close }` is a narrowed payload the router
builds from `WorldFrame`, so the connection task never re-matches the full frame
enum.

## `SessionId` minting & lifecycle edges

- **Minting:** a monotonic `AtomicU64` (starts at 1) → `NonZeroU64` →
  `SessionId`, owned by `serve`. Overflow is practically unreachable (2⁶⁴
  connections); it maps to `GatewayError::SessionIdOverflow` rather than a
  panic/`unwrap`, preserving the newtype invariant.
- **Connect:** accept → mint → connection sends `Register` then `Connect`.
- **Input:** each `Allow`ed line → `Input`.
- **Exit cause is tracked explicitly**, deciding whether a `Disconnect` is sent:
  - `ClientGone` (EOF or socket error) ⇒ `Frame(Disconnect(..))` + `Deregister`.
  - `WorldClosed` (received `ToConnection::Close`, e.g. `quit`/kick) ⇒
    **`Deregister` only, no `Disconnect`** — the World initiated the close and
    already knows; echoing one back would be spurious. (A structured close
    reason, §3.15.2, is deferred.)

## Error handling

`GatewayError` (`thiserror`) with variants: `Accept(io::Error)`,
`Ipc(IpcError)`, `Handshake` (world_id / schema mismatch from the resume
handshake), `SessionIdOverflow`. No `unwrap`/`expect` in production code.
Per-connection socket I/O errors are treated as `ClientGone`. On IPC loss
(`Ok(None)` / `Err`) the router shuts down and `serve` returns; connection tasks
observe their `output_rx` closing and tear down (M1 has no hold-open/reconnect).

## Testing (TDD)

- **`tests/loopback.rs` (the DoD, single-process mode).** `in_memory_pair()`;
  the gateway-side endpoint drives `serve` on `TcpListener::bind(127.0.0.1:0)`;
  the world-side endpoint is driven by a **stub World** (await
  `ResumeHandshake` → reply `ResumeAck`; on `Connect` record the session; on
  `Input` reply with an echo `Output`; able to emit `Close`). A raw `TcpStream`
  test client asserts:
  1. opening telnet negotiation bytes arrive on connect;
  2. `line\r\n` → echoed `Output` + prompt frame round-trips;
  3. client drop → the stub World receives `Disconnect`;
  4. World `Close` → the socket is closed with no spurious `Disconnect`.
- **Unit:** `SessionId` minting is monotonic and non-zero; the router drops a
  `WorldFrame` addressed to an unknown/departed session without erroring.
- **Rate limit:** mechanics are already unit-tested in `mud-net`; add one
  gateway-level assertion that throttled lines do not reach the stub World.
- **Gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo fmt --all --check`.

## Housekeeping

- **Docs site:** no change this PR — no runnable binary, config key, or
  otherwise observable surface lands until M1-22's CLI. Noted in the journal.
- **PLAN edits:** reword M1-21 to "library, generic over `Endpoint`; binary
  deferred to M1-22"; annotate the deferred `rate_limited` structured-event
  obligation at the M3/GMCP stage; note the M7 World-down/reconnect deferral.
