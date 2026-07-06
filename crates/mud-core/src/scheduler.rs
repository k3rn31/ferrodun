//! Scheduler tick and the `MutationCommand` write model (§2.5.3.3, §2.5.3.5,
//! §3.16.2).
//!
//! Every world mutation flows through a [`MutationCommand`] submitted to the
//! [`Scheduler`]. A command pairs a primitive [`Effect`] with an optional
//! [`Precondition`]: the precondition is evaluated against the [`World`] *at
//! apply time*, and on failure the effect is skipped and a structured
//! [`TickEvent::PreconditionFailed`] is emitted rather than a partial effect
//! applied (§2.5.3.5). This is how a composite read-then-write ("take the rusty
//! sword if it is still here") is expressed atomically.
//!
//! [`Scheduler::tick`] drains the whole queue in **arrival order**. Because the
//! drain is single-threaded and sequential, per-entity serialization,
//! arrival-order application, and last-writer-wins all hold by construction:
//! two commands against the same entity apply in submission order, the second
//! overwriting the first. §2.5.3.5 permits mutations against *different*
//! entities to proceed in parallel (a MAY), so no per-entity lock or parallel
//! executor is built here. The per-tick work budget (§2.3.4.1) is likewise not
//! enforced.
//!
//! ## Cadence and the wall-clock driver
//!
//! [`TICK_HZ`] / [`TICK_PERIOD`] pin the normative 20 Hz / 50 ms cadence
//! (§3.16.2). This module provides only the deterministic logical tick — **not**
//! the wall-clock loop that drives it. The driver — which owns a [`World`] and a
//! [`Scheduler`] and calls [`Scheduler::tick`] every [`TICK_PERIOD`], consuming
//! the returned [`TickEvent`]s — lives outside this module, since it needs an
//! async runtime the engine wires up elsewhere.

use std::collections::VecDeque;
use std::time::Duration;

use crate::World;
use crate::write_model::{MutationCommand, TickEvent};

/// Scheduler tick rate in hertz — fixed at 20 Hz and not tenant-configurable
/// (§3.16.2).
pub const TICK_HZ: u32 = 20;

/// Scheduler tick period — the 50 ms wall-clock cadence the driver runs
/// [`Scheduler::tick`] at (§3.16.2). Pinned here; the loop that consumes it
/// lives outside this module.
pub const TICK_PERIOD: Duration = Duration::from_millis(1000 / TICK_HZ as u64);

/// Serializes [`MutationCommand`]s and applies them to a [`World`] on each tick.
///
/// Holds a FIFO queue and a monotonic tick counter. Commands submitted via
/// [`submit`](Scheduler::submit) are applied, in submission order, on the next
/// [`tick`](Scheduler::tick).
#[derive(Debug, Default)]
#[must_use]
pub struct Scheduler {
    queue: VecDeque<MutationCommand>,
    tick: u64,
}

impl Scheduler {
    /// Creates an empty scheduler with an empty queue and tick counter zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues `command` for application on the next [`tick`](Scheduler::tick).
    /// Submission order is the arrival order commands apply in.
    pub fn submit(&mut self, command: MutationCommand) {
        self.queue.push_back(command);
    }

    /// The current tick number — zero before the first tick, incremented once
    /// per [`tick`](Scheduler::tick). This is the source for `mud.time.tick()`
    /// (§3.16.4).
    #[must_use]
    pub fn tick_number(&self) -> u64 {
        self.tick
    }

    /// Drains and applies every queued command against `world`, in arrival
    /// order, returning the events produced.
    ///
    /// For each command: if it carries a precondition that does not hold against
    /// `world`, the effect is skipped and a [`TickEvent::PreconditionFailed`] is
    /// recorded; otherwise the effect is dispatched to `world` and any arena
    /// rejection becomes a [`TickEvent::Rejected`]. Increments the tick counter
    /// once (saturating at [`u64::MAX`]).
    pub fn tick(&mut self, world: &mut World) -> Vec<TickEvent> {
        let mut events = Vec::new();
        for command in self.drain() {
            if let Some(precondition) = command.precondition()
                && !world.satisfies(precondition)
            {
                events.push(TickEvent::PreconditionFailed {
                    precondition,
                    effect: command.effect(),
                });
                continue;
            }
            if let Some(event) = world.apply_effect(command.effect()) {
                events.push(event);
            }
        }
        events
    }

    /// Advances the tick counter once (saturating at [`u64::MAX`]) and hands
    /// off every queued command, in arrival order, emptying the queue.
    ///
    /// This is the seam a durable applier consumes: unlike
    /// [`tick`](Scheduler::tick), it does not apply commands against a
    /// [`World`] itself.
    pub fn drain(&mut self) -> Vec<MutationCommand> {
        self.tick = self.tick.saturating_add(1);
        self.queue.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArenaError, EntityId, Effect, PlaceId, Precondition, TenantTag};
    use std::num::NonZeroU64;

