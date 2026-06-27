//! Gateway↔World IPC transport (§2.1.3).
//!
//! Carries the version-locked `mud-schema` frame vocabulary between Gateway and
//! World over two interchangeable transports behind one [`Endpoint`] trait:
//!
//! - [`in_memory_pair`] — an in-process channel for single-process mode (§2.1.3.3).
//! - [`connect`] / [`accept`] — length-prefixed `postcard` over a unix socket for
//!   split mode (§2.1.3.1).
//!
//! Both carry the same frame contract, so the resume handshake
//! ([`announce_sessions`] / [`accept_resume`], §2.1.3.2) is written once and runs
//! over either. Selecting split vs. single deployment is the `mudd` binary's call;
//! this crate provides both.

mod error;
mod handshake;
mod transport;

pub use error::IpcError;
pub use handshake::{accept_resume, announce_sessions};
pub use transport::{
    Endpoint, InMemoryEndpoint, MAX_FRAME_BYTES, SocketEndpoint, accept, connect, in_memory_pair,
};
