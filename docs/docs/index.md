# Ferrodun

**Ferrodun is a pure-Rust MUD/MU\* engine.**

It aims to be a modern foundation for text-based multiplayer worlds: type-safe
at its core, scriptable by non-programmers, multi-tenant from day one, and
ready for everything from classic telnet clients to a browser SPA.

!!! warning "Under construction"

    Ferrodun is in early development. This site is a placeholder and will grow
    alongside the engine. Documentation is versioned: **`next`** tracks the
    `main` branch, and each release is snapshotted to its own version using the
    selector in the header.

## What it will offer

- **Type-driven engine core** — entities, components, archetypes, places, and a
  fixed-tick scheduler, with illegal states made unrepresentable.
- **Builders without Rust** — sandboxed Lua 5.4 for archetypes, components,
  commands, and prototypes, all hot-reloadable with no restart.
- **A broad client matrix** — telnet with MCCP2/GMCP/MSDP/MXP/MSSP, SSH, TLS,
  and a WebSocket-based web client.
- **Living worlds** — wilderness tiles and ships, behavior-tree NPCs, combat and
  economy primitives, and optional LLM-driven dialogue.
- **Production-ready operations** — multi-tenancy, graceful upgrades, backups,
  and an admin dashboard.

