# i18n per-world locale rework — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make locale a per-tenant (per-world) property in the spec, the plan, and the as-built engine plumbing — collapsing the per-caller locale field into a single per-world source on `Pipeline`.

**Architecture:** Two doc tasks (SPEC, PLAN) rewrite §3.14 and its dependents to a single tenant-configured locale (default `en`), removing per-session resolution, mid-session switching, `mud.i18n.locale_of`, and the `Core.Locale` GMCP message. Two code tasks move the locale from `CallerContext` (per-session, hardcoded `Locale::EN`) to a `locale` field on `Pipeline`, threaded into `CommandContext` so `ctx.locale()` keeps working unchanged for built-ins. Actual tenant-config *sourcing* stays deferred to M1-22; this PR defaults the single source to `en`.

**Tech Stack:** Rust workspace (mud-engine, mud-i18n), `jj` VCS, `cargo test`/`clippy`/`fmt`. SPEC.md and PLAN.md are root markdown (not the MkDocs site).

## Global Constraints

- **VCS is `jj` (Jujutsu), not git.** Commit with `jj commit -m "..."`. Never run `git commit`.
- **`unwrap()` forbidden; `expect()` tests-only with a descriptive message; no `panic!`/`todo!`/`unreachable!`** in production (workspace clippy denies `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`).
- **Newtype pattern is mandatory**; raw primitives MUST NOT cross public APIs where a domain meaning exists. `Locale` is the domain type — keep it typed end to end.
- **Never suppress lints** except in the smallest scope with a `// LINT:` comment.
- **Code and comments in English; comment *why*, not *how*.**
- **Add deps with `cargo add`** — never hand-edit `Cargo.toml`. (No new deps expected here.)
- **When any document disagrees with SPEC.md, SPEC.md wins.**
- Full gate before the final commit: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`.

---

### Task 1: SPEC.md — rewrite §3.14 and its dependents to per-tenant locale

**Files:**
- Modify: `SPEC.md` (§3.14.4.2, §3.14.5.2, §3.14.5.3, §3.14.6, §3.14.7.1, §3.14.8.1, §2.8.3.3, §2.8.3.4, §3.20.6.1)

**Interfaces:**
- Consumes: nothing.
- Produces: the normative text the code tasks implement (single tenant locale, default `en`, no per-session resolution). Task 2 mirrors these section numbers into PLAN.md.

- [ ] **Step 1: Rewrite §3.14.6 (heading + .6.1, remove .6.3, keep .6.2)**

Replace this block:

```
#### 3.14.6 Locale resolution per session

3.14.6.1 A session's effective locale MUST be resolved in the
following order, first match wins:
1. An explicit GMCP `Core.Locale` message from the client (additive
   wire-protocol change per §2.8.5.1).
2. A persisted per-account preference in the DB.
3. The tenant's default locale (configurable; defaults to `en`).
4. `en`.

3.14.6.2 At world load, the engine MUST verify that every key
referenced via `t!` or `mud.i18n.t` exists in the `en` bundle. A
missing English key MUST be a load-time error. Non-English bundles
MAY be incomplete; missing keys fall back per §3.14.4.3.

3.14.6.3 Locale switching mid-session MUST take effect on the next
command without disconnect.
```

with:

```
#### 3.14.6 Tenant locale and load-time verification

3.14.6.1 A tenant's effective locale MUST be a single value
configured per tenant (§3.11), defaulting to `en`. The locale is a
property of the world, not of a session or account: every session
connected to a tenant renders engine-emitted strings in that one
locale. There MUST be no per-session or per-account locale resolution
and no mid-session switching — the world's content language and the
engine's UI language are one builder-owned choice.

