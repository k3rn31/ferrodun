//! Ferrodun IPC frame schema (§2.1.3, §2.8.3).
//!
//! `mud-schema` is the shared contract crate for the Gateway↔World IPC channel:
//! it defines the `postcard` frame vocabulary both components speak. The crate
//! is a leaf — it depends on no domain crate — so Gateway and World can both
//! depend on it without coupling to each other.
//!
//! Per §2.8.5.7, these `postcard` IPC frames are *version-locked at build time*
//! and are deliberately **not** part of the code-generated wire protocol
//! (§2.8.3.1, the map / vitals / NPC-action messages rendered to Rust,
//! TypeScript, and GMCP docs). That codegen mechanism arrives with the first
//! real wire protocol in M3; the M1 frames here are hand-written Rust.
//!
//! No M1 frame carries an `EntityKey` — the only entity reference that may cross
//! the IPC boundary (§2.3.1.4). M1 frames carry session text and a [`SessionId`];
//! structured, entity-bearing frames arrive in M3+.

mod codec;
mod frame;
mod session;

pub use codec::{SchemaError, decode, encode};
pub use frame::{
    EchoMode, GatewayFrame, HandshakeAck, InputLine, OutputText, ResumeHandshake, SessionClose,
    SessionConnect, SessionDisconnect, SessionEcho, SessionInput, SessionOutput, WorldFrame,
};
pub use session::{SCHEMA_VERSION, SchemaVersion, SessionId, WorldId};
