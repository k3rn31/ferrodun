//! Transport + resume-handshake behavior, exercised over both the in-memory and
//! unix-socket endpoints so the two are held to one frame contract (§2.1.3).
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::num::NonZeroU64;

use mud_ipc::{
    Endpoint, IpcError, MAX_FRAME_BYTES, SocketEndpoint, accept, accept_resume, announce_sessions,
    connect, in_memory_pair,
};
use mud_schema::{
    GatewayFrame, InputLine, OutputText, ResumeHandshake, SCHEMA_VERSION, SchemaVersion, SessionId,
    SessionInput, SessionOutput, WorldFrame, WorldId,
};
use tokio::net::UnixListener;

fn session(value: u64) -> SessionId {
    SessionId::new(NonZeroU64::new(value).expect("session id must be non-zero"))
}

fn world_id(value: u64) -> WorldId {
    WorldId::new(NonZeroU64::new(value).expect("world id must be non-zero"))
}

/// Binds a unix socket and returns a connected Gateway/World endpoint pair. The
/// `TempDir` guard is returned so the socket file outlives the test.
async fn socket_pair() -> (
    SocketEndpoint<GatewayFrame, WorldFrame>,
    SocketEndpoint<WorldFrame, GatewayFrame>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("world.sock");
    let listener = UnixListener::bind(&path).expect("bind unix socket");
    let accept_task = tokio::spawn(async move { accept(&listener).await });
    let gateway = connect(&path).await.expect("gateway connects");
    let world = accept_task
        .await
        .expect("accept task joins")
        .expect("world accepts");
    (gateway, world, dir)
}

/// One frame each way must survive the round trip unchanged. Shared by the
/// in-memory and socket cases so both transports prove the identical contract.
async fn assert_round_trips<G, W>(gateway: &mut G, world: &mut W)
where
    G: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame>,
    W: Endpoint<Outbound = WorldFrame, Inbound = GatewayFrame>,
{
    let input = GatewayFrame::Input(SessionInput {
        session_id: session(1),
        line: InputLine::new("look"),
    });
    gateway.send(input.clone()).await.expect("gateway sends");
    assert_eq!(world.recv().await.expect("world receives"), Some(input));

    let output = WorldFrame::Output(SessionOutput {
        session_id: session(1),
        text: OutputText::new("a quiet room"),
    });
    world.send(output.clone()).await.expect("world sends");
    assert_eq!(
        gateway.recv().await.expect("gateway receives"),
        Some(output)
    );
}

/// A resume handshake announcing `live` must hand the World back exactly `live`.
/// Shared by both transports.
async fn assert_handshake_replays<G, W>(gateway: &mut G, world: &mut W)
where
    G: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame>,
    W: Endpoint<Outbound = WorldFrame, Inbound = GatewayFrame>,
{
    let live = vec![session(1), session(2), session(3)];
    let (announced, adopted) = tokio::join!(
        announce_sessions(gateway, world_id(7), live.clone()),
        accept_resume(world, world_id(7)),
    );
    announced.expect("gateway handshake succeeds");
    assert_eq!(adopted.expect("world adopts the session set"), live);
}

#[tokio::test]
async fn in_memory_round_trips_both_directions() {
    let (mut gateway, mut world) = in_memory_pair();
    assert_round_trips(&mut gateway, &mut world).await;
}

#[tokio::test]
async fn unix_socket_round_trips_both_directions() {
    let (mut gateway, mut world, _dir) = socket_pair().await;
    assert_round_trips(&mut gateway, &mut world).await;
}

#[tokio::test]
async fn in_memory_resume_handshake_replays_live_sessions() {
    let (mut gateway, mut world) = in_memory_pair();
    assert_handshake_replays(&mut gateway, &mut world).await;
}

#[tokio::test]
async fn unix_socket_resume_handshake_replays_live_sessions() {
    let (mut gateway, mut world, _dir) = socket_pair().await;
    assert_handshake_replays(&mut gateway, &mut world).await;
}

#[tokio::test]
async fn resume_handshake_rejects_a_schema_version_mismatch() {
    let (mut gateway, mut world) = in_memory_pair();
    let bogus = SchemaVersion::new(SCHEMA_VERSION.get() + 1);
    let handshake = GatewayFrame::Resume(ResumeHandshake {
        world_id: world_id(7),
        schema_version: bogus,
        live_sessions: vec![],
    });
    // Send the forged frame directly rather than via `announce_sessions`, which
    // always stamps this build's `SCHEMA_VERSION`.
    let (sent, accepted) = tokio::join!(
        gateway.send(handshake),
        accept_resume(&mut world, world_id(7))
    );
    sent.expect("gateway sends the forged handshake");
    assert!(matches!(accepted, Err(IpcError::SchemaMismatch { .. })));
}

