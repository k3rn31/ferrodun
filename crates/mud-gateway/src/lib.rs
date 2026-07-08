//! # Gateway — telnet listener bridged to a World over IPC
//!
//! [`serve`] accepts inbound telnet connections and bridges each to a World
//! over the session-multiplexed [`mud_ipc::Endpoint`]. It drives the resume
//! handshake, then runs an actor-style router task (sole owner of the endpoint
//! and the `SessionId → connection` registry) and one task per connection
//! (driving the telnet state machine and rate limiter). Generic over
//! [`Endpoint`]: `mudd` embeds it in-proc in single-process mode or drives it
//! over the unix socket in split mode (§2.1.1, §2.1.3).
//!
//! The Gateway holds no session state of its own beyond the live sockets and
//! their negotiated protocol state; session lifecycle lives in the World. On
//! IPC loss the gateway shuts down cleanly — holding connections open for a
//! reconnect is a later milestone (M7).

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
use tracing::Instrument;

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
                let (socket, addr) = accepted.map_err(GatewayError::Accept)?;
                let session_id = minter.next()?;
                // The peer IP is PII: logged exactly once, at debug, keyed by
                // session_id for on-demand abuse correlation — never at info
                // and never repeated per-frame (design §6).
                tracing::debug!(%session_id, peer = %addr, "connection accepted");
                let limiter = RateLimiter::new(config.rate, config.burst, Instant::now());
                let span = tracing::info_span!("session", %session_id);
                tokio::spawn(run_connection(socket, session_id, to_router.clone(), limiter).instrument(span));
            }
        }
    }
}
