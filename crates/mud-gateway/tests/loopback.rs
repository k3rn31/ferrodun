//! The M1-21 Definition of Done: a gateway↔World loopback test in
//! single-process mode (design doc §Testing). A stub World sits on the
//! world-side in-memory endpoint; a raw `TcpStream` plays the telnet client.
#![allow(clippy::expect_used, clippy::panic)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-{expect,panic}-in-tests do not cover their helpers; both are permitted in tests per policy

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use mud_core::{Palette, RoleName, StyledText};
use mud_gateway::{GatewayConfig, GatewayError, serve};
use mud_ipc::{Endpoint, InMemoryEndpoint, accept_resume, in_memory_pair};
use mud_net::{Burst, SustainedRate, Tier};
use mud_schema::{
    GatewayFrame, OutputKind, OutputText, SessionClose, SessionId, SessionOutput, WorldFrame,
    WorldId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const TICK: Duration = Duration::from_secs(5);

fn world_id() -> WorldId {
    WorldId::new(NonZeroU64::new(1).expect("world id literal is non-zero"))
}

fn config() -> GatewayConfig {
    GatewayConfig {
        world_id: world_id(),
        rate: SustainedRate::DEFAULT,
        burst: Burst::DEFAULT,
        palette: Arc::new(Palette::baseline()),
        tier: Tier::Ansi16,
    }
}

/// Boots a gateway on an ephemeral port with an in-memory IPC channel,
/// completing the resume handshake from the World side. Returns the client
/// address and the world-side endpoint.
async fn boot_gateway(
    config: GatewayConfig,
) -> (
    std::net::SocketAddr,
    InMemoryEndpoint<WorldFrame, GatewayFrame>,
) {
    let (gateway_end, mut world_end) = in_memory_pair();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind must succeed");
    let addr = listener
        .local_addr()
        .expect("bound listener has an address");
    tokio::spawn(serve(listener, gateway_end, config));
    let live = timeout(TICK, accept_resume(&mut world_end, world_id()))
        .await
        .expect("handshake must complete promptly")
        .expect("handshake must succeed");
    assert!(
        live.is_empty(),
        "a fresh gateway announces no live sessions"
    );
    (addr, world_end)
}

/// Receives the next gateway frame, failing the test on close or timeout.
async fn next_frame(world_end: &mut InMemoryEndpoint<WorldFrame, GatewayFrame>) -> GatewayFrame {
    timeout(TICK, world_end.recv())
        .await
        .expect("frame must arrive promptly")
        .expect("endpoint must stay open")
        .expect("gateway must not close the channel")
}

async fn expect_connect(world_end: &mut InMemoryEndpoint<WorldFrame, GatewayFrame>) -> SessionId {
    match next_frame(world_end).await {
        GatewayFrame::Connect(connect) => connect.session_id,
        other => panic!("expected Connect, got {other:?}"),
    }
}

/// Reads from the client socket until `needle` has appeared in the stream.
async fn read_until(client: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
    let mut seen = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = timeout(TICK, client.read(&mut buf))
            .await
            .expect("client read must not time out")
            .expect("client read must succeed");
        assert!(n > 0, "socket closed before expected bytes arrived");
        seen.extend_from_slice(buf.get(..n).expect("read length is within buffer"));
        if seen.windows(needle.len()).any(|w| w == needle) {
            return seen;
        }
    }
}

#[tokio::test]
async fn echo_round_trip_with_negotiation_and_prompt_frame() {
    let (addr, mut world_end) = boot_gateway(config()).await;

    let mut client = TcpStream::connect(addr).await.expect("client must connect");
    let session_id = expect_connect(&mut world_end).await;

    // (a) Opening negotiation bytes arrive on connect.
    let offers = read_until(&mut client, &[255, 251, 42]).await;
    assert!(
        offers.windows(3).any(|w| w == [255, 253, 31]),
        "DO NAWS must be offered, got {offers:?}"
    );

    // (b) A command line round-trips: Input to the World, echoed Output +
    // prompt frame back to the client.
    client
        .write_all(b"look\r\n")
        .await
        .expect("client write must succeed");
    match next_frame(&mut world_end).await {
        GatewayFrame::Input(input) => {
            assert_eq!(input.session_id, session_id);
            assert_eq!(input.line.as_str(), "look");
        }
        other => panic!("expected Input, got {other:?}"),
    }
    world_end
        .send(WorldFrame::Output(SessionOutput {
            session_id,
            text: OutputText::new("echo: look"),
            kind: OutputKind::Line,
        }))
        .await
        .expect("world must send output");
    // IAC GA prompt frame follows the block (client offered no EOR).
    let output = read_until(&mut client, &[255, 249]).await;
    let framed = b"\r\necho: look\r\n";
    assert!(
        output.windows(framed.len()).any(|w| w == framed.as_slice()),
        "block must arrive blank-line-prefixed and CRLF-terminated, got {output:?}"
    );
}

