# Tenant catalogue and `mudd` subcommand CLI — design

**Date:** 2026-07-11
**Status:** approved

## Problem

Two operator/builder concerns are misplaced today:

1. **`tenant_tag` lives in the builder's tenant `config.toml`**
   (`crates/mud-world/src/config.rs`), defaulting to `0`, while
   `crates/mudd/src/boot.rs` enforces registry-wide uniqueness. Two tenants
   that both omit the field collide on `0` and the server refuses to start —
   a confusing failure over a value the builder never knew they had to set.
   PLAN M1-12 explicitly said tenant `config.toml` holds no `tenant_tag`
   (a runtime concern); the current field is drift from the plan. The tag is
   runtime-only: `PersistentWorld::load` re-mints every `EntityId` from
   durable `entity_key`s at boot, so the tag never touches persisted data and
   only needs uniqueness within one running process.
2. **Tenant listen addresses are hand-authored** in the server config's
   `[[tenants]]` array. Choosing ports is an operator chore and a source of
   duplicate-address startup errors.

Separately, `mudd` is a flat-flag binary: running it bare starts serving,
and there is no way to manage tenants except hand-editing config files.

## Decision summary

- Remove `tenant_tag` from tenant `config.toml` entirely. Tags are
  **assigned** by the tenant catalogue at `mudd tenant add` time.
- Introduce a machine-owned **tenant catalogue** file, sibling of the
  server config, that records each tenant's name, assigned port, and
  assigned tag. The `[[tenants]]` array in the server config goes away.
- `mudd` becomes a subcommand CLI: `mudd serve`, `mudd tenant add`,
  `mudd tenant remove`, `mudd tenant list`. Bare `mudd` prints help.
- No new crates: the catalogue is a module inside `mudd`. If the CLI surface
  grows enough that `mudd` accumulates unrelated concerns, revisit splitting
  subcommand-specific crates from common ones then — not now (YAGNI).

## CLI surface

`mudd` uses clap subcommands with `arg_required_else_help`:

| Invocation | Behavior |
|---|---|
| `mudd` | Print help listing subcommands. **Breaking change:** bare `mudd` no longer serves (acceptable pre-1.0). |
| `mudd serve` | Today's serving behavior. The existing flags move under it: `--tenant-dir`, `--listen`, `--rate`, `--burst`, `--log-format` (`--config` becomes global, see below). Precedence unchanged: defaults < config.toml < `MUDD_` env < flags. |
| `mudd tenant add <name>` | Register a tenant: assign port and tag, scaffold the folder, save the catalogue. |
| `mudd tenant remove <name> [--purge]` | Deregister a tenant; `--purge` also deletes the folder after confirmation. |
| `mudd tenant list` | Print the catalogue: name, port, tag, dir. |

`--config` is a **global flag** on `mudd` itself (clap `global = true`):
it applies to `serve` and every `tenant` subcommand alike, since all of
them need the server config (and through it, `tenants_dir` and the
catalogue location).

`mudd serve --tenant-dir <dir>` keeps bypassing the registry for dev use:
one tenant, tag `0`, listening on `--listen` or the default
`127.0.0.1:4000`. Tag `0` is reserved for this mode (the catalogue assigns
from `1`), so dev-mode and catalogue tenants are visually distinct in logs.

## Server config changes

`~/.config/ferrodun/config.toml` (figment: defaults < TOML < `MUDD_` env <
flags) changes as follows:

| Key | Change | Default |
|---|---|---|
| `tenants_dir` | **new** — root under which tenant folders live | `$XDG_DATA_HOME/ferrodun/tenants`, falling back to `~/.local/share/ferrodun/tenants` |
| `bind` | **new** — host address for every tenant listener | `127.0.0.1` (operators set `0.0.0.0` to expose publicly) |
| `base_port` | **new** — lowest port the catalogue may assign | `4000` |
| `[[tenants]]` | **removed** — replaced by the catalogue | — |

`rate`, `burst`, and `log_format` are unchanged.

## The catalogue

`catalog.toml`, sibling of the server config (`$XDG_CONFIG_HOME/ferrodun/
catalog.toml`, same `~/.config` fallback; when `--config` points elsewhere,
the catalogue sits next to that file). All operator-facing files live in
one directory, and backing up the config directory captures the full
serving topology. The file is machine-owned: no env or flag overrides, and
saves serialize the whole file (no comment preservation needed — humans are
not expected to edit it, though hand-edits are validated on load).

```toml
[[tenants]]
name = "mygame"
port = 4000
tag = 1
```

- A tenant's directory is always derived as `<tenants_dir>/<name>` — no
  path is stored, so the catalogue stays valid even when the tenants tree
  moves (only the `tenants_dir` config key needs updating).
