# Ferrodun

**Ferrodun is a pure-Rust MUD/MU\* engine.** A `mudd` process serves one or
more tenants over telnet, each tenant an isolated stack with its own
database and its own hand-authored world loaded from plain-text files.

## Current key features

- **A telnet server** — `mudd` accepts telnet connections and serves one or
  more tenants, each with its own isolated database and in-memory world.
- **The built-in player command set** — login/registration, puppet
  selection, and in-world movement and interaction.
- **KDL-authored worlds** — rooms, exits, regions, and a color palette are
  written as plain KDL files and loaded with strict, specific error
  reporting. The palette is currently an authoring-time feature: room text
  can carry markup resolved against it at world-load time, but the server
  sends plain text to players today.
- **Per-tenant multi-tenancy** — each tenant is a fully isolated stack (its
  own database, its own in-memory world, its own listener); there is no
  shared state between tenants.
- **English message rendering** — all engine-emitted player-facing text
  resolves through a single, translatable message-key seam, currently
  populated with English only.

## Vision

Ferrodun's ambition goes beyond the current feature set: a type-driven
engine where illegal states are unrepresentable, worlds scriptable by
non-programmers through plain-text authoring formats, first-class
multi-tenancy, and a broad client matrix beyond raw telnet. For the full
roadmap, see [`SPEC.md`](SPEC.md) (the specification) and
[`PLAN.md`](PLAN.md) (the phased implementation plan).

## Quickstart

```
mudd --tenant-dir /path/to/tenant
```

This serves a tenant directory over telnet on `127.0.0.1:4000`. For the full
walkthrough, see the [docs site](https://k3rn31.github.io/ferrodun/).

## Documentation

Full user- and builder-facing documentation is published at
[k3rn31.github.io/ferrodun](https://k3rn31.github.io/ferrodun/). To build and
serve it locally:

```
cd docs && uv run mkdocs serve
```

## License

Ferrodun is licensed under the [BSD 3-Clause License](LICENSE).
