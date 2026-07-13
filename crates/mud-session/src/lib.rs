//! Pure, sans-IO login state machine (§3.19.1, §2.7 step 1).
//!
//! `mud-session` turns a pre-login connection into an in-world caller through a
//! banner → login/register → puppet-select flow. It performs **no I/O**: it maps
//! `(state, input line)` to typed [`SessionMessage`]s and [`Effect`]s that the
//! World-side driver executes, feeding results back via
//! [`SessionFsm::on_effect`]. This keeps the machine trivially testable and its
//! password handling confined to [`secrecy`].

mod effect;
mod fsm;
mod message;

pub use effect::{Effect, EffectResult};
pub use fsm::{InputEcho, SessionFsm, Terminal, Transition};
pub use message::SessionMessage;
