//! The M1 IPC frame vocabulary (§2.1.3, §2.7 step 2).
//!
//! Frames are split by direction so an illegal direction is unrepresentable:
//! the Gateway can only construct a [`GatewayFrame`], the World only a
//! [`WorldFrame`]. The transport (M1-11) length-prefixes and multiplexes these;
//! it never reinterprets a frame as the other direction.

use serde::{Deserialize, Serialize};

use crate::session::SessionId;

/// A single line of raw player input, as decoded from telnet/IAC by the Gateway
/// (§2.7 step 2).
///
/// A marker newtype: no invariant is enforced here. Content limits and
/// control-char/ANSI stripping (§3.6.4) are command-scoped (`say`/`emote`) and
/// applied downstream in the command pipeline (M1-17), so the IPC boundary
/// carries the line verbatim and must not re-validate it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct InputLine(String);

impl InputLine {
    /// Wraps a decoded line of player input.
    pub fn new(line: impl Into<String>) -> Self {
        Self(line.into())
    }

    /// Returns the input line.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Text rendered by the World for presentation to a client.
///
/// A marker newtype over the M1 text payload. M1-13 replaces it with
/// transport-neutral styled text (§3.20.1) rendered to ANSI per session on the
/// Gateway side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct OutputText(String);

impl OutputText {
    /// Wraps rendered output text.
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Returns the output text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A line of player input forwarded from Gateway to World (§2.7 step 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionInput {
    /// The session the input belongs to.
    pub session_id: SessionId,
    /// The decoded input line, stripped of telnet/IAC framing by the Gateway.
    pub line: InputLine,
}

/// Rendered output destined for one client session.
///
/// Carries plain [`OutputText`] for M1. M1-13 swaps the payload for styled text;
/// because the IPC schema is version-locked at build time (§2.8.5.7), Gateway and
/// World rebuild together and the change is free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionOutput {
    /// The session the output is destined for.
    pub session_id: SessionId,
    /// The text to present to the client.
    pub text: OutputText,
}

/// Announces that a client has connected and a new session exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionConnect {
    /// The newly minted session.
    pub session_id: SessionId,
}

/// Announces that a client connection has dropped (Gateway → World).
///
/// A structured reason (graceful vs. linkdead, §3.15.2) is deferred to M7-grade
/// linkdead handling; M1 needs only clean connect and quit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionDisconnect {
    /// The session that dropped.
    pub session_id: SessionId,
}

/// Instructs the Gateway to close a session's connection (World → Gateway;
/// World-initiated close, e.g. `quit` or a kick).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct SessionClose {
    /// The session to close.
    pub session_id: SessionId,
}

/// A frame sent from Gateway to World (§2.1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[must_use]
pub enum GatewayFrame {
    /// A client connected; a new session exists.
    Connect(SessionConnect),
    /// A line of player input.
    Input(SessionInput),
    /// A client connection dropped.
    Disconnect(SessionDisconnect),
}

/// A frame sent from World to Gateway (§2.1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[must_use]
pub enum WorldFrame {
    /// Rendered output for a session.
    Output(SessionOutput),
    /// A World-initiated request to close a session's connection.
    Close(SessionClose),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::codec::{decode, encode};

    fn session(value: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(value).expect("test session id must be non-zero"))
    }

    #[test]
    fn gateway_connect_round_trips() {
        let frame = GatewayFrame::Connect(SessionConnect {
            session_id: session(1),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<GatewayFrame>(&bytes).expect("decode"), frame);
    }

    #[test]
    fn gateway_input_round_trips() {
        let frame = GatewayFrame::Input(SessionInput {
            session_id: session(2),
            line: InputLine::new("look"),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<GatewayFrame>(&bytes).expect("decode"), frame);
    }

    #[test]
    fn gateway_disconnect_round_trips() {
        let frame = GatewayFrame::Disconnect(SessionDisconnect {
            session_id: session(3),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<GatewayFrame>(&bytes).expect("decode"), frame);
    }

    #[test]
    fn world_output_round_trips() {
        let frame = WorldFrame::Output(SessionOutput {
            session_id: session(4),
            text: OutputText::new("You see a room."),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<WorldFrame>(&bytes).expect("decode"), frame);
    }

    #[test]
    fn world_close_round_trips() {
        let frame = WorldFrame::Close(SessionClose {
            session_id: session(5),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(decode::<WorldFrame>(&bytes).expect("decode"), frame);
    }

    // Pins the postcard encoding of a representative frame so an accidental field
    // reorder or variant-order change is caught loudly. postcard encodes the enum
    // variant index as a varint, then the struct fields in declaration order:
    // GatewayFrame::Input = index 1; session_id = NonZeroU64 varint 2; line =
    // length-prefixed "hi" (len 2, bytes 0x68 0x69). The `InputLine` newtype is
    // serde-transparent, so wrapping the string does not change the bytes.
    #[test]
    fn input_frame_has_a_stable_encoding() {
        let frame = GatewayFrame::Input(SessionInput {
            session_id: session(2),
            line: InputLine::new("hi"),
        });
        let bytes = encode(&frame).expect("encode");
        assert_eq!(bytes, vec![0x01, 0x02, 0x02, 0x68, 0x69]);
    }
}