    fn world() -> World {
        World::new(TenantTag::new(1).expect("test tenant tag must be in range"))
    }

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    const HALL: u64 = 10;
    const STUDY: u64 = 11;
    const LIBRARY: u64 = 12;

    /// Drains a freshly created entity's handle out of a single-`Create` tick.
    fn create_entity(scheduler: &mut Scheduler, world: &mut World) -> EntityId {
        scheduler.submit(MutationCommand::new(Effect::Create));
        scheduler
            .tick(world)
            .into_iter()
            .find_map(|event| match event {
                TickEvent::Created { entity } => Some(entity),
                TickEvent::PreconditionFailed { .. } | TickEvent::Rejected { .. } => None,
            })
            .expect("Create command must emit a Created event")
    }

    #[test]
    fn create_command_mints_a_usable_entity() {
        let mut scheduler = Scheduler::new();
        let mut world = world();

        let entity = create_entity(&mut scheduler, &mut world);

        // The minted handle is live: a follow-up move applies without rejection.
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(HALL),
        }));
        assert_eq!(scheduler.tick(&mut world), vec![]);
        assert!(world.is_located_in(entity, place(HALL)));
    }

    #[test]
    fn commands_apply_in_arrival_order_per_entity_last_writer_wins() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let entity = create_entity(&mut scheduler, &mut world);

        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(HALL),
        }));
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(STUDY),
        }));
        let events = scheduler.tick(&mut world);

        assert_eq!(events, vec![]);
        // The second writer wins; the entity is in exactly the last destination.
        assert!(world.is_located_in(entity, place(STUDY)));
        assert!(!world.is_located_in(entity, place(HALL)));
    }

    // Two entities mutated in one tick must each land in the state implied by
    // their own last command; interleaving must not cross-contaminate them.
    #[test]
    fn interleaved_commands_for_two_entities_stay_independent() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let a = create_entity(&mut scheduler, &mut world);
        let b = create_entity(&mut scheduler, &mut world);

        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity: a,
            place: place(HALL),
        }));
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity: b,
            place: place(STUDY),
        }));
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity: a,
            place: place(LIBRARY),
        }));
        scheduler.tick(&mut world);

        assert!(world.is_located_in(a, place(LIBRARY)));
        assert!(world.is_located_in(b, place(STUDY)));
    }

    #[test]
    fn failed_precondition_skips_effect_and_emits_event() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let entity = create_entity(&mut scheduler, &mut world);
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(HALL),
        }));
        scheduler.tick(&mut world);

        // Guard the move on the entity being in the LIBRARY, where it is not.
        let guarded = MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(STUDY),
        })
        .with_precondition(Precondition::LocatedIn {
            entity,
            place: place(LIBRARY),
        });
        scheduler.submit(guarded);
        let events = scheduler.tick(&mut world);

        assert_eq!(
            events,
            vec![TickEvent::PreconditionFailed {
                precondition: Precondition::LocatedIn {
                    entity,
                    place: place(LIBRARY),
                },
                effect: Effect::MoveTo {
                    entity,
                    place: place(STUDY),
                },
            }]
        );
        // No partial effect: the entity stayed in the HALL.
        assert!(world.is_located_in(entity, place(HALL)));
    }

    #[test]
    fn satisfied_precondition_applies_effect() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let entity = create_entity(&mut scheduler, &mut world);
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(HALL),
        }));
        scheduler.tick(&mut world);

        let guarded = MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(STUDY),
        })
        .with_precondition(Precondition::LocatedIn {
            entity,
            place: place(HALL),
        });
        scheduler.submit(guarded);
        let events = scheduler.tick(&mut world);

        assert_eq!(events, vec![]);
        assert!(world.is_located_in(entity, place(STUDY)));
    }

    #[test]
    fn teardown_command_invalidates_and_clears() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let entity = create_entity(&mut scheduler, &mut world);
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(HALL),
        }));
        scheduler.tick(&mut world);

        scheduler.submit(MutationCommand::new(Effect::Teardown { entity }));
        let events = scheduler.tick(&mut world);

        assert_eq!(events, vec![]);
        assert!(!world.is_located_in(entity, place(HALL)));
        // A later effect on the now-stale handle is rejected.
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity,
            place: place(STUDY),
        }));
        assert_eq!(
            scheduler.tick(&mut world),
            vec![TickEvent::Rejected {
                effect: Effect::MoveTo {
                    entity,
                    place: place(STUDY),
                },
                error: ArenaError::StaleHandle,
            }]
        );
    }

    #[test]
    fn effect_on_a_foreign_handle_is_rejected() {
        let mut other_scheduler = Scheduler::new();
        let mut other_world = World::new(TenantTag::new(2).expect("tenant 2 must be in range"));
        let foreign = create_entity(&mut other_scheduler, &mut other_world);

        let mut scheduler = Scheduler::new();
        let mut world = world();
        scheduler.submit(MutationCommand::new(Effect::MoveTo {
            entity: foreign,
            place: place(HALL),
        }));
        let events = scheduler.tick(&mut world);

        assert_eq!(
            events,
            vec![TickEvent::Rejected {
                effect: Effect::MoveTo {
                    entity: foreign,
                    place: place(HALL),
                },
                error: ArenaError::CrossTenant {
                    arena: TenantTag::new(1).expect("tenant 1 must be in range"),
                    handle: TenantTag::new(2).expect("tenant 2 must be in range"),
                },
            }]
        );
    }

    #[test]
    fn inventory_effects_dispatch_through_a_tick() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let chest = create_entity(&mut scheduler, &mut world);
        let sword = create_entity(&mut scheduler, &mut world);

        scheduler.submit(MutationCommand::new(Effect::InventoryAdd {
            container: chest,
            item: sword,
        }));
        assert_eq!(scheduler.tick(&mut world), vec![]);
        assert!(world.contains(chest, sword));

        scheduler.submit(MutationCommand::new(Effect::InventoryRemove {
            container: chest,
            item: sword,
        }));
        assert_eq!(scheduler.tick(&mut world), vec![]);
        assert!(!world.contains(chest, sword));
    }

    #[test]
    fn contains_precondition_gates_an_effect() {
        let mut scheduler = Scheduler::new();
        let mut world = world();
        let chest = create_entity(&mut scheduler, &mut world);
        let sword = create_entity(&mut scheduler, &mut world);

        // Precondition fails: the chest does not hold the sword yet, so the
        // guarded move is skipped with no partial effect.
        let guarded = MutationCommand::new(Effect::MoveTo {
            entity: sword,
            place: place(HALL),
        })
        .with_precondition(Precondition::Contains {
            container: chest,
            item: sword,
        });
        scheduler.submit(guarded);
        let events = scheduler.tick(&mut world);

        assert_eq!(
            events,
            vec![TickEvent::PreconditionFailed {
                precondition: Precondition::Contains {
                    container: chest,
                    item: sword,
                },
                effect: Effect::MoveTo {
                    entity: sword,
                    place: place(HALL),
                },
            }]
        );
        assert!(!world.is_located_in(sword, place(HALL)));

        // Now the chest holds the sword, so the same precondition holds and the
        // effect applies.
        scheduler.submit(MutationCommand::new(Effect::InventoryAdd {
            container: chest,
            item: sword,
        }));
        scheduler.tick(&mut world);
        scheduler.submit(
            MutationCommand::new(Effect::MoveTo {
                entity: sword,
                place: place(HALL),
            })
            .with_precondition(Precondition::Contains {
                container: chest,
                item: sword,
            }),
        );

        assert_eq!(scheduler.tick(&mut world), vec![]);
        assert!(world.is_located_in(sword, place(HALL)));
    }

    #[test]
    fn tick_number_increments_once_per_tick() {
        let mut scheduler = Scheduler::new();
        let mut world = world();

        assert_eq!(scheduler.tick_number(), 0);
        scheduler.tick(&mut world);
        assert_eq!(scheduler.tick_number(), 1);
        scheduler.tick(&mut world);
        assert_eq!(scheduler.tick_number(), 2);
    }

    #[test]
    fn tick_period_matches_the_fixed_rate() {
        assert_eq!(TICK_HZ, 20);
        assert_eq!(TICK_PERIOD, Duration::from_millis(50));
    }

    #[test]
    fn drain_returns_queued_commands_in_arrival_order_and_advances_the_tick() {
        let mut scheduler = Scheduler::new();
        let first = MutationCommand::new(Effect::Create);
        let second = MutationCommand::new(Effect::Create);
        scheduler.submit(first);
        scheduler.submit(second);

        let drained = scheduler.drain();

        assert_eq!(scheduler.tick_number(), 1, "drain counts as one tick");
        assert_eq!(drained.len(), 2);
        assert!(matches!(
            drained.first().map(MutationCommand::effect),
            Some(Effect::Create)
        ));
    }

    #[test]
    fn drain_empties_the_queue() {
        let mut scheduler = Scheduler::new();
        scheduler.submit(MutationCommand::new(Effect::Create));
        let _ = scheduler.drain();
        assert!(scheduler.drain().is_empty(), "second drain finds nothing");
        assert_eq!(scheduler.tick_number(), 2, "every drain advances the tick");
    }
}
