//! The naming side-table: the keywords each entity answers to.

use crate::EntityId;

/// A single lookup keyword an entity answers to (e.g. `sword`, `goblin`).
///
/// Normalized to lowercase on construction so command-argument matching (§2.7
/// step 5) is case-insensitive without re-casing at every comparison. A keyword
/// is a match *token*, distinct from a display name: an entity may answer to
/// several.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct Keyword(String);

impl Keyword {
    /// Wraps `text` as a keyword, lowercased for case-insensitive matching.
    pub fn new(text: impl AsRef<str>) -> Self {
        Self(text.as_ref().to_lowercase())
    }

    /// The normalized (lowercased) keyword text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The keywords each entity answers to: a dense table (entity → its keywords),
/// keyed by [`SlotIndex`](crate::SlotIndex) like the other side-tables.
///
/// Pure storage: liveness is the arena's concern, joined in [`World`](crate::World).
/// The table records the keywords as authored; prefix matching and
/// disambiguation live in the command layer, not here.
#[derive(Debug, Default)]
#[must_use]
pub struct Naming {
    /// Dense by slot: the keywords each entity answers to.
    by_slot: Vec<Vec<Keyword>>,
}

impl Naming {
    /// Creates an empty naming table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `entity`'s keywords, replacing any previously set list.
    pub fn set(&mut self, entity: EntityId, keywords: Vec<Keyword>) {
        let Some(index) = entity.slot().to_index() else {
            return;
        };

        if index >= self.by_slot.len() {
            let Some(len) = index.checked_add(1) else {
                return;
            };
            self.by_slot.resize_with(len, Vec::new);
        }

        // The slot was just grown to cover `index`; a miss here can only mean an
        // allocation that did not happen, so drop the write rather than panic.
        if let Some(slot) = self.by_slot.get_mut(index) {
            *slot = keywords;
        }
    }

    /// Drops every keyword for `entity`, releasing its slot for teardown
    /// (§2.3.7.3) so a reused slot inherits no stale keywords. A no-op for an
    /// unnamed or unknown entity.
    pub fn clear(&mut self, entity: EntityId) {
        if let Some(slot) = entity
            .slot()
            .to_index()
            .and_then(|index| self.by_slot.get_mut(index))
        {
            slot.clear();
        }
    }

    /// The keywords `entity` answers to. Empty for an unnamed or unknown entity.
    pub fn keywords(&self, entity: EntityId) -> &[Keyword] {
        entity
            .slot()
            .to_index()
            .and_then(|index| self.by_slot.get(index))
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SlotIndex;

    fn entity(slot: u32) -> EntityId {
        // The slot is the only field the side-tables key on, so tenant and
        // generation are arbitrary here.
        EntityId::new(tenant(), SlotIndex::new(slot), generation())
    }

    fn reused_slot(slot: u32, generation: u32) -> EntityId {
        let generation =
            crate::Generation::new(generation).expect("test generation must be in range");
        EntityId::new(tenant(), SlotIndex::new(slot), generation)
    }

    fn tenant() -> crate::TenantTag {
        crate::TenantTag::new(1).expect("test tenant tag must be in range")
    }

    fn generation() -> crate::Generation {
        crate::Generation::FIRST
    }

    #[test]
    fn keyword_is_lowercased_on_construction() {
        assert_eq!(Keyword::new("Rusty SWORD").as_str(), "rusty sword");
    }

    #[test]
    fn set_then_keywords_round_trips() {
        let mut naming = Naming::new();
        let sword = entity(5);

        naming.set(sword, vec![Keyword::new("sword"), Keyword::new("rusty")]);

        assert_eq!(
            naming.keywords(sword),
            &[Keyword::new("sword"), Keyword::new("rusty")]
        );
    }

    #[test]
    fn set_replaces_a_previous_list() {
        let mut naming = Naming::new();
        let it = entity(5);
        naming.set(it, vec![Keyword::new("old")]);

        naming.set(it, vec![Keyword::new("new")]);

        assert_eq!(naming.keywords(it), &[Keyword::new("new")]);
    }

    #[test]
    fn an_unnamed_entity_has_no_keywords() {
        let naming = Naming::new();

        assert!(naming.keywords(entity(99)).is_empty());
    }

    // Slot-reuse safety: clearing a torn-down entity's slot must leave nothing
    // for a fresh handle reusing that slot to inherit.
    #[test]
    fn cleared_slot_carries_no_keywords_to_a_reused_handle() {
        let mut naming = Naming::new();
        let goblin = entity(2);
        naming.set(goblin, vec![Keyword::new("goblin")]);

        naming.clear(goblin);

        assert!(naming.keywords(goblin).is_empty());
        assert!(naming.keywords(reused_slot(2, 1)).is_empty());
    }
}
