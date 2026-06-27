//! Errors raised by the IPC transport.

use mud_schema::{SchemaVersion, WorldId};

/// An error on the Gateway↔World IPC channel (§2.1.3).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IpcError {
    /// The underlying socket transport failed.
    #[error("ipc transport i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// A frame could not be encoded to or decoded from `postcard` bytes.
    ///
    /// The codec error is boxed so neither `mud_schema` nor `postcard` leaks
    /// into this crate's public API.
    #[error("ipc frame codec error: {0}")]
    Codec(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The peer announced a schema version this build does not speak (§2.8.5.7).
    #[error("ipc schema version mismatch: this build speaks {expected}, peer announced {got}")]
    SchemaMismatch {
        /// The schema version this build was compiled against.
        expected: SchemaVersion,
        /// The version the peer announced in the handshake.
        got: SchemaVersion,
    },

    /// The peer addressed a different World than this channel serves (§2.1.3.2).
    #[error("ipc world id mismatch: channel serves {expected}, peer announced {got}")]
    WorldIdMismatch {
        /// The World this channel serves.
        expected: WorldId,
        /// The World the peer announced in the handshake.
        got: WorldId,
    },

    /// A frame exceeded the maximum on-wire size (§3.6.4-adjacent untrusted-input bound).
    #[error("ipc frame of {size} bytes exceeds the maximum of {max} bytes")]
    FrameTooLarge {
        /// The encoded size of the offending frame.
        size: usize,
        /// The configured maximum, [`MAX_FRAME_BYTES`](crate::MAX_FRAME_BYTES).
        max: usize,
    },

    /// A non-handshake frame arrived where the resume handshake was expected (§2.1.3.2).
    #[error("ipc handshake expected but a different frame arrived")]
    UnexpectedFrame,

    /// The peer closed the channel before the expected frame arrived.
    #[error("ipc peer closed the channel")]
    PeerClosed,
}
