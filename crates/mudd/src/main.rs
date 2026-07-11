use std::path::Path;

use anyhow::Context;
use clap::Parser;
use mudd::boot::boot;
use mudd::config::{
    Cli, Command, LogFormat, Overrides, ServeArgs, ServerConfig, Settings, TenantCommand,
};
use mudd::{serve_tenants, tenant};
use tracing_subscriber::EnvFilter;

/// Installs the process-global subscriber. JSON mode emits current-span and
/// span-list fields so the tenant/session/command span taxonomy (design §4)
/// is visible to aggregators; the text formatter shows spans in its prefix.
fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init(),
    }
}

/// Entry point for the `mudd` server binary: dispatches `serve` and
/// `tenant` subcommands. Bare `mudd` prints the subcommand help (clap
/// `arg_required_else_help`).
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => serve(cli.config.as_deref(), &args),
        Command::Tenant(command) => run_tenant(cli.config.as_deref(), command),
    }
}

/// Resolves settings, builds the boot registry (catalogue or
/// `--tenant-dir`), and serves until shutdown. Fail-stop: any tenant task
/// fault or a panicked task ends the process (design §8).
fn serve(config: Option<&Path>, args: &ServeArgs) -> anyhow::Result<()> {
    // Resolve config before installing the subscriber so the log format is
    // itself a configured value (flag > MUDD_ env > config.toml > default).
    // Nothing logs before `boot`, so a config error surfacing here without a
    // tracing subscriber loses no diagnostics.
    let overrides = Overrides {
        rate: args.rate,
        burst: args.burst,
        log_format: args.log_format,
    };
    let settings = Settings::resolve(config, &overrides)?;
    init_tracing(settings.log_format);

    let tenants = serve_tenants(&settings, args)?;
    let server = ServerConfig {
        rate: settings.rate,
        burst: settings.burst,
        tenants,
        log_format: settings.log_format,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async_main(server))
}

/// Dispatches a `mudd tenant` subcommand against the catalogue.
fn run_tenant(config: Option<&Path>, command: TenantCommand) -> anyhow::Result<()> {
    let settings = Settings::resolve(config, &Overrides::default())?;
    let mut out = std::io::stdout().lock();
    match command {
        TenantCommand::Add { name } => tenant::add(&settings, &name, &mut out),
        TenantCommand::Remove { name, purge } => {
            let mut input = std::io::stdin().lock();
            tenant::remove(&settings, &name, purge, &mut input, &mut out)
        }
        TenantCommand::List => tenant::list(&settings, &mut out),
    }
}

/// Boots every configured tenant and runs until shutdown is requested or a
/// tenant task ends.
async fn async_main(config: ServerConfig) -> anyhow::Result<()> {
    let (addrs, mut tasks) = boot(config).await?;
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
            Some(Ok(Err(error))) => {
                // `?error`: anyhow's Debug prints the whole context chain;
                // Display would keep only the outermost message.
                tracing::error!(error = ?error, "tenant task failed");
                Err(error)
            }
            Some(Err(join_error)) => {
                tracing::error!(error = %join_error, "tenant task panicked");
                Err(anyhow::anyhow!(join_error)).context("tenant task panicked")
            }
        }
    }
}