/// The M1-26 wiring assertion: a styled World frame reaches the client as
/// ansi16 SGR bytes — the piece M1-23's "assert ANSI" clause leans on.
#[tokio::test]
async fn styled_output_renders_ansi16_sgr_to_the_client() {
    let (addr, mut world_end) = boot_gateway(config()).await;
    let mut client = TcpStream::connect(addr).await.expect("client connects");
    let session_id = expect_connect(&mut world_end).await;

    let styled = StyledText::new()
        .role("Alice", RoleName::SAY)
        .plain(" waves");
    world_end
        .send(WorldFrame::Output(SessionOutput {
            session_id,
            text: OutputText::new(styled),
            kind: OutputKind::Line,
        }))
        .await
        .expect("world sends styled output");

    let bytes = read_until(&mut client, b"waves").await;
    // Baseline SAY (#cdd6f4) downsamples to bright white (SGR 97) at ansi16 —
    // pinned by mud-net's ansi16_render_is_stable snapshot test.
    let sgr = b"\x1b[97mAlice\x1b[0m";
    assert!(
        bytes.windows(sgr.len()).any(|w| w == sgr),
        "expected ansi16 SGR around the say span, got {bytes:?}"
    );
}

#[tokio::test]
async fn client_drop_reaches_the_world_as_disconnect() {
    let (addr, mut world_end) = boot_gateway(config()).await;

    let client = TcpStream::connect(addr).await.expect("client must connect");
    let session_id = expect_connect(&mut world_end).await;

    drop(client);

    match next_frame(&mut world_end).await {
        GatewayFrame::Disconnect(disconnect) => {
            assert_eq!(disconnect.session_id, session_id);
        }
        other => panic!("expected Disconnect, got {other:?}"),
    }
}

#[tokio::test]
async fn world_close_shuts_the_socket_without_disconnect_echo() {
    let (addr, mut world_end) = boot_gateway(config()).await;

    let mut client = TcpStream::connect(addr).await.expect("client must connect");
    let session_id = expect_connect(&mut world_end).await;

    world_end
        .send(WorldFrame::Close(SessionClose { session_id }))
        .await
        .expect("world must send close");

    // The client observes EOF (after draining any negotiation bytes).
    let mut buf = [0u8; 512];
    loop {
        let n = timeout(TICK, client.read(&mut buf))
            .await
            .expect("close must reach the client promptly")
            .expect("read must succeed until EOF");
        if n == 0 {
            break;
        }
    }

    // No spurious Disconnect: prove ordering with a second session whose
    // Connect must be the next frame the World sees.
    let _probe = TcpStream::connect(addr).await.expect("probe must connect");
    match next_frame(&mut world_end).await {
        GatewayFrame::Connect(_) => {}
        other => panic!("expected the probe's Connect, got a stale {other:?}"),
    }
}

#[tokio::test]
async fn throttled_commands_never_reach_the_world() {
    // burst 1, rate 1/s: of three instant lines only the first forwards.
    let one = std::num::NonZeroU32::MIN;
    let tight = GatewayConfig {
        world_id: world_id(),
        rate: SustainedRate::new(one),
        burst: Burst::new(one),
        palette: Arc::new(Palette::baseline()),
        tier: Tier::Ansi16,
    };
    let (addr, mut world_end) = boot_gateway(tight).await;

    let mut client = TcpStream::connect(addr).await.expect("client must connect");
    let session_id = expect_connect(&mut world_end).await;

    client
        .write_all(b"one\r\ntwo\r\nthree\r\n")
        .await
        .expect("client write must succeed");
    drop(client);

    // FIFO ordering connection→router→endpoint: everything the session
    // forwarded precedes its Disconnect, so counting up to the Disconnect is
    // deterministic.
    let mut inputs = Vec::new();
    loop {
        match next_frame(&mut world_end).await {
            GatewayFrame::Input(input) => inputs.push(input.line.as_str().to_owned()),
            GatewayFrame::Disconnect(disconnect) => {
                assert_eq!(disconnect.session_id, session_id);
                break;
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(
        inputs,
        vec!["one".to_owned()],
        "throttled lines must be dropped"
    );
}

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
