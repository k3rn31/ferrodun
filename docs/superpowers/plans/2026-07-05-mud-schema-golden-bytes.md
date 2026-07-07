# mud-schema Golden-Bytes Pinning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin the exact `postcard` wire bytes of every `GatewayFrame`/`WorldFrame` variant (not just `Input`), so a silent format drift — a field reorder, a variant-index shift, a changed `SCHEMA_VERSION` encoding — fails loudly, enforcing the §2.8.5.7 build-time version lock.

**Architecture:** The frame tests already round-trip every variant, but a symmetric encode/decode change passes round-trip while silently breaking the wire format across versions. Golden-byte assertions catch exactly that. Today only `GatewayFrame::Input` is pinned (`input_frame_has_a_stable_encoding`). This plan adds a byte-literal pin for each remaining variant, in the same in-file test module, next to the existing pin.

**Tech Stack:** Rust 2024, `postcard` + `serde`, workspace clippy lints denied, `jj` for VCS.

## postcard encoding rules (for deriving the literals)

`postcard` encodes: an enum variant as a **varint of its index**; then each field in declaration order. `NonZeroU64`/`u64`/`u32` are varints (LEB128); values 1–127 are a single byte equal to the value. A `String` (and a serde-transparent newtype wrapping one, like `InputLine`/`OutputText`) is a varint length prefix then the UTF-8 bytes. A `Vec<T>` is a varint length prefix then the elements. Variant indices here: `GatewayFrame` = `Connect(0)`, `Input(1)`, `Disconnect(2)`, `Resume(3)`; `WorldFrame` = `Output(0)`, `Close(1)`, `ResumeAck(2)`. `SCHEMA_VERSION` is `SchemaVersion(1)` → one byte `0x01`. `"hi"` → `0x02, 0x68, 0x69`.

## Global Constraints

- Code and comments in English. Comment *why*, not *how*.
- `expect()` in tests must carry a descriptive message; no `unwrap()`.
- Must compile clean under `cargo clippy -p mud-schema --all-targets`.
- Tests-only change; no production code modified. A byte pin that does **not** match a derivation is a signal to investigate, not to blindly overwrite with the observed bytes.
- VCS is `jj`. Commit with `jj commit -m "..."`.

---

## Baseline (before Task 1)

- [ ] **Step 0: Confirm green**

Run: `cargo test -p mud-schema`
Expected: PASS, including the existing `input_frame_has_a_stable_encoding`.

---

### Task 1: Pin the remaining frame variants' encodings

**Files:**
- Modify: `crates/mud-schema/src/frame.rs`

- [ ] **Step 1: Add the golden-byte tests**

Append these tests inside `frame.rs`'s `#[cfg(test)] mod tests` (after `input_frame_has_a_stable_encoding`). Each expected vector is derived from the rules above; the inline comment shows the derivation.

```rust
    // Connect = variant 0; session_id = 1.
    #[test]
    fn connect_frame_has_a_stable_encoding() {
        let frame = GatewayFrame::Connect(SessionConnect {
            session_id: session(1),
        });
        assert_eq!(encode(&frame).expect("encode"), vec![0x00, 0x01]);
    }

    // Disconnect = variant 2; session_id = 3.
    #[test]
    fn disconnect_frame_has_a_stable_encoding() {
        let frame = GatewayFrame::Disconnect(SessionDisconnect {
            session_id: session(3),
        });
        assert_eq!(encode(&frame).expect("encode"), vec![0x02, 0x03]);
    }

    // Resume = variant 3; world_id = 7; schema_version = 1; live_sessions =
    // [1, 2] (len 2 then the two ids). Pins the multi-field, vec-bearing frame.
    #[test]
    fn resume_frame_has_a_stable_encoding() {
        let frame = GatewayFrame::Resume(ResumeHandshake {
            world_id: world(7),
            schema_version: crate::SCHEMA_VERSION,
            live_sessions: vec![session(1), session(2)],
        });
        assert_eq!(
            encode(&frame).expect("encode"),
            vec![0x03, 0x07, 0x01, 0x02, 0x01, 0x02]
        );
    }

    // Output = variant 0; session_id = 4; text = "hi" (len 2, 0x68 0x69).
    #[test]
    fn output_frame_has_a_stable_encoding() {
        let frame = WorldFrame::Output(SessionOutput {
            session_id: session(4),
            text: OutputText::new("hi"),
        });
        assert_eq!(
            encode(&frame).expect("encode"),
            vec![0x00, 0x04, 0x02, 0x68, 0x69]
        );
    }

    // Close = variant 1; session_id = 5.
    #[test]
    fn close_frame_has_a_stable_encoding() {
        let frame = WorldFrame::Close(SessionClose {
            session_id: session(5),
        });
        assert_eq!(encode(&frame).expect("encode"), vec![0x01, 0x05]);
    }

    // ResumeAck = variant 2; world_id = 7; schema_version = 1.
    #[test]
    fn resume_ack_frame_has_a_stable_encoding() {
        let frame = WorldFrame::ResumeAck(HandshakeAck {
            world_id: world(7),
            schema_version: crate::SCHEMA_VERSION,
        });
        assert_eq!(encode(&frame).expect("encode"), vec![0x02, 0x07, 0x01]);
    }
```

- [ ] **Step 2: Run the pins**

Run: `cargo test -p mud-schema --lib frame`
Expected: PASS. If a pin fails, do **not** blindly paste the observed bytes — re-derive from the rules first. A mismatch on `Output`/`Input` shape means `OutputText`/`InputLine` isn't serde-transparent (then the derivation, and possibly the comment, must be corrected); a mismatch on a *variant index* means the enum order changed and a downstream compatibility decision is needed — surface it.

- [ ] **Step 3: Full crate + clippy**

Run: `cargo test -p mud-schema && cargo clippy -p mud-schema --all-targets`
Expected: PASS, clippy clean.

- [ ] **Step 4: Commit**

```bash
jj commit -m "test(mud-schema): pin postcard encoding of every frame variant"
```

---

## Self-review checklist

- [ ] Every `GatewayFrame` and `WorldFrame` variant now has a byte-literal pin, including the multi-field `Resume` and `ResumeAck`.
- [ ] Each pin's comment shows its derivation; the vectors are consistent with the existing `Input` pin (`[0x01, 0x02, 0x02, 0x68, 0x69]`).
- [ ] No production code touched.
- [ ] `cargo test --workspace` green; clippy clean.
