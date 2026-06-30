//! Account domain for the Ferrodun engine (§3.15.1).
//!
//! The durable per-player identity layer: accounts, their lifecycle state, the
//! `argon2id` password [`Credential`], and the [`Puppet`]s an account owns. This
//! crate is pure domain — it holds the types and the credential KDF, but no
//! persistence and no async. The `mud-db` account repository persists these
//! values; the M1-19 session FSM consumes them to resolve a login to a caller.
//!
//! Names and credentials are parsed into typed values at construction
//! ([`Username::parse`], [`Credential::hash`]); downstream code never
//! re-validates. Login and registration outcomes ([`LoginError`],
//! [`RegisterError`]) are domain values kept distinct from any persistence
//! failure, so a caller can tell a re-promptable refusal from a server fault.
//! They are intentionally *not* `#[non_exhaustive]`: the persistence layer
//! constructs them, and an exhaustive set lets a new variant surface as a
//! compile error at every match site.

mod account;
mod credential;
mod name;
mod puppet;

pub use account::{
    Account, AccountId, AccountState, LoginError, RegisterError, UnknownAccountState,
};
pub use credential::{Credential, CredentialError};
pub use name::{NameError, PuppetName, Username};
pub use puppet::Puppet;
