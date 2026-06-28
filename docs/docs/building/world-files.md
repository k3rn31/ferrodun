# World files

A Ferrodun game lives in a **tenant folder**: a directory holding the tenant's
configuration, its rooms, and its welcome banner. Builders author all of this as
plain text — [KDL](https://kdl.dev) for structure and a small TOML config.

```
my-game/
  config.toml      # tenant configuration
  welcome.kdl      # the pre-login welcome banner
  world/           # rooms, scanned recursively
    town.kdl
    keep/
      region.kdl   # declares the region the keep's rooms belong to
      cellar.kdl
```

## Tenant configuration (`config.toml`)

```toml
start_room = "town_square"  # required: the slug of the room new players start in
banner     = "welcome.kdl"  # optional: banner file, relative to the tenant folder
```

Any value may be overridden by an environment variable prefixed with
`FERRODUN_`. For example, `FERRODUN_START_ROOM=secret_lair` overrides
`start_room` without editing the file.

## Rooms (`world/*.kdl`)

Every room lives under `world/`. The folder is scanned **recursively**, so you can
split rooms across as many files and subfolders as you like — organize them
however suits your game.

```kdl
room "town_square" {
    title "Town Square"
    description "A bustling cobbled square ringed by shuttered stalls."
    exit "north" "market"
    exit "down" "cellar"
}
```

Each `room` node takes:

| Field | Required | Meaning |
|---|---|---|
| slug (the node argument, e.g. `"town_square"`) | yes | The room's durable name. Other rooms, exits, and `start_room` refer to it. Must be a slug: lowercase letters, digits, `_`, and `-`. |
| `title` | no | A short display name, distinct from the description. |
| `description` | yes | The text a player sees when looking at the room. |
| `exit "<direction>" "<slug>"` | any number | A way out, leading to another room's slug. Directions: `north`, `east`, `south`, `west`, `up`, `down`. |

Exits may point to rooms defined in any file — slugs are resolved across the whole
`world/` folder after every file is read.

### Naming rooms

A room's slug is its permanent name. Choose something short and descriptive
(`town_square`, `keep_cellar`) and treat it as fixed once your game is live:
players' saved locations are remembered by slug, so renaming a slug effectively
moves a room out from under anyone standing in it. (The game refuses to start
rather than silently relocating them.)

The `title` is just the display name and is safe to change any time — edit it
freely without affecting saved locations.

!!! tip "Pick a naming convention early"

    Slugs share one flat namespace across your whole game, so a consistent
    pattern keeps them unique and easy to scan once you have hundreds of rooms.
    A common choice is **`<region>_<room>`** — prefix every slug with its region:

    ```kdl
    room "harbor_docks"     { description "..." }
    room "harbor_warehouse" { description "..." }
    room "keep_gate"        { description "..." }
    room "keep_cellar"      { description "..." }
    ```

    This groups a region's rooms together alphabetically, makes exits read clearly
    (`exit "north" "keep_gate"`), and lets you mirror the pattern in your folder
    layout (`world/harbor/`, `world/keep/`). Pick whatever scheme suits you, then
    apply it everywhere.

## Regions (`region.kdl`)

A **region** is a named group of places — a town, a keep, a stretch of
wilderness. Every room belongs to exactly one region. You declare a region by
dropping a `region.kdl` file at the root of a folder: every room anywhere under
that folder then belongs to it.

```kdl
// world/keep/region.kdl
region "old_keep" {
    name "The Old Keep"   // optional display name
}
```

With this file in place, `world/keep/cellar.kdl` — and any other room under
`world/keep/` — belongs to the `old_keep` region. Rooms that sit under no
`region.kdl` (like `world/town.kdl` above) belong to an implicit **default**
region, so you only author regions where you want them.

A few rules keep regions predictable:

- **The slug is the identity, not the folder name.** A region is named by the
  slug inside its `region.kdl` (`old_keep`), never by the folder it lives in. You
  can rename or move the folder freely without changing the region.
- **One region per folder subtree.** All the rooms under a region's folder share
  that region. Regions cannot nest: putting a `region.kdl` inside another
  region's folder is rejected.
- **Slugs are unique.** Two regions cannot share a slug, and `default` is
  reserved for the implicit region.

!!! note "Who may edit a region"

    Ferrodun does not manage builder permissions itself. Because each region is a
    self-contained folder, "who may edit this region" is simply a question of who
    can write to that directory — handle it with your filesystem or version
    control (for example, a `CODEOWNERS` entry per region folder).

## Welcome banner (`welcome.kdl`)

The banner is shown to a connection before login. It is a single KDL node with one
string:

```kdl
banner "Welcome to Ferrodun.\nType `register` or `login`; `help` for commands."
```

## Errors

Loading is strict: a malformed KDL file, a duplicate slug, an exit to an unknown
room, an unknown direction, an invalid slug, a room without a description, an
unrecognized node or field (such as a misspelled `descriptipn`), a `banner` path
that points outside the tenant folder, or a `start_room` that names no room all
stop the load with a clear, specific error naming the offending file or field.

The same applies to regions: a duplicate region slug, a region authored as the
reserved `default` slug, a nested `region.kdl`, or a manifest that does not
declare exactly one region each stop the load with a specific error.
