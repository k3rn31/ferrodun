# Ferrodun — Journal

Breadcrumb trail of implementation work. Newest entries at the bottom, one
per implementation PR. Format defined in `CLAUDE.md`. Code is the source of
truth when this log drifts.

## 2026-06-25 — Roadmap established

- **Spec:** §0–§11 — full normative spec reviewed.
- **Done:** Authored `PLAN.md` (master roadmap: Phase 0 + M1–M8 decomposed
  into PRs/epics, execution principles, per-PR Definition of Done).
  Updated `CLAUDE.md` to name `SPEC.md`/`PLAN.md`/`JOURNAL.md` roles.
- **Verify:** Documentation only; no code. `PLAN.md` cross-checked against
  §5 (repo layout), §7 (workstreams/milestones), §7.5 (ordering).
- **Next:** Begin **P0-01** — convert the single `ferrodun` package into a
  Cargo workspace, move `main` into `mudd`, wire CI (fmt/clippy/test).
