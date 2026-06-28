# Regions (`region.kdl`)

A **region** is a named group of places — a town, a keep, a stretch of
wilderness. Every room belongs to exactly one region, and **regions are
mandatory**: you declare one by dropping a `region.kdl` file at the root of a
subfolder of `world/`, and every room anywhere under that subfolder belongs to
it.

```kdl
// world/keep/region.kdl
region "old_keep" {
    name "The Old Keep"   // optional display name
}
```

With this file in place, `world/keep/cellar.kdl` — and any other [room](rooms.md)
under `world/keep/` — belongs to the `old_keep` region. Even a single-region game
needs one region folder; group your rooms under it (`world/town/`, etc.).

A few rules keep regions predictable:

- **Every room must live in a region.** A room not covered by any `region.kdl`
  is rejected at load — there is no implicit fallback region. In particular, you
  cannot place room files directly at the root of `world/`; put them inside a
  region subfolder.
- **The slug is the identity, not the folder name.** A region is named by the
  slug inside its `region.kdl` (`old_keep`), never by the folder it lives in. You
  can rename or move the folder freely without changing the region.
- **One region per folder subtree.** All the rooms under a region's folder share
  that region. Regions cannot nest: putting a `region.kdl` inside another
  region's folder is rejected.
- **Slugs are unique.** Two regions cannot share a slug.
- **The `world/` root is reserved.** A `region.kdl` placed directly at the root
  of `world/` is rejected; that slot is held for future world-wide region
  defaults.

!!! note "Who may edit a region"

    Ferrodun does not manage builder permissions itself. Because each region is a
    self-contained folder, "who may edit this region" is simply a question of who
    can write to that directory — handle it with your filesystem or version
    control (for example, a `CODEOWNERS` entry per region folder).
