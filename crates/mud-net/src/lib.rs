//! The session/transport edge: telnet protocol, per-session rendering, and rate
//! limiting (§2.8.2, §3.20.5, §2.1.1).
//!
//! `mud-net` sits between the raw socket and the engine, and does three things,
//! all sans-IO (it never owns a socket — the M1-21 gateway drives it):
//!
//! - **Telnet core** — [`TelnetMachine`] turns raw client bytes into typed
//!   [`TelnetEvent`]s and negotiation replies (the §2.8.2 M1 subset: NAWS,
//!   TTYPE, EOR, CHARSET).
//! - **Rendering** — [`render`] compiles transport-neutral
//!   [`StyledText`](mud_core::StyledText) against a [`Palette`](mud_core::Palette)
//!   into ANSI escapes for a session's color [`Tier`]. This is the one place
//!   escape sequences are generated (§3.20.1.2); downsampling and SGR emission
//!   reuse `anstyle` / `anstyle-lossy`, confined to the conversion adapter.
//! - **Rate limiting** — [`RateLimiter`] enforces the §2.1.1 per-session command
//!   rate limit.

mod convert;
mod ratelimit;
mod render;
mod telnet;
mod tier;

pub use ratelimit::{Burst, Decision, RateLimiter, SustainedRate};
pub use render::render;
pub use telnet::{TelnetEvent, TelnetMachine};
pub use tier::{DEFAULT_TENANT_TIER, Tier, process_no_color, resolve_tier};