#[tokio::test]
async fn resume_handshake_rejects_a_world_id_mismatch() {
    let (mut gateway, mut world) = in_memory_pair();
    let handshake = GatewayFrame::Resume(ResumeHandshake {
        world_id: world_id(2),
        schema_version: SCHEMA_VERSION,
        live_sessions: vec![],
    });
    let (sent, accepted) = tokio::join!(
        gateway.send(handshake),
        accept_resume(&mut world, world_id(1))
    );
    sent.expect("gateway sends the handshake");
    assert!(matches!(accepted, Err(IpcError::WorldIdMismatch { .. })));
}

#[tokio::test]
async fn recv_returns_none_when_the_in_memory_peer_closes() {
    let (gateway, mut world) = in_memory_pair();
    drop(gateway);
    assert_eq!(world.recv().await.expect("clean close, not an error"), None);
}

#[tokio::test]
async fn recv_returns_none_when_the_socket_peer_closes() {
    let (gateway, mut world, _dir) = socket_pair().await;
    drop(gateway);
    assert_eq!(world.recv().await.expect("clean close, not an error"), None);
}

#[tokio::test]
async fn socket_rejects_an_oversized_frame() {
    let (mut gateway, _world, _dir) = socket_pair().await;
    let oversized = "x".repeat(MAX_FRAME_BYTES + 1);
    let frame = GatewayFrame::Input(SessionInput {
        session_id: session(1),
        line: InputLine::new(oversized),
    });
    assert!(matches!(
        gateway.send(frame).await,
        Err(IpcError::FrameTooLarge { .. })
    ));
}

#[tokio::test]
async fn socket_recv_rejects_an_oversized_inbound_frame() {
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

    // A peer-supplied length prefix one byte past the cap. The codec must reject
    // it on the header alone, before allocating the body — this is
    // the untrusted-input bound MAX_FRAME_BYTES enforces, reported symmetrically
    // with the send path as FrameTooLarge.
    let oversized_len = u32::try_from(MAX_FRAME_BYTES + 1).expect("cap fits in u32");
    raw.write_all(&oversized_len.to_be_bytes())
        .await
        .expect("write oversized length prefix");
    raw.flush().await.expect("flush length prefix");

    assert!(matches!(
        world.recv().await,
        Err(IpcError::FrameTooLarge { size: None, max: MAX_FRAME_BYTES })
    ));
}

#[tokio::test]
async fn connect_reports_io_when_the_socket_path_does_not_exist() {
    // No listener is ever bound at this path, so the `connect` syscall itself
    // fails (ENOENT) and surfaces as `IpcError::Io` via `UnixStream::connect`'s
    // `?` conversion — distinct from the framing-level `Io`/`FrameTooLarge`
    // arms exercised by the other socket tests in this file.
    let dir = tempfile::tempdir().expect("create tempdir");
    let missing_path = dir.path().join("no-such.sock");

    assert!(matches!(
        connect(&missing_path).await,
        Err(IpcError::Io(_))
    ));
}

#[tokio::test]
async fn accept_resume_rejects_a_non_handshake_frame() {
    let (mut gateway, mut world) = in_memory_pair();
    let stray = GatewayFrame::Input(SessionInput {
        session_id: session(1),
        line: InputLine::new("look"),
    });
    let (sent, accepted) =
        tokio::join!(gateway.send(stray), accept_resume(&mut world, world_id(1)),);
    sent.expect("gateway sends a stray frame");
    assert!(matches!(accepted, Err(IpcError::UnexpectedFrame)));
}

#[tokio::test]
async fn announce_sessions_rejects_a_non_ack_reply() {
    let (mut gateway, mut world) = in_memory_pair();
    let stray = WorldFrame::Output(SessionOutput {
        session_id: session(1),
        text: OutputText::new("not an ack"),
    });
    let (announced, replied) = tokio::join!(
        announce_sessions(&mut gateway, world_id(1), vec![]),
        async {
            // Consume the Gateway's resume, then reply with a non-ack frame.
            world.recv().await.expect("world receives the resume");
            world.send(stray).await
        },
    );
    replied.expect("world sends a stray reply");
    assert!(matches!(announced, Err(IpcError::UnexpectedFrame)));
}

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
