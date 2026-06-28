# World files

A Ferrodun game lives in a **tenant folder**: a directory holding the tenant's
configuration, its rooms, and its welcome banner. Builders author all of this as
plain text — [KDL](https://kdl.dev) for structure and a small TOML config.

```
my-game/
  config.toml        # tenant configuration
  welcome.kdl        # the pre-login welcome banner
  world/             # rooms, scanned recursively
    town/
      region.kdl     # declares the region the town's rooms belong to
      town.kdl
    keep/
      region.kdl     # declares the region the keep's rooms belong to
      cellar.kdl
```

The two building blocks you author live on their own pages:

- **[Rooms](rooms.md)** — the places players move through, written as `world/*.kdl`.
- **[Regions](regions.md)** — the named groups every room belongs to, declared by
  a `region.kdl`. Every room lives inside a **region folder**; there are no room
  files directly at the root of `world/`.

## Tenant configuration (`config.toml`)

```toml
start_room = "town_square"  # required: the slug of the room new players start in
banner     = "welcome.kdl"  # optional: banner file, relative to the tenant folder
```

Any value may be overridden by an environment variable prefixed with
`FERRODUN_`. For example, `FERRODUN_START_ROOM=secret_lair` overrides
`start_room` without editing the file.

## Welcome banner (`welcome.kdl`)

The banner is shown to a connection before login. It is a single KDL node with one
string:

```kdl
banner "Welcome to Ferrodun.\nType `register` or `login`; `help` for commands."
```

## Errors

Loading is strict, so mistakes surface at load with a clear, specific error
naming the offending file or field rather than failing silently later.

For **rooms**: a malformed KDL file, a duplicate slug, an exit to an unknown room,
an unknown direction, an invalid slug, a room without a description, an
unrecognized node or field (such as a misspelled `descriptipn`), a `banner` path
that points outside the tenant folder, or a `start_room` that names no room all
stop the load.

For **regions**: a room covered by no `region.kdl`, a `region.kdl` at the `world/`
root, a duplicate region slug, a nested `region.kdl`, or a manifest that does not
declare exactly one region each stop the load.
