use anyhow::Context;
use clap::Parser;
use mudd::boot::boot;
use mudd::config::{Cli, ServerConfig};
use tracing_subscriber::EnvFilter;

/// Wire format for the process log stream, selected by `FERRODUN_LOG_FORMAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Text,
    Json,
}

/// Parses `FERRODUN_LOG_FORMAT`. Absent means text; anything but
/// `text`/`json` is a startup error — fail fast rather than silently
/// mis-formatting the log stream an aggregator depends on.
fn parse_log_format(raw: Option<&str>) -> anyhow::Result<LogFormat> {
    match raw {
        None | Some("text") => Ok(LogFormat::Text),
        Some("json") => Ok(LogFormat::Json),
        Some(other) => anyhow::bail!(
            "unknown FERRODUN_LOG_FORMAT {other:?} (expected \"text\" or \"json\")"
        ),
    }
}

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

/// Entry point for the `mudd` server binary.
///
/// Parses CLI flags, resolves the server configuration, and boots every
/// configured tenant (PLAN M1-22). Fail-stop: any tenant task fault or a
/// panicked task ends the process (design §8).
fn main() -> anyhow::Result<()> {
    let format = {
        let raw = std::env::var("FERRODUN_LOG_FORMAT").ok();
        parse_log_format(raw.as_deref())?
    };
    init_tracing(format);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_format_defaults_to_text() {
        let format = parse_log_format(None).expect("default log format must parse");
        assert_eq!(format, LogFormat::Text);
    }

    #[test]
    fn text_and_json_parse() {
        assert_eq!(
            parse_log_format(Some("text")).expect("text must parse"),
            LogFormat::Text
        );
        assert_eq!(
            parse_log_format(Some("json")).expect("json must parse"),
            LogFormat::Json
        );
    }

    #[test]
    fn an_unknown_format_is_a_startup_error() {
        let err = parse_log_format(Some("yaml"))
            .expect_err("unknown log format must fail fast, not silently default");
        assert!(err.to_string().contains("FERRODUN_LOG_FORMAT"));
    }
}
