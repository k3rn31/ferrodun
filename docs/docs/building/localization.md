# Localization

Ferrodun renders every engine message through a translation seam, so a
tenant can declare which language its messages should render in. Today
**only English (`en`) ships** — there is no supported way to add another
language yet. The seam is in place; the message sets are not.

## The `locale` key

A tenant's `config.toml` may set the rendering locale:

```toml
locale = "en" # optional; default "en"
```

The value flows into every engine-emitted line. See [Running a server →
Per-tenant configuration](../operating/running-a-server.md#per-tenant-configuration)
for where this key sits alongside the tenant's other settings.

## What happens with a non-English locale

Message lookup falls back in this order: the requested `(locale, key)`,
then `(en, key)`, then the literal key text. Because only `en` message
templates exist, setting `locale = "fr"` (or any other non-`en` value)
resolves to **English** text via the fallback, silently — an `en` hit is
not a miss. The server logs a one-time warning only for a key absent from
every catalog, `en` included; no such key exists among the built-in
commands today. Nothing breaks — players simply see English.
