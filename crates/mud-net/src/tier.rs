//! Color capability tiers and their resolution (§3.20.5).

/// A session's color target (§3.20.5.1).
///
/// Ordered from least to most capable. The renderer downsamples a 24-bit color to
/// the session's tier (§3.20.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// No color; attributes only.
    Mono,
    /// The 16 standard ANSI colors.
    Ansi16,
    /// The 256-color xterm palette.
    Xterm256,
    /// 24-bit truecolor.
    Truecolor,
}

/// The tenant default tier when nothing else selects one (§3.20.5.2 step 4):
/// `ansi16`, for maximum client compatibility.
pub const DEFAULT_TENANT_TIER: Tier = Tier::Ansi16;

/// Resolves a session's tier from the inputs M1 supports (§3.20.5.2 steps 2 and
/// 4): `NO_COLOR` forces [`Tier::Mono`]; otherwise the tenant default applies.
///
/// The full resolution order also consults an account preference (step 1) and
/// terminal identification (step 3); both arrive with later milestones (M7 / M3)
/// and slot in ahead of these without reshaping this function.
#[must_use]
pub fn resolve_tier(no_color: bool, tenant_default: Tier) -> Tier {
    if no_color { Tier::Mono } else { tenant_default }
}

/// Whether the **host process** environment requests no color, per the
/// `NO_COLOR` convention (set and non-empty) — <https://no-color.org/>.
///
/// This reads the daemon's own environment, so it is a deployment-level default
/// only. It is **not** the §3.20.5.2 step 2 signal, which is `NO_COLOR` as
/// signalled by the *client* over its session: a per-session value that arrives
/// with terminal negotiation (M3) and must be passed to [`resolve_tier`]
/// explicitly. Wiring this into a session as if it were the client's preference
/// would let one operator's shell blank color for every player.
#[must_use]
pub fn process_no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_forces_mono_over_any_default() {
        assert_eq!(resolve_tier(true, Tier::Truecolor), Tier::Mono);
        assert_eq!(resolve_tier(true, DEFAULT_TENANT_TIER), Tier::Mono);
    }

    #[test]
    fn the_tenant_default_applies_without_no_color() {
        assert_eq!(resolve_tier(false, Tier::Ansi16), Tier::Ansi16);
        assert_eq!(resolve_tier(false, Tier::Truecolor), Tier::Truecolor);
    }
}
