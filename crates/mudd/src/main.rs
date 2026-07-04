mod backend;
mod config;
mod places;

use clap::Parser;
use config::{Cli, ServerConfig};

/// Entry point for the `mudd` server binary.
///
/// Parses CLI flags and resolves the server configuration (PLAN M1-22).
/// Boot/run wiring (world load, DB pool, scheduler, gateway) lands in a
/// later task; for now this only proves configuration resolution works.
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = ServerConfig::resolve(&cli)?;

    tracing::info!(
        tenant_count = config.tenants.len(),
        "resolved server config"
    );

    Ok(())
}
