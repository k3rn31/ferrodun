//! # Gateway — telnet connection handler
//!
//! The Gateway accepts inbound telnet connections and routes each to a [`mud_session::Session`].
//! Each session is pinned to a [`mud_engine::World`] at connect time; after handshake it may
//! migrate to another world via OOB commands, but a single [`Session`] is its session lifetime.
//! If the world crashes, the session is dropped; if the player is still connected, the client sees
//! an abrupt disconnect and must reconnect.
//!
//! The Gateway is **stateless**: its only live data is the set of peer TCP/TLS sockets and their
//! negotiated protocol state. Once a [`Session`] is established, the Gateway does not track it —
//! the World does. Reconnection is a new [`Session`].
//!
//! **Task allocation:**
//! - Task 1 (this): crate scaffold, error/config types, session-id minting.
//! - Task 2: telnet socket reader; telnet line/frame parsing (via `mud_net`).
//! - Task 3: telnet socket writer; buffering + backpressure.
//! - Task 4: IPC negotiation + World channel acquire.
//! - Task 5: Session FSM (idle, setup, play, quit), built-in commands.
//! - Task 6: Reconnect handshake.
//! - Task 7: Alternative protocols: login-phase gating, MCP/GMCP (M7), HTTP/TLS/SSH/WebSocket listeners (M3+).

mod config;
mod connection;
mod error;
mod router;
mod session;

use std::time::Instant;

use mud_ipc::{Endpoint, announce_sessions};
use mud_net::RateLimiter;
use mud_schema::{GatewayFrame, WorldFrame};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub use config::GatewayConfig;
pub use error::GatewayError;

use crate::connection::run_connection;
use crate::router::run_router;
use crate::session::SessionMinter;

/// Bound on in-flight commands from all connections to the router.
/// Backpressure only: a full channel makes senders await, never drops.
const COMMAND_CAPACITY: usize = 256;

/// Serves telnet clients on `listener`, bridging them to a World over
/// `endpoint`.
///
/// Drives the resume handshake (§2.1.3.2) first, then accepts connections
/// until the World closes the IPC channel (returns `Ok(())`) or a fatal error
/// occurs. `listener` is passed in already bound so the caller controls the
/// address. M1 assumes the World stays up: on IPC loss the gateway shuts down
/// rather than holding connections for a reconnect (that behavior is M7).
///
/// # Errors
///
/// [`GatewayError::Ipc`] if the handshake or IPC channel fails,
/// [`GatewayError::Accept`] if the listener fails, and the remaining variants
/// per [`GatewayError`].
pub async fn serve<E>(
    listener: TcpListener,
    mut endpoint: E,
    config: GatewayConfig,
) -> Result<(), GatewayError>
where
    E: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame> + Send + 'static,
{
    // A fresh gateway holds no sessions; re-announcing a live set is the M7
    // reconnect path.
    announce_sessions(&mut endpoint, config.world_id, Vec::new()).await?;

    let (to_router, commands) = mpsc::channel(COMMAND_CAPACITY);
    let mut router = tokio::spawn(run_router(endpoint, commands));
    let minter = SessionMinter::new();

    loop {
        tokio::select! {
            finished = &mut router => {
                return finished.map_err(|err| GatewayError::RouterTask(Box::new(err)))?;
            }
            accepted = listener.accept() => {
                let (socket, _addr) = accepted.map_err(GatewayError::Accept)?;
                let session_id = minter.next()?;
                let limiter = RateLimiter::new(config.rate, config.burst, Instant::now());
                tokio::spawn(run_connection(socket, session_id, to_router.clone(), limiter));
            }
        }
    }
}
