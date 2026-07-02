//! Session resolution: the §2.7 step-3 seam.
//!
//! Step 3 of the pipeline resolves `session → account → puppet entity →
//! location stack`. Accounts (M1-18) and the session FSM (M1-19) do not exist
//! yet, so that resolution lives behind the [`SessionResolver`] trait: a test
//! fakes it now, and M1-18/19 supply the real implementation **without
//! reshaping the pipeline**. The result is a [`ResolvedSession`] — the caller's
//! [`CallerContext`] plus the [`LayerCommands`](crate::LayerCommands) to merge.

use mud_account::PuppetName;
use mud_core::{EntityId, LockContext, PlaceId, World};
use mud_schema::SessionId;

use crate::LayerCommands;

/// Everything the pipeline needs about the caller once a session is resolved.
///
/// The caller MAY be a player or an NPC (§2.7 step 6); both reduce to an
/// [`EntityId`] here. The [`LockContext`] carries the caller's *accessor* facts
/// (permissions, statuses) for the lock check, reusing `mud-core`'s lock model
/// rather than a parallel permission store.
#[derive(Debug, Clone)]
#[must_use]
pub struct CallerContext {
    session_id: SessionId,
    caller: EntityId,
    location: PlaceId,
    name: PuppetName,
    access: LockContext,
}

impl CallerContext {
    /// Assembles a caller context from its resolved parts.
    pub fn new(
        session_id: SessionId,
        caller: EntityId,
        location: PlaceId,
        name: PuppetName,
        access: LockContext,
    ) -> Self {
        Self {
            session_id,
            caller,
            location,
            name,
            access,
        }
    }

    /// The session this caller is acting through.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The entity issuing the command (player or NPC).
    pub fn caller(&self) -> EntityId {
        self.caller
    }

    /// The place the caller is currently in.
    pub fn location(&self) -> PlaceId {
        self.location
    }

    /// The caller's display name, used when a command names the actor to other
    /// players (`say`, movement). M1: always a player's puppet name.
    pub fn caller_name(&self) -> &PuppetName {
        &self.name
    }

    /// The caller's accessor facts for lock evaluation (§2.7 step 6).
    pub fn access(&self) -> &LockContext {
        &self.access
    }
}

/// The outcome of §2.7 step 3: who is acting, and which command layers apply.
#[derive(Debug, Clone)]
#[must_use]
pub struct ResolvedSession {
    /// The resolved caller.
    pub caller: CallerContext,
    /// The CmdSet source layers to merge for this caller (§2.7 step 4).
    pub layers: LayerCommands,
}

/// Resolves a [`SessionId`] to its acting caller and command layers (§2.7 step 3).
///
/// This is the seam that absorbs the not-yet-built account/session machinery:
/// the pipeline depends only on this trait, so M1-18 (accounts) and M1-19
/// (session FSM) can supply a real resolver while M1 tests supply a fake one.
pub trait SessionResolver {
    /// Resolves `session` against `world`, or `None` if no caller is bound to it
    /// (an unknown or not-yet-logged-in session).
    fn resolve(&self, session: SessionId, world: &World) -> Option<ResolvedSession>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn session(value: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(value).expect("test session id must be non-zero"))
    }

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    #[test]
    fn caller_context_exposes_its_parts() {
        let mut world = World::new(mud_core::TenantTag::new(1).expect("tenant in range"));
        let caller = world.create().expect("create caller");
        let ctx = CallerContext::new(
            session(1),
            caller,
            place(10),
            PuppetName::parse("hero").expect("name"),
            LockContext::new().with_perm("admin"),
        );

        assert_eq!(ctx.session_id(), session(1));
        assert_eq!(ctx.caller(), caller);
        assert_eq!(ctx.location(), place(10));
        assert_eq!(ctx.caller_name().as_str(), "hero");
        // The access context carries the perm we granted: a lock requiring it passes.
        let lock =
            mud_core::resolve(mud_core::parse("x:perm(admin)").expect("parse")).expect("resolve");
        assert!(lock.evaluate(ctx.access()));
    }
}
