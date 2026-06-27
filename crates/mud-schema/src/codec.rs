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

impl From<postcard::Error> for SchemaError {
    fn from(err: postcard::Error) -> Self {
        Self::Postcard(Box::new(err))
    }
}

/// Serializes an IPC frame to `postcard` bytes.
///
/// # Errors
///
/// Returns [`SchemaError::Postcard`] if serialization fails.
pub fn encode<T: Serialize>(frame: &T) -> Result<Vec<u8>, SchemaError> {
    Ok(postcard::to_stdvec(frame)?)
}

/// Deserializes an IPC frame from `postcard` bytes.
///
/// # Errors
///
/// Returns [`SchemaError::Postcard`] if the bytes are not a valid encoding of
/// `T`.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, SchemaError> {
    Ok(postcard::from_bytes(bytes)?)
}
