# Documentation refactor & README — design

- **Date:** 2026-07-09
- **Status:** approved design, ready for implementation planning
- **Scope:** the user/builder/operator documentation site under `docs/docs/`,
  its MkDocs configuration, and a new repository `README.md`.

## Problem

The documentation site has drifted from the code and leans on aspirational
content. The current homepage is almost entirely a "what it *will* offer"
list (Lua scripting, SSH/TLS/WebSocket, MCCP2/GMCP, NPCs, combat, LLM
dialogue, admin dashboard) — none of which exists today. Individual pages are
well-written but contain claims that must be re-checked against the
implementation (for example, whether `who` is still a stub). The repository
has no `README.md`. The site is also small enough right now to reorganize
cleanly before more pages arrive.

## Goals

1. **Strictly current-state.** Every page describes only what the engine
   supports today. No roadmap, no "coming soon" lists, no development-process
   or milestone content anywhere on the site.
2. **Code-verified accuracy.** Every factual claim is pinned to its source in
   the code before it ships. Anything that cannot be confirmed in code is cut,
   not guessed.
3. **Reorganized, scalable IA.** Restructure the navigation now, while the
   site is small, into an audience-first layout that will absorb future pages
   without churn.
4. **New Architecture section.** A first-class section explaining how the
   running system works today, with Mermaid diagrams.
5. **README.** A concise repository overview that does not duplicate the docs,
   with a small vision section.

## Non-goals

- Documenting unimplemented or planned features.
- Editing `SPEC.md`, `PLAN.md`, or `.claude/JOURNAL.md` (development-process
  documents; they stay out of the user-facing site).
- Wholesale prose rewriting. The existing prose is largely good; changes are
  driven by accuracy, gaps, restructuring, and consistency — not style for its
  own sake.

## Governing decisions

These were settled during brainstorming and are load-bearing for the plan:

- **Docs are current-state only.** The vision/roadmap lives solely in the
  README's short "Vision" section (which points to `SPEC.md`/`PLAN.md`), and
  nowhere on the docs site.
- **Full reorganization now.** Prefer a clean audience-first IA over
  preserving the current structure.
- **Architecture section is in scope**, scoped to *runtime architecture as it
  exists today* — how the running system behaves, never milestones or dev
  phases.
- **Diagrams: Mermaid only.** Text-based, diffable, versioned with `mike`,
  native to Material for MkDocs (no new dependency). No Excalidraw.

## Target information architecture

```
Home                      index.md
Playing/
  Getting started         playing/getting-started.md
  Commands                playing/commands.md
Building/
  World files             building/world-files.md
  Rooms                   building/rooms.md
  Regions                 building/regions.md
  Color & styling         building/styling.md
  Localization            building/localization.md            (new)
Operating/
  Running a server        operating/running-a-server.md       (moved)
  Configuration           operating/configuration.md          (new, split out)
  Logging                 operating/logging.md                (new, split out)
Architecture/
  Overview                architecture/index.md               (new)
  Engine & the tick loop  architecture/engine.md              (new)
  Sessions & login        architecture/sessions.md            (new)
  Rendering & color       architecture/rendering.md           (new)
  Internationalization    architecture/i18n.md                (new)
```

Rationale:

- **Audience-first top level** (Playing / Building / Operating / Architecture)
  gives every future page an obvious home.
- **`running-a-server.md` splits** into Running a server / Configuration /
  Logging: these are distinct operator tasks that will only grow. Split pages
  are kept tight rather than padded.
- **Localization is a Building concern** — choosing a tenant's locale is an
  authoring decision — while the *mechanism* is explained in Architecture →
  Internationalization, cross-linked, not duplicated.
- Existing files move under `operating/`; `edit_uri` and `mike` versioning are
  unaffected (paths inside `docs/docs/` only).

## Per-area plan

Each area lists what to write and the code that grounds it. During
implementation, claims are verified against these sources; unconfirmed claims
are removed.

### Home (`index.md`) — rewrite

Replace the "What it will offer" page with a current-state landing page:

- One paragraph: what Ferrodun is (a pure-Rust MUD/MU\* engine).
- A "What works today" list drawn only from implemented features: a telnet
  server, the real built-in command set, KDL-authored worlds
  (rooms/regions/palette), per-tenant multi-tenancy, palette-driven color with
  per-session downsampling, English message rendering.
- Links into Playing / Building / Operating / Architecture.
- One teaser Mermaid component diagram linking to the Architecture overview.
- Drop the "Under construction / what it will offer" framing entirely.

### Playing

- **Getting started** — audit against the session flow. Reconcile the `who`
  note: in-world `who` is implemented (lists connected players, sorted;
  `mud-engine/src/builtins/session.rs`), whereas the pre-login prompt `who`
  may still be a stub (`session.who-stub` in `mud-i18n`). State each context
  correctly. Verify the pre-login command set (`login`, `register`, `who`,
  `help`/`?`, `quit`) against the login FSM, and puppet selection (`play
  <name>`, `play <number>`, `new <name>`).
- **Commands** — verify the in-world built-in table exactly against
  `mud-engine/src/builtins/mod.rs`: `look` (`l`); movement `north`/`east`/
  `south`/`west`/`up`/`down` (`n`/`e`/`s`/`w`/`u`/`d`); `say`; `who`; `get`
  (`take`); `drop`; `inventory` (`i`/`inv`); `quit`. Confirm prefix-matching
  behavior and object-disambiguation (`sword.2`, `all coin`) against the
  handlers. Remove any command not in the table (e.g. no in-world `help`,
  `tell`, or `emote` today).

