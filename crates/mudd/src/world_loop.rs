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

/// Drives one tenant's World: ticks the durable scheduler and routes gateway
/// frames arriving over `endpoint` to the session driver and command
/// pipeline. Returns `Ok(())` when the gateway closes the channel cleanly;
/// any other outcome is a fatal, fail-stop error (design §8).
///
/// # Errors
///
/// Returns an error if the resume handshake fails, the durable tick fails, the
/// IPC channel faults, or the per-run command-id space is exhausted.
// LINT: each parameter is a distinct piece of one tenant's assembled stack
// (design §Boot); bundling them into a struct would just move the same eight
// fields without adding a meaningful abstraction (single call site).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    mut endpoint: InMemoryEndpoint<WorldFrame, GatewayFrame>,
    world_id: WorldId,
    world: Arc<Mutex<PersistentWorld>>,
    backend: DbBackend,
    mut sessions: SessionService,
    mut pipeline: Pipeline,
    builtins: Vec<Command>,
    places: WorldPlaces,
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
                let events = world.lock().await.tick().await.context("durable tick")?;
                for event in events {
                    log_tick_event(&event);
                }
            }
            frame = endpoint.recv() => match frame.context("ipc recv")? {
                None => return Ok(()), // gateway closed cleanly
                Some(GatewayFrame::Connect(connect)) => {
                    for output in sessions.connect(connect.session_id) {
                        endpoint.send(WorldFrame::Output(output)).await.context("send output")?;
                    }
                }
                Some(GatewayFrame::Disconnect(disconnect)) => sessions.disconnect(disconnect.session_id),
                Some(GatewayFrame::Input(input)) => {
                    handle_input(&mut endpoint, &world, &backend, &mut sessions, &mut pipeline, &builtins, &places, input).await?;
                }
                Some(GatewayFrame::Resume(_)) => {
                    tracing::warn!("unexpected mid-stream resume frame dropped");
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

/// Logs one tick event at the level matching its player/operator relevance:
/// entity creation is routine (`info`), precondition failures and arena
/// rejections are worth an operator's attention (`warn`).
fn log_tick_event(event: &TickEvent) {
    match event {
        TickEvent::Created { entity } => tracing::info!(?entity, "entity created"),
        TickEvent::PreconditionFailed {
            precondition,
            effect,
        } => {
            tracing::warn!(?precondition, ?effect, "tick precondition failed");
        }
        TickEvent::Rejected { effect, error } => {
            tracing::warn!(?effect, %error, "tick effect rejected");
        }
        // INVARIANT: TickEvent is #[non_exhaustive]; log unknown future
        // variants rather than silently dropping them.
        _ => tracing::warn!("unrecognized tick event"),
    }
}

/// Routes one input line: pre-login input goes through the session FSM,
/// in-world input through the command pipeline. Never holds the world lock
/// across `sessions.on_input`; it is acquired only for the in-world dispatch.
// LINT: mirrors `run`'s bundle of one tenant's assembled stack, plus the
// frame being routed; a struct would just move these fields (single call site).
#[allow(clippy::too_many_arguments)]
async fn handle_input(
    endpoint: &mut InMemoryEndpoint<WorldFrame, GatewayFrame>,
    world: &Arc<Mutex<PersistentWorld>>,
    backend: &DbBackend,
    sessions: &mut SessionService,
    pipeline: &mut Pipeline,
    builtins: &[Command],
    places: &WorldPlaces,
    input: SessionInput,
) -> anyhow::Result<()> {
    let session_id = input.session_id;
    match sessions
        .on_input(session_id, input.line.as_str(), backend)
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
                sessions.disconnect(session_id);
            }
        }
        Routing::InWorld => {
            let mut guard = world.lock().await;
            let dispatched =
                pipeline.dispatch(guard.world(), places, &sessions.resolver(builtins), &input);
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
                        sessions.disconnect(session_id);
                    }
                }
                Err(PipelineError::UnknownSession(session)) => {
                    drop(guard);
                    tracing::warn!(%session, "dispatch for unknown session dropped");
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
        Routing::Unknown => tracing::warn!(%session_id, "input for unknown session dropped"),
    }
    Ok(())
}
