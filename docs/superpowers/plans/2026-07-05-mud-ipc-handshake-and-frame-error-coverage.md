# mud-ipc Handshake & Frame-Error Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining error-path gaps in `mud-ipc` — the two handshake `PeerClosed` arms, the frame-decode `Codec` arm, and the send/receive **asymmetry** where an oversized outbound frame is a typed `FrameTooLarge` but an oversized *inbound* frame degrades to a generic `Io` — so the untrusted-input contract (§2.1.3) is enforced symmetrically and every documented `IpcError` variant is reachable in a test.

**Architecture:** The transport and handshake round-trips are already well-tested (`tests/transport.rs`). Three arms are not: `announce_sessions`'s and `accept_resume`'s `None => Err(PeerClosed)` (peer drops before the ack/announcement), and `recv`'s `decode(..).map_err(Codec)` (a well-framed but malformed body). Separately, `recv` maps *all* framing errors — including the codec's max-frame-length rejection — to `IpcError::Io`, while `send` reports `FrameTooLarge`. This plan adds the missing tests and makes the recv path remap the length-cap rejection to `FrameTooLarge`, matching the send side.

**Tech Stack:** Rust 2024, `tokio` + `tokio-util` (`LengthDelimitedCodec`), `postcard` codec via `mud-schema`, `thiserror`, workspace clippy lints denied, `jj` for VCS.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`.
- Libraries define error types with `thiserror`; no third-party error type leaks through a public variant.
- Integration tests live in `tests/`; they see only the crate's public API.
- Must compile clean under `cargo clippy -p mud-ipc --all-targets`.
- VCS is `jj`. Commit with `jj commit -m "..."`.

## Design decision (confirm at review)

- **`IpcError::FrameTooLarge.size` becomes `Option<usize>`.** On send we know the encoded size (`Some(n)`); on recv the length-delimited codec rejects the frame from its length header *before* reading the body, so the exact size is unknown (`None`). Faking a `size` on the recv path would be a lie; an `Option` makes "size unknown" a representable, honest state (Type-Driven Design). The `#[error]` message drops the concrete size (still available programmatically) so one variant serves both directions. This is a small public-API change to a `#[non_exhaustive]` enum — surfaced here rather than done silently.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-ipc`
Expected: PASS (existing `tests/transport.rs` suite).

---

### Task 1: Cover the handshake `PeerClosed` arms

**Files:**
- Modify: `crates/mud-ipc/tests/transport.rs`

**Interfaces consumed (public):** `in_memory_pair`, `announce_sessions`, `accept_resume`, `Endpoint::recv`, `IpcError::PeerClosed`, `world_id` test helper.

- [ ] **Step 1: Write the two failing-arm tests**

Append to `crates/mud-ipc/tests/transport.rs`:

```rust
#[tokio::test]
async fn accept_resume_reports_peer_closed_when_the_gateway_drops() {
    // World is waiting for the resume announcement; the Gateway disappears
    // instead of sending it (handshake.rs: `None => Err(PeerClosed)`).
    let (gateway, mut world) = in_memory_pair();
    drop(gateway);
    assert!(matches!(
        accept_resume(&mut world, world_id(1)).await,
        Err(IpcError::PeerClosed)
    ));
}

#[tokio::test]
async fn announce_sessions_reports_peer_closed_when_the_world_drops() {
    // Gateway announces, the World consumes the resume and then vanishes without
    // acknowledging (announce_sessions: `None => Err(PeerClosed)`).
    let (mut gateway, world) = in_memory_pair();
    let (announced, _) = tokio::join!(announce_sessions(&mut gateway, world_id(1), vec![]), async {
        let mut world = world;
        world.recv().await.expect("world receives the resume");
        drop(world);
    });
    assert!(matches!(announced, Err(IpcError::PeerClosed)));
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-ipc --test transport peer_closed`
Expected: PASS. Both drive the `None` arms the round-trip tests never reach.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-ipc): cover the handshake PeerClosed arms"
```

---

### Task 2: Cover the frame-decode `Codec` arm

**Files:**
- Modify: `crates/mud-ipc/tests/transport.rs`

**Interfaces consumed (public):** `accept`, `IpcError::Codec`; raw `tokio::net::UnixStream` write (mirrors the existing `socket_recv_rejects_an_oversized_inbound_frame`).

- [ ] **Step 1: Write the failing test**

Append to `crates/mud-ipc/tests/transport.rs`:

```rust
#[tokio::test]
async fn socket_recv_rejects_a_well_framed_but_undecodable_body() {
    use tokio::io::AsyncWriteExt;

    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("world.sock");
    let listener = UnixListener::bind(&path).expect("bind unix socket");
    let accept_task = tokio::spawn(async move { accept(&listener).await });
    let mut raw = tokio::net::UnixStream::connect(&path)
        .await
        .expect("raw gateway connects");
    let mut world = accept_task
        .await
        .expect("accept task joins")
        .expect("world accepts");

    // A valid length prefix (1 byte) framing a truncated GatewayFrame: variant 1
    // (`Input`) with no `SessionInput` payload. The codec hands a complete frame
    // to `decode`, which then fails for want of the session id — exercising the
    // `Codec` arm, distinct from the framing-level `FrameTooLarge`/`Io` arms.
    let body = [0x01u8];
    let len = u32::try_from(body.len()).expect("len fits in u32");
    raw.write_all(&len.to_be_bytes())
        .await
        .expect("write length prefix");
    raw.write_all(&body).await.expect("write truncated body");
    raw.flush().await.expect("flush frame");

    assert!(matches!(world.recv().await, Err(IpcError::Codec(_))));
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p mud-ipc --test transport undecodable`
Expected: PASS. If `[0x01]` happens to decode (it must not — `Input` requires a non-zero session id that is absent), replace the body with `[0x7f]` (variant index 127, out of range) which also forces a decode error; keep the comment truthful to whichever body is used.

- [ ] **Step 3: Commit**

```bash
jj commit -m "test(mud-ipc): cover the frame-decode Codec error arm"
```

---

### Task 3: Make an oversized inbound frame a typed `FrameTooLarge`

**Files:**
- Modify: `crates/mud-ipc/src/error.rs`
- Modify: `crates/mud-ipc/src/transport.rs`
- Modify: `crates/mud-ipc/tests/transport.rs`

**Interfaces:**
- Produces: `IpcError::FrameTooLarge { size: Option<usize>, max: usize }` (was `size: usize`).

- [ ] **Step 1: Widen `FrameTooLarge.size` to `Option<usize>`**

In `crates/mud-ipc/src/error.rs`, replace the `FrameTooLarge` variant:

```rust
    /// A frame exceeded the maximum on-wire size (§3.6.4-adjacent untrusted-input bound).
    ///
    /// `size` is the encoded byte count when known (the send path); it is `None`
    /// when a peer announced an over-cap length that the length-delimited codec
    /// rejected from its header, before the body was read (the recv path).
    #[error("ipc frame exceeds the maximum of {max} bytes")]
    FrameTooLarge {
        /// The encoded size of the offending frame, when known.
        size: Option<usize>,
        /// The configured maximum, [`MAX_FRAME_BYTES`](crate::MAX_FRAME_BYTES).
        max: usize,
    },
```

- [ ] **Step 2: Update the send path and remap the recv framing error**

In `crates/mud-ipc/src/transport.rs`, in `SocketEndpoint`'s `send`, change the `FrameTooLarge` construction to wrap the size:

```rust
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(IpcError::FrameTooLarge {
                size: Some(bytes.len()),
                max: MAX_FRAME_BYTES,
            });
        }
