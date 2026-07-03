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
mod error;
mod session;

pub use config::GatewayConfig;
pub use error::GatewayError;
