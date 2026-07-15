//! The palette: semantic roles and named colors → concrete styles (§3.20.3).
//!
//! The engine ships a [`baseline`](Palette::baseline) palette defining its own
//! roles (§3.20.3.2). A tenant palette is layered on top by the world loader,
//! which overrides roles and adds named colors. Resolution is pure: an unknown
//! role returns `None`, and the *renderer* — not the domain — emits the
//! §3.20.2.2 warning and falls back to unstyled, keeping escape and logging
//! concerns at the session boundary.

use std::collections::HashMap;

use super::attributes::Attributes;
use super::color::Color;
use super::style::{RoleName, Style};

/// A map from semantic roles and named colors to concrete styles (§3.20.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Palette {
    roles: HashMap<RoleName, Style>,
    colors: HashMap<String, Color>,
}

impl Palette {
    /// The engine-default baseline palette (§3.20.3.2).
    ///
    /// Defines the baseline roles and the sixteen standard named colors. Built in
    /// Rust rather than parsed from embedded KDL so it is infallible and pulls in
    /// no parser; the KDL form is only the authoring/override surface, owned by
    /// the world loader.
    pub fn baseline() -> Self {
        let colors = [
            ("black", Color::rgb(0x00, 0x00, 0x00)),
            ("red", Color::rgb(0xcc, 0x00, 0x00)),
            ("green", Color::rgb(0x4e, 0x9a, 0x06)),
            ("yellow", Color::rgb(0xc4, 0xa0, 0x00)),
            ("blue", Color::rgb(0x34, 0x65, 0xa4)),
            ("magenta", Color::rgb(0x75, 0x50, 0x7b)),
            ("cyan", Color::rgb(0x06, 0x98, 0x9a)),
            ("white", Color::rgb(0xd3, 0xd7, 0xcf)),
            ("bright_black", Color::rgb(0x55, 0x57, 0x53)),
            ("bright_red", Color::rgb(0xef, 0x29, 0x29)),
            ("bright_green", Color::rgb(0x8a, 0xe2, 0x34)),
            ("bright_yellow", Color::rgb(0xfc, 0xe9, 0x4f)),
            ("bright_blue", Color::rgb(0x72, 0x9f, 0xcf)),
            ("bright_magenta", Color::rgb(0xad, 0x7f, 0xa8)),
            ("bright_cyan", Color::rgb(0x34, 0xe2, 0xe2)),
            ("bright_white", Color::rgb(0xee, 0xee, 0xec)),
        ]
        .into_iter()
        .map(|(name, color)| (name.to_owned(), color))
        .collect();

        // The §3.20.3.2 baseline roles. Values follow the §3.20.3.1 normative
        // shape where it gives them and pick readable defaults otherwise.
        // The full mandated set is defined even where the command that emits a
        // role (`emote` §3.6.3, `tell` §3.6.2) lands in a later milestone:
        // the spec requires the roles "at minimum", and tenant palettes may
        // already override them.
        let roles = [
            (
                RoleName::ERROR,
                Style::new().with_fg(Color::rgb(0xff, 0x55, 0x55)),
            ),
            (
                RoleName::SYSTEM,
                Style::new().with_fg(Color::rgb(0x7a, 0xa2, 0xf7)),
            ),
            (
                RoleName::ALERT,
                Style::new()
                    .with_fg(Color::rgb(0xff, 0xff, 0xff))
                    .with_bg(Color::rgb(0xaa, 0x00, 0x00))
                    .with_attrs(Attributes::BOLD),
            ),
            (
                RoleName::PROMPT,
                Style::new().with_fg(Color::rgb(0x9e, 0xce, 0x6a)),
            ),
            (
                RoleName::SAY,
                Style::new().with_fg(Color::rgb(0xcd, 0xd6, 0xf4)),
            ),
            (
                RoleName::EMOTE,
                Style::new().with_fg(Color::rgb(0xba, 0xc2, 0xde)),
            ),
            (
                RoleName::TELL,
                Style::new().with_fg(Color::rgb(0xf5, 0xc2, 0xe7)),
            ),
        ]
        .into_iter()
        .collect();

        Self { roles, colors }
    }

    /// The style for `role`, or `None` if the palette does not define it
    /// (§3.20.2.2 — the caller renders unstyled and warns).
    #[must_use]
    pub fn resolve_role(&self, role: &RoleName) -> Option<Style> {
        self.roles.get(role).copied()
    }

    /// The color named `name`, or `None` if the palette does not define it.
    #[must_use]
    pub fn color(&self, name: &str) -> Option<Color> {
        self.colors.get(name).copied()
    }

    /// Defines or overrides the style for `role`. Used by the world loader to
    /// layer a tenant palette over the baseline.
    pub fn insert_role(&mut self, role: RoleName, style: Style) {
        self.roles.insert(role, style);
    }

    /// Defines or overrides a named color. Used by the world loader to layer a
    /// tenant palette over the baseline.
    pub fn insert_color(&mut self, name: impl Into<String>, color: Color) {
        self.colors.insert(name.into(), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_defines_every_required_role() {
        let palette = Palette::baseline();
        for role in [
            RoleName::ERROR,
            RoleName::SYSTEM,
            RoleName::ALERT,
            RoleName::PROMPT,
            RoleName::SAY,
            RoleName::EMOTE,
            RoleName::TELL,
        ] {
            assert!(palette.resolve_role(&role).is_some(), "missing {role}");
        }
    }

    #[test]
    fn baseline_alert_carries_background_and_bold() {
        let alert = Palette::baseline()
            .resolve_role(&RoleName::ALERT)
            .expect("alert role");
        assert!(alert.bg().is_some());
        assert!(alert.attrs().contains(Attributes::BOLD));
    }

    #[test]
    fn baseline_defines_the_standard_named_colors() {
        let palette = Palette::baseline();
        assert!(palette.color("cyan").is_some());
        assert!(palette.color("bright_white").is_some());
    }

    #[test]
    fn unknown_role_and_color_resolve_to_none() {
        let palette = Palette::baseline();
        assert_eq!(palette.resolve_role(&RoleName::new("nope")), None);
        assert_eq!(palette.color("chartreuse"), None);
    }

    #[test]
    fn insert_overrides_a_baseline_role() {
        let mut palette = Palette::baseline();
        let override_style = Style::new().with_fg(Color::rgb(1, 2, 3));
        palette.insert_role(RoleName::SAY, override_style);
        assert_eq!(palette.resolve_role(&RoleName::SAY), Some(override_style));
    }
}
