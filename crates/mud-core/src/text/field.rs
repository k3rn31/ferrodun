//! Per-field markup policy (§3.20.2).
//!
//! The engine — not the builder — decides what styling each authored field
//! admits. A room `title` is bold by default and takes no inline markup; a
//! `description` takes no default style but admits palette colors and a few
//! attributes. A [`FieldStyle`] is passed to [`compile_markup`](super::compile_markup)
//! so the same compiler enforces a different policy per field.
//!
//! Builder markup carries *direct* styling only — palette colors and attributes
//! (§3.20.2.1). Semantic roles are applied by engine emission sites, not authored
//! inline, so a field policy gates colors and attributes but not roles.
//!
//! The policies are hard-coded constants for M1; making them tenant-configurable
//! is a later seam.

use super::attributes::Attributes;
use super::style::Style;

/// The styling policy for one authored field.
///
/// `default` seeds every span the field produces (so a bold-by-default field
/// renders bold even with no markup); `allow_colors` and `allowed_attrs` gate
/// which inline markup tags are honored — a tag outside the policy keeps its
/// inner text but contributes no style, alongside a diagnostic (§3.20.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct FieldStyle {
    /// The base style applied to the whole field before any inline markup.
    pub default: Style,
    /// Whether `{fg=…}` / `{bg=…}` palette-color tags are honored.
    pub allow_colors: bool,
    /// Which attribute tags (`{b}`/`{i}`/`{u}`) are honored.
    pub allowed_attrs: Attributes,
}

impl FieldStyle {
    /// A room title: bold by default, no inline markup (§2.2.2).
    pub const TITLE: Self = Self {
        default: Style::new().with_attrs(Attributes::BOLD),
        allow_colors: false,
        allowed_attrs: Attributes::NONE,
    };

    /// A room description: no default style; admits palette colors and the
    /// bold / italic / underline attributes for builder flavor (§3.20.2.1).
    pub const DESCRIPTION: Self = Self {
        default: Style::new(),
        allow_colors: true,
        allowed_attrs: Attributes::BOLD
            .insert(Attributes::ITALIC)
            .insert(Attributes::UNDERLINE),
    };
}
