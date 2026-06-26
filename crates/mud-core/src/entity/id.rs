//! `EntityId` and its component newtypes (§2.3.1).
//!
//! An `EntityId` is the engine's ephemeral, tenant-scoped handle to an entity.
//! It packs three fields into a single 64-bit word so it fits in a machine
//! register and iterates densely on the combat hot path (§2.3.1.3):
//!
//! | field         | width   | meaning                                      |
//! |---------------|---------|----------------------------------------------|
//! | tenant tag    | 12 bits | scopes the id to one tenant (§3.11)          |
//! | slot index    | 32 bits | index into that tenant's arena              |
//! | generation    | 20 bits | bumped on slot reuse to catch stale handles |
//!
//! The slot index + generation pair is the standard generational index that
//! prevents use-after-free across teardown (§2.3.7.3). `EntityId` is an
//! ephemeral in-memory handle — it is never persisted or sent on the wire
//! (§2.3.1.4), so the bit layout below is an internal `mud-core` detail that
//! MAY change without a version bump. An entity's durable identity is carried
//! separately by `EntityKey` (§2.3.1.5).

// Bit layout within the 64-bit word. Tenant occupies the high bits, the
// generation the low bits, so consecutive slots in one tenant stay numerically
// adjacent for dense iteration.
const TENANT_BITS: u32 = 12;
const SLOT_BITS: u32 = 32;
const GENERATION_BITS: u32 = 20;

const GENERATION_SHIFT: u32 = 0;
const SLOT_SHIFT: u32 = GENERATION_BITS;
const TENANT_SHIFT: u32 = GENERATION_BITS + SLOT_BITS;

const TENANT_MASK: u64 = (1 << TENANT_BITS) - 1;
const SLOT_MASK: u64 = (1 << SLOT_BITS) - 1;
const GENERATION_MASK: u64 = (1 << GENERATION_BITS) - 1;

/// Errors from parsing raw integers into id component newtypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EntityIdError {
    /// A tenant tag exceeded the 12-bit range (`0..=4095`).
    #[error("tenant tag {0} exceeds the 12-bit maximum of {max}", max = TenantTag::MAX)]
    TenantTagOutOfRange(u16),
    /// A generation counter exceeded the 20-bit range (`0..=1048575`).
    #[error("generation {0} exceeds the 20-bit maximum of {max}", max = Generation::MAX)]
    GenerationOutOfRange(u32),
}

/// Identifies the tenant that owns an entity (§3.11). 12 bits, so up to 4096
/// tenants can coexist in one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct TenantTag(u16);

impl TenantTag {
    /// Largest representable tenant tag.
    pub const MAX: u16 = (1 << TENANT_BITS) - 1;

