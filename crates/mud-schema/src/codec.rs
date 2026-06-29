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

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::frame::{GatewayFrame, SessionConnect, WorldFrame};
    use crate::session::SessionId;

    fn session(value: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(value).expect("test session id must be non-zero"))
    }

    #[test]
    fn decode_rejects_garbage_bytes() {
        let result = decode::<GatewayFrame>(&[0xFF, 0xFE, 0xFD]);
        assert!(matches!(result, Err(SchemaError::Postcard(_))));
    }

    #[test]
    fn decode_rejects_a_truncated_frame() {
        let frame = GatewayFrame::Input(crate::frame::SessionInput {
            session_id: session(2),
            line: crate::frame::InputLine::new("look"),
        });
        let bytes = encode(&frame).expect("encode");

        // Drop the payload's trailing bytes: the length prefix now over-promises.
        let truncated = bytes
            .get(..bytes.len() - 2)
            .expect("encoded input frame is longer than two bytes");
        assert!(matches!(
            decode::<GatewayFrame>(truncated),
            Err(SchemaError::Postcard(_))
        ));
    }

    #[test]
    fn decode_rejects_a_frame_of_the_wrong_direction() {
        // A GatewayFrame must not decode as a WorldFrame. Their variant index
        // spaces differ, so the directional split is enforced at the wire too.
        let frame = GatewayFrame::Connect(SessionConnect {
            session_id: session(1),
        });
        let bytes = encode(&frame).expect("encode");
        assert!(decode::<WorldFrame>(&bytes).is_err());
    }

    #[test]
    fn schema_error_display_names_the_codec() {
        let err = decode::<GatewayFrame>(&[0xFF, 0xFE, 0xFD]).expect_err("garbage must not decode");
        assert!(err.to_string().contains("postcard"));
    }
}
