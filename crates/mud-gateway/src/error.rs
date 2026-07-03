//! Errors raised by the gateway runtime.

use mud_ipc::IpcError;

/// A fatal error that stops [`serve`](crate::serve).
///
/// Per-connection failures (client socket errors, EOF) are not represented
/// here — they end that connection only. `serve` returns an error only when
/// the listener or the IPC channel to the World is unusable.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GatewayError {
    /// Accepting a new client connection failed.
    #[error("gateway failed to accept a connection: {0}")]
    Accept(#[source] std::io::Error),

    /// The IPC channel to the World failed, including a rejected resume
    /// handshake ([`IpcError::SchemaMismatch`] / [`IpcError::WorldIdMismatch`]).
    #[error("gateway ipc channel error: {0}")]
    Ipc(#[from] IpcError),

    /// The session id counter wrapped to zero (2^64 connections).
    #[error("gateway session id space exhausted")]
    SessionIdOverflow,

    /// The router task terminated abnormally. Boxed so `tokio`'s `JoinError`
    /// does not leak into this crate's public API.
    #[error("gateway router task failed: {0}")]
    RouterTask(#[source] Box<dyn std::error::Error + Send + Sync>),
}
