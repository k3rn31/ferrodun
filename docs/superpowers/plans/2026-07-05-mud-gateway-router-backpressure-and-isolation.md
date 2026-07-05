# mud-gateway Router Backpressure, Isolation & Barrier-Helper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cover the router's untested backpressure drop branch (`router.rs:100`, a full per-connection buffer drops output rather than stalling the router), prove cross-session isolation and correct addressing, add a `serve`-level IPC-failure path test, and DRY up the thrice-duplicated ordering-barrier boilerplate in the router tests.

**Architecture:** `run_router` fans World frames to per-connection `mpsc` senders via `route`, which `try_send`s so one slow client cannot stall every session — the overflow is dropped with a warning. Nothing exercises that drop, nor that a flooded session leaves its neighbors untouched, nor that `serve` surfaces a dead IPC peer as `GatewayError::Ipc`. All are reachable: the drop branch by never draining a registered session's receiver and flooding past `OUTPUT_CAPACITY`; isolation by checking a second session still receives; the `serve` path by dropping the World endpoint before the handshake. The three router tests also copy an identical "send a probe frame through the FIFO command channel and await it World-side" barrier — extracted here into one helper.

**Tech Stack:** Rust 2024, `tokio` (mpsc, `tokio::test`), `mud-ipc` in-memory endpoints, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`. `panic!` in a test match arm mirrors the existing router-test style.
- In-crate (white-box) tests reach `pub(crate)` items (`run_router`, `ToRouter`, `ToConnection`, `OUTPUT_CAPACITY`) and live in `router.rs`'s `#[cfg(test)] mod tests`; the `serve`-level test uses only the public API and lives in `tests/loopback.rs`.
- Must compile clean under `cargo clippy -p mud-gateway --all-targets`.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-gateway`
Expected: PASS (router unit tests + `tests/loopback.rs`).

---

### Task 1: Extract the ordering-barrier helper (DRY the router tests)

**Files:**
- Modify: `crates/mud-gateway/src/router.rs` (the `#[cfg(test)] mod tests` block only)

**Interfaces:**
- Produces: `async fn drain_barrier<E>(commands_tx: &mpsc::Sender<ToRouter>, world_end: &mut E, probe: SessionId)` where `E: Endpoint<Outbound = WorldFrame, Inbound = GatewayFrame>` — round-trips a probe `Connect` to prove every earlier command was processed.

- [ ] **Step 1: Add the helper to the test module**

In `crates/mud-gateway/src/router.rs`, inside `#[cfg(test)] mod tests`, add `SessionConnect` to the `mud_schema` test import if not already there (it is), and add this helper below the `session` helper:

```rust
    /// Round-trips a probe `Connect` through the FIFO command channel and the
    /// endpoint. Because an mpsc preserves per-sender order, the router has
    /// processed every command enqueued before this probe (a `Register` or
    /// `Deregister`) by the time the probe surfaces World-side — without it, an
    /// `Output` on the endpoint channel can race ahead of a `Register` on the
    /// command channel in the router's `select!` and be dropped as unknown.
    async fn drain_barrier<E>(
        commands_tx: &mpsc::Sender<ToRouter>,
        world_end: &mut E,
        probe: SessionId,
    ) where
        E: Endpoint<Outbound = WorldFrame, Inbound = GatewayFrame>,
    {
        commands_tx
            .send(ToRouter::Frame(GatewayFrame::Connect(SessionConnect {
                session_id: probe,
            })))
            .await
            .expect("router must accept the barrier frame");
        match world_end.recv().await {
            Ok(Some(GatewayFrame::Connect(connect))) => {
                assert_eq!(connect.session_id, probe, "barrier frame must arrive");
            }
            other => panic!("expected the barrier Connect frame, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Replace the three inline barriers**

In each of `routes_output_to_the_registered_session`, `close_frame_reaches_the_session_and_unknown_sessions_are_ignored`, and `deregistered_session_no_longer_receives_output`, delete the inline barrier block (the `let probe = session(N);` + `commands_tx.send(ToRouter::Frame(...Connect...))` + `match world_end.recv()...` sequence, including its explanatory comment) and replace it with a single call using that test's existing probe id:

```rust
        drain_barrier(&commands_tx, &mut world_end, session(2)).await;
