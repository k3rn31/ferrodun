//! Per-tenant World loop: ticks the durable scheduler and routes gateway
//! frames to the session driver and command pipeline (design §6).

use std::sync::Arc;

use anyhow::Context;
use mud_cmd::Command;
use mud_core::{MutationCommand, TICK_PERIOD, TickEvent};
use mud_db::PersistentWorld;
use mud_engine::{Pipeline, PipelineError, Routing, SessionDisposition, SessionService};
use mud_ipc::{Endpoint, InMemoryEndpoint, accept_resume};
use mud_schema::{GatewayFrame, SessionClose, SessionInput, WorldFrame, WorldId};
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;

use crate::backend::DbBackend;
use crate::places::WorldPlaces;

/// One tenant's assembled service stack (design §Boot): the durable world plus
/// the session, command, and place machinery that [`run`] threads through the
/// tick/route loop and into [`handle_input`].
pub struct TenantRuntime {
    pub world: Arc<Mutex<PersistentWorld>>,
    pub backend: DbBackend,
    pub sessions: SessionService,
    pub pipeline: Pipeline,
    pub builtins: Vec<Command>,
    pub places: WorldPlaces,
}

/// Drives one tenant's World: ticks the durable scheduler and routes gateway
/// frames arriving over `endpoint` to the session driver and command
/// pipeline. Returns `Ok(())` when the gateway closes the channel cleanly;
/// any other outcome is a fatal, fail-stop error (design §8).
///
/// # Errors
///
/// Returns an error if the resume handshake fails, the durable tick fails, the
/// IPC channel faults, or the per-run command-id space is exhausted.
pub async fn run(
    mut endpoint: InMemoryEndpoint<WorldFrame, GatewayFrame>,
    world_id: WorldId,
    mut rt: TenantRuntime,
) -> anyhow::Result<()> {
    let live = accept_resume(&mut endpoint, world_id)
        .await
        .context("ipc resume handshake")?;
    anyhow::ensure!(
        live.is_empty(),
        "a fresh gateway must announce no live sessions"
    );

    let mut ticker = tokio::time::interval(TICK_PERIOD);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Fail-stop on DbError: `?` ends the loop, `run` returns Err,
                // main exits non-zero, the supervisor restarts (design §8).
                let events = rt.world.lock().await.tick().await.context("durable tick")?;
                for event in events {
                    log_tick_event(&event);
                }
            }
            frame = endpoint.recv() => match frame.context("ipc recv")? {
                None => return Ok(()), // gateway closed cleanly
                Some(GatewayFrame::Connect(connect)) => {
                    for output in rt.sessions.connect(connect.session_id) {
                        endpoint.send(WorldFrame::Output(output)).await.context("send output")?;
                    }
                }
                Some(GatewayFrame::Disconnect(disconnect)) => rt.sessions.disconnect(disconnect.session_id),
                Some(GatewayFrame::Input(input)) => {
                    handle_input(&mut endpoint, &mut rt, input).await?;
                }
                Some(GatewayFrame::Resume(_)) => {
                    tracing::debug!("unexpected mid-stream resume frame dropped");
                }
                Some(_) => {
                    // INVARIANT: GatewayFrame is #[non_exhaustive]; a future
                    // variant this build doesn't know is treated as fatal
                    // rather than silently dropped.
                    anyhow::bail!("unknown gateway frame variant");
                }
            }
        }
    }
}

/// Logs one tick event. Precondition failures and rejections are routine
/// gameplay outcomes on the 20 Hz hot path — `trace`, never `warn`, or a
/// blocked action floods the log (design §3). Effect/precondition payloads
/// are omitted: they are `#[non_exhaustive]` and a future variant may carry
/// player text (design §6 never-log rules).
fn log_tick_event(event: &TickEvent) {
    match event {
        TickEvent::Created { entity } => tracing::debug!(?entity, "entity created"),
        TickEvent::PreconditionFailed { .. } => tracing::trace!("tick precondition failed"),
        TickEvent::Rejected { error, .. } => tracing::trace!(%error, "tick effect rejected"),
        // INVARIANT: TickEvent is #[non_exhaustive]; an unknown variant means
        // this build disagrees with itself — an operator-actionable fault.
        _ => tracing::error!("unrecognized tick event"),
    }
}

/// Routes one input line: pre-login input goes through the session FSM,
/// in-world input through the command pipeline. Never holds the world lock
/// across `sessions.on_input`; it is acquired only for the in-world dispatch.
#[tracing::instrument(name = "world_input", level = "info", skip_all, fields(session_id = %input.session_id))]
async fn handle_input(
    endpoint: &mut InMemoryEndpoint<WorldFrame, GatewayFrame>,
    rt: &mut TenantRuntime,
    input: SessionInput,
) -> anyhow::Result<()> {
    let session_id = input.session_id;
    match rt
        .sessions
        .on_input(session_id, input.line.as_str(), &rt.backend)
        .await
    {
        Routing::Login { outputs, close } => {
            for output in outputs {
                endpoint
                    .send(WorldFrame::Output(output))
                    .await
                    .context("send output")?;
            }
            if close {
                endpoint
                    .send(WorldFrame::Close(SessionClose { session_id }))
                    .await
                    .context("send close")?;
                rt.sessions.disconnect(session_id);
            }
        }
        Routing::InWorld => {
            let mut guard = rt.world.lock().await;
            let dispatched = rt.pipeline.dispatch(
                guard.world(),
                &rt.places,
                &rt.sessions.resolver(&rt.builtins),
                &input,
            );
            match dispatched {
                Ok(outcome) => {
                    for effect in outcome.effects {
                        guard.submit(MutationCommand::new(effect));
                    }
                    drop(guard);
                    for output in outcome.outputs {
                        endpoint
                            .send(WorldFrame::Output(output))
                            .await
                            .context("send output")?;
                    }
                    if matches!(outcome.disposition, SessionDisposition::Close) {
                        endpoint
                            .send(WorldFrame::Close(SessionClose { session_id }))
                            .await
                            .context("send close")?;
                        rt.sessions.disconnect(session_id);
                    }
                }
                Err(PipelineError::UnknownSession(session)) => {
                    drop(guard);
                    tracing::debug!(session_id = %session, "dispatch for unknown session dropped");
                }
                Err(PipelineError::CommandIdExhausted) => {
                    drop(guard);
                    anyhow::bail!("command-id space exhausted");
                }
                // INVARIANT: PipelineError is #[non_exhaustive]; treat any
                // future variant as fatal rather than silently dropping it.
                Err(error) => {
                    drop(guard);
                    return Err(anyhow::Error::from(error)).context("pipeline dispatch");
                }
            }
        }
        Routing::Unknown => tracing::debug!(%session_id, "input for unknown session dropped"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mud_core::{EntityId, Generation, SlotIndex, TenantTag, TickEvent};
    use tracing_test::traced_test;

    use super::log_tick_event;

    #[test]
    #[traced_test]
    fn entity_creation_logs_at_debug_not_info() {
        let entity = EntityId::new(
            TenantTag::default(),
            SlotIndex::new(1),
            Generation::new(0).expect("generation 0 is in range"),
        );
        log_tick_event(&TickEvent::Created { entity });

        logs_assert(|lines: &[&str]| {
            let created: Vec<_> = lines
                .iter()
                .filter(|line| line.contains("entity created"))
                .collect();
            match created.as_slice() {
                [line] if line.contains("DEBUG") => Ok(()),
                [line] => Err(format!("expected DEBUG, got: {line}")),
                other => Err(format!("expected exactly one line, got {}", other.len())),
            }
        });
    }
}
