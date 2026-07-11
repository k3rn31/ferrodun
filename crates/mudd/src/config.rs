//! Server configuration for the `mudd` binary (PLAN M1-22).
//!
//! Resolution precedence (low to high): built-in defaults, `config.toml`,
//! `MUDD_`-prefixed environment variables, CLI flags. The config file is
//! located via `--config` or, absent that, the XDG base directory
//! (`$XDG_CONFIG_HOME/ferrodun/config.toml`, falling back to
//! `$HOME/.config/ferrodun/config.toml`; Linux is the only supported target).

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use mud_core::TenantTag;
use mud_net::{Burst, SustainedRate};
use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;

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

/// Command-line interface for the `mudd` server binary.
#[derive(Debug, clap::Parser)]
#[command(
    name = "mudd",
    version,
    about = "The Ferrodun MUD server",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Server config path (default: $XDG_CONFIG_HOME/ferrodun/config.toml).
    /// The tenant catalogue (catalog.toml) always sits beside it.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level `mudd` subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Serve every tenant registered in the catalogue.
    Serve(ServeArgs),
    /// Manage the tenant catalogue.
    #[command(subcommand)]
    Tenant(TenantCommand),
}

/// Flags for `mudd serve`; each overrides the server config.
#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Boot exactly this tenant, bypassing the catalogue (dev mode).
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

/// `mudd tenant` subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum TenantCommand {
    /// Register a tenant: assign a port and tag, scaffold its folder.
    Add { name: String },
    /// Deregister a tenant. --purge also deletes its folder (asks for
    /// confirmation).
    Remove {
        name: String,
        #[arg(long)]
        purge: bool,
    },
    /// List registered tenants.
    List,
}

/// One registered tenant: its folder, telnet listen address, and the runtime
/// tenant tag stamped into its `EntityId`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantEntry {
    pub dir: PathBuf,
    pub listen: SocketAddr,
    pub tag: TenantTag,
}

/// The resolved server configuration (defaults < config.toml < MUDD_* env < flags).
#[derive(Debug, PartialEq, Eq)]
pub struct ServerConfig {
    pub rate: SustainedRate,
    pub burst: Burst,
    pub tenants: Vec<TenantEntry>,
    pub log_format: LogFormat,
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

/// Resolves the default server config file path: the XDG-standard
/// `ferrodun/config.toml` under `$XDG_CONFIG_HOME` (or `$HOME/.config`).
/// Callers that support a `--config` flag should short-circuit to it before
/// falling back to this.
fn default_config_path() -> anyhow::Result<PathBuf> {
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

/// Server-wide runtime config keys for the catalogue-era subcommand CLI:
/// tenant registry root, listener bind address, and where the tenant
/// catalogue lives. `MUDD_`-prefixed env vars override the config file;
/// [`Overrides`] (parsed from flags) override the environment.
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub rate: SustainedRate,
    pub burst: Burst,
    pub log_format: LogFormat,
    pub tenants_dir: PathBuf,
    pub bind: IpAddr,
    pub base_port: u16,
    pub catalog_path: PathBuf,
}

/// Flag-level overrides for [`Settings::resolve`]; each field beats both the
/// config file and the environment when set.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub rate: Option<NonZeroU32>,
    pub burst: Option<NonZeroU32>,
    pub log_format: Option<LogFormat>,
}

/// Untyped shape extracted from figment before conversion to typed values.
#[derive(Debug, Serialize, Deserialize)]
struct RawSettings {
    #[serde(default = "default_rate")]
    rate: NonZeroU32,
    #[serde(default = "default_burst")]
    burst: NonZeroU32,
    #[serde(default)]
    log_format: LogFormat,
    tenants_dir: Option<PathBuf>,
    #[serde(default = "default_bind")]
    bind: IpAddr,
    #[serde(default = "default_base_port")]
    base_port: u16,
}

impl Default for RawSettings {
    fn default() -> Self {
        RawSettings {
            rate: default_rate(),
            burst: default_burst(),
            log_format: LogFormat::default(),
            tenants_dir: None,
            bind: default_bind(),
            base_port: default_base_port(),
        }
    }
}

