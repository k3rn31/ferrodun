//! Library surface of the `mudd` server: configuration resolution and the
//! multi-tenant boot entry point, exposed for integration tests.
mod backend;
pub mod boot;
pub mod config;
mod places;
mod world_loop;

pub use boot::boot;
pub use config::{Cli, ServerConfig, TenantEntry};
