//! Per-session rendering of styled text to a terminal (§3.20.5).
//!
//! `mud-net` is the session/transport edge. For M1-13 it provides the per-session
//! ANSI [`render`]er: it takes transport-neutral [`StyledText`](mud_core::StyledText)
//! and a [`Palette`](mud_core::Palette) and compiles them to ANSI escapes for a
//! session's color [`Tier`], the one place escape sequences are generated
//! (§3.20.1.2). Color downsampling and SGR emission reuse `anstyle` /
//! `anstyle-lossy`, confined to the conversion adapter.

mod convert;
mod ratelimit;
mod render;
mod tier;

pub use ratelimit::{Burst, Decision, RateLimiter, SustainedRate};
pub use render::render;
pub use tier::{DEFAULT_TENANT_TIER, Tier, process_no_color, resolve_tier};