/// Default listener host: loopback, so a fresh install is never publicly
/// exposed by accident.
fn default_bind() -> IpAddr {
    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

/// Default lowest assignable tenant port.
fn default_base_port() -> u16 {
    4000
}

impl Settings {
    /// Resolves settings (built-in defaults < `config.toml` < `MUDD_`-prefixed
    /// env < `overrides`). `config` overrides the XDG config-file location;
    /// `None` uses the XDG default.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file is unreadable or malformed, or if
    /// neither `XDG_DATA_HOME` nor `HOME` is set and the default tenants
    /// directory is needed.
    pub fn resolve(config: Option<&Path>, overrides: &Overrides) -> anyhow::Result<Settings> {
        let config_path = match config {
            Some(path) => path.to_path_buf(),
            None => default_config_path()?,
        };
        let raw: RawSettings = Figment::from(Serialized::defaults(RawSettings::default()))
            .merge(Toml::file(&config_path))
            .merge(Env::prefixed("MUDD_"))
            .extract()?;

        let tenants_dir = match raw.tenants_dir {
            Some(dir) => dir,
            None => default_tenants_dir()?,
        };
        let catalog_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("catalog.toml");

        Ok(Settings {
            rate: SustainedRate::new(overrides.rate.unwrap_or(raw.rate)),
            burst: Burst::new(overrides.burst.unwrap_or(raw.burst)),
            log_format: overrides.log_format.unwrap_or(raw.log_format),
            tenants_dir,
            bind: raw.bind,
            base_port: raw.base_port,
            catalog_path,
        })
    }
}

/// Maps the tenant catalogue onto [`TenantEntry`] values ready to boot:
/// each entry's folder under `tenants_dir`, its listen address (`bind` +
/// its assigned port), and its runtime tag.
///
/// # Errors
///
/// Returns an error if the catalogue is empty — nothing to serve.
pub fn tenants_from_catalog(
    settings: &Settings,
    catalog: &Catalog,
) -> anyhow::Result<Vec<TenantEntry>> {
    if catalog.entries().is_empty() {
        anyhow::bail!("no tenants registered: run `mudd tenant add <name>`");
    }
    Ok(catalog
        .entries()
        .iter()
        .map(|entry| TenantEntry {
            dir: settings.tenants_dir.join(entry.name.as_str()),
            listen: SocketAddr::new(settings.bind, entry.port),
            tag: entry.tag,
        })
        .collect())
}

/// Builds the boot registry for `mudd serve`: the single `--tenant-dir`
/// tenant (tag 0) in dev mode, otherwise the catalogue via
/// [`tenants_from_catalog`].
///
/// # Errors
///
/// Returns an error if the catalogue is unreadable, invalid, or empty.
pub fn serve_tenants(settings: &Settings, args: &ServeArgs) -> anyhow::Result<Vec<TenantEntry>> {
    match &args.tenant_dir {
        Some(dir) => Ok(vec![TenantEntry {
            dir: dir.clone(),
            listen: args.listen.unwrap_or(DEFAULT_LISTEN),
            tag: TenantTag::default(),
        }]),
        None => {
            let catalog = Catalog::load(&settings.catalog_path)?;
            tenants_from_catalog(settings, &catalog)
        }
    }
}

/// XDG-standard tenants root: `$XDG_DATA_HOME/ferrodun/tenants`, falling
/// back to `~/.local/share/ferrodun/tenants`.
fn default_tenants_dir() -> anyhow::Result<PathBuf> {
    let data_home = match std::env::var("XDG_DATA_HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => {
            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("neither XDG_DATA_HOME nor HOME set"))?;
            PathBuf::from(home).join(".local").join("share")
        }
    };
    Ok(data_home.join("ferrodun").join("tenants"))
}

#[cfg(test)]
// LINT: figment::Jail::expect_with returns figment::Error in the closure's
// Result; it's test harness's type, not ours.
#[allow(clippy::result_large_err)]
mod tests {
    use std::num::NonZeroU32;

    use mud_core::TenantTag;

    use super::*;

