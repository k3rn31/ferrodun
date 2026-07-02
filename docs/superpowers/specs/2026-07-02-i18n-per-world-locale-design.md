# i18n per-world locale rework: Design

**Date:** 2026-07-02
**Spec:** §3.14 (primary), §2.8.3.3, §3.20.6.1; PLAN §M2-I, §M2 acceptance, §M3-B
**Status:** Draft — awaiting review
**Sequenced:** immediately after M1-19a (the broadcast PR that motivates it)

## Goal

Make locale a **per-tenant (per-world)** property instead of per-session, in the
spec, the plan, and the as-built plumbing. One configured locale per game, `en`
as default and reference. No per-session resolution, no mid-session switching, no
per-recipient rendering.

## Why

Two independent reasons, one architectural and one product:

1. **Broadcast correctness (architectural).** M1-19a renders one `StyledText`
   once and fans it to every recipient in a room. That is only correct if all
   recipients share a locale — locale determines the *words* and is fixed before
   fan-out. A per-recipient locale would force N renders and make the pre-rendered
   broadcast wrong. Per-world locale makes the M1-19a broadcast contract correct
   by construction.
2. **Content coherence (product, the deciding reason).** Engine-emitted prose
   must match the builder-authored prose it wraps. A French UI around English
   room descriptions is broken. The builder owns the world's content language, so
   the engine's locale follows the world. A per-account locale override cannot be
   made coherent — it would desync the UI from the content — so it should not
   exist.

In Ferrodun a tenant *is* a world/game (§3.11.3: own DB, world directory,
settings, port set), so "per-tenant" and "per-world" denote the same scope.

## Non-goal: color stays per-account

Color preference (§3.20.6) is **not** moving to per-world, and the divergence is
principled. Color carries no meaning — the meaning is in the words — so the
palette is applied *after* `StyledText`, at the per-connection telnet renderer
(§3.20.5.4). Swapping the palette leaves the player reading the same world, which
makes an individual override "free" for color in a way it can never be for
locale. On top of that, color must stay per-connection/per-account for reasons
locale has no analogue to: the color *tier* is bounded by negotiated terminal
capability (truecolor / xterm256 / ansi16 / mono via TTYPE), and the
colorblind-safe palette (§3.20.6.3) and `NO_COLOR` (§3.20.6.2) are accessibility
accommodations for the individual human.

The layered color model is: builder-owned default palette (§3.20.3) → account
override (accessibility / capability) → engine default. A separate product
question — whether the account override is *scoped to accessibility/capability*
or a *free "pick any theme" knob* — is deferred (see Deferred, below); this PR
does not redesign the color model.

## Design

### Principle

The locale is sourced once per world and threaded from a single place. Actual
tenant-config *sourcing* stays deferred to M1-22 (the driver that will own tenant
config and the gateway); this PR collapses the plumbing to one per-world source
that still defaults to `en`. `t!` and `Catalog` stay keyed by `Locale`; only how
the locale is *sourced* changes.

### SPEC edits (§3.14)

- **§3.14.6** — retitle from "Locale resolution per session." Rewrite **§3.14.6.1**
  as: the effective locale is a single tenant-configured locale, defaulting to
  `en`.
- **§3.14.6.2** — **preserved** (load-time verification that every `t!` /
  `mud.i18n.t` key exists in `en`). It is orthogonal to per-session resolution;
  it keeps its number and its inbound references (§3.14.2.1, PLAN §M2-I).
- **§3.14.6.3** — **removed** (mid-session locale switching).
- **§3.14.7.1** — the LLM persona slice includes the **tenant locale** (was "the
  active session's locale").
- **§3.14.4.2** — **remove `mud.i18n.locale_of(entity)`**; keep `mud.i18n.t`.
- **§3.14.5.2 / §3.14.5.3** — localized aliases and command help render in **the
  tenant's locale** (was "the active session's locale").
- **§3.14.8.1** — acceptance renders in **the tenant's configured locale** (was
  "a session whose locale resolves to it").
- **Keep** §3.14.2 (`en` default/reference) and §3.14.3 (Fluent, two-source
  tenant-overriding discovery, tenant-scoped loader, hot-reload).

### SPEC collateral edits (outside §3.14)

- **§2.8.3.3** — `Core.Welcome` announces the tenant locale; retarget its
  `§3.14.6.1` reference. **Remove the `Core.Locale` message entirely** — with a
  fixed per-world locale it has nothing to carry in either direction, and
  `Core.Welcome` is the sole locale announcement.
- **§3.20.6.1** — replace "resolved like locale (§3.14.6.1) and switchable
  mid-session" with color's own inline resolution (account preference →
  tenant/builder default → engine default) and a one-line note on why color
  diverges from locale (non-semantic, applied at the render edge; accessibility /
  capability). Existing per-account-preference semantics are otherwise unchanged.

### PLAN edits

- **§M2-I** — drop "locale resolution per session (§3.14.6)", `mud.i18n.locale_of`,
  and "active session's locale"; restate as **tenant-locale selection**. Keep
  Fluent, two-source discovery, tenant-scoped loader, hot-reload, §3.14.6.2 key
  verification, and localized command aliases.
- **§M2 acceptance** — a localized engine string "renders in the tenant's
  configured locale" (was "a session whose locale resolves to it").
- **§M3-B** — remove `Locale` from the reserved `Core.*` handshake message list.

### Implementation (mud-engine)

- **`caller.rs`** — remove the `CallerContext.locale` field, the `locale`
  constructor parameter, and the `.locale()` accessor. Update the unit test.
- **`session/resolver.rs`** — `RegistryResolver::resolve` stops passing
  `Locale::EN` into `CallerContext`.
- **`pipeline.rs`** — `Pipeline` gains a `locale: Locale` field, supplied at
  `Pipeline::new` (callers pass `Locale::EN` until M1-22 wires tenant config).
  `dispatch` uses `self.locale` everywhere it currently reads `caller.locale()`.
  This matches the existing invariant that one pipeline serves one World.
- **`dispatch.rs`** — `CommandContext` sources its locale from the pipeline's
  value, so **`ctx.locale()` keeps working unchanged** and `builtins.rs` needs no
  edits.
- **`session/render.rs`** — signature unchanged (already takes `&Locale`); the
  M1-22 driver will feed it the same tenant locale.

## Testing

- `caller.rs` unit test updated for the removed locale field.
- Pipeline tests construct with an explicit locale and assert output renders in
  the pipeline's locale, independent of any session.
- A regression test: two co-located sessions' broadcast renders in the one world
  locale (ties back to the M1-19a broadcast contract).
- Gates: `cargo test --workspace`, `cargo clippy --workspace --all-targets -D
  warnings`, `cargo fmt --all --check`, `uv run mkdocs build --strict`.

## Documentation

Scan `docs/docs/` for any player/operator-facing mention of per-session locale or
locale switching; correct to per-world if present (likely none yet — verify in
the plan).

## Deferred (out of scope for this PR)

- **Tenant-config locale sourcing** — reading the configured locale from tenant
  config lands at M1-22; this PR defaults to `en` at the single source.
- **Color override scope** — whether the per-account color override is scoped to
  accessibility/capability or a free theme choice is a separate product decision;
  §3.20.6.1 keeps its current preference semantics here.

## Known tension

Touches mud-engine plus several SPEC and PLAN sections. It is cohesive — one
conceptual change, locale's *scope* — so a single PR is appropriate, mirroring the
M1-19a precedent for a spec + code change that spans surfaces.
