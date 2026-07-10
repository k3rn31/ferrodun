# Logging

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