```

Use `session(2)` in `routes_output...`, `session(3)` in `close_frame...`, and `session(5)` in `deregistered_session...` (matching each test's current probe id, so no behavior changes).

- [ ] **Step 3: Run — behavior unchanged**

Run: `cargo test -p mud-gateway --lib router`
Expected: PASS. The three refactored tests still pass; only duplication was removed.

- [ ] **Step 4: Commit**

```bash
jj commit -m "test(mud-gateway): extract the router ordering-barrier helper"
```

---

### Task 2: Cover the backpressure drop branch and slow-client isolation

**Files:**
- Modify: `crates/mud-gateway/src/router.rs` (the `#[cfg(test)] mod tests` block only)

**Interfaces consumed:** `run_router`, `ToRouter::{Register, Frame}`, `ToConnection::Output`, `OUTPUT_CAPACITY`, `in_memory_pair`, `drain_barrier` (Task 1).

- [ ] **Step 1: Write the failing test**

Append to `router.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[tokio::test]
    async fn a_slow_session_drops_overflow_without_stalling_its_neighbor() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        // Session A never drains its receiver (a stuck-socket client); session B
        // drains normally.
        let (tx_a, mut rx_a) = mpsc::channel(OUTPUT_CAPACITY);
        let (tx_b, mut rx_b) = mpsc::channel(OUTPUT_CAPACITY);
        let a = session(1);
        let b = session(2);
        commands_tx
            .send(ToRouter::Register { session_id: a, tx: tx_a })
            .await
            .expect("router must accept A's registration");
        commands_tx
            .send(ToRouter::Register { session_id: b, tx: tx_b })
            .await
            .expect("router must accept B's registration");
        drain_barrier(&commands_tx, &mut world_end, session(9)).await;

        // Flood A past its buffer; the endpoint channel is FIFO, so all of these
        // are routed before B's marker below.
        let flood = OUTPUT_CAPACITY + 8;
        for _ in 0..flood {
            world_end
                .send(WorldFrame::Output(SessionOutput {
                    session_id: a,
                    text: OutputText::new("flood"),
                }))
                .await
                .expect("world endpoint must send A's flood");
        }
        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: b,
                text: OutputText::new("b-marker"),
            }))
            .await
            .expect("world endpoint must send B's marker");

        // B receives despite A being wedged: one slow client does not stall the
        // router (proves isolation). Its arrival also means every A-frame ahead
        // of it in the FIFO has already been routed.
        let marker = rx_b.recv().await.expect("B receives its output while A is flooded");
        assert!(matches!(marker, ToConnection::Output(t) if t.as_str() == "b-marker"));

        // A buffered exactly its capacity; the overflow hit the drop branch.
        let mut delivered = 0usize;
        while rx_a.try_recv().is_ok() {
            delivered += 1;
        }
        assert_eq!(
            delivered, OUTPUT_CAPACITY,
            "A buffers its capacity; the excess was dropped, not stalled"
        );
        assert!(delivered < flood, "the slow session's overflow was dropped");

        drop(world_end); // clean shutdown
        router
            .await
            .expect("router task must not panic")
            .expect("closed peer is a clean shutdown");
    }
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-gateway --lib a_slow_session_drops_overflow`
Expected: PASS. The essential invariant is *overflow is dropped, not stalled, and a neighbor is unaffected*. If `tokio`'s exact buffer accounting delivers other than `OUTPUT_CAPACITY`, keep the `delivered < flood` and the B-isolation assertions and relax the `assert_eq!` to `assert!(delivered <= OUTPUT_CAPACITY)` — do not "fix" it by changing production code.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-gateway): cover router backpressure drop and slow-client isolation"
```

---

### Task 3: Prove output is addressed to one session, not broadcast

**Files:**
- Modify: `crates/mud-gateway/src/router.rs` (the `#[cfg(test)] mod tests` block only)