### Building

- **World files / Rooms / Regions** — audit every KDL example and every
  stated rule and error condition against the `mud-world` loaders (config,
  rooms, regions) and their error paths. Confirm the tenant folder layout, the
  "every room lives in a region" rule, the reserved `world/` root, slug rules,
  and directions.
- **Color & styling** — audit against `mud-core/src/text/{markup,palette,
  style}.rs` and `mud-world/src/palette.rs`: palette file, named colors, roles
  and their attributes, field markup rules, the baseline roles/colors set, and
  the trusted-vs-player-input distinction. Cross-link the tier mechanics to
  Architecture → Rendering rather than restating them.
- **Localization** (new) — document honestly: the per-tenant `locale` key
  exists and is wired end-to-end into the render path
  (`mud-world/src/config.rs` → `mudd/src/boot.rs` → the engine's `t!` calls),
  **but only the English (`en`) message set ships**; there is no supported way
  to add another language yet, and a non-`en` locale falls back to English
  (and logs missing-key warnings). No claim of multilingual support.

### Operating

- **Running a server** — quickstart (`mudd --tenant-dir …`) and running under
  a supervisor (systemd unit, fail-stop rationale). Verify defaults
  (`127.0.0.1:4000`) and the fail-stop behavior against `mudd`.
- **Configuration** (new, split out) — the complete key reference verified
  against `mudd::config` (server-wide: `rate`, `burst`, `log_format`,
  `[[tenants]]` registry) and `mud-world/src/config.rs` (per-tenant:
  `start_room`, `tenant_tag`, `locale`, `banner`, `palette`), with defaults,
  requiredness, and the precedence chain (defaults < `config.toml` < `MUDD_*`
  env < flags). Confirm the `MUDD_` vs `FERRODUN_` prefix split.
- **Logging** (new, split out) — levels and their meaning, `RUST_LOG`, and the
  `--log-format` / `MUDD_LOG_FORMAT` / `log_format` knob and its precedence,
  verified against `mudd` and the logging strategy already implemented.

### Architecture (new section)

Runtime architecture as it exists today. No milestones, no roadmap. Every
diagram is Mermaid. Content is grounded in the crate layout and verified
during implementation; specifics that cannot be confirmed (e.g. exact tick
rate, DB backends available) are verified against code before being stated.

- **Overview** — the per-tenant stack: telnet client → `mud-net` →
  `mud-gateway` → `mud-engine` → `mud-core`/`mud-world` + `mud-db`; tick-driven
  engine; durable state in the database with the in-memory world rebuilt on
  boot; fail-stop supervision; per-tenant isolation. Mermaid component diagram.
- **Engine & the tick loop** — the fixed-tick scheduler and the command
  pipeline: parse (prefix match) → dispatch → handler → effects → apply to
  world → render. Mermaid flow diagram. Grounded in `mud-core` (scheduler,
  world), `mud-cmd`, and `mud-engine` (dispatch, pipeline, builtins).
- **Sessions & login** — connection lifecycle, telnet negotiation, the login
  FSM, and puppet selection. Mermaid state diagram. Grounded in `mud-net`
  (negotiation), `mud-session` (FSM), `mud-account`, `mud-gateway`.
- **Rendering & color** — the render pipeline: authored 24-bit truecolor +
  palette roles + markup, downsampled per session to `mono` / `ansi16` /
  `xterm256` / `truecolor`. Mermaid flow diagram. Grounded in
  `mud-core/src/text/*` and `mud-net/src/convert.rs` (`Tier`). Complements the
  builder-side Color & styling page.
- **Internationalization** — the `t!(locale, key)` message-key seam and the
  resolution order `(locale, key)` → `(en, key)` → literal key, with the
  English-only reality stated plainly. Grounded in `mud-i18n`.

## MkDocs / diagram wiring

- Enable Mermaid by adding a `superfences` custom fence for `mermaid` in
  `docs/mkdocs.yml` (Material 9.7.6 renders it natively; no dependency change).
- Update the `nav:` tree to the target IA above.
- Diagrams auto-theme to the existing deep-orange/amber light+dark palette.
- Verify the whole site builds with `uv run mkdocs build --strict` from
  `docs/`.

## README.md (new, repository root)

Concise, non-overlapping with the docs:

- **What Ferrodun is** — one short paragraph.
- **Current key features** — only what works today (mirrors the homepage's
  "works today" list, phrased for a repo reader).
- **Vision** — a few sentences on the ambition, explicitly pointing to
  `SPEC.md` and `PLAN.md` for the roadmap. This is the *only* place vision
  lives in the deliverable.
- **Quickstart pointer** — the one-line `mudd --tenant-dir …` and a link to the
  docs site; no how-to depth.
- **Docs / build / license** — links to the published docs, how to build docs
  locally (`uv run mkdocs serve` in `docs/`), and the license.

## Verification

- Content: each claim traced to the code sources named above; a final read
  confirms no unimplemented feature is described anywhere on the site.
- Consistency: one terminology sweep across all pages (puppet/character,
  tenant/world, region rules, direction set) and heading/formatting
  conventions.
- Build: `uv run mkdocs build --strict` passes from `docs/`; all internal links
  resolve; Mermaid diagrams render.
- README: renders on the git host; all links resolve.
