//! Server configuration for the `mudd` binary (PLAN M1-22).
//!
//! Resolution precedence (low to high): built-in defaults, `config.toml`,
//! `MUDD_`-prefixed environment variables, CLI flags. The config file is
//! located via `--config` or, absent that, the XDG base directory
//! (`$XDG_CONFIG_HOME/ferrodun/config.toml`, falling back to
//! `$HOME/.config/ferrodun/config.toml`; Linux is the only supported target).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use mud_core::TenantTag;
use mud_net::{Burst, SustainedRate};
use serde::{Deserialize, Serialize};

/// Default telnet listen address for `--tenant-dir` mode.
const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 4000);

/// Wire format for the process log stream. Server-wide: the subscriber is
/// process-global, so this cannot vary per tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text with a span prefix.
    #[default]
    Text,
    /// One JSON object per line, for log aggregators.
    Json,
}

/// Command-line arguments; every flag overrides the server config (PLAN M1-22).
#[derive(Debug, clap::Parser)]
pub struct Cli {
    /// Server config path (default: $XDG_CONFIG_HOME/ferrodun/config.toml).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Boot exactly this tenant, replacing the configured registry.
    #[arg(long)]
    pub tenant_dir: Option<PathBuf>,
    /// Listen address for --tenant-dir mode (default 127.0.0.1:4000).
    #[arg(long)]
    pub listen: Option<SocketAddr>,
    /// Per-session sustained command rate (commands/second).
    #[arg(long)]
    pub rate: Option<NonZeroU32>,
    /// Per-session command burst allowance.
    #[arg(long)]
    pub burst: Option<NonZeroU32>,
    /// Log wire format: `text` (default) or `json`.
    #[arg(long, value_enum)]
    pub log_format: Option<LogFormat>,
}

/// One registered tenant: its folder, telnet listen address, and the runtime
/// tenant tag stamped into its `EntityId`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantEntry {
    pub dir: PathBuf,
    pub listen: SocketAddr,
    pub tag: TenantTag,
}

/// The `[[tenants]]` shape as authored in the server config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawTenantEntry {
    dir: PathBuf,
    listen: SocketAddr,
}

/// The resolved server configuration (defaults < config.toml < MUDD_* env < flags).
#[derive(Debug, PartialEq, Eq)]
pub struct ServerConfig {
    pub rate: SustainedRate,
    pub burst: Burst,
    pub tenants: Vec<TenantEntry>,
    pub log_format: LogFormat,
}

/// Untyped shape extracted from figment before conversion to typed values.
#[derive(Debug, Serialize, Deserialize)]
struct RawServerConfig {
    #[serde(default = "default_rate")]
    rate: NonZeroU32,
    #[serde(default = "default_burst")]
    burst: NonZeroU32,
    #[serde(default)]
    tenants: Vec<RawTenantEntry>,
    #[serde(default)]
    log_format: LogFormat,
}

impl Default for RawServerConfig {
    fn default() -> Self {
        RawServerConfig {
            rate: default_rate(),
            burst: default_burst(),
            tenants: Vec::new(),
            log_format: LogFormat::default(),
        }
    }
}

/// Default sustained command rate (commands/second), matching [`SustainedRate::DEFAULT`].
const DEFAULT_RATE: u32 = 10;

/// Default command burst allowance, matching [`Burst::DEFAULT`].
const DEFAULT_BURST: u32 = 20;

/// Default sustained command rate: mirrors [`SustainedRate::DEFAULT`].
fn default_rate() -> NonZeroU32 {
    NonZeroU32::new(DEFAULT_RATE).unwrap_or(NonZeroU32::MIN)
}

/// Default command burst allowance: mirrors [`Burst::DEFAULT`].
fn default_burst() -> NonZeroU32 {
    NonZeroU32::new(DEFAULT_BURST).unwrap_or(NonZeroU32::MIN)
}

