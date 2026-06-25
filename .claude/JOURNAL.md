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

## 2026-06-25 — P0-01 Workspace skeleton + CI

- **Spec:** §5 (repo layout), PLAN P0-01 — virtual Cargo workspace that
  builds green in CI before any domain code exists.
- **Done:** Converted root `Cargo.toml` into a virtual workspace (`resolver
  = "3"`, `[workspace.package]` version/edition, lints kept at workspace
  root). Created only `crates/mudd` (others created lazily per YAGNI); moved
  `src/main.rs` → `crates/mudd/src/main.rs` as a placeholder `main` that
  emits one line via `stdout().write_all` (avoids the denied `print_stdout`
  lint). Wired workspace lints into `mudd` via `[lints] workspace = true`.
  Added `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo
  clippy --workspace --all-targets -D warnings`, `cargo test --workspace`.
- **Verify:** Locally green — fmt clean, clippy clean under deny lints,
  `cargo test --workspace` (0 tests) ok, `cargo run -p mudd` prints
  `ferrodun mudd placeholder`.
- **Next:** **M1-01** — `EntityId` + `TenantTag` newtype with the normative
  bit layout (§2.3.1.3); first PR to create `crates/mud-core`.
