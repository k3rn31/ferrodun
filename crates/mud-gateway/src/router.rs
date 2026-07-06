//! The router task: sole owner of the IPC endpoint and the session registry.
//!
//! `Endpoint::send`/`recv` take `&mut self`, so exactly one task may own the
//! channel. Every connection funnels registration and outbound frames through
//! one FIFO command channel: because an mpsc preserves per-sender order, a
//! connection's `Register` always precedes its `Connect`, so the World can
//! never address a session the router does not know yet.

use std::collections::HashMap;

use mud_ipc::Endpoint;
use mud_schema::{GatewayFrame, OutputText, SessionId, WorldFrame};
use tokio::sync::mpsc;

use crate::error::GatewayError;

/// Bound on per-connection output frames buffered ahead of the client socket.
/// Backpressure, not correctness: a slower client loses output beyond this
/// (see [`route`]) rather than stalling the router for every session.
pub(crate) const OUTPUT_CAPACITY: usize = 64;

/// What the router tells one connection task.
#[derive(Debug)]
pub(crate) enum ToConnection {
    /// Rendered text to write to the client, followed by a prompt frame.
    Output(OutputText),
    /// World-initiated close (§2.1.3): drop the connection, no `Disconnect` echo.
    Close,
}

/// What a connection task (or `serve`) tells the router.
#[derive(Debug)]
pub(crate) enum ToRouter {
    /// A new session exists; route its frames to `tx`. Sent before `Connect`.
    Register {
        session_id: SessionId,
        tx: mpsc::Sender<ToConnection>,
    },
    /// Forward a frame to the World.
    Frame(GatewayFrame),
    /// The session is gone; drop its registry entry.
    Deregister { session_id: SessionId },
}

/// Runs the router until the World closes the endpoint (`Ok`), every command
/// sender is dropped (`Ok`), or the endpoint fails (`Err`).
pub(crate) async fn run_router<E>(
    mut endpoint: E,
    mut commands: mpsc::Receiver<ToRouter>,
) -> Result<(), GatewayError>
where
    E: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame> + Send + 'static,
{
    let mut registry: HashMap<SessionId, mpsc::Sender<ToConnection>> = HashMap::new();
    loop {
        tokio::select! {
            frame = endpoint.recv() => match frame? {
                None => return Ok(()),
                Some(WorldFrame::Output(output)) => {
                    route(&registry, output.session_id, ToConnection::Output(output.text));
                }
                Some(WorldFrame::Close(close)) => {
                    route(&registry, close.session_id, ToConnection::Close);
                }
                // ResumeAck is consumed by the handshake before the router
                // starts; WorldFrame is #[non_exhaustive], so future variants
                // land here too until the gateway learns them.
                Some(other) => {
                    tracing::warn!(frame = ?other, "gateway router ignoring unexpected world frame");
                }
            },
            command = commands.recv() => match command {
                // All senders gone means `serve` and every connection ended.
                None => return Ok(()),
                Some(ToRouter::Register { session_id, tx }) => {
                    registry.insert(session_id, tx);
                }
                Some(ToRouter::Frame(frame)) => endpoint.send(frame).await?,
                Some(ToRouter::Deregister { session_id }) => {
                    registry.remove(&session_id);
                }
            },
        }
    }
}

