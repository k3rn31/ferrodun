# Logging Instrumentation (L0–L5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the workspace logging strategy from `docs/superpowers/specs/2026-07-08-logging-strategy-design.md`: subscriber config, tenant/session span taxonomy, level reclassification, and boundary-crate instrumentation.

**Architecture:** Log at boundaries, stay silent in the domain core. Three nested spans (tenant → session → command) opened by `mudd`/`mud-gateway`/`mud-engine` give every event ambient `tenant`/`world_id`/`session_id` context with no per-call-site plumbing. Levels follow two razors: `info` is a boot/shutdown heartbeat only; `warn` is builder-content faults only — everything operational is `error` if broken, `debug`/`trace` if not.

**Tech Stack:** Rust, `tracing`, `tracing-subscriber` (fmt + env-filter + json), `tracing-test` for assertions, jj for VCS.

## Global Constraints

- `unwrap()` forbidden everywhere; `expect()` only in tests, with a descriptive message.
- No `println!`/`eprintln!`/`dbg!` — the workspace denies `print_stdout`/`print_stderr`.
- Never suppress lints. `cargo clippy --workspace --all-targets` must stay clean after every task.
- Add dependencies with `cargo add` / `cargo add --dev` — never hand-edit `Cargo.toml`.
- **Never log:** passwords, hashes, email, tokens, raw input lines, raw payload bytes, usernames. Never `?`-dump `#[non_exhaustive]` frames/effects — log a variant name or omit the field.
- **Canonical field names:** `tenant`, `world_id`, `session_id`, `command_id`, `command`, `account_id`, `entity`, `place`, `error`. `%` (Display) for IDs and errors; `?` (Debug) only for opaque diagnostic structs.
- **Message style:** lowercase, no trailing punctuation, terse noun/verb phrase.
- Domain crates (`mud-core`, `mud-cmd`, `mud-account`, `mud-session`, `mud-net`, `mud-schema`) get **no** new logging and no `tracing` dependency.
- VCS is **jj**, not git: commit with `jj commit -m "<message>"` (commits the working copy). Do not use `git add`/`git commit`.
- After each task, append a journal entry to `.claude/JOURNAL.md` (format in that file's header; newest at the bottom) and include it in the task's commit.
- Comments explain *why*, in English. Doc comments describe behavior, not implementation.

---

### Task 1 (L0): Subscriber configuration + operator docs

**Files:**
- Modify: `crates/mudd/src/main.rs`
- Modify: `crates/mudd/Cargo.toml` (via `cargo add` only)
- Modify: `docs/docs/running-a-server.md`
- Test: unit tests at the bottom of `crates/mudd/src/main.rs`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `FERRODUN_LOG_FORMAT` env knob (`text` default | `json`); JSON output emits current-span and span-list fields — Task 2's spans rely on this to surface `tenant`/`session_id` on every event. Internal: `enum LogFormat { Text, Json }`, `fn parse_log_format(raw: Option<&str>) -> anyhow::Result<LogFormat>`, `fn init_tracing(format: LogFormat)`.

- [ ] **Step 1: Add the `json` feature to tracing-subscriber**

```bash
cargo add tracing-subscriber --package mudd --features env-filter,json
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/mudd/src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_format_defaults_to_text() {
        let format = parse_log_format(None).expect("default log format must parse");
        assert_eq!(format, LogFormat::Text);
    }

    #[test]
    fn text_and_json_parse() {
        assert_eq!(
            parse_log_format(Some("text")).expect("text must parse"),
            LogFormat::Text
        );
        assert_eq!(
            parse_log_format(Some("json")).expect("json must parse"),
            LogFormat::Json
        );
    }

    #[test]
    fn an_unknown_format_is_a_startup_error() {
        let err = parse_log_format(Some("yaml"))
            .expect_err("unknown log format must fail fast, not silently default");
        assert!(err.to_string().contains("FERRODUN_LOG_FORMAT"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --package mudd parse_log_format -- --nocapture` (or `cargo test -p mudd`)
Expected: COMPILE ERROR — `LogFormat` and `parse_log_format` not found.

- [ ] **Step 4: Implement the subscriber configuration**

Replace the subscriber block in `crates/mudd/src/main.rs` (currently lines 12–17). New content — full `main` plus new items:

```rust
/// Wire format for the process log stream, selected by `FERRODUN_LOG_FORMAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Text,
    Json,
}

