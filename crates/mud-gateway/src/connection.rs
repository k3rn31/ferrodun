//! The per-connection task: owns one client socket, its telnet state machine,
//! and its rate limiter.
//!
//! Exit cause decides the goodbye: a client hang-up owes the World a
//! `Disconnect`; a World-initiated close does not (the World already knows,
//! an echo would be spurious). Either way the session deregisters from the
//! router.

use std::time::Instant;

use mud_net::{Decision, RateLimiter, TelnetEvent, TelnetMachine};
use mud_schema::{GatewayFrame, InputLine, SessionDisconnect, SessionId, SessionInput};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc;

use crate::router::{OUTPUT_CAPACITY, ToConnection, ToRouter};

/// Why the connection loop ended.
enum ExitCause {
    /// EOF or socket error: the World must be told via `Disconnect`.
    ClientGone,
    /// The World closed the session (or the router is gone): no echo owed.
    WorldClosed,
}

/// Serves one client connection until the client hangs up or the World closes
/// the session. Infallible from the caller's view: every failure path is a
/// per-connection teardown, never a gateway-wide error.
pub(crate) async fn run_connection<S>(
    socket: S,
    session_id: SessionId,
    to_router: mpsc::Sender<ToRouter>,
    mut limiter: RateLimiter,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Split so the select! read future can borrow the read half while the
    // arm handlers write to the write half — one `&mut socket` in both arms
    // would not borrow-check.
    let (mut reader, mut writer) = tokio::io::split(socket);
    let (tx, mut output_rx) = mpsc::channel(OUTPUT_CAPACITY);
    // Register before Connect on the same FIFO channel: the router knows the
    // session before the World can possibly address it.
    if to_router
        .send(ToRouter::Register { session_id, tx })
        .await
        .is_err()
    {
        return; // router gone: the gateway is shutting down
    }
    let connect = GatewayFrame::Connect(mud_schema::SessionConnect { session_id });
    if to_router.send(ToRouter::Frame(connect)).await.is_err() {
        return;
    }

    let mut machine = TelnetMachine::new();
    let cause = if writer.write_all(&machine.take_output()).await.is_err() {
        ExitCause::ClientGone
    } else {
        connection_loop(
            &mut reader,
            &mut writer,
            &mut machine,
            &mut limiter,
            session_id,
            &to_router,
            &mut output_rx,
        )
        .await
    };

    match cause {
        ExitCause::ClientGone => {
            let disconnect = GatewayFrame::Disconnect(SessionDisconnect { session_id });
            // Best effort: if the router is gone the gateway is shutting down
            // anyway and the World learns nothing more from us.
            let _ = to_router.send(ToRouter::Frame(disconnect)).await;
        }
        ExitCause::WorldClosed => {}
    }
    let _ = to_router.send(ToRouter::Deregister { session_id }).await;
}

/// The steady-state read/write loop; returns why it stopped.
async fn connection_loop<S>(
    reader: &mut ReadHalf<S>,
    writer: &mut WriteHalf<S>,
    machine: &mut TelnetMachine,
    limiter: &mut RateLimiter,
    session_id: SessionId,
    to_router: &mpsc::Sender<ToRouter>,
    output_rx: &mut mpsc::Receiver<ToConnection>,
) -> ExitCause
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buf = [0u8; 1024];
    loop {
        tokio::select! {
            read = reader.read(&mut buf) => {
                let bytes = match read {
                    Ok(0) | Err(_) => return ExitCause::ClientGone,
                    Ok(n) => {
                        let Some(bytes) = buf.get(..n) else {
                            // INVARIANT: `read` returns at most `buf.len()`.
                            return ExitCause::ClientGone;
                        };
                        bytes
                    }
                };
                for event in machine.receive(bytes) {
                    if handle_event(event, limiter, session_id, to_router).await.is_err() {
                        return ExitCause::WorldClosed; // router gone
                    }
                }
                // Negotiation replies queued by the received bytes.
                let replies = machine.take_output();
                if !replies.is_empty() && writer.write_all(&replies).await.is_err() {
                    return ExitCause::ClientGone;
                }
            }
            output = output_rx.recv() => match output {
                Some(ToConnection::Output(text)) => {
                    let mut bytes = machine.encode_output(text.as_str());
                    // One rendered block = one prompt frame (§2.8.2 EOR/GA).
                    bytes.extend_from_slice(&machine.prompt_frame());
                    if writer.write_all(&bytes).await.is_err() {
                        return ExitCause::ClientGone;
                    }
                }
                // Close, or the router dropped the registry entry / shut down.
                Some(ToConnection::Close) | None => return ExitCause::WorldClosed,
            },
        }
    }
}

