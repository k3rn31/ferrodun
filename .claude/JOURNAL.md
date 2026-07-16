# Ferrodun — Journal

Breadcrumb trail of implementation work. Newest entries at the bottom. Code is
the source of truth when this log drifts. Entries through M1 are compacted to
one line each; full-format entries (per `CLAUDE.md`) resume for new work below
the `--- current work ---` marker.

## Phase 0 — foundations (2026-06-25)

- **Roadmap** — authored `PLAN.md` (Phase 0 + M1–M8 decomposition); set the
  `SPEC`/`PLAN`/`JOURNAL` roles in `CLAUDE.md`.
- **P0-01** — virtual Cargo workspace (`resolver = "3"`, root lints); `main`
  moved to `crates/mudd`; CI runs fmt/clippy/test.
- **P0-02** — versioned MkDocs + Material docs site under `docs/` (uv project,
  `mike`); `docs.yml` builds strict on PRs, deploys `next` on `main`, snapshots
  on tags.

## M1 — Walk and talk (2026-06-25 → 2026-07-16)

Core runtime (`mud-core`):
- **M1-01** — `EntityId`/`TenantTag` newtypes; 8-byte 12/32/20-bit layout;
  burn-on-generation-wraparound encoded in the type.
- **M1-02** — per-tenant generational arena (hand-rolled); alloc/free/resolve;
  cross-tenant resolution rejected (`CrossTenant` ≠ `StaleHandle`).
- **EntityKey/EntityId split** — durable persistence identity vs ephemeral
  in-memory handle.
- **M1-03** — `EntityKey` (durable entity identity) + core domain newtypes.
- **M1-04** — `Place` enum (Room only) + spatial surface.
- **M1-05** — hot side-tables (`LocationOf`/`Inventory`) + `Place::occupants`.
- **M1-06** — 20 Hz scheduler tick + `MutationCommand` (M1 subset).
- **M1-07** — locks DSL (chumsky parse → resolve → eval).
- **housekeeping** — mud-core reorg, comment cleanup, integration tests.

Persistence (`mud-db`):
- **M1-08** — SQLx + per-tenant SQLite backend.
- **M1-09** — write-through + boot load (arena-as-cache keyed by `EntityKey`).
- **review & polish** — mud-db crate review.

Wire/IPC + Gateway/World split (`mud-schema`, `mud-ipc`):
- **M1-10** — `mud-schema` directional postcard IPC frames.
- **M1-11a** — `mud-schema` resume-handshake vocabulary.
- **M1-11b** — `mud-ipc` transport + resume handshake + single-process mode.
- **checkpoint** — unify the write-model dispatch.
- **checkpoint** — error boundaries & visibility audit + fixes.

World loading (`mud-world`):
- **M1-12** — KDL room loader + tenant config; **unit test coverage** follow-up.
- **M1-12a** — `RegionKey` (region spec gap closed).
- **M1-12b** — region manifest loader + room binding.
- **M1-12c** — regions mandatory (dropped the implicit default).

Styled output + i18n (`mud-core`, `mud-net`, `mud-i18n`):
- **M1-13a** — styled-text model + palette + builder markup.
- **M1-13b** — per-session ANSI renderer (`mud-net`).
- **M1-13** — review fixes + SPEC §3.20.2.4.
- **M1-14** — engine-string lookup seam (`mud-i18n`).

Command pipeline (`mud-cmd`, `mud-engine`):
- **M1-15** — `mud-cmd` CmdSet merge + trie parser (+ review fixes: alias
  ownership, empty-token parse).
- **M1-16** — command pipeline in World (new `mud-engine` crate).
- **M1-17** — built-in command substrate (PR-A), handlers (PR-B),
  review fixes, movement-to-nowhere follow-up.

Accounts + sessions (`mud-account`, `mud-session`):
- **M1-18** — accounts + login (argon2id; `mud-account` + `mud-db` repository).
- **M1-19** — session FSM (login states); **M1-19a** session-dependent
  built-ins (`who`, `quit`, broadcast).
- **i18n rework** — per-world locale; dropped `CallerContext.locale`.

Networking + runtime (`mud-net`, `mud-gateway`, `mudd`):
- **M1-20** — `mud-net` telnet core.
- **M1-21** — `mud-gateway` library.
- **M1-22** — `mudd` single-process multi-tenant wiring.
- **M1-24** — tenant catalogue + `mudd serve`/`tenant` subcommand CLI
  (+ `command.*` pipeline en templates).
- **M1-25** — password echo suppression.
- **M1-26** — ANSI renderer wiring (+ emote/tell palette-role fix).
- **M1-27** — room presence (spawn/leave announcements, players in look).
- **M1-28** — telnet line discipline (block termination + prompt framing).

Cross-cutting (2026-07-07 → 07-11):
- **crate-audit hardening** — 12 SDD plans applied across crates.
- **#37** — consolidated the Direction↔word contract.
- **clippy suppressions** — audited and eliminated.
- **logging L0–L5** — subscriber config; tenant/session spans + level
  reclassification; i18n missing-key inheritance; mud-db lifecycle; ipc/gateway
  connection lifecycle; session auth-outcome events (+ L0 follow-up, review
  cleanup).
- **config** — removed tenant `FERRODUN_` env overrides.
- **docs** — consistency sweep, nav reorder, strict build.

## --- current work ---
