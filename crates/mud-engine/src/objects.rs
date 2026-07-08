//! Object disambiguation on command arguments (§2.7 step 5).
//!
//! Commands that target an object (`get`, `drop`, …) resolve a player's argument
//! token to a concrete [`EntityId`] through [`resolve`]. Candidates are gathered
//! from the caller's inventory then the current place's occupants, deduplicated
//! by id, and matched by case-insensitive keyword prefix. The argument MAY use
//! the ordinal suffix `name.N` (1-based) to pick the Nth match, or the leading
//! keyword `all` to select every match; otherwise multiple matches resolve to
//! [`Resolution::Ambiguous`], which the caller renders as a one-shot numbered
//! prompt (the next command is parsed fresh — no session state is held).
//!
//! Exits are part of the §2.7-step-5 gather set, but no M1 command targets an
//! exit (movement uses explicit direction commands), so they are not gathered
//! until a command needs them.

use mud_core::{EntityId, World};

/// The outcome of resolving an argument token against the caller's surroundings
/// (§2.7 step 5).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum Resolution {
    /// Exactly one object matched (directly or via an `name.N` ordinal).
    One(EntityId),
    /// The `all` keyword selected every match, in gather order.
    All(Vec<EntityId>),
    /// Several objects matched with no ordinal to choose between them; render a
    /// numbered prompt in gather order.
    Ambiguous(Vec<EntityId>),
    /// Nothing matched (or the ordinal was out of range).
    NoMatch,
}

/// Resolves `arg` against an explicit `candidates` list in the given order
/// (§2.7 step 5: prefix match, `name.N` ordinal, `all`).
///
/// Callers scope the candidate list to the command's reach — `get` to the
/// floor, `drop` to the inventory. A command that targets anything visible (the
/// general inventory-then-occupants gather of §2.7 step 5) does not exist yet,
/// so that gather is intentionally not built until one needs it.
///
/// `arg` is the raw argument remainder; an empty `arg` yields
/// [`Resolution::NoMatch`].
pub fn resolve_among(world: &World, candidates: &[EntityId], arg: &str) -> Resolution {
    let arg = arg.trim();
    if arg.is_empty() {
        return Resolution::NoMatch;
    }

    if let Some(rest) = strip_all_prefix(arg) {
        let matches = matching(world, candidates, rest);
        return if matches.is_empty() {
            Resolution::NoMatch
        } else {
            Resolution::All(matches)
        };
    }

    let (keyword, ordinal) = split_ordinal(arg);
    let matches = matching(world, candidates, keyword);
    match ordinal {
        Some(n) => n
            .checked_sub(1)
            .and_then(|index| matches.get(index))
            .copied()
            .map_or(Resolution::NoMatch, Resolution::One),
        None => match matches.as_slice() {
            [] => Resolution::NoMatch,
            [only] => Resolution::One(*only),
            _ => Resolution::Ambiguous(matches),
        },
    }
}

/// The candidates whose keywords prefix-match `token`, preserving gather order.
fn matching(world: &World, candidates: &[EntityId], token: &str) -> Vec<EntityId> {
    let token = token.trim().to_lowercase();
    if token.is_empty() {
        return Vec::new();
    }
    candidates
        .iter()
        .copied()
        .filter(|&entity| {
            world
                .keywords_of(entity)
                .iter()
                .any(|keyword| keyword.as_str().starts_with(&token))
        })
        .collect()
}

