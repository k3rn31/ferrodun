# Ferrodun

A pure-Rust MUD/MU* engine. See `@SPEC.md`.

## The three governing documents

Ferrodun is developed in small, incremental, self-contained PRs. Three
documents govern that work, each with a distinct role:

- **`SPEC.md` — the specification (what).** The normative source of truth.
  Always consult the relevant section before implementing, and honor RFC 2119
  keywords (MUST/SHOULD/MAY) exactly. When any other document disagrees with
  `SPEC.md`, `SPEC.md` wins.
- **`PLAN.md` — the roadmap (when/in what order).** The master plan that
  sequences the spec into ordered PRs grouped by milestone (Phase 0, M1–M8).
  **Follow it top to bottom.** Each PR must be small, independently
  reviewable and testable, and leave nothing half-built behind. Honor its
  per-PR Definition of Done and its core principle: **don't implement or stub
  anything until you need it** (YAGNI) — including not creating a crate before
  the PR that first uses it. If reality diverges from the plan, update
  `PLAN.md` rather than silently deviating.
- **`.claude/JOURNAL.md` — the progress log (what was actually done).** The
  breadcrumb trail for the next session. **Code is the source of truth for
  current state**; the journal is a log of intent and may drift — when journal
  and code disagree, trust the code.

## Journal

After completing **any implementation task** (typically one PR from
`PLAN.md`), append an entry to `.claude/JOURNAL.md` so the next session knows
where things stand. Format:

```markdown
## YYYY-MM-DD — <short title>

- **Spec:** §<section(s)> — <what the spec required>
- **Done:** <what was implemented/changed>
- **Verify:** <how it was checked: tests, command, etc.>
- **Next:** <follow-ups, known gaps, or TODOs>
```

Newest entries at the bottom. One entry per task. Keep it terse — it is a breadcrumb trail, not documentation.

## Documentation site

User- and builder-facing documentation lives in `docs/` (MkDocs + Material),
published to GitHub Pages and **versioned with `mike`**: the `main` branch is
the `next` version, and each `vX.Y.Z` tag is snapshotted to its own version.
Markdown pages live under `docs/docs/`; navigation and theme are in
`docs/mkdocs.yml`; the CI build/deploy is `.github/workflows/docs.yml`.

**Keep the docs current as features land.** Whenever a PR adds or changes
behavior a player, builder, or operator can observe — a command, a config key,
a script API, a network feature, a CLI subcommand, a deployment knob — update
or add the relevant page under `docs/docs/` (and `nav` in `mkdocs.yml`) **in
the same PR**, and note the doc change in the journal entry. Purely internal
changes (refactors, engine plumbing with no external surface) do not need doc
updates. Keep the prose accurate and concise; when docs and `SPEC.md` disagree,
`SPEC.md` wins and the docs are corrected. The toolchain is a uv project in
`docs/` (`pyproject.toml` + `uv.lock`); verify with `uv run mkdocs build
--strict` from `docs/`.

## Mandatory engineering rules

These are hard constraints, not preferences (cf. SPEC §1.7):

- **Type-Driven Design.** Make illegal states unrepresentable. Encode invariants in types so the compiler rejects invalid values; do not validate at runtime with `if`/`assert` what a type could forbid.
- **Newtype pattern is mandatory.** Distinct domain concepts get distinct types (`EntityId`, `PlaceId`, `TenantTag`, …). Raw primitives MUST NOT cross public APIs where a domain meaning exists. Parse inputs into typed domain values at boundaries; inner code MUST NOT re-validate.
- **`unwrap()` is strictly forbidden.** No exceptions.
- **`expect()` is allowed only in tests**, never in production code, and must carry a descriptive message.
- **Errors are always handled.** Libraries define error types with `thiserror`; applications use `anyhow`. `panic!`/`todo!`/`unreachable!` are forbidden in production unless guarded by a documented `// INVARIANT:` comment.

## Conventions

- Add dependencies with `cargo add` / `cargo add --dev` — never hand-edit `Cargo.toml`.
- Code and comments in English. Comment *why*, not *how*.
- Follow TDD: failing test → minimal code → refactor.
- Must compile clean under `cargo clippy` (workspace denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`).