    /// Parses a raw value into a `TenantTag`, rejecting values that do not fit
    /// in 12 bits.
    pub const fn new(value: u16) -> Result<Self, EntityIdError> {
        if value > Self::MAX {
            return Err(EntityIdError::TenantTagOutOfRange(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying value, always in `0..=MAX`.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for TenantTag {
    type Error = EntityIdError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Indexes a slot in a tenant's entity arena. The full 32-bit range is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct SlotIndex(u32);

impl SlotIndex {
    /// Wraps a raw slot index. Every `u32` is a valid slot index.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the underlying index.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Maps this slot to a position in a slot-indexed vector, or `None` on
    /// targets where `usize` is narrower than `u32` — where such a slot could
    /// never have been allocated. Shared by the arena and the hot side-tables,
    /// which all index dense `Vec`s by slot.
    pub(crate) fn to_index(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

/// Generation counter for a slot, bumped each time the slot is reused so that
/// handles to a previous occupant are detectably stale (§2.3.7.3). 20 bits, so
/// a slot can be reused ~1M times before the counter would wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct Generation(u32);

impl Generation {
    /// The generation of a freshly allocated slot, before any reuse.
    pub const FIRST: Self = Self(0);

    /// Largest representable generation.
    pub const MAX: u32 = (1 << GENERATION_BITS) - 1;

    /// Parses a raw value into a `Generation`, rejecting values that do not fit
    /// in 20 bits.
    pub const fn new(value: u32) -> Result<Self, EntityIdError> {
        if value > Self::MAX {
            return Err(EntityIdError::GenerationOutOfRange(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying value, always in `0..=MAX`.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns the next generation, or `None` when incrementing would wrap past
    /// `MAX`. A `None` result means the slot has exhausted its generations and
    /// MUST be burned rather than recycled, so that no future id can collide
    /// with a stale handle (§2.3.1.3).
    pub const fn next(self) -> Option<Self> {
        if self.0 >= Self::MAX {
            return None;
        }
        Some(Self(self.0 + 1))
    }
}

impl TryFrom<u32> for Generation {
    type Error = EntityIdError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A tenant-scoped generational handle to an entity (§2.3.1). Eight bytes; see
/// the module docs for the bit layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct EntityId(u64);

impl EntityId {
    /// Packs the three components into an id.
    pub const fn new(tenant: TenantTag, slot: SlotIndex, generation: Generation) -> Self {
        let bits = ((tenant.0 as u64) << TENANT_SHIFT)
            | ((slot.0 as u64) << SLOT_SHIFT)
            | ((generation.0 as u64) << GENERATION_SHIFT);
        Self(bits)
    }

    /// The tenant that owns this entity.
    pub const fn tenant(self) -> TenantTag {
        let value = ((self.0 >> TENANT_SHIFT) & TENANT_MASK) as u16;
        TenantTag(value)
    }

    /// The arena slot this id refers to.
    pub const fn slot(self) -> SlotIndex {
        let value = ((self.0 >> SLOT_SHIFT) & SLOT_MASK) as u32;
        SlotIndex(value)
    }

    /// The generation of this id; mismatch against the live slot means stale.
    pub const fn generation(self) -> Generation {
        let value = ((self.0 >> GENERATION_SHIFT) & GENERATION_MASK) as u32;
        Generation(value)
    }

    /// The raw 64-bit in-memory encoding. An internal detail — `EntityId` is
    /// not persisted or sent on the wire (§2.3.1.4); durable identity is
    /// `EntityKey` (§2.3.1.5).
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// Reconstructs an id from its raw encoding. Total: every `u64` maps to a
    /// well-formed id because each field occupies its full bit width.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unwrap_tenant(value: u16) -> TenantTag {
        TenantTag::new(value).expect("test tenant tag must be in range")
    }

    fn unwrap_generation(value: u32) -> Generation {
        Generation::new(value).expect("test generation must be in range")
    }

    #[test]
    fn entity_id_is_eight_bytes() {
        assert_eq!(size_of::<EntityId>(), 8);
    }

    #[test]
    fn packs_and_unpacks_each_field() {
        let tenant = unwrap_tenant(0xABC);
        let slot = SlotIndex::new(0xDEAD_BEEF);
        let generation = unwrap_generation(0x9_1234);

        let id = EntityId::new(tenant, slot, generation);

        assert_eq!(id.tenant(), tenant);
        assert_eq!(id.slot(), slot);
        assert_eq!(id.generation(), generation);
    }

    // Each field at its maximum (all bits set) while its neighbors are zero:
    // if any field's bit region overlapped another, the all-ones field would
    // spill into a neighbor and read back as non-zero. (Setting *every* field
    // to its max cannot detect this — overlapping ones still read as ones.)
    #[test]
    fn each_field_is_isolated_from_its_neighbors() {
        let zero_tenant = unwrap_tenant(0);
        let zero_slot = SlotIndex::new(0);
        let zero_generation = unwrap_generation(0);

        let max_tenant = EntityId::new(unwrap_tenant(TenantTag::MAX), zero_slot, zero_generation);
        assert_eq!(max_tenant.tenant().get(), TenantTag::MAX);
        assert_eq!(max_tenant.slot().get(), 0);
        assert_eq!(max_tenant.generation().get(), 0);

        let max_slot = EntityId::new(zero_tenant, SlotIndex::new(u32::MAX), zero_generation);
        assert_eq!(max_slot.slot().get(), u32::MAX);
        assert_eq!(max_slot.tenant().get(), 0);
        assert_eq!(max_slot.generation().get(), 0);

        let max_generation =
            EntityId::new(zero_tenant, zero_slot, unwrap_generation(Generation::MAX));
        assert_eq!(max_generation.generation().get(), Generation::MAX);
        assert_eq!(max_generation.tenant().get(), 0);
        assert_eq!(max_generation.slot().get(), 0);
    }

    #[test]
    fn round_trips_through_raw_bits() {
        let id = EntityId::new(unwrap_tenant(7), SlotIndex::new(42), unwrap_generation(3));

        assert_eq!(EntityId::from_bits(id.to_bits()), id);
    }

    // Pin the current internal bit layout to a concrete pattern so an
    // accidental change to field placement is caught (a change a self-
    // consistent round-trip would miss). The layout is a `mud-core` detail,
    // not a persistence/wire contract (§2.3.1.4), so this test may be updated
    // deliberately if the layout changes.
    #[test]
    fn packs_to_the_documented_bit_layout() {
        let id = EntityId::new(unwrap_tenant(1), SlotIndex::new(1), unwrap_generation(1));

        assert_eq!(id.to_bits(), (1 << 52) | (1 << 20) | 1);
    }

    // Every field at its maximum packs to an all-ones word: confirms the three
    // fields tile the full 64 bits with no padding or gaps.
    #[test]
    fn all_fields_max_packs_to_full_word() {
        let id = EntityId::new(
            unwrap_tenant(TenantTag::MAX),
            SlotIndex::new(u32::MAX),
            unwrap_generation(Generation::MAX),
        );

        assert_eq!(id.to_bits(), u64::MAX);
    }

    #[test]
    fn tenant_tag_rejects_out_of_range() {
        assert_eq!(
            TenantTag::new(TenantTag::MAX + 1),
            Err(EntityIdError::TenantTagOutOfRange(TenantTag::MAX + 1)),
        );
    }

    #[test]
    fn generation_rejects_out_of_range() {
        assert_eq!(
            Generation::new(Generation::MAX + 1),
            Err(EntityIdError::GenerationOutOfRange(Generation::MAX + 1)),
        );
    }

    #[test]
    fn try_from_parses_like_new() {
        assert_eq!(TenantTag::try_from(7), TenantTag::new(7));
        assert_eq!(
            TenantTag::try_from(TenantTag::MAX + 1),
            Err(EntityIdError::TenantTagOutOfRange(TenantTag::MAX + 1)),
        );
        assert_eq!(Generation::try_from(7), Generation::new(7));
        assert_eq!(
            Generation::try_from(Generation::MAX + 1),
            Err(EntityIdError::GenerationOutOfRange(Generation::MAX + 1)),
        );
    }

    #[test]
    fn generation_advances_by_one() {
        let generation = unwrap_generation(5);
        assert_eq!(generation.next(), Some(unwrap_generation(6)));
    }

    #[test]
    fn generation_wraparound_burns_the_slot() {
        let last = unwrap_generation(Generation::MAX);
        // None signals the arena to burn the slot rather than recycle it.
        assert_eq!(last.next(), None);
    }
}
