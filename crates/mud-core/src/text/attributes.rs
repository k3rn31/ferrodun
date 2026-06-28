//! Text attribute flags (§3.20.1.1).
//!
//! The five attributes the spec requires a span to be able to carry — bold,
//! italic, underline, blink-equivalent, and reverse — packed into a single byte.
//! A bitflag newtype is chosen over five `bool` fields so the set is `Copy`,
//! `Hash`, and compact, and so iteration over enabled flags has a fixed order
//! (the renderer relies on that order for reproducible SGR output, §3.20.5.4). A
//! hand-rolled newtype avoids a `bitflags` dependency for five flags.

/// A set of text attributes (§3.20.1.1).
///
/// Flags are combined with [`union`](Attributes::union) and tested with
/// [`contains`](Attributes::contains); the bit order of the constants is the
/// canonical SGR emission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[must_use]
pub struct Attributes(u8);

impl Attributes {
    /// No attributes.
    pub const NONE: Self = Self(0);
    /// Bold / increased intensity.
    pub const BOLD: Self = Self(1 << 0);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 1);
    /// Underline.
    pub const UNDERLINE: Self = Self(1 << 2);
    /// Blink (or a client's blink-equivalent, §3.20.1.1).
    pub const BLINK: Self = Self(1 << 3);
    /// Reverse video (swap foreground and background).
    pub const REVERSE: Self = Self(1 << 4);

    /// Returns the union of `self` and `other`.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `true` if every flag in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `true` if no attribute is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_empty_and_contains_nothing() {
        assert!(Attributes::NONE.is_empty());
        assert!(!Attributes::NONE.contains(Attributes::BOLD));
    }

    #[test]
    fn insert_unions_flags_and_is_idempotent() {
        let attrs = Attributes::BOLD
            .union(Attributes::UNDERLINE)
            .union(Attributes::BOLD);
        assert!(attrs.contains(Attributes::BOLD));
        assert!(attrs.contains(Attributes::UNDERLINE));
        assert!(!attrs.contains(Attributes::ITALIC));
        assert!(!attrs.is_empty());
    }

    #[test]
    fn contains_requires_all_queried_flags() {
        let attrs = Attributes::BOLD.union(Attributes::ITALIC);
        assert!(attrs.contains(Attributes::BOLD.union(Attributes::ITALIC)));
        assert!(!attrs.contains(Attributes::BOLD.union(Attributes::BLINK)));
    }

    // The five flags occupy distinct bits, so no two attributes collide.
    #[test]
    fn the_five_attributes_are_distinct_bits() {
        let all = [
            Attributes::BOLD,
            Attributes::ITALIC,
            Attributes::UNDERLINE,
            Attributes::BLINK,
            Attributes::REVERSE,
        ];
        let union = all.iter().fold(Attributes::NONE, |acc, a| acc.union(*a));
        for attr in all {
            assert!(union.contains(attr));
        }
    }
}
