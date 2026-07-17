# Design: unambiguous `play` resolution (issue #32)

**Date:** 2026-07-17
**Issue:** [#32](https://github.com/k3rn31/ferrodun/issues/32) — puppet with an
all-digit name is unselectable by name (`play <digits>` always parses as an
ordinal)

## Problem

`match_puppet` (`crates/mud-session/src/fsm.rs`) tries to parse the `play`
argument as a 1-based ordinal before attempting a name match. `PuppetName`
allows all-digit names such as `42`, so a puppet with such a name can never be
selected by it — the digits always win as an ordinal. Meanwhile the character
menu never displays ordinals, so positional selection is an undocumented-in-UI
feature even though the docs describe `play <number>`.

## Decision

Keep both selection forms (name and ordinal), make the ordinal discoverable by
numbering the character menu, and eliminate the ambiguity at the type level by
forbidding all-digit names. Once no valid name can be all digits, a pure-digit
argument is provably an ordinal and the parse order in `match_puppet` cannot
shadow a name.

Alternatives considered and rejected:

- **Name-only selection** (drop ordinals): simplest, but ordinal selection is
  already documented player-facing behavior.
- **Name-first, ordinal fallback**: fixes the shadowing but leaves ordinals a
  hidden feature and keeps two overlapping meanings for digit input.
- **Puppet-names-only ban**: would fix the bug, but a single shared name rule
  for `Username` and `PuppetName` is simpler; all-digit usernames have no
  redeeming use.

## Design

### 1. Name rule: no all-digit names

The shared `validate()` in `crates/mud-account/src/name.rs` gains one rule: a
name must not consist entirely of ASCII digits. New `NameError::AllDigits`
variant with a player-explainable message. The rule applies to both `Username`
and `PuppetName`.

Sufficiency of the rule (why "not all digits" is exactly enough):

- `usize::from_str` accepts a leading `+` (e.g. `+42`), but `+` is outside the
  name alphabet `[A-Za-z0-9_'-]`, so no valid name can collide that way.
- Leading zeros (`007` → ordinal 7) are covered: `007` is all digits and
  therefore not a valid name.
- Any name containing a non-digit (e.g. `4rden`, `42_`) fails `usize` parsing
  and can only match by name.

### 2. Selection semantics

`match_puppet` keeps its ordinal-first order; the doc comment states the
invariant that all-digit names are unrepresentable, so the order cannot shadow
a name. Ordinals stay 1-based and index the same `Vec<Puppet>` held in
`State::PuppetSelect` that the menu was rendered from, so displayed numbers
and selection always agree.

### 3. Character menu: multi-line numbered list

`session.puppet-list` becomes a multi-line menu:

```
Your characters:
  1) arden
  2) borel
Type 'play <name or number>' or 'new <name>'.
```

The renderer (`crates/mud-engine/src/session/render.rs`) builds the numbered
lines; the catalog string in `crates/mud-i18n/src/catalog.rs` is updated to
match.

### 4. Failure message

A `play` argument that resolves to no puppet (out-of-range ordinal, unknown
name) returns a new `SessionMessage::NoSuchPuppet` ("No such character.")
instead of the generic `UnknownCommand`. One enum variant, one catalog entry.

### 5. Persistence

No migration. The DB loader already funnels every stored name through
`parse_puppet_name`, mapping rejects to `DbError::CorruptValue`
(`crates/mud-db/src/sqlite/accounts.rs`), so the tightened rule is enforced on
load for free. Pre-0.1 there is no production data; a dev database holding a
name like `42` surfaces as `CorruptValue`, which is acceptable.

### 6. SPEC.md amendment

§3.15.1.4 gains two requirements:

- Account and puppet names MUST NOT consist entirely of digits.
- Puppet selection MUST accept the puppet's name (compared case-insensitively)
  and MUST accept the 1-based ordinal as displayed in the character list.

### 7. Testing (TDD)

New failing tests first:

- `NameError::AllDigits` for `"42"` and `"007"` on both newtypes; acceptance
  of mixed names such as `"4rden"`.
- `match_puppet`: by ordinal in range, out of range, and by name.
- FSM level: `play 2` and `play arden` both enter the world; a non-resolving
  argument yields `NoSuchPuppet`.
- Renderer: numbered multi-line menu output; `NoSuchPuppet` message text.

### 8. Documentation (same PR)

- `docs/docs/playing/getting-started.md`: document the name rule (names cannot
  be all digits) and update the menu example if one is shown.
- `docs/docs/architecture/sessions.md`: already describes `play <name>` /
  `play <number>`; skim for accuracy, no structural change expected.