    #[test]
    fn an_unknown_log_format_is_a_startup_error() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "log_format = \"yaml\"")?;
            let config_path = jail.directory().join("config.toml");
            let result = Settings::resolve(Some(&config_path), &Overrides::default());
            assert!(
                result.is_err(),
                "unknown log format must fail fast, got {result:?}"
            );
            Ok(())
        });
    }

    use std::net::IpAddr;

    use crate::catalog::{Catalog, TenantName};

    #[test]
    fn settings_defaults_apply_when_the_config_file_is_absent() {
        figment::Jail::expect_with(|jail| {
            let home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", home.display());
            jail.set_env("XDG_DATA_HOME", home.display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            assert_eq!(settings.rate, SustainedRate::DEFAULT);
            assert_eq!(settings.burst, Burst::DEFAULT);
            assert_eq!(settings.log_format, LogFormat::Text);
            assert_eq!(
                settings.tenants_dir,
                jail.directory().join("ferrodun").join("tenants")
            );
            assert_eq!(settings.bind, "127.0.0.1".parse::<IpAddr>().expect("ip"));
            assert_eq!(settings.base_port, 4000);
            assert_eq!(
                settings.catalog_path,
                jail.directory().join("ferrodun").join("catalog.toml")
            );
            Ok(())
        });
    }

    #[test]
    fn settings_read_the_file_and_env_overrides_it() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
rate = 5
bind = "0.0.0.0"
base_port = 5000
tenants_dir = "/srv/ferrodun/tenants"
log_format = "json"
"#,
            )?;
            jail.set_env("MUDD_BASE_PORT", "6000");
            let config_path = jail.directory().join("config.toml");
            let settings = Settings::resolve(Some(&config_path), &Overrides::default())
                .expect("settings resolve");

            assert_eq!(settings.bind, "0.0.0.0".parse::<IpAddr>().expect("ip"));
            assert_eq!(settings.base_port, 6000, "env overrides the file");
            assert_eq!(settings.tenants_dir, PathBuf::from("/srv/ferrodun/tenants"));
            assert_eq!(settings.log_format, LogFormat::Json);
            assert_eq!(
                settings.catalog_path,
                jail.directory().join("catalog.toml"),
                "the catalogue sits beside the config file"
            );
            Ok(())
        });
    }

    #[test]
    fn overrides_beat_file_and_env() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "rate = 5")?;
            jail.set_env("MUDD_RATE", "7");
            let config_path = jail.directory().join("config.toml");
            let overrides = Overrides {
                rate: Some(NonZeroU32::new(9).expect("nonzero")),
                ..Overrides::default()
            };
            let settings =
                Settings::resolve(Some(&config_path), &overrides).expect("settings resolve");

            assert_eq!(
                settings.rate,
                SustainedRate::new(NonZeroU32::new(9).expect("nonzero"))
            );
            Ok(())
        });
    }

    #[test]
    fn the_catalog_maps_to_tenant_entries() {
        figment::Jail::expect_with(|jail| {
            let home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", home.display());
            jail.set_env("XDG_DATA_HOME", home.display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            let mut catalog = Catalog::default();
            catalog
                .add(
                    TenantName::parse("alpha").expect("slug"),
                    settings.base_port,
                )
                .expect("add alpha");
            let tenants = tenants_from_catalog(&settings, &catalog).expect("mapping");

            assert_eq!(tenants.len(), 1);
            let tenant = tenants.first().expect("one tenant");
            assert_eq!(tenant.dir, settings.tenants_dir.join("alpha"));
            assert_eq!(
                tenant.listen,
                "127.0.0.1:4000".parse().expect("socket addr")
            );
            assert_eq!(tenant.tag.get(), 1);
            Ok(())
        });
    }

    #[test]
    fn an_empty_catalog_is_a_serve_error() {
        figment::Jail::expect_with(|jail| {
            let home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", home.display());
            jail.set_env("XDG_DATA_HOME", home.display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            let result = tenants_from_catalog(&settings, &Catalog::default());
            assert!(result.is_err(), "expected an error, got {result:?}");
            Ok(())
        });
    }

    use clap::Parser as _;

    #[test]
    fn bare_mudd_asks_for_a_subcommand() {
        let error = Cli::try_parse_from(["mudd"]).expect_err("bare mudd must not serve");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn config_is_a_global_flag() {
        let cli = Cli::try_parse_from(["mudd", "tenant", "list", "--config", "/etc/f.toml"])
            .expect("global --config parses after the subcommand");
        assert_eq!(cli.config, Some(PathBuf::from("/etc/f.toml")));
        assert!(matches!(cli.command, Command::Tenant(TenantCommand::List)));
    }

    #[test]
    fn serve_accepts_the_dev_flags() {
        let cli = Cli::try_parse_from([
            "mudd",
            "serve",
            "--tenant-dir",
            "/t",
            "--listen",
            "127.0.0.1:5000",
        ])
        .expect("serve flags parse");
        let Command::Serve(args) = cli.command else {
            panic!("expected the serve subcommand");
        };
        assert_eq!(args.tenant_dir, Some(PathBuf::from("/t")));
        assert_eq!(args.listen, Some("127.0.0.1:5000".parse().expect("addr")));
    }

    #[test]
    fn tenant_remove_takes_a_purge_flag() {
        let cli = Cli::try_parse_from(["mudd", "tenant", "remove", "old", "--purge"])
            .expect("remove parses");
        assert!(matches!(
            cli.command,
            Command::Tenant(TenantCommand::Remove { ref name, purge: true }) if name == "old"
        ));
    }

    #[test]
    fn tenant_dir_bypasses_the_catalogue() {
        figment::Jail::expect_with(|jail| {
            let home = jail.directory().to_path_buf();
            jail.set_env("XDG_CONFIG_HOME", home.display());
            jail.set_env("XDG_DATA_HOME", home.display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");
            let args = ServeArgs {
                tenant_dir: Some(PathBuf::from("/t")),
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };

            let tenants = serve_tenants(&settings, &args).expect("dev mode needs no catalogue");
            assert_eq!(
                tenants,
                vec![TenantEntry {
                    dir: PathBuf::from("/t"),
                    listen: DEFAULT_LISTEN,
                    tag: TenantTag::default(),
                }]
            );
            Ok(())
        });
    }
}