```

Change the `recv` framing arm from `Some(Err(err)) => Err(IpcError::Io(err))` to route through a remap helper:

```rust
    async fn recv(&mut self) -> Result<Option<R>, IpcError> {
        match self.framed.next().await {
            None => Ok(None),
            Some(Ok(bytes)) => decode(&bytes)
                .map(Some)
                .map_err(|e| IpcError::Codec(Box::new(e))),
            Some(Err(err)) => Err(map_inbound_framing_error(err)),
        }
    }
```

Add the helper as a free function in `transport.rs` (e.g. below the `impl ... Endpoint for SocketEndpoint` block), and add the import near the other `tokio_util` use:

```rust
use tokio_util::codec::length_delimited::LengthDelimitedCodecError;
```

```rust
/// Remaps a length-delimited framing error: the codec's max-frame-length
/// rejection becomes the typed [`IpcError::FrameTooLarge`], matching the send
/// path; any other transport failure stays [`IpcError::Io`]. The codec rejects
/// on the length header, so the exact frame size is unknown here (`size: None`).
fn map_inbound_framing_error(err: std::io::Error) -> IpcError {
    if err
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<LengthDelimitedCodecError>().is_some())
    {
        return IpcError::FrameTooLarge {
            size: None,
            max: MAX_FRAME_BYTES,
        };
    }
    IpcError::Io(err)
}
```

Note: verify the import path resolves — in `tokio-util` 0.7 the type is `tokio_util::codec::length_delimited::LengthDelimitedCodecError`. If `cargo build` cannot find it there, run `cargo doc -p tokio-util --open` (or grep the dependency source) for the exact public path; do not guess a different type.

- [ ] **Step 3: Flip the existing inbound-oversized assertion**

In `crates/mud-ipc/tests/transport.rs`, in `socket_recv_rejects_an_oversized_inbound_frame`, change the final assertion from `Io` to the now-typed variant:

```rust
    assert!(matches!(
        world.recv().await,
        Err(IpcError::FrameTooLarge { size: None, max: MAX_FRAME_BYTES })
    ));
```

Update that test's trailing comment to read: `// ... the untrusted-input bound MAX_FRAME_BYTES enforces, reported symmetrically with the send path as FrameTooLarge.`

- [ ] **Step 4: Run the whole crate**

Run: `cargo test -p mud-ipc`
Expected: PASS — including the flipped inbound test and the unchanged `socket_rejects_an_oversized_frame` (its `FrameTooLarge { .. }` pattern still matches).

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p mud-ipc --all-targets`
Expected: clean. (`is_some_and` and `downcast_ref` are the idiomatic, lint-clean way to probe the inner error; no `unwrap`.)

- [ ] **Step 6: Commit**

```bash
jj commit -m "fix(mud-ipc): report an oversized inbound frame as typed FrameTooLarge"
```

---

## Self-review checklist

- [ ] Every `IpcError` variant is now reachable in a test: `Io`, `Codec`, `SchemaMismatch`, `WorldIdMismatch`, `FrameTooLarge` (both directions), `UnexpectedFrame`, `PeerClosed` (both handshake sides).
- [ ] `FrameTooLarge.size` is `Option<usize>`; send reports `Some`, recv reports `None`; the enum change is flagged for review above.
- [ ] The `LengthDelimitedCodecError` import path was verified against the actual `tokio-util` version, not guessed.
- [ ] `cargo test --workspace` green; clippy clean.
