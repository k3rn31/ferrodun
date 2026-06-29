//! Rendering styled text to ANSI for a session's tier (§3.20.5).
//!
//! This is the per-session telnet renderer (§3.20.1.2): the one place escape
//! sequences are generated. Roles are resolved against the session palette here,
//! at render time, so a tenant palette override restyles output without touching
//! content (§3.20.4.2); an unknown role degrades to unstyled and warns
//! (§3.20.2.2), never aborting.

use mud_core::{Palette, SpanStyle, Style, StyledText};

use crate::convert::to_anstyle;
use crate::tier::Tier;

/// Renders `text` to a string of ANSI escapes for `tier`, resolving roles through
/// `palette`.
///
/// Each span is emitted with its own style prefix and a reset, so styles never
/// bleed between spans. A plain or unstyled span emits its text verbatim, with no
/// escapes (§3.20.1.2). Under [`Tier::Mono`](crate::Tier::Mono) colors are dropped
/// but attributes are kept (§3.20.5.4).
#[must_use]
pub fn render(text: &StyledText, palette: &Palette, tier: Tier) -> String {
    let mut out = String::new();
    for span in text.spans() {
        let style = resolve(span.style(), palette);
        let anstyle = to_anstyle(style, tier);
        out.push_str(&anstyle.render().to_string());
        out.push_str(span.text());
        out.push_str(&anstyle.render_reset().to_string());
    }
    out
}

/// Resolves a span's style choice to a concrete [`Style`], warning on an unknown
/// role and falling back to unstyled (§3.20.2.2).
fn resolve(style: &SpanStyle, palette: &Palette) -> Style {
    match style {
        SpanStyle::Plain => Style::new(),
        SpanStyle::Direct(style) => *style,
        SpanStyle::Role(role) => palette.resolve_role(role).unwrap_or_else(|| {
            tracing::warn!(role = %role, "unknown role; rendering unstyled");
            Style::new()
        }),
    }
}

#[cfg(test)]
mod tests {
    use mud_core::{Attributes, Color, RoleName};

    use super::*;

    // A fixture mixing every span kind: a role (resolved via the baseline palette),
    // a direct palette color + attribute, an attribute-only span, and plain text.
    fn fixture() -> StyledText {
        StyledText::new()
            .role("Alice", RoleName::SAY)
            .plain(" says ")
            .styled(
                "danger",
                Style::new()
                    .with_fg(Color::rgb(0xff, 0x00, 0x00))
                    .with_attrs(Attributes::BOLD),
            )
            .styled(" note", Style::new().with_attrs(Attributes::UNDERLINE))
            .plain("!")
    }

    #[test]
    fn ansi16_render_is_stable() {
        let rendered = render(&fixture(), &Palette::baseline(), Tier::Ansi16);
        // SAY (#cdd6f4) → bright white (97); red (#ff0000) → red (31); 4-bit SGR.
        assert_eq!(
            rendered,
            "\u{1b}[97mAlice\u{1b}[0m says \u{1b}[1m\u{1b}[31mdanger\u{1b}[0m\u{1b}[4m note\u{1b}[0m!"
        );
    }

    #[test]
    fn mono_render_drops_color_but_keeps_attributes() {
        let rendered = render(&fixture(), &Palette::baseline(), Tier::Mono);
        assert_eq!(
            rendered,
            "Alice says \u{1b}[1mdanger\u{1b}[0m\u{1b}[4m note\u{1b}[0m!"
        );
    }

    #[test]
    fn xterm256_render_downsamples_to_the_256_palette() {
        let rendered = render(&fixture(), &Palette::baseline(), Tier::Xterm256);
        // Both colors map to fixed 256-palette indices via anstyle-lossy.
        assert_eq!(
            rendered,
            "\u{1b}[38;5;189mAlice\u{1b}[0m says \u{1b}[1m\u{1b}[38;5;196mdanger\u{1b}[0m\u{1b}[4m note\u{1b}[0m!"
        );
    }