3.14.6.2 At world load, the engine MUST verify that every key
referenced via `t!` or `mud.i18n.t` exists in the `en` bundle. A
missing English key MUST be a load-time error. Non-English bundles
MAY be incomplete; missing keys fall back per §3.14.4.3.
```

- [ ] **Step 2: Drop `mud.i18n.locale_of` from §3.14.4.2**

Replace:

```
3.14.4.2 The `mud` stdlib (§2.4.5) MUST expose `mud.i18n.t(key,
args)` and `mud.i18n.locale_of(entity)` so scripts can localize
builder-authored strings using the same bundles.
```

with:

```
3.14.4.2 The `mud` stdlib (§2.4.5) MUST expose `mud.i18n.t(key,
args)` so scripts can localize builder-authored strings using the
same bundles.
```

- [ ] **Step 3: Retarget §3.14.5.2 and §3.14.5.3 to the tenant's locale**

In §3.14.5.2, replace `localized aliases contributed by the active session's locale, merged via the standard` with `localized aliases contributed by the tenant's locale, merged via the standard`.

In §3.14.5.3, replace `Command help output MUST render in the session's locale` with `Command help output MUST render in the tenant's locale`.

- [ ] **Step 4: Retarget §3.14.7.1 to the tenant locale**

Replace:

```
3.14.7.1 The LLM subsystem (§3.1) MUST be locale-aware. The active
session's locale MUST be included in the system / persona prompt
slice (§3.1.3 #1) so generated speech matches the player's locale.
```

with:

```
3.14.7.1 The LLM subsystem (§3.1) MUST be locale-aware. The tenant's
locale MUST be included in the system / persona prompt slice (§3.1.3
#1) so generated speech matches the world's locale.
```

- [ ] **Step 5: Clarify §3.14.8.1 acceptance renders in the tenant locale**

Replace `directory and reloading, with no engine recompilation and no` with `directory and reloading (the tenant then renders engine strings in that locale), with no engine recompilation and no`.

- [ ] **Step 6: §2.8.3.3 — retarget Core.Welcome and remove the Core.Locale message**

Replace:

```
- `Core.Welcome` (server → client, REQUIRED response): carries the
  selected major version, the server's package list, the
  `session_id`, and the resolved locale (§3.14.6.1).
- `Core.Locale` (either direction, OPTIONAL): client-initiated
  request to switch locale, or server-initiated notification that
  locale resolution changed. See §3.14.6.3.
```

with:

```
- `Core.Welcome` (server → client, REQUIRED response): carries the
  selected major version, the server's package list, the
  `session_id`, and the tenant's locale (§3.14.6.1). The locale is
  fixed for the world; the client cannot switch it.
```

- [ ] **Step 7: §2.8.3.4 — drop the client-profile locale**

Replace:

```
2.8.3.4 If `Core.Hello` does not arrive within 5 s of negotiation
completing, the server MUST assume the **default profile**: wire
protocol major version 1, no GMCP packages subscribed, `en` locale.
```

with:

```
2.8.3.4 If `Core.Hello` does not arrive within 5 s of negotiation
completing, the server MUST assume the **default profile**: wire
protocol major version 1 and no GMCP packages subscribed. The
tenant's locale (§3.14.6.1) applies regardless — locale is not part
of the client profile.
```

- [ ] **Step 8: §3.20.6.1 — fix the broken "resolved like locale" reference**

Replace:

```
3.20.6.1 An account MUST be able to persist a color preference (tier
override and/or named palette selection), resolved like locale
(§3.14.6.1) and switchable mid-session without disconnect.
```

with:

```
3.20.6.1 An account MUST be able to persist a color preference (tier
override and/or named palette selection), resolved first-match-wins
from: the account preference, then the tenant's default palette
(§3.20.3), then the engine default. Unlike locale (§3.14.6), color
stays per-account and switchable mid-session without disconnect:
color carries no meaning and is applied per-connection at the render
edge (§3.20.5.4), and the color tier and colorblind-safe palette
(§3.20.6.3) depend on the individual terminal and player.
```

- [ ] **Step 9: Verify no dangling references remain**

Run: `grep -n "per session\|locale_of\|Core.Locale\|3.14.6.3\|switchable mid-session\|resolved like locale" SPEC.md`
Expected: only the §3.20.6.1 "switchable mid-session" phrase remains (now correct in context); no `Locale resolution per session`, no `locale_of`, no `Core.Locale`, no `3.14.6.3`, no `resolved like locale`.

- [ ] **Step 10: Commit**

```bash
jj commit -m "docs(spec): make locale a per-tenant property (§3.14)

Rewrite §3.14.6 as a single tenant-configured locale (default en),
removing per-session resolution, mid-session switching, and
mud.i18n.locale_of. Retarget §3.14.5/.7/.8, §2.8.3.3-.4 (remove the
Core.Locale message; Core.Welcome carries the tenant locale), and fix
§3.20.6.1's now-dangling 'resolved like locale' reference (color stays
per-account for accessibility/capability reasons)."
```

---

### Task 2: PLAN.md — restate M2-I, M2 acceptance, and M3-B

**Files:**
- Modify: `PLAN.md` (M2 acceptance line, §M2-I, §M3-B)

**Interfaces:**
- Consumes: the SPEC section numbers finalized in Task 1.
- Produces: nothing downstream.

- [ ] **Step 1: M2 acceptance — render in the tenant's configured locale**

Replace:

```
tenant's `i18n/`, hot-reloaded, and a localized engine string renders to a
session whose locale resolves to it (§3.14.8.1).
```

with:

```
tenant's `i18n/`, hot-reloaded, and a localized engine string renders in the
tenant's configured locale (§3.14.8.1).
```

- [ ] **Step 2: §M2-I — per-tenant locale selection, drop `locale_of`**

Replace:

```
- **M2-I — `mud-i18n` (Fluent).** Replace the M1-14 static `en` table with
  `fluent-rs`; two-source tenant-overriding bundle discovery (§3.14.3.2);
  tenant-scoped loader; hot-reloadable bundles; locale resolution per session
  (§3.14.6); `mud.i18n.t` / `mud.i18n.locale_of` for scripts (§3.14.4.2);
  localized command aliases in the CmdSet merge (§3.14.5.2); load-time
  verification that every `t!`/`mud.i18n.t` key exists in `en` (§3.14.6.2).
```

with:

```
- **M2-I — `mud-i18n` (Fluent).** Replace the M1-14 static `en` table with
  `fluent-rs`; two-source tenant-overriding bundle discovery (§3.14.3.2);
  tenant-scoped loader; hot-reloadable bundles; per-tenant locale selection
  (§3.14.6); `mud.i18n.t` for scripts (§3.14.4.2); localized command aliases
  in the CmdSet merge (§3.14.5.2); load-time verification that every
  `t!`/`mud.i18n.t` key exists in `en` (§3.14.6.2).
```

- [ ] **Step 3: §M3-B — drop `Locale` from the reserved Core.* list**

Replace:

```
  reserved `Core.*` handshake messages (`Hello`/`Welcome`/`Locale`/`Ping`/
  `Pong`/`Goodbye`) defined in `mud-schema` first (§2.8.3.3, §8 rule 4),
```

with:

```
  reserved `Core.*` handshake messages (`Hello`/`Welcome`/`Ping`/
  `Pong`/`Goodbye`) defined in `mud-schema` first (§2.8.3.3, §8 rule 4),
```

- [ ] **Step 4: Confirm the docs site needs no change**

Run: `grep -rin "locale\|per-session\|Core.Locale" docs/docs/`
Expected: no output (no player/operator-facing locale docs exist yet — nothing to correct).

- [ ] **Step 5: Commit**

```bash
jj commit -m "docs(plan): restate i18n as per-tenant locale (M2-I, M3-B)

Drop per-session locale resolution, mud.i18n.locale_of, and the
Core.Locale reserved message; M2 acceptance renders in the tenant's
configured locale."
```

---

### Task 3: Source the locale from `Pipeline`, thread it into `CommandContext`

**Files:**
- Modify: `crates/mud-engine/src/dispatch.rs` (`CommandContext` struct, `new`, `locale`)
- Modify: `crates/mud-engine/src/pipeline.rs` (`Pipeline` struct, `new`, add `with_locale`, `dispatch`, `run_matched`, tests)

**Interfaces:**
- Consumes: `mud_i18n::Locale` (has `Locale::EN`), the existing `CommandContext`/`Pipeline`.
- Produces:
  - `Pipeline::with_locale(self, locale: Locale) -> Pipeline` — sets the tenant locale (builder; default `en`).
  - `CommandContext::new(command_id: CommandId, caller: &CallerContext, locale: &Locale, switches: &[Switch], args: &str, world: &World, places: &dyn Places, roster: &dyn Roster)` — now takes `locale` as its third argument.
  - `CommandContext::locale(&self) -> &Locale` — unchanged signature; now returns the pipeline-sourced locale (built-ins keep calling `ctx.locale()` unchanged).

This task leaves `CallerContext::locale` in place (Task 4 removes it); after this task nothing *reads* it, so the tree compiles and all tests stay green.

- [ ] **Step 1: Baseline — the suite is green before changes**

Run: `cargo test -p mud-engine`
Expected: PASS (all existing tests).

- [ ] **Step 2: Write the failing test — the pipeline renders in its own locale**

Add to the `#[cfg(test)] mod tests` in `crates/mud-engine/src/pipeline.rs` (the module already has `use mud_i18n::Locale;`, `FakeResolver`, `NoPlaces`, `input`):

```rust
#[test]
fn output_renders_in_the_pipeline_locale_not_the_caller() {
    let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
    let caller = world.create().expect("create caller");
    let resolver = FakeResolver { caller };
    // The pipeline carries the tenant locale; the caller no longer supplies one.
    let mut pipeline = Pipeline::new(Dispatcher::new()).with_locale(Locale::EN);

    let outcome = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input(1, "look"))
        .expect("dispatch");

    // `look` parses to NotFound against the empty table, rendered via the
    // pipeline's locale (§3.14.6).
    let expected = t!(Locale::EN, "command.not-found");
    assert!(
        outcome
            .outputs
            .iter()
            .any(|o| o.text.as_str() == expected),
        "expected not-found rendered in the pipeline locale, got {:?}",
        outcome.outputs,
    );
}
```

- [ ] **Step 3: Run the test to verify it fails to compile**

Run: `cargo test -p mud-engine output_renders_in_the_pipeline_locale_not_the_caller`
Expected: FAIL — `no method named with_locale found for struct Pipeline`.

- [ ] **Step 4: Add the `locale` field and builder to `Pipeline`**

In `crates/mud-engine/src/pipeline.rs`, widen the import at the top:

```rust
use mud_i18n::{Locale, t};
```

Add the field to the struct:

```rust
#[must_use]
pub struct Pipeline {
    dispatcher: Dispatcher,
    ids: CommandIdGen,
    locale: Locale,
}
```

Default it in `new` and add the builder:

```rust
impl Pipeline {
    /// A pipeline dispatching to `dispatcher`'s bound commands.
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            ids: CommandIdGen::new(),
            locale: Locale::EN,
        }
    }

    /// Sets the tenant locale engine strings render in (§3.14.6). Defaults to
    /// `en`; the driver (M1-22) supplies the tenant's configured locale.
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }
```

- [ ] **Step 5: Read the locale from the pipeline in `dispatch` and `run_matched`**

In `dispatch`, replace:

```rust
        let caller = resolved.caller;
        let locale = caller.locale().clone();
```

with:

```rust
        let caller = resolved.caller;
        let locale = self.locale.clone();
```

In `run_matched`, replace:

```rust
        let session_id = caller.session_id();
        let locale = caller.locale();
```

with:

```rust
        let session_id = caller.session_id();
        let locale = &self.locale;
```

- [ ] **Step 6: Give `CommandContext` its own `locale` and pass it from the pipeline**

In `crates/mud-engine/src/dispatch.rs`, add the field:

```rust
#[must_use]
pub struct CommandContext<'a> {
    command_id: CommandId,
    caller: &'a CallerContext,
    locale: &'a Locale,
    switches: &'a [Switch],
    args: &'a str,
    world: &'a World,
    places: &'a dyn Places,
    roster: &'a dyn Roster,
}
```

Update `new` (add the `locale` parameter and its doc):

```rust
    /// Assembles the context for one handler invocation.
    ///
    /// Borrows the resolved [`CallerContext`] (session, caller entity, location,
    /// name) and the tenant `locale` separately, so adding a caller fact does not
    /// widen this signature.
    pub(crate) fn new(
        command_id: CommandId,
        caller: &'a CallerContext,
        locale: &'a Locale,
        switches: &'a [Switch],
        args: &'a str,
        world: &'a World,
        places: &'a dyn Places,
        roster: &'a dyn Roster,
    ) -> Self {
        Self {
            command_id,
            caller,
            locale,
            switches,
            args,
            world,
            places,
            roster,
        }
    }
```

Change `locale()` to return the context's own field:

```rust
    /// The tenant locale engine messages resolve against (§3.14.6).
    pub fn locale(&self) -> &Locale {
        self.locale
    }
```

In `crates/mud-engine/src/pipeline.rs` `run_matched`, update the `CommandContext::new` call (add `&self.locale` as the third argument):

```rust
        let reply = {
            let ctx = CommandContext::new(
                command_id,
                caller,
                &self.locale,
                switches,
                args,
                &*world,
                places,
                roster,
            );
            binding.handler().run(&ctx)
        };
```

- [ ] **Step 7: Run the new test — it passes**

Run: `cargo test -p mud-engine output_renders_in_the_pipeline_locale_not_the_caller`
Expected: PASS.

- [ ] **Step 8: Run the whole engine suite — still green**

Run: `cargo test -p mud-engine`
Expected: PASS. (Built-ins still read `ctx.locale()`; `CallerContext` still compiles with its now-unread `locale` field.)

- [ ] **Step 9: Commit**

```bash
jj commit -m "refactor(mud-engine): source the locale from the pipeline

Pipeline carries a single per-world locale (default en, driver-set via
with_locale at M1-22); CommandContext borrows it directly so ctx.locale()
is unchanged for built-ins. CallerContext.locale is now unread (removed
next)."
```

---

### Task 4: Remove `locale` from `CallerContext` and fix every call site

**Files:**
- Modify: `crates/mud-engine/src/caller.rs` (remove field, param, accessor, `Locale` import, test assertion)
- Modify: `crates/mud-engine/src/session/resolver.rs` (drop the `Locale::EN` arg, remove `Locale` import)
- Modify: `crates/mud-engine/src/pipeline.rs` (two test resolvers drop the `Locale::EN` arg)
- Modify: `crates/mud-engine/tests/broadcast.rs` (drop arg, remove `Locale` import)
- Modify: `crates/mud-engine/tests/builtins.rs` (drop arg, remove `Locale` import)
- Modify: `crates/mud-engine/tests/command_pipeline.rs` (drop arg, remove `Locale` import)

**Interfaces:**
- Consumes: `Pipeline::with_locale` / `CommandContext` locale from Task 3.
- Produces: `CallerContext::new(session_id, caller, location, name, access)` — the `locale` parameter is gone; `CallerContext::locale` no longer exists.

- [ ] **Step 1: Remove the field, constructor param, and accessor in `caller.rs`**

Delete the import `use mud_i18n::Locale;`.

Remove `locale: Locale,` from the struct and from `new`'s parameter list and body, so:

```rust
#[derive(Debug, Clone)]
#[must_use]
pub struct CallerContext {
    session_id: SessionId,
    caller: EntityId,
    location: PlaceId,
    name: PuppetName,
    access: LockContext,
}

impl CallerContext {
    /// Assembles a caller context from its resolved parts.
    pub fn new(
        session_id: SessionId,
        caller: EntityId,
        location: PlaceId,
        name: PuppetName,
        access: LockContext,
    ) -> Self {
        Self {
            session_id,
            caller,
            location,
            name,
            access,
        }
    }
```

Delete the accessor:

```rust
    /// The locale engine messages for this caller resolve against.
    pub fn locale(&self) -> &Locale {
        &self.locale
    }
```

- [ ] **Step 2: Fix the `caller.rs` unit test**

In `caller_context_exposes_its_parts`, drop the `Locale::EN,` argument and the locale assertion:

```rust
        let ctx = CallerContext::new(
            session(1),
            caller,
            place(10),
            PuppetName::parse("hero").expect("name"),
            LockContext::new().with_perm("admin"),
        );

        assert_eq!(ctx.session_id(), session(1));
        assert_eq!(ctx.caller(), caller);
        assert_eq!(ctx.location(), place(10));
        assert_eq!(ctx.caller_name().as_str(), "hero");
```

(The line `assert_eq!(ctx.locale().as_str(), "en");` is removed.)

- [ ] **Step 3: Fix the real resolver**

In `crates/mud-engine/src/session/resolver.rs`, delete the import `use mud_i18n::Locale;` and drop the `Locale::EN,` line from `CallerContext::new`:

```rust
            caller: CallerContext::new(
                session,
                binding.puppet,
                location,
                binding.name.clone(),
                LockContext::new(),
            ),
```

- [ ] **Step 4: Fix the two in-crate pipeline test resolvers**

In `crates/mud-engine/src/pipeline.rs`, in both `FakeResolver::resolve` and `TwoSessionResolver::resolve`, drop the `Locale::EN,` line from `CallerContext::new` (leaving the `PuppetName::parse(...)` line followed directly by `LockContext::new(),`). Keep the module's `use mud_i18n::Locale;` — it is still used by `Locale::EN` in the Task 3 `with_locale` test.

- [ ] **Step 5: Fix the integration test files**

In `crates/mud-engine/tests/broadcast.rs`: delete `use mud_i18n::Locale;` and change the `CallerContext::new` call to drop the `Locale::EN` argument:

```rust
            caller: CallerContext::new(s, entity, location, name, LockContext::new()),
```

In `crates/mud-engine/tests/builtins.rs`: delete `use mud_i18n::Locale;` and drop the `Locale::EN,` line:

```rust
            caller: CallerContext::new(
                session_id,
                self.caller,
                location,
                mud_account::PuppetName::parse("hero").expect("name"),
                LockContext::new(),
            ),
```

In `crates/mud-engine/tests/command_pipeline.rs`: delete `use mud_i18n::Locale;` and drop the `Locale::EN,` line:

```rust
            caller: CallerContext::new(
                session_id,
                self.caller,
                place(HALL),
                mud_account::PuppetName::parse("hero").expect("name"),
                self.access.clone(),
            ),
```

- [ ] **Step 6: Build the tests to confirm every call site is fixed**

Run: `cargo test -p mud-engine --no-run`
Expected: compiles cleanly — no `Locale::EN` argument-count errors, no unused-import warnings for `mud_i18n::Locale`.

- [ ] **Step 7: Run the full engine suite**

Run: `cargo test -p mud-engine`
Expected: PASS.

- [ ] **Step 8: Full workspace gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all PASS, no warnings.

- [ ] **Step 9: Journal entry**

Append to `.claude/JOURNAL.md` (newest at bottom):

```markdown
## 2026-07-02 — i18n per-world locale rework

- **Spec:** §3.14 (locale is per-tenant, not per-session), §2.8.3.3-.4, §3.20.6.1 — single tenant-configured locale (default `en`); removed per-session resolution, mid-session switching, `mud.i18n.locale_of`, and the `Core.Locale` GMCP message.
- **Done:** Rewrote SPEC §3.14.6 + dependents and PLAN §M2-I/§M3-B/M2-acceptance. Moved the locale from `CallerContext` (per-session, hardcoded `Locale::EN`) to a `Pipeline.locale` field (default `en`, `with_locale` builder for the M1-22 driver); `CommandContext` borrows it so `ctx.locale()` is unchanged for built-ins. Color stays per-account (§3.20.6.1) for accessibility/capability reasons.
- **Verify:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`; new pipeline test asserts output renders in the pipeline locale, not a caller locale.
- **Next:** M1-22 driver sources the tenant locale from tenant config into `Pipeline::with_locale` and the pre-login `render` path (`session/mod.rs` still passes `Locale::EN`). A true cross-locale render test lands with M2-I (second `.ftl` bundle). Deferred: whether the per-account color override is accessibility-scoped or a free theme choice.
```

- [ ] **Step 10: Commit**

```bash
jj commit -m "refactor(mud-engine): drop the per-caller locale field

CallerContext no longer carries a locale; the pipeline's per-world locale
is the single source. Fixes all call sites and the caller unit test."
```

---

## Notes for the implementer

- **Why this is mostly a refactor, not new behavior:** M1 ships only the `en` catalog, so there is no second locale to render differently yet. The change is structural — locale stops being a per-session fact and becomes one per-world value. The strong cross-locale acceptance test (drop a `fr` bundle, see it render) belongs to M2-I when Fluent and a second bundle exist (SPEC §3.14.8.1).
- **Do not touch `crates/mud-engine/src/session/mod.rs`:** the pre-login `render(..., &Locale::EN)` call stays as-is. The pre-login path is fed the tenant locale by the M1-22 driver, exactly like `Pipeline::with_locale`; both default to `en` until then.
- **`t!` and `Catalog` stay keyed by `Locale`.** Only *how the locale is sourced* changes — never how it is looked up.