/// The keyword after a leading `all` (e.g. `all sword` → `sword`, bare `all` →
/// `""`), or `None` when `arg` is not an `all` selection.
fn strip_all_prefix(arg: &str) -> Option<&str> {
    let rest = arg.strip_prefix("all")?;
    if rest.is_empty() || rest.starts_with(' ') {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Splits an `name.N` ordinal suffix off `arg`. Returns the keyword and the
/// 1-based ordinal, or `(arg, None)` when there is no valid numeric suffix.
fn split_ordinal(arg: &str) -> (&str, Option<usize>) {
    match arg.rsplit_once('.') {
        Some((keyword, suffix)) => match suffix.parse::<usize>() {
            Ok(n) => (keyword, Some(n)),
            Err(_) => (arg, None),
        },
        None => (arg, None),
    }
}

#[cfg(test)]
mod tests {

    use std::num::NonZeroU64;

    use mud_core::{Keyword, PlaceId, TenantTag, World};

    use super::*;

    const ROOM: u64 = 10;

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("place id non-zero"))
    }

    /// A world with a caller in ROOM, plus helpers to add named items to the
    /// caller's inventory or the room floor.
    struct Fixture {
        world: World,
        caller: EntityId,
    }

    impl Fixture {
        fn new() -> Self {
            let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
            let caller = world.create().expect("create caller");
            world.move_to(caller, place(ROOM)).expect("place caller");
            Self { world, caller }
        }

        fn carried(&mut self, keywords: &[&str]) -> EntityId {
            let item = self.world.create().expect("create item");
            self.name(item, keywords);
            self.world
                .inventory_add(self.caller, item)
                .expect("add to inventory");
            item
        }

        fn on_floor(&mut self, keywords: &[&str]) -> EntityId {
            let item = self.world.create().expect("create item");
            self.name(item, keywords);
            self.world.move_to(item, place(ROOM)).expect("place item");
            item
        }

        fn name(&mut self, item: EntityId, keywords: &[&str]) {
            let keywords = keywords.iter().map(Keyword::new).collect();
            self.world.name_entity(item, keywords).expect("name item");
        }

        /// Resolves against the caller's inventory then the room floor, the
        /// gather order these matching tests exercise.
        fn resolve(&self, arg: &str) -> Resolution {
            let mut candidates: Vec<EntityId> = Vec::new();
            for entity in self
                .world
                .inventory_of(self.caller)
                .chain(self.world.occupants_of(place(ROOM)))
            {
                if !candidates.contains(&entity) {
                    candidates.push(entity);
                }
            }
            resolve_among(&self.world, &candidates, arg)
        }
    }

    #[test]
    fn a_unique_prefix_resolves_to_one() {
        let mut fx = Fixture::new();
        let sword = fx.carried(&["sword", "rusty"]);

        assert_eq!(fx.resolve("sw"), Resolution::One(sword));
    }

    #[test]
    fn an_unknown_token_matches_nothing() {
        let mut fx = Fixture::new();
        let _ = fx.carried(&["sword"]);

        assert_eq!(fx.resolve("shield"), Resolution::NoMatch);
        assert_eq!(fx.resolve(""), Resolution::NoMatch);
    }

    #[test]
    fn inventory_is_gathered_before_floor() {
        let mut fx = Fixture::new();
        let carried = fx.carried(&["sword"]);
        let floor = fx.on_floor(&["sword"]);

        // Two `sword`s with no ordinal → ambiguous, inventory first.
        assert_eq!(
            fx.resolve("sword"),
            Resolution::Ambiguous(vec![carried, floor])
        );
    }

    #[test]
    fn an_ordinal_selects_the_nth_match() {
        let mut fx = Fixture::new();
        let _first = fx.carried(&["sword"]);
        let second = fx.on_floor(&["sword"]);

        assert_eq!(fx.resolve("sword.2"), Resolution::One(second));
        assert_eq!(fx.resolve("sword.3"), Resolution::NoMatch);
    }

    #[test]
    fn all_selects_every_match_in_gather_order() {
        let mut fx = Fixture::new();
        let carried = fx.carried(&["coin"]);
        let floor = fx.on_floor(&["coin"]);
        let _other = fx.on_floor(&["rock"]);

        assert_eq!(
            fx.resolve("all coin"),
            Resolution::All(vec![carried, floor])
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        let mut fx = Fixture::new();
        let goblin = fx.on_floor(&["goblin"]);

        assert_eq!(fx.resolve("GOB"), Resolution::One(goblin));
    }

    #[test]
    fn resolve_among_restricts_to_the_given_candidates() {
        // A floor sword and a carried sword both match `sword`, but a `get`-style
        // scope of floor-only candidates resolves to just the floor one.
        let mut fx = Fixture::new();
        let _carried = fx.carried(&["sword"]);
        let floor = fx.on_floor(&["sword"]);

        let floor_only: Vec<EntityId> = fx.world.occupants_of(place(ROOM)).collect();
        assert_eq!(
            resolve_among(&fx.world, &floor_only, "sword"),
            Resolution::One(floor)
        );
    }
}
