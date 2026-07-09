//! The duplex frame transport: an in-memory channel (single-process mode,
//! §2.1.3.3) and a length-prefixed `postcard` unix socket (split mode, §2.1.3.1).
//!
//! Both implement one [`Endpoint`] trait so the resume handshake and any consumer
//! can be written once over either transport. Direction is encoded in the
//! associated types: a Gateway-side endpoint sends `GatewayFrame` and receives
//! `WorldFrame`; a World-side endpoint is the mirror. Frames self-identify by
//! `session_id`, so a single channel multiplexes every session (§2.1.3.1);
//! splitting that stream into per-session sinks is the consumer's job, not the
//! transport's.

use std::future::Future;
use std::marker::PhantomData;
use std::path::Path;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use mud_schema::{GatewayFrame, WorldFrame, decode, encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::codec::length_delimited::LengthDelimitedCodecError;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::error::IpcError;

/// The maximum size of a single encoded IPC frame.
///
/// An explicit bound on an untrusted payload: a peer cannot make the transport
/// allocate without limit. Frames larger than this are rejected on send with
/// [`IpcError::FrameTooLarge`] and on receive by the length-delimited codec.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Bound on in-flight frames buffered by the in-memory transport before a sender
/// awaits. Backpressure, not a correctness bound — the value only trades memory
/// for fewer await points.
const IN_MEMORY_CAPACITY: usize = 256;

/// One end of a bidirectional, session-multiplexed IPC channel (§2.1.3).
///
/// `Ok(None)` from [`recv`](Endpoint::recv) means the peer closed the channel
/// cleanly; [`send`](Endpoint::send) fails with [`IpcError::PeerClosed`] once the
/// peer is gone.
pub trait Endpoint {
    /// The frames this end emits (`GatewayFrame` on the Gateway side).
    type Outbound: Send;
    /// The frames this end consumes (`WorldFrame` on the Gateway side).
    type Inbound: Send;

    /// Sends one frame to the peer.
    fn send(&mut self, frame: Self::Outbound) -> impl Future<Output = Result<(), IpcError>> + Send;

    /// Receives the next frame, or `Ok(None)` when the peer has closed the channel.
    fn recv(&mut self) -> impl Future<Output = Result<Option<Self::Inbound>, IpcError>> + Send;
}

/// An in-process endpoint backed by `tokio` channels (single-process mode, §2.1.3.3).
///
/// Carries typed frames directly, with no serialization — the same frame *contract*
/// as the socket transport, without the wire round-trip.
#[must_use]
pub struct InMemoryEndpoint<S, R> {
    tx: mpsc::Sender<S>,
    rx: mpsc::Receiver<R>,
}

impl<S: Send, R: Send> Endpoint for InMemoryEndpoint<S, R> {
    type Outbound = S;
    type Inbound = R;

    async fn send(&mut self, frame: S) -> Result<(), IpcError> {
        self.tx.send(frame).await.map_err(|_| IpcError::PeerClosed)
    }

    async fn recv(&mut self) -> Result<Option<R>, IpcError> {
        Ok(self.rx.recv().await)
    }
}

/// Creates a connected Gateway/World endpoint pair sharing two in-memory channels
/// (§2.1.3.3). The first endpoint speaks the Gateway side, the second the World
/// side; a frame sent on one arrives on the other.
pub fn in_memory_pair() -> (
    InMemoryEndpoint<GatewayFrame, WorldFrame>,
    InMemoryEndpoint<WorldFrame, GatewayFrame>,
) {
    let (gateway_tx, gateway_rx) = mpsc::channel(IN_MEMORY_CAPACITY);
    let (world_tx, world_rx) = mpsc::channel(IN_MEMORY_CAPACITY);
    let gateway = InMemoryEndpoint {
        tx: gateway_tx,
        rx: world_rx,
    };
    let world = InMemoryEndpoint {
        tx: world_tx,
        rx: gateway_rx,
    };
    (gateway, world)
}

/// A split-mode endpoint: length-prefixed `postcard` frames over a unix socket
/// (§2.1.3.1).
#[must_use]
pub struct SocketEndpoint<S, R> {
    framed: Framed<UnixStream, LengthDelimitedCodec>,
    // Records the send/receive frame direction without owning either; `fn() -> _`
    // keeps the endpoint `Send`/variance-neutral regardless of `S`/`R`.
    _direction: PhantomData<fn() -> (S, R)>,
}

impl<S, R> SocketEndpoint<S, R> {
    fn from_stream(stream: UnixStream) -> Self {
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(MAX_FRAME_BYTES)
            .new_codec();
        Self {
            framed: Framed::new(stream, codec),
            _direction: PhantomData,
        }
    }
}

impl<S, R> Endpoint for SocketEndpoint<S, R>
where
    S: Serialize + Send,
    R: DeserializeOwned + Send,
{
    type Outbound = S;
    type Inbound = R;

    async fn send(&mut self, frame: S) -> Result<(), IpcError> {
        let bytes = encode(&frame).map_err(|e| IpcError::Codec(Box::new(e)))?;
        if bytes.len() > MAX_FRAME_BYTES {
            // Length only — the payload may carry credentials (design §6).
            tracing::debug!(
                size = bytes.len(),
                max = MAX_FRAME_BYTES,
                "outbound ipc frame exceeds size cap"
            );
            return Err(IpcError::FrameTooLarge {
                size: Some(bytes.len()),
                max: MAX_FRAME_BYTES,
            });
        }
        self.framed.send(Bytes::from(bytes)).await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<R>, IpcError> {
        match self.framed.next().await {
            None => Ok(None),
            Some(Ok(bytes)) => decode(&bytes).map(Some).map_err(|e| {
                // Length only — never the bytes (design §6).
                tracing::debug!(len = bytes.len(), "inbound ipc frame failed to decode");
                IpcError::Codec(Box::new(e))
            }),
            Some(Err(err)) => Err(map_inbound_framing_error(err)),
        }
    }
}

/// Remaps a length-delimited framing error: the codec's max-frame-length
/// rejection becomes the typed [`IpcError::FrameTooLarge`], matching the send
/// path; any other transport failure stays [`IpcError::Io`]. The codec rejects
/// on the length header, so the exact frame size is unknown here (`size: None`).
fn map_inbound_framing_error(err: std::io::Error) -> IpcError {
    if err
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<LengthDelimitedCodecError>().is_some())
    {
        tracing::debug!(max = MAX_FRAME_BYTES, "inbound ipc frame exceeds size cap");
        return IpcError::FrameTooLarge {
            size: None,
            max: MAX_FRAME_BYTES,
        };
    }
    IpcError::Io(err)
}

/// Connects to a World listening on `path`, yielding the Gateway-side endpoint
/// (§2.1.3.1). The caller drives the resume handshake
/// ([`announce_sessions`](crate::announce_sessions)) before exchanging gameplay
/// frames.
///
/// # Errors
///
/// Returns [`IpcError::Io`] if the socket cannot be reached.
pub async fn connect(
    path: impl AsRef<Path>,
) -> Result<SocketEndpoint<GatewayFrame, WorldFrame>, IpcError> {
    let stream = UnixStream::connect(path).await?;
    Ok(SocketEndpoint::from_stream(stream))
}

/// Accepts the next Gateway connection on `listener`, yielding the World-side
/// endpoint (§2.1.3.1). The caller drives the resume handshake
/// ([`accept_resume`](crate::accept_resume)) before exchanging gameplay frames.
///
/// # Errors
///
/// Returns [`IpcError::Io`] if accepting the connection fails.
pub async fn accept(
    listener: &UnixListener,
) -> Result<SocketEndpoint<WorldFrame, GatewayFrame>, IpcError> {
    let (stream, _addr) = listener.accept().await?;
    Ok(SocketEndpoint::from_stream(stream))
}
