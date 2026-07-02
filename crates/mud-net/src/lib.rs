//! Per-session rendering of styled text to a terminal (§3.20.5).
//!
//! `mud-net` is the session/transport edge. For M1-13 it provides the per-session
//! ANSI [`render`]er: it takes transport-neutral [`StyledText`](mud_core::StyledText)
//! and a [`Palette`](mud_core::Palette) and compiles them to ANSI escapes for a
//! session's color [`Tier`], the one place escape sequences are generated
//! (§3.20.1.2). Color downsampling and SGR emission reuse `anstyle` /
//! `anstyle-lossy`, confined to the conversion adapter.
//!
//! For M1-20 it adds the sans-IO telnet core (§2.8.2 M1 subset): [`TelnetMachine`]
//! turns raw socket bytes into [`TelnetEvent`]s and negotiation replies, and
//! [`RateLimiter`] enforces the §2.1.1 per-session command rate limit.

mod convert;
mod ratelimit;
mod render;
mod telnet;
mod tier;

pub use ratelimit::{Burst, Decision, RateLimiter, SustainedRate};
pub use render::render;
pub use telnet::{TelnetEvent, TelnetMachine};
pub use tier::{DEFAULT_TENANT_TIER, Tier, process_no_color, resolve_tier};
