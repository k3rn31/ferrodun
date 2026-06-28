//! Adapting domain styles to `anstyle` and downsampling per tier (§3.20.5.4).
//!
//! `mud-core` carries pure-domain styles; this module is the only place that
//! touches `anstyle`, keeping the terminal-rendering dependency at the edge.
//! Downsampling delegates to `anstyle-lossy`, whose conversions are fixed and
//! deterministic. The 4-bit match is done against the **VGA** palette explicitly
//! (not the platform default) so ansi16 rendering is reproducible across
//! operating systems for snapshot tests (§3.20.5.4, §3.10.3).

use anstyle::{Color, Effects, RgbColor};
use anstyle_lossy::palette;
use mud_core::{Attributes, Color as DomainColor, Style as DomainStyle};

use crate::tier::Tier;

/// The fixed 16-color palette ansi16 downsampling matches against. Pinned so the
/// nearest-color result does not vary by platform.
const ANSI16_PALETTE: palette::Palette = palette::VGA;

/// Converts a resolved domain [`Style`](DomainStyle) into an [`anstyle::Style`]
/// for `tier`, downsampling colors and dropping them entirely under
/// [`Tier::Mono`] while keeping attributes (§3.20.5.4).
///
/// All attributes are preserved under every tier, including [`Tier::Mono`]. The
/// spec's "preserve attributes the terminal supports (bold/underline)" narrowing
/// needs per-terminal capability detection (TTYPE / `Core.Hello`, §3.20.5.2 step
/// 3) that arrives with M3; until then M1 keeps the full attribute set rather
/// than guess what a terminal supports.
pub(crate) fn to_anstyle(style: DomainStyle, tier: Tier) -> anstyle::Style {
    anstyle::Style::new()
        .fg_color(style.fg().and_then(|color| color_for_tier(color, tier)))
        .bg_color(style.bg().and_then(|color| color_for_tier(color, tier)))
        .effects(to_effects(style.attrs()))
}

/// Downsamples a 24-bit domain color to a concrete `anstyle` color for `tier`, or
/// `None` under [`Tier::Mono`] (color dropped).
fn color_for_tier(color: DomainColor, tier: Tier) -> Option<Color> {
    let rgb = RgbColor(color.r(), color.g(), color.b());
    match tier {
        Tier::Mono => None,
        Tier::Truecolor => Some(Color::Rgb(rgb)),
        Tier::Xterm256 => Some(Color::Ansi256(anstyle_lossy::rgb_to_xterm(rgb))),
        Tier::Ansi16 => Some(Color::Ansi(anstyle_lossy::rgb_to_ansi(rgb, ANSI16_PALETTE))),
    }
}

/// Maps domain [`Attributes`] to `anstyle` [`Effects`] (reverse → invert).
fn to_effects(attrs: Attributes) -> Effects {
    let mapping = [
        (Attributes::BOLD, Effects::BOLD),
        (Attributes::ITALIC, Effects::ITALIC),
        (Attributes::UNDERLINE, Effects::UNDERLINE),
        (Attributes::BLINK, Effects::BLINK),
        (Attributes::REVERSE, Effects::INVERT),
    ];
    mapping
        .into_iter()
        .fold(Effects::new(), |effects, (domain, anstyle_effect)| {
            if attrs.contains(domain) {
                effects.insert(anstyle_effect)
            } else {
                effects
            }
        })
}