impl ServerConfig {
    /// Resolves the server configuration from defaults, the config file,
    /// environment variables, and CLI flags, in that precedence order.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file is unreadable or malformed, if
    /// the resulting tenant registry is empty and `--tenant-dir` was not
    /// given, or if two tenants share a listen address.
    pub fn resolve(cli: &Cli) -> anyhow::Result<ServerConfig> {
        let config_path = config_file_path(cli)?;

        let raw: RawServerConfig = Figment::from(Serialized::defaults(RawServerConfig::default()))
            .merge(Toml::file(config_path))
            .merge(Env::prefixed("MUDD_"))
            .extract()?;

        let rate = cli.rate.unwrap_or(raw.rate);
        let burst = cli.burst.unwrap_or(raw.burst);
        let log_format = cli.log_format.unwrap_or(raw.log_format);

        let tenants = match &cli.tenant_dir {
            Some(dir) => vec![TenantEntry {
                dir: dir.clone(),
                listen: cli.listen.unwrap_or(DEFAULT_LISTEN),
                tag: TenantTag::default(),
            }],
            None => raw
                .tenants
                .into_iter()
                .enumerate()
                .map(|(index, raw)| {
                    let position = u16::try_from(index + 1)
                        .ok()
                        .and_then(|value| TenantTag::new(value).ok())
                        .ok_or_else(|| {
                            anyhow::anyhow!("too many tenants: at most 4095 fit in one process")
                        })?;
                    Ok(TenantEntry {
                        dir: raw.dir,
                        listen: raw.listen,
                        tag: position,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        };

        if tenants.is_empty() {
            anyhow::bail!(
                "no tenants configured: add [[tenants]] entries to the server config, or pass --tenant-dir"
            );
        }

        let mut seen_addrs = HashSet::with_capacity(tenants.len());
        for tenant in &tenants {
            if !seen_addrs.insert(tenant.listen) {
                anyhow::bail!("duplicate listen address {} across tenants", tenant.listen);
            }
        }

        Ok(ServerConfig {
            rate: SustainedRate::new(rate),
            burst: Burst::new(burst),
            tenants,
            log_format,
        })
    }
}

/// Resolves the server config file path: `--config` if given, else the
/// XDG-standard `ferrodun/config.toml` under `$XDG_CONFIG_HOME` (or
/// `$HOME/.config`).
fn config_file_path(cli: &Cli) -> anyhow::Result<PathBuf> {
    if let Some(path) = &cli.config {
        return Ok(path.clone());
    }

    let config_home = match std::env::var("XDG_CONFIG_HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => {
            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("neither XDG_CONFIG_HOME nor HOME is set"))?;
            PathBuf::from(home).join(".config")
        }
    };

    Ok(config_home.join("ferrodun").join("config.toml"))
}

#[cfg(test)]
// LINT: figment::Jail::expect_with returns figment::Error in the closure's
// Result; it's test harness's type, not ours.
#[allow(clippy::result_large_err)]
mod tests {
    use std::num::NonZeroU32;

    use mud_core::TenantTag;

    use super::*;

    fn cli_with_tenant_dir(dir: &str) -> Cli {
        Cli {
            config: None,
            tenant_dir: Some(PathBuf::from(dir)),
            listen: None,
            rate: None,
            burst: None,
            log_format: None,
        }
    }

    #[test]
    fn defaults_apply_when_the_config_file_is_absent() {
        figment::Jail::expect_with(|jail| {
            let config_home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", config_home.display());
            let cli = cli_with_tenant_dir("/t");
            let config = ServerConfig::resolve(&cli).expect("config resolves");

            assert_eq!(
                config.tenants,
                vec![TenantEntry {
                    dir: PathBuf::from("/t"),
                    listen: DEFAULT_LISTEN,
                    tag: TenantTag::default(),
                }]
            );
            assert_eq!(config.rate, SustainedRate::DEFAULT);
            assert_eq!(config.burst, Burst::DEFAULT);
            Ok(())
        });
    }

    #[test]
    fn the_registry_loads_from_the_server_config() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
rate = 5

[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"

[[tenants]]
dir = "/tenants/b"
listen = "127.0.0.1:4002"
"#,
            )?;
            let cli = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };
            let config = ServerConfig::resolve(&cli).expect("config resolves");

            assert_eq!(config.tenants.len(), 2);
            assert_eq!(
                config.rate,
                SustainedRate::new(NonZeroU32::new(5).expect("nonzero"))
            );
            Ok(())
        });
    }

    #[test]
    fn tenant_dir_flag_replaces_the_registry() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"

