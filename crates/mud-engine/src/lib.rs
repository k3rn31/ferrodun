//! The command pipeline that runs player input against a World (§2.7).
//!
//! `mud-engine` is the application tier that ties the pieces together: it takes a
//! [`SessionInput`](mud_schema::SessionInput) line, resolves the session to its
//! caller and command layers, merges those layers (reusing `mud-cmd`), parses
//! the line, lock-checks the caller (reusing `mud-core`'s locks), dispatches a
//! Rust-native handler, and renders the reply. Every run carries a [`CommandId`]
//! for trace correlation (§2.7.1).
//!
//! It depends on `mud-core` (the domain), `mud-cmd` (command model + parser),
//! `mud-schema` (IPC frames), and `mud-i18n` (the engine-string seam); it does
//! **not** depend on the transport (`mud-net`) or the binary (`mudd`), which sit
//! above it. Accounts (M1-18) and the session FSM (M1-19) plug in through the
//! [`SessionResolver`] seam without reshaping the pipeline; built-in command
//! bodies (M1-17) plug in as [`CommandHandler`]s.

mod builtins;
mod caller;
mod command_id;
mod dispatch;
mod layers;
mod objects;
mod pipeline;
mod places;
mod roster;
pub mod session;
mod text;

pub use builtins::register;
pub use caller::{CallerContext, ResolvedSession, SessionResolver};
pub use command_id::CommandId;
pub use dispatch::{
    Broadcast, CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher,
    SessionDisposition,
};
pub use layers::LayerCommands;
pub use objects::{Resolution, resolve_among};
pub use pipeline::{DispatchOutcome, Pipeline, PipelineError};
pub use places::Places;
pub use roster::{Presence, Roster};
pub use session::{
    BackendError, InWorldBinding, LoginBackend, LoginOutput, RegistryResolver, Routing,
    SessionService,
};
pub use text::{ContentTooLong, MAX_CONTENT_BYTES, sanitize};
