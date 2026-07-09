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
    let live_count = live_sessions.len();
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
            tracing::debug!(%world_id, live_sessions = live_count, "ipc resume handshake accepted");
            Ok(())
        }
        Some(_) => {
            tracing::debug!("ipc resume handshake rejected: unexpected frame");
            Err(IpcError::UnexpectedFrame)
        }
        None => {
            tracing::debug!("ipc resume handshake rejected: peer closed");
            Err(IpcError::PeerClosed)
        }
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
            tracing::debug!(
                world_id = %expected_world_id,
                live_sessions = handshake.live_sessions.len(),
                "ipc resume handshake accepted"
            );
            Ok(handshake.live_sessions)
        }
        Some(_) => {
            tracing::debug!("ipc resume handshake rejected: unexpected frame");
            Err(IpcError::UnexpectedFrame)
        }
        None => {
            tracing::debug!("ipc resume handshake rejected: peer closed");
            Err(IpcError::PeerClosed)
        }
    }
}

fn check_schema_version(got: SchemaVersion) -> Result<(), IpcError> {
    if got == SCHEMA_VERSION {
        return Ok(());
    }
    // Debug, not warn: the mismatch propagates as a typed error and becomes a
    // fatal `error` at the boundary; the site event is diagnostic detail
    // (design §3 — warn is reserved for builder-content faults).
    tracing::debug!(
        expected = %SCHEMA_VERSION,
        got = %got,
        "ipc resume handshake rejected: schema version mismatch",
    );
    Err(IpcError::SchemaMismatch {
        expected: SCHEMA_VERSION,
        got,
    })
}

fn check_world_id(expected: WorldId, got: WorldId) -> Result<(), IpcError> {
    if got == expected {
        return Ok(());
    }
    // Debug, not warn: the mismatch propagates as a typed error and becomes a
    // fatal `error` at the boundary; the site event is diagnostic detail
    // (design §3 — warn is reserved for builder-content faults).
    tracing::debug!(
        expected = %expected,
        got = %got,
        "ipc resume handshake rejected: world id mismatch",
    );
    Err(IpcError::WorldIdMismatch { expected, got })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use tracing_test::traced_test;

    use super::*;
    use crate::transport::in_memory_pair;

    #[tokio::test]
    #[traced_test]
    async fn a_successful_handshake_logs_acceptance_at_debug() {
        let (mut gateway, mut world) = in_memory_pair();
        let world_id = WorldId::new(NonZeroU64::new(42).expect("42 is non-zero"));

        let (announced, accepted) = tokio::join!(
            announce_sessions(&mut gateway, world_id, Vec::new()),
            accept_resume(&mut world, world_id),
        );
        announced.expect("gateway side of the handshake succeeds");
        accepted.expect("world side of the handshake succeeds");

        assert!(logs_contain("ipc resume handshake accepted"));
    }
}
