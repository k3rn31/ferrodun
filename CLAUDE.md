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

The documentation lives in `docs/` (MkDocs + Material), published to GitHub
Pages and **versioned with `mike`**: `main` is the `next` version, each
`vX.Y.Z` tag its own snapshot. Pages live under `docs/docs/`; nav and theme in
`docs/mkdocs.yml`; CI in `.github/workflows/docs.yml`. The toolchain is a uv
project in `docs/`; verify with `uv run mkdocs build --strict` from `docs/`.

How the docs are treated:

- **Current state only.** Document what the engine supports *today* — never
  roadmap, planned features, or development-process/milestone content. Vision
  belongs in `README.md`, the roadmap in `PLAN.md`. Never guess: pin every
  claim to the code and cut what you cannot confirm there.
- **Persona-driven structure.** Pages are organized by audience — **Playing**
  (players), **Building** (builders), **Operating** (operators), and
  **Architecture** (how the running system works today). New pages go under the
  matching persona.
- **Accuracy wins.** Code is the source of truth for current behavior; when
  docs and code disagree, correct the docs. When docs and `SPEC.md` disagree on
  intended behavior, `SPEC.md` wins.
- **Diagrams are Mermaid** — text-based fenced blocks, native to Material, no
  extra dependencies.
- **Update in the same PR.** Whenever a PR changes observable behavior — a
  command, config key, script API, network feature, CLI subcommand, deployment
  knob — update the relevant page (and `nav`) in that PR and note it in the
  journal. Purely internal changes (refactors, plumbing with no external
  surface) need no doc update.

## Mandatory engineering rules

These are hard constraints, not preferences (cf. SPEC §1.7):

- **Type-Driven Design.** Make illegal states unrepresentable. Encode invariants in types so the compiler rejects invalid values; do not validate at runtime with `if`/`assert` what a type could forbid.
- **Newtype pattern is mandatory.** Distinct domain concepts get distinct types (`EntityId`, `PlaceId`, `TenantTag`, …). Raw primitives MUST NOT cross public APIs where a domain meaning exists. Parse inputs into typed domain values at boundaries; inner code MUST NOT re-validate.
- **`unwrap()` is strictly forbidden.** No exceptions.
- **`expect()` is allowed only in tests**, never in production code, and must carry a descriptive message.
- **Errors are always handled.** Libraries define error types with `thiserror`; applications use `anyhow`. `panic!`/`todo!`/`unreachable!` are forbidden in production unless guarded by a documented `// INVARIANT:` comment. Never leak third-party errors through public API.
- **Database schemas are normalized to 3NF.** Design relational schemas to Third Normal Form: atomic columns, every non-key attribute fully dependent on the key, no transitive dependencies. Denormalize only for a measured reason, and document it in the migration with a `-- DENORMALIZED:` comment explaining the tradeoff.
- **Never suppress lints.** Lints are there for a reason. If in very rare cases a lint is truly inappropriate, suppression must be in the smallest scope possible, with a `// LINT:` comment explaining why it is safe to ignore. Don't be lazy!

## Conventions

- Add dependencies with `cargo add` / `cargo add --dev` — never hand-edit `Cargo.toml`.
- Code and comments in English. Comment *why*, not *how*.
- Follow TDD: failing test → minimal code → refactor.
- Must compile clean under `cargo clippy` (workspace denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`).

## Logging

Instrumentation follows `docs/superpowers/specs/2026-07-08-logging-strategy-design.md`; consult it before adding logs. The essentials:

- **Log at boundaries, stay silent in the core.** Pure/domain crates (`mud-core`, `mud-cmd`, `mud-account`, `mud-session`, `mud-schema`) take no `tracing` dependency and emit nothing — they return typed outcomes. Only boundary crates instrument. (`mud-net`/`mud-world` are the sole exception: one builder `warn` each for broken content.)
- **Two level razors.** `info` = boot/shutdown heartbeat only (a healthy server is near-silent at `info`); `warn` = broken *builder content* only; otherwise `error` if an operator must act, `debug` for per-session diagnostics, `trace` for the per-tick firehose. Never `warn` on the 20 Hz tick hot path.
- **Never-log discipline.** No passwords, hashes, email, tokens, usernames, raw player input, or payload bytes. Log `account_id` not username; frame diagnostics are length-only; the peer IP is logged once at `debug`. Enforce it with a negative test (`!logs_contain(secret)`).
- **Context comes from ambient spans** (`tenant`, `session`), not per-call plumbing — open the span at the boundary and downstream events inherit it.
