# Color & styling (`palette.kdl`)

Ferrodun treats color as a **render-time concern**. Builders never write raw
terminal escape codes; you author *meaning* — a semantic role, or a named color —
and the engine compiles it to whatever each player's client can display. This
page covers the two things a builder controls: the **palette** and the **markup**
you put in room fields.

## The palette

A **palette** maps semantic roles (like `error` or `say`) and named colors (like
`cyan`) to concrete 24-bit colors and attributes. The engine ships a built-in
**baseline** palette, so you do not need a palette file at all. To restyle the
game, drop a `palette.kdl` in your tenant folder; everything you declare there is
layered on top of the baseline.

```kdl
// palette.kdl

// Named colors: reusable, referenced by builder markup and roles.
color "flame" "#ff7733"
color "moss"  "#5f8b4c"

// Roles: the engine's own output categories. Override a baseline role to
// restyle every message that uses it, without touching any content.
role "error"  fg="#ff5555"
role "alert"  fg="#ffffff" bg="#aa0000" bold=#true
role "say"    fg="moss"
```

- **Colors** are authored as `#rrggbb` (24-bit). A named color may be referenced
  by markup (`{fg=flame}`) and by roles.
- **Roles** take `fg=…` / `bg=…` (a `#rrggbb` literal *or* the name of a color)
  and the attribute flags `bold` / `italic` / `underline` / `blink` / `reverse`,
  written with KDL 2.0 keyword booleans: `bold=#true`.
- Anything you declare **overrides or extends** the baseline; anything you leave
  out keeps its baseline value.

The baseline defines, at minimum, the roles `error`, `system`, `alert`,
`prompt`, `say`, `emote`, and `tell`, plus the sixteen standard named colors
(`black`, `red`, …, `bright_white`). Overriding a role restyles all of the
engine's output in that category — for example, repaint every `say` line by
overriding the `say` role rather than editing any command.

## Styling room fields

Room `title` and `description` fields accept a compact markup. What you may use
depends on the field — **the engine decides per field**:

| Field         | Default       | Inline markup allowed                          |
|---------------|---------------|------------------------------------------------|
| `title`       | **bold**      | none — the title is simply bold                |
| `description` | none          | palette colors + `bold` / `italic` / `underline` |

```kdl
room "shrine" {
    title "Sunken Shrine"
    description "Water laps at a {fg=flame}glowing rune{/}. The air is {b}cold{/}."
}
```

The markup tags:

- `{fg=<name>}` … `{/}` — foreground color. `<name>` must be a **palette color
  name** (raw `#hex` is not accepted in field markup, so every color stays
  palette-curated and restyleable).
- `{bg=<name>}` … `{/}` — background color.
- `{b}` / `{i}` / `{u}` … `{/}` — bold / italic / underline.
- `{/}` closes the nearest open tag; tags may nest.
- A literal brace is written `{{` or `}}`.

If you use a tag a field does not allow, or name a color the palette does not
define, the engine **does not fail to load** — it keeps your text, drops the
unknown styling, and logs a warning so you can spot the typo. Malformed markup
(an unterminated tag) is likewise kept as literal text with a warning.

!!! note "Builder text is trusted"

    Markup in builder-authored files is compiled normally. Markup typed by
    *players* is a separate, locked-down path (it is escaped by default), so
    players cannot inject styling into other players' output.

## How color reaches each player

You author colors once, in 24-bit truecolor. The engine downsamples to whatever
each player's client can show, **once per session at the output edge** — you
never think about a player's terminal while building.

A session renders at one of four **tiers**: `mono`, `ansi16`, `xterm256`, or
`truecolor`. The tier is resolved like this (first match wins):

1. The player's saved color preference *(a later feature)*.
2. `NO_COLOR` set in the player's environment → **`mono`** (no color, attributes
   like bold/underline preserved). See [no-color.org](https://no-color.org/).
3. What the client advertises it can do *(a later feature)*.
4. The **tenant default**, which is `ansi16` for the widest compatibility.

Downsampling is deterministic: the same authored color always becomes the same
ansi16 / xterm256 code, so output is reproducible. You can author freely in
truecolor knowing it degrades cleanly on a 16-color client.
