# Configuration

The full key reference for `mudd`'s server-wide config file, the tenant
catalogue, and each tenant's `config.toml`.

## Server-wide configuration

`mudd` reads a server-wide configuration file from
`$XDG_CONFIG_HOME/ferrodun/config.toml` (by default
`~/.config/ferrodun/config.toml`). Override the location with the global
`--config` flag. Every key is optional:

```toml
rate = 10           # per-session sustained commands/second
burst = 20          # per-session burst allowance
log_format = "text" # log wire format: "text" (default) or "json"
bind = "127.0.0.1"  # host address every tenant listener binds to
base_port = 4000    # lowest port the catalogue may assign
tenants_dir = "/srv/ferrodun/tenants" # root holding one folder per tenant
```

| Key | Default | Meaning |
|---|---|---|
| `rate` | `10` | Per-session sustained commands/second. |
| `burst` | `20` | Per-session burst allowance. |
| `log_format` | `text` | Log wire format: `text` or `json`. Also settable via `--log-format` or `MUDD_LOG_FORMAT`. |
| `bind` | `127.0.0.1` | Host address every tenant listener binds to. Set `0.0.0.0` to expose publicly. |
| `base_port` | `4000` | Lowest port `mudd tenant add` may assign. |
| `tenants_dir` | `$XDG_DATA_HOME/ferrodun/tenants` | Root directory holding one folder per tenant, named after it. |

Configuration is layered, weakest first:

1. built-in defaults,
2. `config.toml`,
3. `MUDD_*` environment variables (e.g. `MUDD_RATE=5`),
4. command-line flags.

## The tenant catalogue

The tenant registry lives in `catalog.toml`, a sibling of the server config
file. It is **machine-managed**: `mudd tenant add` and `mudd tenant remove`
are its writers, and there are no environment or flag overrides for its
contents. Each entry records the tenant's name and its assigned values:

```toml
[[tenants]]
name = "midgard"
port = 4000
tag = 1
```

- The tenant's directory is always `<tenants_dir>/<name>` — no path is
  stored.
- `port` is assigned by `mudd tenant add`: the lowest free port at or above
  `base_port`. Ports freed by `mudd tenant remove` are reused.
- `tag` is the runtime tenant tag stamped into the tenant's entity ids
  (12-bit, `1..=4095`; `0` is reserved for `--tenant-dir` dev mode).
  Assigned lowest-free, reused after removal.

Hand-edits are validated when the file loads: names, ports, and tags must
be unique, and tags must be in range. See
[Running a server](running-a-server.md) for the `mudd tenant` commands.

## Per-tenant configuration

Inside a tenant directory, `config.toml` describes that one world. It is
builder content only — no ports, no tags. There is no environment-variable
override for these keys — the file is the sole source of a tenant's
configuration.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `start_room` | yes | — | Slug of the room new characters begin in. |
| `locale` | no | `"en"` | Language engine messages render in. See [Localization](../building/localization.md). |
| `banner` | no | `welcome.kdl` | Welcome-banner file, relative to the tenant directory. |
| `palette` | no | `palette.kdl` | Color palette file, relative to the tenant directory. |
