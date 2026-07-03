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
#[allow(dead_code)] // LINT: used by Task 3 (connection task)
pub(crate) const OUTPUT_CAPACITY: usize = 64;

/// What the router tells one connection task.
#[derive(Debug)]
#[allow(dead_code)] // LINT: used by Task 3 (connection task)
pub(crate) enum ToConnection {
    /// Rendered text to write to the client, followed by a prompt frame.
    Output(OutputText),
    /// World-initiated close (§2.1.3): drop the connection, no `Disconnect` echo.
    Close,
}

/// What a connection task (or `serve`) tells the router.
#[derive(Debug)]
#[allow(dead_code)] // LINT: used by Task 3 (connection task) and Task 4 (serve task)
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
#[allow(dead_code)] // LINT: used by Task 4 (serve task)
pub(crate) async fn run_router<E>(
    mut endpoint: E,
    mut commands: mpsc::Receiver<ToRouter>,
) -> Result<(), GatewayError>
where
    E: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame>,
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
#[allow(dead_code)] // LINT: used by run_router (both marked dead_code in this task)
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
        world_end
            .send(WorldFrame::Output(SessionOutput {
                session_id: id,
                text: OutputText::new("late"),
            }))
            .await
            .expect("world endpoint must send");

        drop(world_end); // ends the router, closing the registry entry it removed
        router
            .await
            .expect("router task must not panic")
            .expect("clean shutdown");
        assert!(
            output_rx.recv().await.is_none(),
            "deregistered session must see its channel close, not receive output"
        );
    }
}