**Interfaces consumed:** as Task 2, plus `drain_barrier`.

- [ ] **Step 1: Write the failing test**

Append to `router.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[tokio::test]
    async fn output_reaches_only_the_addressed_session() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        let (tx_a, mut rx_a) = mpsc::channel(OUTPUT_CAPACITY);
        let (tx_b, mut rx_b) = mpsc::channel(OUTPUT_CAPACITY);
        let a = session(1);
        let b = session(2);
        commands_tx
            .send(ToRouter::Register { session_id: a, tx: tx_a })
            .await
            .expect("router must accept A's registration");
        commands_tx
            .send(ToRouter::Register { session_id: b, tx: tx_b })
            .await
            .expect("router must accept B's registration");
        drain_barrier(&commands_tx, &mut world_end, session(9)).await;

        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: a,
                text: OutputText::new("for-a"),
            }))
            .await
            .expect("world endpoint must send");

        let got = rx_a.recv().await.expect("A receives its output");
        assert!(matches!(got, ToConnection::Output(t) if t.as_str() == "for-a"));
        // Blocking on A's receiver means the frame is fully routed; B, never
        // addressed, has nothing waiting.
        assert!(
            rx_b.try_recv().is_err(),
            "output addressed to A must not reach B"
        );

        drop(world_end);
        router
            .await
            .expect("router task must not panic")
            .expect("closed peer is a clean shutdown");
    }
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-gateway --lib output_reaches_only`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-gateway): assert output is routed to a single session"
```

---

### Task 4: Cover `serve`'s IPC-failure exit

**Files:**
- Modify: `crates/mud-gateway/tests/loopback.rs`

**Interfaces consumed (public):** `mud_gateway::{serve, GatewayError}`, the existing `config()` helper, `in_memory_pair`, `TcpListener`, `timeout`, `TICK`.

- [ ] **Step 1: Extend the import**

In `crates/mud-gateway/tests/loopback.rs`, change:

```rust
use mud_gateway::{GatewayConfig, serve};
```
to:
```rust
use mud_gateway::{GatewayConfig, GatewayError, serve};
```

- [ ] **Step 2: Write the failing test**

Append to `crates/mud-gateway/tests/loopback.rs`:

```rust
#[tokio::test]
async fn serve_fails_when_the_ipc_peer_is_gone_before_the_handshake() {
    // No World on the other end: the resume announcement cannot be delivered, so
    // `serve` must terminate with a fatal IPC error rather than accept clients.
    let (gateway_end, world_end) = in_memory_pair();
    drop(world_end);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind must succeed");

    let result = timeout(TICK, serve(listener, gateway_end, config()))
        .await
        .expect("serve returns promptly once the handshake peer is gone");

    assert!(matches!(result, Err(GatewayError::Ipc(_))));
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p mud-gateway --test loopback serve_fails_when`
Expected: PASS.

- [ ] **Step 4: Full crate + clippy**

Run: `cargo test -p mud-gateway && cargo clippy -p mud-gateway --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 5: Commit**

```bash
jj commit -m "test(mud-gateway): cover serve's fatal IPC-failure exit"
```

---

## Self-review checklist

- [ ] The ordering barrier lives in one `drain_barrier` helper; the three original router tests call it and still pass (no behavior change).
- [ ] The backpressure test drives `route`'s `try_send` drop branch and asserts a neighbor session is unaffected.
- [ ] The addressing test proves output is not broadcast.
- [ ] `serve` returning `GatewayError::Ipc` on a dead handshake peer is covered without duplicating the existing loopback round-trip tests.
- [ ] `cargo test --workspace` green; clippy clean.
