//! Library surface of the `mudd` server: configuration resolution and the
//! multi-tenant boot entry point, exposed for integration tests.
mod backend;
pub mod boot;
pub mod catalog;
pub mod config;
mod places;
pub mod scaffold;
pub mod tenant;
mod world_loop;

pub use boot::boot;
pub use config::{
    Cli, LogFormat, Overrides, ServerConfig, Settings, TenantEntry, tenants_from_catalog,
};