/// Delivers one payload to a session, without blocking the router: a frame for
/// a departed session is dropped silently (the race is benign — the World
/// learns of the disconnect from the in-flight `Disconnect`), and a full
/// buffer drops the frame with a warning rather than stalling every session
/// behind one slow client.
fn route(
    registry: &HashMap<SessionId, mpsc::Sender<ToConnection>>,
    session_id: SessionId,
    payload: ToConnection,
) {
    let Some(tx) = registry.get(&session_id) else {
        return;
    };
    if let Err(err) = tx.try_send(payload) {
        tracing::warn!(%session_id, %err, "gateway dropping output for slow or departed session");
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use mud_ipc::{Endpoint, in_memory_pair};
    use mud_schema::{
        GatewayFrame, OutputText, SessionClose, SessionConnect, SessionId, SessionOutput,
        WorldFrame,
    };
    use tokio::sync::mpsc;

    use super::*;

    fn session(value: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(value).expect("test session id must be non-zero"))
    }

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

    #[tokio::test]
    async fn routes_output_to_the_registered_session() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        let (tx, mut output_rx) = mpsc::channel(OUTPUT_CAPACITY);
        let id = session(1);
        commands_tx
            .send(ToRouter::Register { session_id: id, tx })
            .await
            .expect("router must accept registration");

        drain_barrier(&commands_tx, &mut world_end, session(2)).await;

        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: id,
                text: OutputText::new("hello"),
            }))
            .await
            .expect("world endpoint must send");

        let routed = output_rx.recv().await.expect("output must be routed");
        assert!(matches!(routed, ToConnection::Output(text) if text.as_str() == "hello"));

        drop(world_end); // peer closes -> clean shutdown
        router
            .await
            .expect("router task must not panic")
            .expect("closed peer is a clean shutdown");
    }

    #[tokio::test]
    async fn close_frame_reaches_the_session_and_unknown_sessions_are_ignored() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        // A frame for a never-registered session must not error the router.
        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: session(99),
                text: OutputText::new("ghost"),
            }))
            .await
            .expect("world endpoint must send");

        let (tx, mut output_rx) = mpsc::channel(OUTPUT_CAPACITY);
        let id = session(2);
        commands_tx
            .send(ToRouter::Register { session_id: id, tx })
            .await
            .expect("router must accept registration");

        drain_barrier(&commands_tx, &mut world_end, session(3)).await;

        world_end
            .send(WorldFrame::Close(SessionClose { session_id: id }))
            .await
            .expect("world endpoint must send");

        let routed = output_rx.recv().await.expect("close must be routed");
        assert!(matches!(routed, ToConnection::Close));

        drop(commands_tx); // serve dropped -> clean shutdown
        router
            .await
            .expect("router task must not panic")
            .expect("dropped command channel is a clean shutdown");
    }

    #[tokio::test]
    async fn forwards_gateway_frames_to_the_world() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let _router = tokio::spawn(run_router(gateway_end, commands_rx));

        let id = session(3);
        commands_tx
            .send(ToRouter::Frame(GatewayFrame::Connect(SessionConnect {
                session_id: id,
            })))
            .await
            .expect("router must accept frames");

        let frame = world_end
            .recv()
            .await
            .expect("world endpoint must receive")
            .expect("frame must arrive");
        assert!(matches!(
            frame,
            GatewayFrame::Connect(SessionConnect { session_id }) if session_id == id
        ));
    }

    #[tokio::test]
    async fn deregistered_session_no_longer_receives_output() {
        let (gateway_end, mut world_end) = in_memory_pair();
        let (commands_tx, commands_rx) = mpsc::channel(8);
        let router = tokio::spawn(run_router(gateway_end, commands_rx));

        let (tx, mut output_rx) = mpsc::channel(OUTPUT_CAPACITY);
        let id = session(4);
        commands_tx
            .send(ToRouter::Register { session_id: id, tx })
            .await
            .expect("router must accept registration");
        commands_tx
            .send(ToRouter::Deregister { session_id: id })
            .await
            .expect("router must accept deregistration");

        drain_barrier(&commands_tx, &mut world_end, session(5)).await;

        // Now enqueue Output for the deregistered session; it must be dropped.
        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: id,
                text: OutputText::new("late"),
            }))
            .await
            .expect("world endpoint must send");

        drop(world_end); // endpoint closes -> router returns Ok after draining
        router
            .await
            .expect("router task must not panic")
            .expect("clean shutdown");
        assert!(
            output_rx.recv().await.is_none(),
            "deregistered session must see its channel close, not receive output"
        );
    }

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
}