/// Forwards one telnet event to the World, rate-limiting command lines.
/// `Err` means the router is gone.
async fn handle_event(
    event: TelnetEvent,
    limiter: &mut RateLimiter,
    session_id: SessionId,
    to_router: &mpsc::Sender<ToRouter>,
) -> Result<(), ()> {
    match event {
        TelnetEvent::Line(line) => match limiter.check(Instant::now()) {
            Decision::Forward => {
                let input = GatewayFrame::Input(SessionInput {
                    session_id,
                    line: InputLine::new(line),
                });
                to_router.send(ToRouter::Frame(input)).await.map_err(|_| ())
            }
            Decision::Drop => {
                // M1 drops silently; the structured `rate_limited` event
                // needs a structured channel (GMCP) and lands in M3
                // (see PLAN.md, M3 telnet extensions).
                tracing::debug!(%session_id, "gateway dropped rate-limited command");
                Ok(())
            }
        },
        // No M1 consumer: NAWS-driven pagination and TTYPE-driven defaults
        // arrive with later milestones. TelnetEvent is #[non_exhaustive].
        TelnetEvent::WindowSize { .. } | TelnetEvent::TerminalType(_) => Ok(()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::Instant;

    use mud_net::{Burst, RateLimiter, SustainedRate};
    use mud_schema::{GatewayFrame, OutputText, SessionId};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
    use tokio::sync::mpsc;

    use crate::router::{ToConnection, ToRouter};

    use super::*;

    const TEST_SESSION: u64 = 7;

    fn session() -> SessionId {
        SessionId::new(NonZeroU64::new(TEST_SESSION).expect("test session id must be non-zero"))
    }

    fn default_limiter() -> RateLimiter {
        RateLimiter::new(SustainedRate::DEFAULT, Burst::DEFAULT, Instant::now())
    }

    /// Spawns a connection task over an in-memory duplex "socket", returning
    /// the client half and the router-side receiver.
    fn spawn_connection(
        limiter: RateLimiter,
    ) -> (
        DuplexStream,
        mpsc::Receiver<ToRouter>,
        tokio::task::JoinHandle<()>,
    ) {
        let (client, server) = tokio::io::duplex(4096);
        let (to_router, router_rx) = mpsc::channel(8);
        let task = tokio::spawn(run_connection(server, session(), to_router, limiter));
        (client, router_rx, task)
    }

    /// Receives from the router channel, skipping the initial Register.
    async fn expect_register(
        router_rx: &mut mpsc::Receiver<ToRouter>,
    ) -> mpsc::Sender<ToConnection> {
        match router_rx
            .recv()
            .await
            .expect("connection must register first")
        {
            ToRouter::Register { session_id, tx } => {
                assert_eq!(
                    session_id,
                    session(),
                    "registration must carry the session id"
                );
                tx
            }
            other => panic!("expected Register first, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn registers_then_connects_then_forwards_input_lines() {
        let (mut client, mut router_rx, _task) = spawn_connection(default_limiter());

        let _tx = expect_register(&mut router_rx).await;
        let connect = router_rx
            .recv()
            .await
            .expect("connect must follow register");
        assert!(matches!(
            connect,
            ToRouter::Frame(GatewayFrame::Connect(ref c)) if c.session_id == session()
        ));

        // Opening negotiation offers must be written to the client on start.
        let mut offers = [0u8; 12];
        client
            .read_exact(&mut offers)
            .await
            .expect("opening offers must be written");
        assert_eq!(
            offers,
            [255, 253, 31, 255, 253, 24, 255, 251, 25, 255, 251, 42],
            "DO NAWS, DO TTYPE, WILL EOR, WILL CHARSET"
        );

        client
            .write_all(b"look\r\n")
            .await
            .expect("client write must succeed");
        let input = router_rx.recv().await.expect("input must be forwarded");
        assert!(matches!(
            input,
            ToRouter::Frame(GatewayFrame::Input(ref i))
                if i.session_id == session() && i.line.as_str() == "look"
        ));
    }

    #[tokio::test]
    async fn output_is_encoded_and_prompt_framed() {
        let (mut client, mut router_rx, _task) = spawn_connection(default_limiter());

        let tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        let mut offers = [0u8; 12];
        client
            .read_exact(&mut offers)
            .await
            .expect("opening offers");

        tx.send(ToConnection::Output(OutputText::new("hello\n")))
            .await
            .expect("connection must accept output");

        // "hello\n" -> "hello\r\n"; no EOR negotiated -> IAC GA prompt frame.
        let mut buf = [0u8; 9];
        client
            .read_exact(&mut buf)
            .await
            .expect("output must be written");
        assert_eq!(&buf, b"hello\r\n\xff\xf9");
    }

    #[tokio::test]
    async fn client_eof_sends_disconnect_then_deregisters() {
        let (client, mut router_rx, task) = spawn_connection(default_limiter());

        let _tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        drop(client); // client hangs up

        let disconnect = router_rx.recv().await.expect("disconnect must be sent");
        assert!(matches!(
            disconnect,
            ToRouter::Frame(GatewayFrame::Disconnect(ref d)) if d.session_id == session()
        ));
        let deregister = router_rx.recv().await.expect("deregister must follow");
        assert!(matches!(
            deregister,
            ToRouter::Deregister { session_id } if session_id == session()
        ));
        task.await.expect("connection task must not panic");
    }

    #[tokio::test]
    async fn world_close_deregisters_without_disconnect_echo() {
        let (_client, mut router_rx, task) = spawn_connection(default_limiter());

        let tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        tx.send(ToConnection::Close)
            .await
            .expect("close must be accepted");

        // The very next router message must be Deregister — no Disconnect echo:
        // the World initiated the close and already knows.
        let next = router_rx.recv().await.expect("deregister must be sent");
        assert!(matches!(
            next,
            ToRouter::Deregister { session_id } if session_id == session()
        ));
        task.await.expect("connection task must not panic");
    }

    #[tokio::test]
    async fn throttled_lines_are_dropped_silently() {
        // burst 1, rate 1/s: the first line forwards, immediate repeats drop.
        let one = NonZeroU32::MIN;
        let limiter = RateLimiter::new(SustainedRate::new(one), Burst::new(one), Instant::now());
        let (mut client, mut router_rx, task) = spawn_connection(limiter);

        let _tx = expect_register(&mut router_rx).await;
        let _connect = router_rx.recv().await.expect("connect frame");

        client
            .write_all(b"one\r\ntwo\r\nthree\r\n")
            .await
            .expect("client write must succeed");
        drop(client);

        let first = router_rx.recv().await.expect("first line must forward");
        assert!(matches!(
            first,
            ToRouter::Frame(GatewayFrame::Input(ref i)) if i.line.as_str() == "one"
        ));
        // M1 drops throttled commands silently (structured `rate_limited`
        // event deferred to M3/GMCP) — so after EOF the next frames must be
        // Disconnect + Deregister, never "two"/"three".
        let after = router_rx.recv().await.expect("disconnect must follow");
        assert!(matches!(
            after,
            ToRouter::Frame(GatewayFrame::Disconnect(_))
        ));
        task.await.expect("connection task must not panic");
    }
}