    #[test]
    fn truecolor_render_emits_24_bit_color() {
        let rendered = render(&fixture(), &Palette::baseline(), Tier::Truecolor);
        // The direct red span renders as a 24-bit foreground.
        assert!(
            rendered.contains("\u{1b}[1m\u{1b}[38;2;255;0;0mdanger\u{1b}[0m"),
            "got {rendered:?}"
        );
    }

    #[test]
    fn blink_and_reverse_attributes_emit_their_sgr_codes() {
        let text = StyledText::new().styled(
            "x",
            Style::new().with_attrs(Attributes::BLINK.union(Attributes::REVERSE)),
        );
        // Attributes survive even under mono; blink → SGR 5, reverse/invert → 7.
        let rendered = render(&text, &Palette::baseline(), Tier::Mono);
        assert!(rendered.contains("\u{1b}[5m"), "got {rendered:?}");
        assert!(rendered.contains("\u{1b}[7m"), "got {rendered:?}");
    }

    #[test]
    fn an_unknown_role_renders_unstyled() {
        let text = StyledText::new().role("x", RoleName::new("nope"));
        // No palette entry → no escapes, just the text.
        assert_eq!(render(&text, &Palette::baseline(), Tier::Ansi16), "x");
    }

    #[test]
    fn plain_text_emits_no_escapes() {
        let text = StyledText::from("just text");
        assert_eq!(
            render(&text, &Palette::baseline(), Tier::Truecolor),
            "just text"
        );
    }

    #[test]
    fn empty_styled_text_renders_an_empty_string() {
        // No spans → no prefixes, no text, no resets.
        assert_eq!(
            render(&StyledText::new(), &Palette::baseline(), Tier::Ansi16),
            ""
        );
    }

    #[test]
    fn background_color_is_emitted_at_truecolor_and_dropped_at_mono() {
        let text =
            StyledText::new().styled("x", Style::new().with_bg(Color::rgb(0x00, 0x00, 0xff)));

        // Truecolor carries the 24-bit background (SGR 48;2;r;g;b).
        let truecolor = render(&text, &Palette::baseline(), Tier::Truecolor);
        assert!(
            truecolor.contains("\u{1b}[48;2;0;0;255m"),
            "got {truecolor:?}"
        );

        // Mono drops color entirely; with no attributes the span is verbatim.
        assert_eq!(render(&text, &Palette::baseline(), Tier::Mono), "x");
    }

    #[test]
    fn all_attributes_in_one_span_emit_every_sgr() {
        let attrs = Attributes::BOLD
            .union(Attributes::ITALIC)
            .union(Attributes::UNDERLINE)
            .union(Attributes::BLINK)
            .union(Attributes::REVERSE);
        let text = StyledText::new().styled("x", Style::new().with_attrs(attrs));

        // anstyle renders each effect as its own SGR: bold 1, italic 3, underline 4,
        // blink 5, reverse/invert 7. Attributes survive even under mono.
        let rendered = render(&text, &Palette::baseline(), Tier::Mono);
        for sgr in [
            "\u{1b}[1m",
            "\u{1b}[3m",
            "\u{1b}[4m",
            "\u{1b}[5m",
            "\u{1b}[7m",
        ] {
            assert!(rendered.contains(sgr), "missing {sgr:?} in {rendered:?}");
        }
    }

    #[test]
    fn multiple_distinct_roles_render_independently() {
        let text = StyledText::new()
            .role("Alpha", RoleName::SAY)
            .role("Beta", RoleName::ERROR)
            .role("Gamma", RoleName::SYSTEM);
        let rendered = render(&text, &Palette::baseline(), Tier::Ansi16);

        // Each styled span is independently wrapped and reset, so one reset per span.
        assert_eq!(rendered.matches("\u{1b}[0m").count(), 3, "got {rendered:?}");
        for word in ["Alpha", "Beta", "Gamma"] {
            assert!(rendered.contains(word), "missing {word} in {rendered:?}");
        }
    }
}
