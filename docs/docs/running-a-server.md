# Running a server

This page is for operators: how to start `mudd`, configure one or more
tenants, and keep the process running in production.

!!! note
    Linux is the only supported deployment target for now.

## Quick start

The fastest way to get a single world online is to point `mudd` at a tenant
directory:

```
mudd --tenant-dir /path/to/tenant
```

This serves that tenant over telnet on `127.0.0.1:4000`. The tenant directory
holds the world's `config.toml`, its `world/` room files, and its welcome
banner — see [World files](building/world-files.md).

## Server configuration

`mudd` reads a server-wide configuration file from
`$XDG_CONFIG_HOME/ferrodun/config.toml` (by default
`~/.config/ferrodun/config.toml`). Override the location with `--config`. Every
key is optional:

```toml
rate = 10        # per-session sustained commands/second
burst = 20       # per-session burst allowance

[[tenants]]      # the tenant registry: one block per tenant
dir = "/srv/ferrodun/tenants/midgard"
listen = "127.0.0.1:4000"

[[tenants]]
dir = "/srv/ferrodun/tenants/asgard"
listen = "127.0.0.1:4001"
```

Each tenant is an isolated stack — its own database, its own world, its own
listener. Listen addresses must be distinct, and each tenant's `config.toml`
must carry a `tenant_tag` that is unique across the registry; `mudd` refuses to
start otherwise.

Configuration is layered, weakest first:

1. built-in defaults,
2. `config.toml`,
3. `MUDD_*` environment variables (e.g. `MUDD_RATE=5`),
4. command-line flags.

`--tenant-dir` replaces the whole registry with a single tenant, listening on
`--listen` (default `127.0.0.1:4000`).

## Per-tenant configuration

Inside a tenant directory, `config.toml` describes that one world:

| Key | Required | Default | Meaning |
|---|---|---|---|
| `start_room` | yes | — | Slug of the room new characters begin in. |
| `tenant_tag` | no | `0` | Identity of this tenant, unique across the registry. |
| `locale` | no | `"en"` | Language engine messages render in. |
| `banner` | no | `welcome.kdl` | Welcome-banner file, relative to the tenant directory. |
| `palette` | no | `palette.kdl` | Color palette file, relative to the tenant directory. |

## Running under a supervisor

`mudd` is **fail-stop** by design: on an unrecoverable error — a database write
failure or loss of its internal IPC channel — it stops serving immediately and
exits non-zero rather than continuing in a possibly-inconsistent state. All
durable state lives in the database, and the in-memory world is rebuilt from it
at boot, so the recovery mechanism is simply *restart the process*. Run `mudd`
under a supervisor that does this for you.

A minimal systemd unit:

```ini
# /etc/systemd/system/ferrodun.service
[Unit]
Description=Ferrodun MUD server
After=network.target

[Service]
ExecStart=/usr/local/bin/mudd
Restart=on-failure
RestartSec=2
# A persistent fault (e.g. a full disk) must not crash-loop forever:
StartLimitIntervalSec=60
StartLimitBurst=5

[Install]
WantedBy=multi-user.target
```

Under a container runtime, use an equivalent restart policy — `restart:
on-failure` in Compose, or the platform's on-failure restart setting.

## Logging

`mudd` writes structured logs to stdout via `tracing`. Two environment
variables control the stream:

| Variable | Values | Default | Effect |
|---|---|---|---|
| `RUST_LOG` | any `tracing` filter (`info`, `debug`, `mud_db=debug`, …) | `info` | Log level filter |
| `FERRODUN_LOG_FORMAT` | `text`, `json` | `text` | Human-readable text or JSON for log aggregators |

What the levels mean:

- **error** — the operator must act: a database or IPC failure, a fatal
  tenant task exit.
- **warn** — world *content* is broken but degraded gracefully: a missing
  i18n key, a bad markup tag, an unknown style role. Fix the content.
- **info** — boot/shutdown heartbeat only. A healthy server is nearly
  silent at `info`: a few lines per tenant at startup, then nothing.
- **debug** — per-session diagnostics: connections, logins, command
  dispatch, IPC handshakes.
- **trace** — the per-tick firehose.

Every line is tagged with the owning `tenant` and, where applicable,
`session_id`, so one tenant's traffic can be filtered out of a
multi-tenant process.
