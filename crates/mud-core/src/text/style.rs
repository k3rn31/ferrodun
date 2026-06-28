//! Concrete styles, semantic role names, and the per-span style choice.

use std::borrow::Cow;
use std::fmt;

use super::attributes::Attributes;
use super::color::Color;

/// A concrete, already-resolved style: an optional foreground and background
/// color plus a set of attributes (§3.20.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[must_use]
pub struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: Attributes,
}

impl Style {
    /// An unstyled style: no colors, no attributes.
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            attrs: Attributes::NONE,
        }
    }

    /// Returns `self` with the foreground set to `color`.
    pub const fn with_fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Returns `self` with the background set to `color`.
    pub const fn with_bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Returns `self` with `attrs` added to its attribute set.
    pub const fn with_attrs(mut self, attrs: Attributes) -> Self {
        self.attrs = self.attrs.insert(attrs);
        self
    }

    /// The foreground color, if any.
    #[must_use]
    pub const fn fg(self) -> Option<Color> {
        self.fg
    }

    /// The background color, if any.
    #[must_use]
    pub const fn bg(self) -> Option<Color> {
        self.bg
    }

    /// The attribute set.
    pub const fn attrs(self) -> Attributes {
        self.attrs
    }

    /// Returns `true` if this style carries no color and no attribute, so
    /// rendering it produces no escape sequence (§3.20.1.2).
    #[must_use]
    pub const fn is_unstyled(self) -> bool {
        self.fg.is_none() && self.bg.is_none() && self.attrs.is_empty()
    }
}

/// The name of a semantic role (§3.20.3.2), e.g. `error` or `say`.
///
/// An open string newtype rather than a closed enum: §3.20.3.2 fixes a *minimum*
/// baseline and lets a tenant palette add roles, and §3.20.2.2 requires an
/// unknown role to resolve to unstyled rather than fail to compile. The baseline
/// names are zero-allocation `'static` constants; a role parsed from a palette or
/// markup is owned. The `Cow` compares and hashes by its string contents, so a
/// baseline constant and a parsed copy of the same name are equal and hash alike
/// — a palette keyed by baseline constants resolves a parsed role regardless of
/// provenance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct RoleName(Cow<'static, str>);

impl RoleName {
    /// A role name from a `'static` string, without allocating. Used for the
    /// baseline constants.
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// A role name owning its text, for names parsed from a palette or markup.
    pub fn new(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    /// The role name text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `error` role (§3.20.3.2).
    pub const ERROR: Self = Self::from_static("error");
    /// The `system` role (§3.20.3.2).
    pub const SYSTEM: Self = Self::from_static("system");
    /// The `alert` role (§3.20.3.2).
    pub const ALERT: Self = Self::from_static("alert");
    /// The `prompt` role (§3.20.3.2).
    pub const PROMPT: Self = Self::from_static("prompt");
    /// The `say` role (§3.20.3.2).
    pub const SAY: Self = Self::from_static("say");
    /// The `emote` role (§3.20.3.2).
    pub const EMOTE: Self = Self::from_static("emote");
    /// The `tell` role (§3.20.3.2).
    pub const TELL: Self = Self::from_static("tell");
}

impl fmt::Display for RoleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a single span is styled.
///
/// A separate enum (rather than a `Style` with an optional role) makes the
/// illegal "a role *and* a conflicting direct color" state unrepresentable, and
/// keeps a [`Role`](SpanStyle::Role) unresolved until render time — which is what
/// lets a tenant restyle `say`/`emote`/`tell` by overriding the palette instead
/// of editing content (§3.20.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStyle {
    /// Unstyled text: emits no escape sequence under any tier (§3.20.1.2).
    Plain,
    /// An already-resolved direct style (e.g. an authored color or attribute).
    Direct(Style),
    /// A semantic role, resolved against the session palette at render time
    /// (§3.20.4.2); an unknown role resolves to unstyled (§3.20.2.2).
    Role(RoleName),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_unstyled() {
        assert!(Style::new().is_unstyled());
    }

    #[test]
    fn builders_set_each_field() {
        let style = Style::new()
            .with_fg(Color::rgb(1, 2, 3))
            .with_bg(Color::rgb(4, 5, 6))
            .with_attrs(Attributes::BOLD);
        assert_eq!(style.fg(), Some(Color::rgb(1, 2, 3)));
        assert_eq!(style.bg(), Some(Color::rgb(4, 5, 6)));
        assert!(style.attrs().contains(Attributes::BOLD));
        assert!(!style.is_unstyled());
    }

    #[test]
    fn with_attrs_accumulates() {
        let style = Style::new()
            .with_attrs(Attributes::BOLD)
            .with_attrs(Attributes::ITALIC);
        assert!(
            style
                .attrs()
                .contains(Attributes::BOLD.insert(Attributes::ITALIC))
        );
    }

    // A baseline constant and a parsed copy of the same name must be equal and
    // hash alike, so a palette keyed by the constants resolves a parsed role.
    #[test]
    fn role_name_static_const_equals_owned_copy() {
        let owned = RoleName::new("say");
        assert_eq!(RoleName::SAY, owned);
        assert_eq!(RoleName::SAY.as_str(), "say");

        let mut set = std::collections::HashSet::new();
        set.insert(RoleName::SAY);
        assert!(set.contains(&RoleName::new("say")));
    }
}
