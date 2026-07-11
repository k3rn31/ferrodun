# Design: `en` templates for the command-pipeline `command.*` keys

**Date:** 2026-07-11
**Status:** Approved
**Spec refs:** SPEC §3.14.4.3 (fallback order), §3.14.6.2 (every `t!` key MUST exist in `en`), §2.7 (command pipeline)

## Problem

The command pipeline (`crates/mud-engine/src/pipeline.rs`) emits five `t!`
keys that have no `en` template in the builtin catalog
(`crates/mud-i18n/src/catalog.rs`):

| Key | Trigger |
|---|---|
| `command.not-found` | line matches no command |
| `command.ambiguous` | prefix matches several commands |
| `command.bad-switch` | malformed switch syntax |
| `command.unbound` | parsed name has no bound handler |
| `command.denied` | lock denies the caller |

Consequences:

1. **Player-facing garbage.** `translate()` falls through to the literal key
   (§3.14.4.3c), so a mistyped command prints `command.not-found` instead of a
   message. For `ambiguous` and `bad-switch`, the candidate list / reason are
   computed and passed as args but never rendered.
2. **Spec violation.** §3.14.6.2 requires every `t!`-referenced key to exist
   in the `en` bundle. M2-I adds the *load-time verification* of that rule but
   does not add these keys.
3. **Misclassified WARN.** The miss triggers `warn_missing_once` — one WARN
   per `(locale, key)` per process (not per keystroke; the guard dedupes) —
   framing a player typo as an operator-actionable signal, against the logging
   razor ("warn = broken builder content only").

Existing tests enshrine the bug: `crates/mud-engine/tests/command_pipeline.rs`
asserts the literal keys at lines 262, 282, 300, 313, 340, 359, 422–423, and
`catalog.rs`'s unit test asserts `command.not-found` is absent.

## Decision

Add the five `en` templates to the static builtin catalog and update the
tests to assert the real player-facing strings. No structural change: the
static-table catalog, `translate()` fallback, and `warn_missing_once` all stay
as-is. Once the keys resolve, the WARN never fires for these paths, so the
razor violation disappears without touching logging code.

Alternatives rejected:

- **Guard test enumerating pipeline `t!` keys** — a hand-maintained list that
  drifts; the per-path assertions below already fail if a template is removed,
  and M2-I brings the real §3.14.6.2 verification.
- **Typed message enum + render boundary** (like the pre-login
  `SessionMessage`/`render.rs` split) — larger diff, zero behavioral gain,
  and M2-I overhauls this layer anyway.

## Change 1 — catalog entries

In `crates/mud-i18n/src/catalog.rs`, add to `ENTRIES` under a new
`// command pipeline (§2.7)` comment group:

| Key | Template |
|---|---|
| `command.not-found` | `Unrecognized command. Type 'help'.` |
| `command.ambiguous` | `Which do you mean? { $options }` |
| `command.bad-switch` | `Invalid switch: { $reason }.` |
| `command.unbound` | `That command isn't available right now.` |
| `command.denied` | `You can't do that.` |

Wording rationale:

- `not-found` mirrors the existing pre-login `session.unknown` so unknown
  input reads the same before and after login.
- `ambiguous` mirrors the existing `object.ambiguous` phrasing; `$options` is
  the comma-joined candidate list the pipeline already passes.
- `bad-switch` interpolates `$reason` (the `SwitchError` Display text, e.g.
  "switch must not be empty").
- `unbound` is deliberately generic: the code comment forbids leaking the
  unbound command name to the player; the operator WARN already carries it.
- `denied` is the classic generic MUD denial; the lock WARN carries detail.

Also update the `Catalog::builtin` doc comment, which currently notes the
`command.*` outcomes fall through as literals.

## Change 2 — tests

- **`catalog.rs` unit tests.** `the_builtin_catalog_holds_the_m1_17_keys`
  uses `command.not-found` as its example of a key that misses — swap in a
  genuinely absent key (e.g. `absent.key`). Add a `command.*` positive-lookup
  block in the same style as `the_builtin_catalog_holds_the_session_keys`.
- **`crates/mud-engine/tests/command_pipeline.rs`.** Update the eight literal-
  key assertions to the real strings. For `ambiguous` and `bad-switch`, assert
  the interpolated form (e.g. `Which do you mean? say, score`), which finally
  verifies the candidate list and reason reach the player. Correct the stale
  comment at lines 292–294 claiming `command.ambiguous` is "a M2 catalog
  concern".
- **`pipeline.rs` in-crate test (line 433)** compares against
  `t!(Locale::EN, "command.not-found")`, so it self-adjusts; no change.

## Out of scope

- §3.14.6.2 load-time verification (owned by M2-I).
- Fluent migration, non-English bundles, localized aliases (M2-I).
- Pre-login `session.*` path (already has templates).
- Doc-site updates: no page documents these strings; message wording changes
  are not an observable-surface change requiring docs.

## Verification

TDD order: update the `command_pipeline.rs` assertions to the real strings
first (red), add the `ENTRIES` rows (green), adjust the catalog unit tests
alongside. Done when `cargo test -p mud-i18n -p mud-engine` passes and
`cargo clippy` is clean workspace-wide.