- A missing `catalog.toml` is an empty catalogue. `mudd serve` with an empty
  catalogue (and no `--tenant-dir`) fails with: *"no tenants: run
  `mudd tenant add <name>`"*.

### Assignment rules

- **Port:** lowest free port ≥ `base_port`. Freed ports (after `remove`)
  are reused.
- **Tag:** lowest free tag ≥ `1` (`0` is the dev-mode tag). Freed tags are
  reused. Tags are bounded by `TenantTag::MAX` (4095); assignment fails if
  the space is exhausted.

Because the catalogue assigns both values, the duplicate-`tenant_tag` and
duplicate-listen-address boot errors disappear as user-facing failures.

### Load-time validation

Guards against hand-edits; each violation is an error naming the file and
the offending entry:

- names are valid slugs and unique
- ports are unique
- tags are unique, ≥ 1, and ≤ `TenantTag::MAX`

## Tenant name

A `TenantName` newtype (in `mudd`): lowercase ASCII alphanumeric plus `-`
and `_`, must start with an alphanumeric. Parsed at the CLI boundary; the
inner code never re-validates. It doubles as the folder name.

## Subcommand behavior

### `tenant add <name>`

1. Parse `<name>` into `TenantName`; reject if already in the catalogue.
2. If `<tenants_dir>/<name>` does not exist: create it and scaffold a
   **minimal bootable world** —
   - `config.toml` with `start_room = "start"`
   - `world/start.kdl` containing one starter room
   - `welcome.kdl` banner
   The moment `add` finishes, `mudd serve` boots the tenant and a player
   can connect.
3. If the folder exists **and contains a `config.toml`**: register it as-is
   (the re-add-after-remove path). Existing files are never overwritten.
   A folder that exists without a `config.toml` is an error.
4. Assign port and tag, append to the catalogue, save, and print the
   assignment (name, port, tag, dir).

### `tenant remove <name> [--purge]`

- Drop the entry from the catalogue; its port and tag become reusable.
  The folder is left on disk — the engine never deletes player data by
  default.
- `--purge` additionally deletes the folder, after an interactive
  confirmation that requires re-typing the tenant name.
- An unknown name is an error.

### `tenant list`

One line per tenant: name, port, tag, derived dir. Output goes through
`writeln!` to a locked stdout handle (the workspace denies
`print_stdout`); write failures propagate as errors.

## Internal changes

- **`mud-world`:** `TenantConfig` loses the `tenant_tag` field, its
  accessor, and the `TenantTagOutOfRange` error variant (plus their tests).
  Tenant `config.toml` is purely builder content again, as PLAN M1-12
  specified.
- **`mudd`:** new `catalog` module owning the catalogue: typed load /
  validate / mutate / save, plus port and tag assignment. `TenantEntry`
  becomes `{ dir, listen, tag }`; `boot()` takes the tag from the entry
  (passing it to `PersistentWorld::load` and the tenant span) and drops its
  duplicate-tag `HashSet` check. `main.rs` dispatches subcommands; `serve`
  resolves `ServerConfig`, then loads the catalogue and builds the tenant
  list (`listen` = `bind` + assigned port).
- **SPEC:** no conflict. §4/§5 do not mandate `tenant_tag` in tenant
  config; §3.11.3 ("each game owns its port set") is satisfied by the
  catalogue.

## Error handling

- CLI paths use `anyhow` with `.context()` naming the operation and path.
- Catalogue validation errors name `catalog.toml` and the offending entry.
- Every filesystem create/delete reports the path it acted on.
- `--purge` confirmation mismatch aborts without touching disk.

## Testing (TDD)

- **Catalog unit tests:** lowest-free port and tag assignment; reuse after
  remove; duplicate name/port/tag rejection on load; tag range validation;
  slug rejection; empty-catalogue behavior.
- **Scaffold test:** a freshly added tenant's folder loads cleanly through
  the existing `TenantConfig::load` + `load_world` path.
- **Boot test:** a catalogue-built registry boots on ephemeral ports
  (existing integration seam stays green).
- **Config tests:** `tenants_dir` / `bind` / `base_port` resolution and
  precedence; removal of `[[tenants]]` handling.

## Documentation impact (same PR)

- **Operating pages:** rewrite server-config and invocation docs around
  `mudd serve` + `mudd tenant`; document `tenants_dir`, `bind`,
  `base_port`, and the catalogue file.
- **Building pages:** drop any mention of `tenant_tag` in tenant
  `config.toml`.
- **PLAN.md:** add a PR entry recording this change, noting that M1-12's
  "config.toml carries only content fields" guidance is now honored.
- **Journal:** entry per CLAUDE.md after implementation.
