//! Encoding and decoding of IPC frames to and from `postcard` bytes.
//!
//! These helpers carry the frame *body* only. Length-prefixing and the socket /
//! in-memory transport that frames travel over are the transport layer's concern
//! (M1-11), not this crate's.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// An error encoding or decoding an IPC frame.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// The `postcard` codec failed to serialize or deserialize a frame.
    ///
    /// The underlying `postcard` error is boxed so the codec dependency does not
    /// leak into this crate's public API and a `postcard` version bump cannot
    /// break it.
    #[error("postcard codec error: {0}")]
    Postcard(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl SchemaError {
    /// Wraps a `postcard` failure, boxing it so the dependency stays out of the
    /// public API. Crate-internal: callers convert at the codec boundary only.
    pub(crate) fn from_postcard(err: postcard::Error) -> Self {
        Self::Postcard(Box::new(err))
    }
}

/// Serializes an IPC frame to `postcard` bytes.
///
/// # Errors
///
/// Returns [`SchemaError::Postcard`] if serialization fails.
pub fn encode<T: Serialize>(frame: &T) -> Result<Vec<u8>, SchemaError> {
    postcard::to_stdvec(frame).map_err(SchemaError::from_postcard)
}

/// Deserializes an IPC frame from `postcard` bytes.
///
/// # Errors
///
/// Returns [`SchemaError::Postcard`] if the bytes are not a valid encoding of
/// `T`.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, SchemaError> {
    postcard::from_bytes(bytes).map_err(SchemaError::from_postcard)
}
