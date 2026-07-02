//! The real §2.7 step-3 resolver: it reads the session registry, so a session
//! only resolves once it is in-world (bound to a puppet).

use mud_cmd::Command;
use mud_core::{EntityId, LockContext, World};
use mud_i18n::Locale;
use mud_schema::SessionId;

use crate::caller::{CallerContext, ResolvedSession, SessionResolver};
use crate::layers::LayerCommands;
use crate::roster::{Presence, Roster};
use crate::session::SessionState;

/// Resolves a session against the in-world bindings held by a `SessionService`.
///
/// Built-in commands (M1-17) are contributed at the lowest precedence; account
/// and puppet layers stay empty in M1 (§2.7 step-4 intent).
#[must_use]
pub struct RegistryResolver<'a> {
    sessions: &'a std::collections::HashMap<SessionId, SessionState>,
    builtins: &'a [Command],
}

impl<'a> RegistryResolver<'a> {
    pub(crate) fn new(
        sessions: &'a std::collections::HashMap<SessionId, SessionState>,
        builtins: &'a [Command],
    ) -> Self {
        Self { sessions, builtins }
    }
}

impl SessionResolver for RegistryResolver<'_> {
    fn resolve(&self, session: SessionId, world: &World) -> Option<ResolvedSession> {
        let SessionState::InWorld(binding) = self.sessions.get(&session)? else {
            return None;
        };
        let location = world.location_of(binding.puppet)?;
        Some(ResolvedSession {
            caller: CallerContext::new(
                session,
                binding.puppet,
                location,
                binding.name.clone(),
                Locale::EN,
                LockContext::new(),
            ),
            layers: LayerCommands {
                builtins: self.builtins.to_vec(),
                ..LayerCommands::default()
            },
        })
    }
}

impl Roster for RegistryResolver<'_> {
    fn session_of(&self, entity: EntityId) -> Option<SessionId> {
        self.sessions
            .iter()
            .find_map(|(session, state)| match state {
                SessionState::InWorld(binding) if binding.puppet == entity => Some(*session),
                SessionState::InWorld(_) | SessionState::Login(_) => None,
            })
    }

    fn connected(&self) -> Vec<Presence> {
        self.sessions
            .values()
            .filter_map(|state| match state {
                SessionState::InWorld(binding) => Some(Presence {
                    name: binding.name.clone(),
                }),
                SessionState::Login(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{InWorldBinding, SessionService};
    use mud_core::{PlaceId, TenantTag};
    use std::num::NonZeroU64;

    fn sid(n: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    fn place(n: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    #[test]
    fn an_in_world_session_resolves_to_its_puppet() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let puppet = world.create().expect("create puppet");
        world.move_to(puppet, place(10)).expect("seat puppet");
        let binding = InWorldBinding {
            account: mud_account::AccountId::new(NonZeroU64::new(1).expect("nonzero")),
            puppet,
            name: mud_account::PuppetName::parse("arden").expect("name"),
        };
        let mut svc = SessionService::new("W");
        svc.bind_for_test(sid(1), binding);

        let builtins = Vec::new();
        let resolver = svc.resolver(&builtins);
        let resolved = resolver
            .resolve(sid(1), &world)
            .expect("in-world session resolves");
        assert_eq!(resolved.caller.caller(), puppet);
        assert_eq!(resolved.caller.location(), place(10));

        // An unknown session never resolves.
        assert!(resolver.resolve(sid(2), &world).is_none());
    }

    #[test]
    fn roster_reports_sessions_and_connected_players() {
        use crate::roster::Roster;
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let arden = world.create().expect("create arden");
        let borel = world.create().expect("create borel");
        world.move_to(arden, place(10)).expect("seat arden");
        world.move_to(borel, place(10)).expect("seat borel");

        let mut svc = SessionService::new("W");
        svc.bind_for_test(
            sid(1),
            InWorldBinding {
                account: mud_account::AccountId::new(NonZeroU64::new(1).expect("nonzero")),
                puppet: arden,
                name: mud_account::PuppetName::parse("arden").expect("name"),
            },
        );
        svc.bind_for_test(
            sid(2),
            InWorldBinding {
                account: mud_account::AccountId::new(NonZeroU64::new(2).expect("nonzero")),
                puppet: borel,
                name: mud_account::PuppetName::parse("borel").expect("name"),
            },
        );

        let builtins = Vec::new();
        let resolver = svc.resolver(&builtins);

        assert_eq!(resolver.session_of(arden), Some(sid(1)));
        assert_eq!(resolver.session_of(borel), Some(sid(2)));

        let mut names: Vec<String> = resolver
            .connected()
            .into_iter()
            .map(|p| p.name.as_str().to_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["arden".to_owned(), "borel".to_owned()]);
    }
}
