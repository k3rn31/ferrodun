//! The resume handshake (§2.1.3.2).
//!
//! When a Gateway establishes an IPC channel to a World — including after a World
//! restart — it announces the World's `world_id` and the set of sessions it is
//! already holding, so a freshly started World can re-adopt them. Both sides
//! confirm they speak the same build-locked schema version (§2.8.5.7) before any
//! gameplay frame flows. Written over the [`Endpoint`] trait so the in-memory and
//! socket transports share one implementation.

use mud_schema::{
    GatewayFrame, HandshakeAck, ResumeHandshake, SCHEMA_VERSION, SchemaVersion, SessionId,
    WorldFrame, WorldId,
};

use crate::error::IpcError;
use crate::transport::Endpoint;

/// Gateway side: announces `live_sessions` for `world_id` and awaits the World's
/// acknowledgement (§2.1.3.2).
///
/// # Errors
///
/// Returns [`IpcError::SchemaMismatch`] or [`IpcError::WorldIdMismatch`] if the
/// World's acknowledgement disagrees, [`IpcError::UnexpectedFrame`] if a
/// non-acknowledgement frame arrives first, or [`IpcError::PeerClosed`] if the
/// World closes before acknowledging.
pub async fn announce_sessions<E>(
    endpoint: &mut E,
    world_id: WorldId,
    live_sessions: Vec<SessionId>,
) -> Result<(), IpcError>
where
    E: Endpoint<Outbound = GatewayFrame, Inbound = WorldFrame>,
{
    let handshake = ResumeHandshake {
        world_id,
        schema_version: SCHEMA_VERSION,
        live_sessions,
    };
    endpoint.send(GatewayFrame::Resume(handshake)).await?;

    match endpoint.recv().await? {
        Some(WorldFrame::ResumeAck(ack)) => {
            check_schema_version(ack.schema_version)?;
            check_world_id(world_id, ack.world_id)?;
            Ok(())
        }
        Some(_) => Err(IpcError::UnexpectedFrame),
        None => Err(IpcError::PeerClosed),
    }
}

/// World side: awaits the Gateway's resume announcement for `expected_world_id`,
/// acknowledges it, and returns the live session set to re-adopt (§2.1.3.2).
///
/// # Errors
///
/// Returns [`IpcError::SchemaMismatch`] or [`IpcError::WorldIdMismatch`] if the
/// announcement disagrees, [`IpcError::UnexpectedFrame`] if a non-handshake frame
/// arrives first, or [`IpcError::PeerClosed`] if the Gateway closes before
/// announcing.
pub async fn accept_resume<E>(
    endpoint: &mut E,
    expected_world_id: WorldId,
) -> Result<Vec<SessionId>, IpcError>
where
    E: Endpoint<Outbound = WorldFrame, Inbound = GatewayFrame>,
{
    match endpoint.recv().await? {
        Some(GatewayFrame::Resume(handshake)) => {
            check_schema_version(handshake.schema_version)?;
            check_world_id(expected_world_id, handshake.world_id)?;
            let ack = HandshakeAck {
                world_id: expected_world_id,
                schema_version: SCHEMA_VERSION,
            };
            endpoint.send(WorldFrame::ResumeAck(ack)).await?;
            Ok(handshake.live_sessions)
        }
        Some(_) => Err(IpcError::UnexpectedFrame),
        None => Err(IpcError::PeerClosed),
    }
}

fn check_schema_version(got: SchemaVersion) -> Result<(), IpcError> {
    if got == SCHEMA_VERSION {
        return Ok(());
    }
    tracing::warn!(
        expected = SCHEMA_VERSION.get(),
        got = got.get(),
        "ipc resume handshake rejected: schema version mismatch",
    );
    Err(IpcError::SchemaMismatch {
        expected: SCHEMA_VERSION.get(),
        got: got.get(),
    })
}

fn check_world_id(expected: WorldId, got: WorldId) -> Result<(), IpcError> {
    if got == expected {
        return Ok(());
    }
    tracing::warn!(
        expected = expected.get().get(),
        got = got.get().get(),
        "ipc resume handshake rejected: world id mismatch",
    );
    Err(IpcError::WorldIdMismatch {
        expected: expected.get().get(),
        got: got.get().get(),
    })
}
