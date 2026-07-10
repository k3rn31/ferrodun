# Configuration

The full key reference for `mudd`'s server-wide config file and each
tenant's `config.toml`.

## Server-wide configuration

`mudd` reads a server-wide configuration file from
`$XDG_CONFIG_HOME/ferrodun/config.toml` (by default
`~/.config/ferrodun/config.toml`). Override the location with `--config`. Every
key is optional:

```toml
rate = 10          # per-session sustained commands/second
burst = 20         # per-session burst allowance
log_format = "text" # log wire format: "text" (default) or "json"

[[tenants]]      # the tenant registry: one block per tenant
dir = "/srv/ferrodun/tenants/midgard"
listen = "127.0.0.1:4000"

[[tenants]]
dir = "/srv/ferrodun/tenants/asgard"
listen = "127.0.0.1:4001"
```

| Key | Default | Meaning |
|---|---|---|
| `rate` | `10` | Per-session sustained commands/second. |
| `burst` | `20` | Per-session burst allowance. |
| `log_format` | `text` | Log wire format: `text` or `json`. Also settable via `--log-format` or `MUDD_LOG_FORMAT`. |
| `[[tenants]]` | — | The tenant registry: one block per tenant, each with `dir` and `listen`. |

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

Inside a tenant directory, `config.toml` describes that one world. There is
no environment-variable override for these keys — the file is the sole
source of a tenant's configuration.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `start_room` | yes | — | Slug of the room new characters begin in. |
| `tenant_tag` | no | `0` | Identity of this tenant, unique across the registry. |
| `locale` | no | `"en"` | Language engine messages render in. See [Localization](../building/localization.md). |
| `banner` | no | `welcome.kdl` | Welcome-banner file, relative to the tenant directory. |
| `palette` | no | `palette.kdl` | Color palette file, relative to the tenant directory. |
