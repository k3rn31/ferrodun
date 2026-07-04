mod backend;
mod boot;
mod config;
mod places;
mod world_loop;

use anyhow::Context;
use clap::Parser;
use config::{Cli, ServerConfig};
use tracing_subscriber::EnvFilter;

/// Entry point for the `mudd` server binary.
///
/// Parses CLI flags, resolves the server configuration, and boots every
/// configured tenant (PLAN M1-22). Fail-stop: any tenant task fault or a
/// panicked task ends the process (design §8).
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async_main(cli))
}

/// Boots every configured tenant and runs until shutdown is requested or a
/// tenant task ends.
async fn async_main(cli: Cli) -> anyhow::Result<()> {
    let config = ServerConfig::resolve(&cli)?;
    let (addrs, mut tasks) = boot::boot(config).await?;
    for addr in &addrs {
        tracing::info!(%addr, "tenant listening");
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal");
            Ok(())
        }
        joined = tasks.join_next() => match joined {
            Some(Ok(Ok(()))) | None => Ok(()),
            Some(Ok(Err(error))) => Err(error),
            Some(Err(join_error)) => Err(anyhow::anyhow!(join_error)).context("tenant task panicked"),
        }
    }
}
