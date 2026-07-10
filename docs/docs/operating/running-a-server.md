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
banner — see [World files](../building/world-files.md).

See [Configuration](configuration.md) for every setting.

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

`mudd` writes structured logs to stdout via `tracing`. Two knobs control the
stream:

| Setting | Values | Default | Effect |
|---|---|---|---|
| `RUST_LOG` (env) | any `tracing` filter (`info`, `debug`, `mud_db=debug`, …) | `info` | Log level filter |
| log format (`--log-format` / `MUDD_LOG_FORMAT` / `log_format`) | `text`, `json` | `text` | Human-readable text or JSON for log aggregators |

`RUST_LOG` is the standard `tracing` filter and is read directly from the
environment. The **log format** is a server-wide configuration value like
`rate` and `burst`, so it follows the same precedence — lowest to highest:
built-in default, `log_format` in `config.toml`, the `MUDD_LOG_FORMAT`
environment variable, then the `--log-format` command-line flag. For example,
`--log-format json` or `MUDD_LOG_FORMAT=json` switches the stream to
line-delimited JSON for a log aggregator.

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
