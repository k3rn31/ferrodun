//! The inventory side-table: which entities each container holds.

use crate::EntityId;

/// The entities each container holds: a dense table (container → its items), one
/// of the §2.3.2.2 hot components.
///
/// This table only records containment. Cross-container exclusivity (an item in
/// at most one inventory) and the location-versus-inventory relationship are
/// enforced by the mutation layer, not here.
#[derive(Debug, Default)]
#[must_use]
pub struct Inventory {
    /// Dense by slot: the entities each container holds.
    by_slot: Vec<Vec<EntityId>>,
}

impl Inventory {
    /// Creates an empty inventory table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `item` to `container`'s inventory. A no-op if `item` is already in
    /// that container (an entity cannot be in one container twice).
    pub fn insert(&mut self, container: EntityId, item: EntityId) {
        let Some(index) = container.slot().to_index() else {
            return;
        };

        if index >= self.by_slot.len() {
            let Some(len) = index.checked_add(1) else {
                return;
            };
            self.by_slot.resize_with(len, Vec::new);
        }

        // INVARIANT: `index < by_slot.len()` — just grown to cover it if needed.
        let Some(contents) = self.by_slot.get_mut(index) else {
            unreachable!(
                "container slot {} is out of range after grow",
                container.slot().get()
            );
        };
        if !contents.iter().any(|held| held.slot() == item.slot()) {
            contents.push(item);
        }
    }

    /// Removes `item` from `container`'s inventory. A no-op if absent.
    pub fn remove(&mut self, container: EntityId, item: EntityId) {
        let Some(contents) = container
            .slot()
            .to_index()
            .and_then(|index| self.by_slot.get_mut(index))
        else {
            return;
        };
        contents.retain(|held| held.slot() != item.slot());
    }

    /// Drops every item `container` holds, releasing its hot-component slot for
    /// teardown (§2.3.7.3). A no-op for an empty or unknown container.
    ///
    /// Matches by slot so a freed slot carries no contents into its next tenant:
    /// without this, a reused container slot would inherit the torn-down
    /// entity's items, since [`insert`](Inventory::insert) appends rather than
    /// overwriting a reused slot.
    pub fn clear(&mut self, container: EntityId) {
        if let Some(contents) = container
            .slot()
            .to_index()
            .and_then(|index| self.by_slot.get_mut(index))
        {
            contents.clear();
        }
    }

    /// The entities in `container`. Empty for an empty or unknown container.
    pub fn contents(&self, container: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        container
            .slot()
            .to_index()
            .and_then(|index| self.by_slot.get(index))
            .into_iter()
            .flatten()
            .copied()
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

    /// A handle to the same `slot` at a later generation, modeling the arena
    /// reusing a freed slot for a new entity.
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
    fn insert_then_contents_round_trips() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);
        let shield = entity(6);

        inventory.insert(chest, sword);
        inventory.insert(chest, shield);

        let contents: Vec<EntityId> = inventory.contents(chest).collect();
        assert_eq!(contents, vec![sword, shield]);
    }

    #[test]
    fn duplicate_insert_does_not_double_list() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);

        inventory.insert(chest, sword);
        inventory.insert(chest, sword);

        assert_eq!(inventory.contents(chest).collect::<Vec<_>>(), vec![sword]);
    }

    #[test]
    fn remove_drops_an_item_and_empty_container_is_empty() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);
        inventory.insert(chest, sword);

        inventory.remove(chest, sword);

        assert_eq!(inventory.contents(chest).count(), 0);
        assert_eq!(inventory.contents(entity(99)).count(), 0);
    }

    // Slot-reuse safety for containers: clearing a torn-down container's slot
    // must leave nothing for a fresh handle reusing that slot to inherit, since
    // `insert` appends rather than self-healing a reused slot.
    #[test]
    fn cleared_container_slot_carries_no_contents_to_a_reused_handle() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        inventory.insert(chest, entity(5));
        inventory.insert(chest, entity(6));

        inventory.clear(chest);

        assert_eq!(inventory.contents(chest).count(), 0);
        let reused = reused_slot(2, 1);
        assert_eq!(inventory.contents(reused).count(), 0);
    }
}