/// Parses `FERRODUN_LOG_FORMAT`. Absent means text; anything but
/// `text`/`json` is a startup error — fail fast rather than silently
/// mis-formatting the log stream an aggregator depends on.
fn parse_log_format(raw: Option<&str>) -> anyhow::Result<LogFormat> {
    match raw {
        None | Some("text") => Ok(LogFormat::Text),
        Some("json") => Ok(LogFormat::Json),
        Some(other) => anyhow::bail!(
            "unknown FERRODUN_LOG_FORMAT {other:?} (expected \"text\" or \"json\")"
        ),
    }
}

/// Installs the process-global subscriber. JSON mode emits current-span and
/// span-list fields so the tenant/session/command span taxonomy (design §4)
/// is visible to aggregators; the text formatter shows spans in its prefix.
fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init(),
    }
}

fn main() -> anyhow::Result<()> {
    let format = {
        let raw = std::env::var("FERRODUN_LOG_FORMAT").ok();
        parse_log_format(raw.as_deref())?
    };
    init_tracing(format);

    let cli = Cli::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async_main(cli))
}
```

Keep the existing doc comment on `main` and the existing `use` lines; `EnvFilter` is already imported.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mudd`
Expected: PASS (new tests plus existing suite).

- [ ] **Step 6: Add the operator docs section**

Append to `docs/docs/running-a-server.md`:

```markdown
## Logging

`mudd` writes structured logs to stdout via `tracing`. Two environment
variables control the stream:

| Variable | Values | Default | Effect |
|---|---|---|---|
| `RUST_LOG` | any `tracing` filter (`info`, `debug`, `mud_db=debug`, …) | `info` | Log level filter |
| `FERRODUN_LOG_FORMAT` | `text`, `json` | `text` | Human-readable text or JSON for log aggregators |

What the levels mean:

- **error** — the operator must act: a database or IPC failure, a fatal
  tenant task exit.
- **warn** — world *content* is broken but degraded gracefully: a missing
  i18n key, a bad markup tag, an unknown style role. Fix the content.
- **info** — boot/shutdown heartbeat only. A healthy server is nearly
  silent at `info`: a few lines per tenant at startup, then nothing.
- **debug** — per-session diagnostics: connections, logins, command
  dispatch, IPC handshakes.
- **trace** — the per-tick firehose.

Every line is tagged with the owning `tenant` and, where applicable,
`session_id`, so one tenant's traffic can be filtered out of a
multi-tenant process.
```

- [ ] **Step 7: Verify docs build**

Run: `cd docs && uv run mkdocs build --strict && cd ..`
Expected: clean build (the page is already in `nav`).

- [ ] **Step 8: Verify workspace health, journal, commit**

Run: `cargo clippy --workspace --all-targets` (clean) and `cargo test --workspace` (green).

Append to `.claude/JOURNAL.md`:

```markdown
## 2026-07-08 — L0: log subscriber config (format + span fields)

- **Spec:** design §7 (2026-07-08-logging-strategy) — JSON opt-in, span-field emission, RUST_LOG default info.
- **Done:** `FERRODUN_LOG_FORMAT=text|json` env knob in mudd; json mode emits current-span + span-list; operator Logging section in running-a-server.md.
- **Verify:** `cargo test -p mudd` (parse tests), mkdocs --strict, clippy clean.
- **Next:** L1 tenant/session spans.
```

```bash
jj commit -m "feat(mudd): FERRODUN_LOG_FORMAT knob and span-aware subscriber (L0)"
```

---

### Task 2 (L1): Tenant + session spans, level reclassification, fatal-arm error

**Files:**
- Modify: `crates/mudd/src/boot.rs` (tenant span around both tenant tasks)
- Modify: `crates/mudd/src/world_loop.rs` (reclassify `log_tick_event`, session span on `handle_input`, `session_id` field fix, demote anomaly warns)
- Modify: `crates/mudd/src/main.rs` (fatal-arm `error!`)
- Modify: `crates/mud-gateway/src/lib.rs` (session span around each connection task)
- Modify: `crates/mudd/Cargo.toml`, `crates/mud-gateway/Cargo.toml` (dev-dep via `cargo add --dev` in later tasks as needed; mudd here)
- Test: `#[cfg(test)]` additions in `crates/mudd/src/world_loop.rs`

**Interfaces:**
- Consumes: Task 1's subscriber (span fields visible in output).
- Produces: ambient spans `tenant{tenant: u16, world_id: Display}` wrapping both per-tenant tasks, and `session{session_id: Display}` wrapping gateway connection tasks and world-side input handling. Tasks 3–6 rely on these for context inheritance; none of them add their own tenant/session fields.

- [ ] **Step 1: Add tracing-test to mudd**

```bash
cargo add --dev tracing-test --package mudd
```

- [ ] **Step 2: Write the failing test (level reclassification)**

Append to `crates/mudd/src/world_loop.rs`:

