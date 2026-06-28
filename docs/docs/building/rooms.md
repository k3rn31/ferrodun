# Rooms (`world/*.kdl`)

Every room lives under `world/`. The folder is scanned **recursively**, so you can
split rooms across as many files and subfolders as you like — organize them
however suits your game. Each room must sit inside a [region folder](regions.md);
there are no room files directly at the root of `world/`.

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

## Naming rooms

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
