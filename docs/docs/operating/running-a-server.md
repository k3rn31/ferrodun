# Running a server

This page is for operators: how to start `mudd`, configure one or more
tenants, and keep the process running in production.

!!! note
    Linux is the only supported deployment target for now.

## Quick start

Register a tenant, then serve:

```
mudd tenant add mygame
mudd serve
```

`tenant add` scaffolds a minimal bootable world under the tenants directory
(see [Configuration](configuration.md)) and assigns the tenant a port
(starting at 4000) — the moment it finishes, `mudd serve` boots the tenant
and a player can connect. Manage the registry with `mudd tenant list` and
`mudd tenant remove <name>` (add `--purge` to also delete the folder;
without it, the folder and its database stay on disk).

For a one-off world in a specific folder, bypass the catalogue:

```
mudd serve --tenant-dir /path/to/tenant
```

This serves that tenant over telnet on `127.0.0.1:4000`. The tenant directory
holds the world's `config.toml`, its `world/` room files, and its welcome
banner — see [World files](../building/world-files.md).

Running `mudd` with no subcommand prints the available commands.

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
ExecStart=/usr/local/bin/mudd serve
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

See [Logging](logging.md) for levels and log format.