```rust
#[cfg(test)]
mod tests {
    use mud_core::{EntityId, Generation, SlotIndex, TenantTag, TickEvent};
    use tracing_test::traced_test;

    use super::log_tick_event;

    #[test]
    #[traced_test]
    fn entity_creation_logs_at_debug_not_info() {
        let entity = EntityId::new(
            TenantTag::default(),
            SlotIndex::new(1),
            Generation::new(0).expect("generation 0 is in range"),
        );
        log_tick_event(&TickEvent::Created { entity });

        logs_assert(|lines: &[&str]| {
            let created: Vec<_> = lines
                .iter()
                .filter(|line| line.contains("entity created"))
                .collect();
            match created.as_slice() {
                [line] if line.contains("DEBUG") => Ok(()),
                [line] => Err(format!("expected DEBUG, got: {line}")),
                other => Err(format!("expected exactly one line, got {}", other.len())),
            }
        });
    }
}
```

Note: `Generation::new` returns `Result` (20-bit range); if the existing API differs (e.g. `Generation::default()` exists), prefer the simplest constructor the compiler accepts — the entity value is irrelevant to the assertion.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p mudd entity_creation_logs_at_debug_not_info`
Expected: FAIL — the line logs at INFO today.

- [ ] **Step 4: Reclassify `log_tick_event` and demote anomaly warns**

Replace `log_tick_event` in `crates/mudd/src/world_loop.rs` (currently lines 90–109):

```rust
/// Logs one tick event. Precondition failures and rejections are routine
/// gameplay outcomes on the 20 Hz hot path — `trace`, never `warn`, or a
/// blocked action floods the log (design §3). Effect/precondition payloads
/// are omitted: they are `#[non_exhaustive]` and a future variant may carry
/// player text (design §6 never-log rules).
fn log_tick_event(event: &TickEvent) {
    match event {
        TickEvent::Created { entity } => tracing::debug!(?entity, "entity created"),
        TickEvent::PreconditionFailed { .. } => tracing::trace!("tick precondition failed"),
        TickEvent::Rejected { error, .. } => tracing::trace!(%error, "tick effect rejected"),
        // INVARIANT: TickEvent is #[non_exhaustive]; an unknown variant means
        // this build disagrees with itself — an operator-actionable fault.
        _ => tracing::error!("unrecognized tick event"),
    }
}
```

In the same file: change line 77 `tracing::warn!("unexpected mid-stream resume frame dropped")` → `tracing::debug!(...)` (same message); line 170 → `tracing::debug!(session_id = %session, "dispatch for unknown session dropped")`; line 184 → `tracing::debug!(%session_id, "input for unknown session dropped")`.

- [ ] **Step 5: Add the session span on `handle_input`**

In `crates/mudd/src/world_loop.rs`, put the attribute directly above `async fn handle_input` (keep its doc comment above the attribute):

```rust
#[tracing::instrument(name = "session", level = "info", skip_all, fields(session_id = %input.session_id))]
async fn handle_input(
```

- [ ] **Step 6: Add the tenant span in boot**

In `crates/mudd/src/boot.rs`: add `use tracing::Instrument;` to the imports, then replace the two `tasks.spawn(...)` calls (lines 82–86 and 97):

```rust
        // One span per tenant wraps both tasks: every event below — tick
        // events, dispatch warnings, i18n misses — inherits tenant identity
        // ambiently (design §4; SPEC §3.11.2).
        let tenant_span = tracing::info_span!(
            "tenant",
            tenant = tenant_config.tenant_tag().get(),
            world_id = %world_id,
        );
        tasks.spawn(
            {
                let span = tenant_span.clone();
                async move {
                    mud_gateway::serve(listener, gateway_end, gateway_config)
                        .await
                        .map_err(anyhow::Error::from)
                }
                .instrument(span)
            },
        );
```

and for the world task:

```rust
        tasks.spawn(world_loop::run(world_end, world_id, runtime).instrument(tenant_span));
```

(The `world_id` binding already exists earlier in the loop. Keep the intervening code between the two spawns unchanged; only wrap the futures.)

- [ ] **Step 7: Add the per-connection session span in the gateway**

In `crates/mud-gateway/src/lib.rs`: add `use tracing::Instrument;` to imports, and replace the accept arm's spawn (line 80):

```rust
                let span = tracing::info_span!("session", %session_id);
                tokio::spawn(run_connection(socket, session_id, to_router.clone(), limiter).instrument(span));
```

- [ ] **Step 8: Log the fatal arms in main**

In `crates/mudd/src/main.rs`, replace the `join_next` match arms:

```rust
        joined = tasks.join_next() => match joined {
            Some(Ok(Ok(()))) | None => Ok(()),
            Some(Ok(Err(error))) => {
                // `?error`: anyhow's Debug prints the whole context chain;
                // Display would keep only the outermost message.
                tracing::error!(error = ?error, "tenant task failed");
                Err(error)
            }
            Some(Err(join_error)) => {
                tracing::error!(error = %join_error, "tenant task panicked");
                Err(anyhow::anyhow!(join_error)).context("tenant task panicked")
            }
        }
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p mudd && cargo test -p mud-gateway`
Expected: PASS, including the existing `telnet_login` integration test.

- [ ] **Step 10: Verify, journal, commit**

Run: `cargo clippy --workspace --all-targets` (clean), `cargo test --workspace` (green).

Journal entry (same format as Task 1; title `L1: tenant/session spans + level reclassification`, Spec: design §3–§4, Done: tenant span in boot, session spans in gateway + world loop, tick events → trace/debug, fatal-arm error, `session_id` field unification; Verify: mudd level test + workspace suite).

```bash
jj commit -m "feat: tenant and session tracing spans; tick events to trace (L1)"
```

---

### Task 3 (L2): i18n missing-key — tenant inheritance test + once-per-key dedup

**Files:**
- Modify: `crates/mud-i18n/src/translate.rs`
- Test: same file, `tests` mod

**Interfaces:**
- Consumes: Task 2's ambient tenant span (in production; tests create their own span).
- Produces: no API change. `translate` keeps its exact signature; the missing-key warning fires **once per (locale, key) per process**.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` mod in `crates/mud-i18n/src/translate.rs` (note: the dedup guard is process-global, so **every test must use a unique key** — document this with a comment at the top of the tests mod):

```rust
    #[test]
    #[traced_test]
    fn a_missing_key_warning_carries_the_ambient_tenant() {
        let catalog = Catalog::new();

        // In production the tenant span is opened at boot (mudd); §3.14.4.3
        // requires the miss to record key, locale, AND tenant — inherited
        // here, not passed as an argument.
        let span = tracing::info_span!("tenant", tenant = 7u16);
        let _guard = span.enter();
        let _ = translate(
            &catalog,
            &Locale::EN,
            &MessageKey::from_static("inherit.tenant"),
            &[],
        );

        assert!(logs_contain("missing i18n key"));
        assert!(logs_contain("tenant"));
        assert!(logs_contain("7"));
    }

    #[test]
    #[traced_test]
    fn a_repeated_missing_key_warns_only_once() {
        let catalog = Catalog::new();

        for _ in 0..3 {
            let _ = translate(
                &catalog,
                &Locale::EN,
                &MessageKey::from_static("dedup.key"),
                &[],
            );
        }

        logs_assert(|lines: &[&str]| {
            match lines.iter().filter(|line| line.contains("dedup.key")).count() {
                1 => Ok(()),
                n => Err(format!("expected exactly one warning, saw {n}")),
            }
        });
    }
```

- [ ] **Step 2: Run tests to verify the dedup one fails**

Run: `cargo test -p mud-i18n`
Expected: `a_repeated_missing_key_warns_only_once` FAILS (3 warnings today). The tenant-inheritance test should already PASS (span inheritance is subscriber behavior) — it pins the §3.14.4.3 contract.

- [ ] **Step 3: Implement the once-per-key guard**

In `crates/mud-i18n/src/translate.rs`, add above `translate`:

```rust
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Misses already warned about. A missing key sits on the hot render path
/// (every emitted line, every session): one warning per (locale, key) per
/// process, or a single misspelled key floods the log (design §3).
fn warned_misses() -> &'static Mutex<HashSet<(String, String)>> {
    static WARNED: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Emits the §3.14.4.3 structured warning, deduplicated per (locale, key).
/// `tenant` is inherited from the ambient tenant span (design §4), not passed.
fn warn_missing_once(locale: &Locale, key: &MessageKey) {
    let newly_seen = warned_misses()
        .lock()
        .map(|mut seen| seen.insert((locale.as_str().to_owned(), key.as_str().to_owned())))
        // A poisoned lock only ever costs a duplicate warning; prefer that
        // over silencing a mandated signal.
        .unwrap_or(true);
    if newly_seen {
        tracing::warn!(key = %key, locale = %locale, "missing i18n key; falling back to literal key");
    }
}
```

and replace the `unwrap_or_else` closure body in `translate` so the miss path calls it:

```rust
        .unwrap_or_else(|| {
            // A miss is operator-facing telemetry, never shown to the player; the
            // literal key still renders so the message is legible (§3.14.4.3).
            warn_missing_once(locale, key);
            key.as_str()
        });
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-i18n`
Expected: PASS. If `a_missing_key_warns` (existing test, key `missing.key`) now interferes with another test, confirm every test's key is unique — that is the invariant, not test ordering.

- [ ] **Step 5: Verify, journal, commit**

`cargo clippy --workspace --all-targets`, `cargo test --workspace`.

Journal entry: title `L2: i18n missing-key tenant inheritance + dedup`; Spec §3.14.4.3; Done: once-per-(locale,key) guard, tenant-span inheritance pinned by test; Verify: mud-i18n suite.

```bash
jj commit -m "feat(mud-i18n): dedup missing-key warning; pin tenant span inheritance (L2)"
```

---

### Task 4 (L3): mud-db lifecycle + error instrumentation

**Files:**
- Modify: `crates/mud-db/Cargo.toml` (via `cargo add`)
- Modify: `crates/mud-db/src/sqlite/mod.rs`
- Modify: `crates/mud-db/src/sqlite/persistent_world.rs`
- Test: `tests` mod in `crates/mud-db/src/sqlite/mod.rs`

**Interfaces:**
- Consumes: ambient tenant span (production); nothing at compile time.
- Produces: `info` "tenant database ready" once per tenant at open; `#[instrument(err)]` spans on `TenantDb::open`, `TenantDb::world_id`, `PersistentWorld::load` — failures self-log at `error` named by operation. **`tick()` stays uninstrumented** (20 Hz hot path; its failure is fatal and logs at the main fatal arm from Task 2). The per-account methods stay uninstrumented too: `mudd::backend` already `error!`-logs each fault — instrumenting both layers would double-log.

- [ ] **Step 1: Add dependencies**

```bash
cargo add tracing --package mud-db
cargo add --dev tracing-test --package mud-db
```

- [ ] **Step 2: Write the failing test**

Add to the `tests` mod in `crates/mud-db/src/sqlite/mod.rs` (it already uses `TempDir` and an `open_in` helper):

```rust
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn opening_a_tenant_db_logs_readiness_once() {
        let dir = TempDir::new().expect("create tempdir");
        let _db = open_in(&dir).await;

        assert!(logs_contain("tenant database ready"));
    }
```

(Add `use tracing_test::logs_contain;` — or fully qualify — matching the module's import style.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p mud-db opening_a_tenant_db_logs_readiness_once`
Expected: FAIL — nothing is logged today.

- [ ] **Step 4: Instrument `TenantDb` and `PersistentWorld::load`**

In `crates/mud-db/src/sqlite/mod.rs`, replace the body-opening of `open` (keep the doc comment) and add attributes:

```rust
    #[tracing::instrument(level = "debug", skip_all, fields(dir = %data_dir.display()), err)]
    pub async fn open(data_dir: &Path) -> Result<Self, DbError> {
        let path = data_dir.join(DATABASE_FILE);
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new().connect_with(options).await?;
        MIGRATOR.run(&pool).await?;
        // Once per tenant per boot: the design-§3 info heartbeat.
        tracing::info!(db = %path.display(), "tenant database ready");

        Ok(Self { pool })
    }
```

Add to `world_id` (attribute only, body unchanged):

```rust
    #[tracing::instrument(level = "debug", skip_all, err)]
    pub async fn world_id(&self) -> Result<WorldId, DbError> {
```

In `crates/mud-db/src/sqlite/persistent_world.rs`, add to `load` (line 105; attribute only, body unchanged):

```rust
    #[tracing::instrument(level = "debug", skip_all, err)]
    pub async fn load(db: TenantDb, tenant: TenantTag, places: PlaceMap) -> Result<Self, DbError> {
```

Rationale to preserve in review: `err` makes every failure emit an `error` event named by the span (`open`/`world_id`/`load`) with the `DbError` Display — the "query errors at error with the operation name, never bound values" requirement. `DbError` Display is value-free except non-sensitive slugs/ids (audited).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mud-db`
Expected: PASS.

- [ ] **Step 6: Verify, journal, commit**

`cargo clippy --workspace --all-targets`, `cargo test --workspace`.

Journal entry: title `L3: mud-db lifecycle + err instrumentation`; Spec: design §3/§8-L3; Done: info readiness line, instrument(err) on open/world_id/load, tick left bare; Verify: mud-db readiness test.

```bash
jj commit -m "feat(mud-db): boot lifecycle logging and err-instrumented operations (L3)"
```

---

### Task 5 (L4): IPC handshake/framing events + gateway connection lifecycle

**Files:**
- Modify: `crates/mud-ipc/src/handshake.rs`
- Modify: `crates/mud-ipc/src/transport.rs`
- Modify: `crates/mud-ipc/Cargo.toml` (dev-dep via `cargo add --dev`)
- Modify: `crates/mud-gateway/src/lib.rs`
- Modify: `crates/mud-gateway/src/connection.rs`
- Modify: `crates/mud-gateway/Cargo.toml` (dev-dep via `cargo add --dev`)
- Test: `#[cfg(test)]` in `crates/mud-ipc/src/handshake.rs`; `tests` mod in `crates/mud-gateway/src/connection.rs`

**Interfaces:**
- Consumes: session span from Task 2 (gateway events inherit `session_id` in production).
- Produces: `debug` events only — handshake accepted/rejected, frame-size/decode diagnostics (length only, never payload), `connection accepted` (with `peer` IP, the one place it is ever logged), `connection closed` (with `cause`). The existing handshake mismatch `warn!`s are **demoted to `debug`**: the failure is fatal and surfaces as `error` at the main fatal arm (Task 2); the site event is diagnostic detail, and under the design-§3 razor warn is reserved for builder-content faults.

- [ ] **Step 1: Add dev-deps**

```bash
cargo add --dev tracing-test --package mud-ipc
cargo add --dev tracing-test --package mud-gateway
```

- [ ] **Step 2: Write the failing IPC test**

Append to `crates/mud-ipc/src/handshake.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use tracing_test::traced_test;

    use super::*;
    use crate::transport::in_memory_pair;

    #[tokio::test]
    #[traced_test]
    async fn a_successful_handshake_logs_acceptance_at_debug() {
        let (mut gateway, mut world) = in_memory_pair();
        let world_id = WorldId::new(NonZeroU64::new(42).expect("42 is non-zero"));

        let (announced, accepted) = tokio::join!(
            announce_sessions(&mut gateway, world_id, Vec::new()),
            accept_resume(&mut world, world_id),
        );
        announced.expect("gateway side of the handshake succeeds");
        accepted.expect("world side of the handshake succeeds");

        assert!(logs_contain("ipc resume handshake accepted"));
    }
}
```

(If `in_memory_pair` is exported at the crate root rather than `crate::transport`, import it from where `lib.rs` re-exports it — `use crate::in_memory_pair;`.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p mud-ipc a_successful_handshake_logs_acceptance_at_debug`
Expected: FAIL — success is silent today.

- [ ] **Step 4: Instrument the handshake**

In `crates/mud-ipc/src/handshake.rs`:

`announce_sessions` — capture the count before the struct move, log on success, log the silent rejections:

```rust
    let live_count = live_sessions.len();
    let handshake = ResumeHandshake {
        world_id,
        schema_version: SCHEMA_VERSION,
        live_sessions,
    };
    endpoint.send(GatewayFrame::Resume(handshake)).await?;

    match endpoint.recv().await? {
        Some(WorldFrame::ResumeAck(ack)) => {
            check_schema_version(ack.schema_version)?;
            check_world_id(world_id, ack.world_id)?;
            tracing::debug!(%world_id, live_sessions = live_count, "ipc resume handshake accepted");
            Ok(())
        }
        Some(_) => {
            tracing::debug!("ipc resume handshake rejected: unexpected frame");
            Err(IpcError::UnexpectedFrame)
        }
        None => {
            tracing::debug!("ipc resume handshake rejected: peer closed");
            Err(IpcError::PeerClosed)
        }
    }
```

`accept_resume` — mirror it (success after the ack send: `tracing::debug!(%expected_world_id, live_sessions = handshake.live_sessions.len(), "ipc resume handshake accepted")` placed before `Ok(handshake.live_sessions)`; same two rejection debugs on the `Some(_)`/`None` arms). Note the field is `world_id = %expected_world_id` — keep the canonical field name: `tracing::debug!(world_id = %expected_world_id, ...)`.

`check_schema_version` / `check_world_id` — change both `tracing::warn!` to `tracing::debug!`, keeping messages and fields, and add this comment above each:

```rust
    // Debug, not warn: the mismatch propagates as a typed error and becomes a
    // fatal `error` at the boundary; the site event is diagnostic detail
    // (design §3 — warn is reserved for builder-content faults).
```

- [ ] **Step 5: Add framing diagnostics in transport**

In `crates/mud-ipc/src/transport.rs`:

`send` oversize branch:

```rust
        if bytes.len() > MAX_FRAME_BYTES {
            // Length only — the payload may carry credentials (design §6).
            tracing::debug!(size = bytes.len(), max = MAX_FRAME_BYTES, "outbound ipc frame exceeds size cap");
            return Err(IpcError::FrameTooLarge {
                size: Some(bytes.len()),
                max: MAX_FRAME_BYTES,
            });
        }
```

`recv` decode failure:

```rust
            Some(Ok(bytes)) => decode(&bytes).map(Some).map_err(|e| {
                // Length only — never the bytes (design §6).
                tracing::debug!(len = bytes.len(), "inbound ipc frame failed to decode");
                IpcError::Codec(Box::new(e))
            }),
```

`map_inbound_framing_error` oversize branch: add `tracing::debug!(max = MAX_FRAME_BYTES, "inbound ipc frame exceeds size cap");` before the `return`.

- [ ] **Step 6: Write the failing gateway close-log test**

Add to the `tests` mod in `crates/mud-gateway/src/connection.rs`, reusing the existing `spawn_connection` helper:

```rust
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn a_client_hangup_logs_the_close_at_debug() {
        let (client, mut router_rx, task) = spawn_connection(default_limiter());
        // Drain router messages so the connection task never blocks on send.
        tokio::spawn(async move { while router_rx.recv().await.is_some() {} });

        drop(client); // EOF: the client hung up
        task.await.expect("connection task runs to completion");

        assert!(logs_contain("connection closed"));
    }
```

(`logs_contain` is generated in scope by `#[traced_test]`; add `use tracing_test::traced_test;` to the tests-mod imports.)

Run: `cargo test -p mud-gateway a_client_hangup_logs_the_close_at_debug` — expected FAIL.

- [ ] **Step 7: Add gateway accept/close events**

`crates/mud-gateway/src/lib.rs` accept arm — rename `_addr` and log (this replaces line 77; the span/spawn from Task 2 stays):

```rust
                let (socket, addr) = accepted.map_err(GatewayError::Accept)?;
                let session_id = minter.next()?;
                // The peer IP is PII: logged exactly once, at debug, keyed by
                // session_id for on-demand abuse correlation — never at info
                // and never repeated per-frame (design §6).
                tracing::debug!(%session_id, peer = %addr, "connection accepted");
```

`crates/mud-gateway/src/connection.rs` — in `run_connection`, after `cause` is computed (line 70) and before the `match cause`:

```rust
    let cause_label = match cause {
        ExitCause::ClientGone => "client gone",
        ExitCause::WorldClosed => "world closed",
    };
    tracing::debug!(%session_id, cause = cause_label, "connection closed");
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p mud-ipc && cargo test -p mud-gateway`
Expected: PASS.

- [ ] **Step 9: Verify, journal, commit**

`cargo clippy --workspace --all-targets`, `cargo test --workspace`.

Journal entry: title `L4: ipc handshake/framing + gateway connection lifecycle`; Spec: design §3/§6/§8-L4; Done: handshake accept/reject debugs, mismatch warns demoted, frame-size/decode diagnostics (length only), gateway accept (peer IP once at debug) + close events; Verify: new ipc + gateway tests.

```bash
jj commit -m "feat: ipc handshake and gateway connection lifecycle at debug (L4)"
```

---

### Task 6 (L5): Auth outcome events in the session driver

**Files:**
- Modify: `crates/mud-engine/src/session/mod.rs` (`perform`, `apply_terminal`)
- Modify: `crates/mud-engine/Cargo.toml` — check first: `tracing` is already a dependency and `tracing-test` already a dev-dependency; add nothing if so.
- Test: `tests` mod in `crates/mud-engine/src/session/mod.rs` (reuses the existing `FakeBackend`, which authenticates `alice`/`hunter2`)

**Interfaces:**
- Consumes: ambient session span (Task 2) supplies `session_id` in production; these events carry no session field of their own inside `perform`.
- Produces: `debug` events `login authenticated` / `login rejected` / `account registered` / `registration rejected` / `session bound` / `session closed at login`, carrying `account_id` (Display) and `reason` (Debug of a data-free `LoginError`/`RegisterError` variant) — **never** username or password. `BackendError` arms stay silent here: the backend implementation (`mudd::backend`) already `error!`-logs the underlying fault.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` mod in `crates/mud-engine/src/session/mod.rs` (the mod already has `sid`, `account`, and `FakeBackend`):

```rust
    #[tokio::test]
    #[traced_test]
    async fn a_successful_login_logs_the_account_id_and_no_credentials() {
        let mut service = SessionService::new("welcome", Locale::EN);
        let session = sid(9);
        let _ = service.connect(session);
        let _ = service.on_input(session, "alice", &FakeBackend).await;
        let _ = service.on_input(session, "hunter2", &FakeBackend).await;

        assert!(logs_contain("login authenticated"));
        // The never-log rule (design §6): credentials and usernames stay out.
        assert!(!logs_contain("hunter2"));
        assert!(!logs_contain("alice"));
    }

    #[tokio::test]
    #[traced_test]
    async fn a_failed_login_logs_the_rejection_without_the_password() {
        let mut service = SessionService::new("welcome", Locale::EN);
        let session = sid(10);
        let _ = service.connect(session);
        let _ = service.on_input(session, "alice", &FakeBackend).await;
        let _ = service.on_input(session, "wrong-password", &FakeBackend).await;

        assert!(logs_contain("login rejected"));
        assert!(!logs_contain("wrong-password"));
    }
```

Add `use tracing_test::traced_test;` (and `logs_contain` is in scope inside `#[traced_test]` tests) to the tests mod imports.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mud-engine a_successful_login_logs -- --nocapture`
Expected: FAIL — no auth events exist today.

- [ ] **Step 3: Add the auth outcome events**

In `crates/mud-engine/src/session/mod.rs`, `perform` (line 204), replace the `Effect::Authenticate` and `Effect::Register` arms:

```rust
            Effect::Authenticate { username, password } => {
                match backend.authenticate(&username, &password).await {
                    Ok(Ok(account)) => {
                        // account_id only — never the username (design §6).
                        tracing::debug!(account_id = %account.id, "login authenticated");
                        match backend.puppets_of(account.id).await {
                            Ok(puppets) => EffectResult::Authenticated { account, puppets },
                            Err(BackendError) => EffectResult::BackendError,
                        }
                    }
                    Ok(Err(reason)) => {
                        // LoginError variants are data-free; Debug is safe.
                        tracing::debug!(reason = ?reason, "login rejected");
                        EffectResult::LoginRejected(reason)
                    }
                    // The backend impl already error!-logs the fault; a second
                    // event here would double-log it.
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
            Effect::Register { username, password } => {
                match backend.register(&username, &password).await {
                    Ok(Ok(account)) => {
                        tracing::debug!(account_id = %account.id, "account registered");
                        EffectResult::Registered { account }
                    }
                    Ok(Err(reason)) => {
                        tracing::debug!(reason = ?reason, "registration rejected");
                        EffectResult::RegisterRejected(reason)
                    }
                    Err(BackendError) => EffectResult::BackendError,
                }
            }
```

In `apply_terminal`, add events to the two outcomes (bind success and login-phase close):

```rust
                    Some(entity) => {
                        tracing::debug!(session_id = %session, account_id = %account, ?entity, "session bound");
                        self.sessions.insert(
```

and

```rust
            Terminal::Closed => {
                tracing::debug!(session_id = %session, "session closed at login");
                self.sessions.remove(&session);
                true
            }
```

(`account` here is the `AccountId` destructured from `Terminal::Bound`; `AccountId` implements `Display`. Do not log `name` — a `PuppetName` is player-authored.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mud-engine`
Expected: PASS, including the pre-existing session tests.

- [ ] **Step 5: Verify, journal, commit — and close out the track**

`cargo clippy --workspace --all-targets`, `cargo test --workspace`, and from `docs/`: `uv run mkdocs build --strict` (no doc change expected in this task; confirm nothing broke).

Journal entry: title `L5: auth outcome events in the session driver`; Spec: design §3/§6/§8-L5; Done: debug events for authenticate/register/bind/close keyed by account_id, credentials asserted absent by test; Verify: two new mud-engine tests + workspace suite. Next: none — L0–L5 complete; metrics/admin/journal/LLM spans deferred to M6/M7 per design §1.

```bash
jj commit -m "feat(mud-engine): auth outcome events at debug, credential-free (L5)"
```

---

## Self-Review Notes (already applied)

- **Spec coverage:** design §2 (boundary split — no domain crate touched: ✓ no task modifies `mud-core`/`mud-cmd`/`mud-account`/`mud-session`/`mud-net`/`mud-schema` source), §3 (levels — Tasks 2–6), §4 (spans — Task 2), §5 (fields — enforced in every code block), §6 (never-log — comments + negative test assertions), §7 (subscriber — Task 1), §8 (PR table L0–L5 → Tasks 1–6, `mud-world` explicitly excluded per §2). The §6 landmine (`Credential::verify` bool) is documented as out of scope in the spec — no task addresses it, by design.
- **Type consistency:** `parse_log_format`/`LogFormat` only in Task 1; `warned_misses`/`warn_missing_once` only in Task 3; field names `session_id`/`account_id`/`world_id`/`tenant` uniform across tasks.
- **Known judgment calls encoded above:** handshake mismatch warns demoted (Task 5 rationale comment); `?error` for anyhow chains at the fatal arm (comment in Task 2); `tick()`/account methods left uninstrumented to avoid hot-path cost and double-logging (Task 4 interfaces).