[[tenants]]
dir = "/tenants/b"
listen = "127.0.0.1:4002"
"#,
            )?;
            let cli = Cli {
                config: Some(jail.directory().join("config.toml")),
                ..cli_with_tenant_dir("/override")
            };
            let config = ServerConfig::resolve(&cli).expect("config resolves");

            assert_eq!(config.tenants.len(), 1);
            let tenant = config.tenants.first().expect("one tenant");
            assert_eq!(tenant.dir, PathBuf::from("/override"));
            Ok(())
        });
    }

    #[test]
    fn env_overrides_file_and_flags_override_env() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
rate = 5

[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"
"#,
            )?;
            jail.set_env("MUDD_RATE", "7");

            let cli_env_only = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };
            let env_config = ServerConfig::resolve(&cli_env_only).expect("config resolves");
            assert_eq!(
                env_config.rate,
                SustainedRate::new(NonZeroU32::new(7).expect("nonzero"))
            );

            let cli_with_flag = Cli {
                rate: Some(NonZeroU32::new(9).expect("nonzero")),
                ..cli_env_only
            };
            let flag_config = ServerConfig::resolve(&cli_with_flag).expect("config resolves");
            assert_eq!(
                flag_config.rate,
                SustainedRate::new(NonZeroU32::new(9).expect("nonzero"))
            );
            Ok(())
        });
    }

    #[test]
    fn log_format_defaults_to_text() {
        figment::Jail::expect_with(|jail| {
            let config_home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", config_home.display());
            let cli = cli_with_tenant_dir("/t");
            let config = ServerConfig::resolve(&cli).expect("config resolves");

            assert_eq!(config.log_format, LogFormat::Text);
            Ok(())
        });
    }

    #[test]
    fn log_format_reads_from_the_config_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
log_format = "json"

[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"
"#,
            )?;
            let cli = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };
            let config = ServerConfig::resolve(&cli).expect("config resolves");

            assert_eq!(config.log_format, LogFormat::Json);
            Ok(())
        });
    }

    #[test]
    fn env_sets_log_format_and_the_flag_overrides_it() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"
"#,
            )?;
            jail.set_env("MUDD_LOG_FORMAT", "json");

            let cli_env_only = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };
            let env_config = ServerConfig::resolve(&cli_env_only).expect("config resolves");
            assert_eq!(env_config.log_format, LogFormat::Json);

            let cli_with_flag = Cli {
                log_format: Some(LogFormat::Text),
                ..cli_env_only
            };
            let flag_config = ServerConfig::resolve(&cli_with_flag).expect("config resolves");
            assert_eq!(flag_config.log_format, LogFormat::Text);
            Ok(())
        });
    }

    #[test]
    fn an_unknown_log_format_is_a_startup_error() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
log_format = "yaml"

[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"
"#,
            )?;
            let cli = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };

            let result = ServerConfig::resolve(&cli);
            assert!(
                result.is_err(),
                "unknown log format must fail fast, got {result:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn an_empty_registry_without_tenant_dir_is_an_error() {
        figment::Jail::expect_with(|jail| {
            let config_home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", config_home.display());
            let cli = Cli {
                config: None,
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };

            let result = ServerConfig::resolve(&cli);
            assert!(result.is_err(), "expected an error, got {result:?}");
            Ok(())
        });
    }

    #[test]
    fn duplicate_listen_addresses_are_rejected() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
[[tenants]]
dir = "/tenants/a"
listen = "127.0.0.1:4001"

[[tenants]]
dir = "/tenants/b"
listen = "127.0.0.1:4001"
"#,
            )?;
            let cli = Cli {
                config: Some(jail.directory().join("config.toml")),
                tenant_dir: None,
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };

            let result = ServerConfig::resolve(&cli);
            assert!(result.is_err(), "expected an error, got {result:?}");
            Ok(())
        });
    }
}
