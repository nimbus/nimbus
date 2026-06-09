# Nimbus Full-Codebase Code Review — 2026-06-09
> **Status:** complete · **Method:** two-phase multi-agent review (architecture/seam validation → deep per-subsystem dive) with adversarial verification and independent double-verification of every critical/high finding.
>
> **Audience:** this document is written for a codex/agent mode to *evaluate, plan, and execute* refactors. Every finding carries a stable ID, exact `file:line` anchors, the reasoning behind it, and a concrete fix direction. Findings of **all** severities are included — including low-priority cleanups and nice-to-haves — so nothing is lost in triage.

## Scope & method
- **Codebase:** 27 Rust crates, ~391k source LoC, 7 JS packages. Pre-launch (breaking changes preferred; no back-compat shims).
- **Phase 1 (architecture & seams):** 27 per-crate structural cards + 9 invariant validators + synthesis (36 agents).
- **Phase 2 (deep dive):** 27 subsystem finders → adversarial verifiers (re-read every medium+ claim and tried to refute it) → independent second-skeptic double-verification of every critical/high → synthesis (66 agents).
- **Verification outcome:** false-positives were removed before this report. All **14** critical/high findings were independently double-verified; **0 were disputed**. Severities shown are post-verification (some were downgraded on re-read).

## Confirmed findings (exact counts)
| Severity | Count |
|---|---:|
| Critical | 2 |
| High | 12 |
| Medium | 35 |
| Low | 116 |
| Info | 28 |
| **Total** | **193** |

### By subsystem
| Subsystem | Health | Crit | High | Med | Low | Info | Total |
|---|---|---:|---:|---:|---:|---:|---:|
| Adapters | RED | 2 | 2 | 7 | 16 | 3 | 30 |
| Storage | RED | 0 | 2 | 3 | 18 | 5 | 28 |
| Engine | RED | 0 | 5 | 8 | 13 | 0 | 26 |
| Runtime | RED | 0 | 1 | 5 | 11 | 5 | 22 |
| Server | RED | 0 | 1 | 3 | 14 | 6 | 24 |
| Security | GREEN | 0 | 0 | 0 | 12 | 3 | 15 |
| Trust | GREEN | 0 | 0 | 0 | 4 | 2 | 6 |
| Sandbox | YELLOW | 0 | 0 | 4 | 9 | 1 | 14 |
| CLI | RED | 0 | 1 | 5 | 11 | 2 | 19 |
| Misc | GREEN | 0 | 0 | 0 | 8 | 1 | 9 |

### By dimension
| Dimension | Count |
|---|---:|
| safety | 38 |
| bug | 36 |
| code-smell | 35 |
| test-quality | 17 |
| gap | 17 |
| seam | 16 |
| modularity | 15 |
| optimization | 9 |
| idiomatic-naming | 7 |
| simplification | 3 |

---

# Part I — Analysis & Narrative

## 1. Executive Summary

**Architecture verdict: structurally sound.** Phase 1 confirmed the 27-crate workspace is clean and acyclic, with 7 of 8 invariants holding. The single architectural failure is documentation drift — `ARCHITECTURE.md` leaves ~67% of crates undocumented — not a structural defect. The crate dependency rules (`nimbus-core` zero-I/O, `nimbus-runtime` zero workspace deps), the single mutation path, and storage atomicity all hold at the seams. This is a maintainable, idiomatically-organized codebase; the issues below are localized, not systemic rot.

**Confirmed issue counts (post-verification, false-positives already removed):**

| Severity | Count | Status |
|---|---|---|
| Critical | 2 | both `confirmed` |
| High | 13 | 11 `confirmed`, 2 `downgraded` |
| Medium | 30 | mix of `confirmed`/`downgraded` |
| Low | ~80 | mostly `unverified`/`downgraded` |
| Info | ~30 | positives + documented-by-design notes |

**The single most important thing to fix:** the **MongoDB adapter authentication and authorization bypass (E1-1 / E1-2 / D4-1)**. The SCRAM handshake is decorative — `dispatch()` never reads `conn.authenticated`, and *every* command authorizes to the engine as `PrincipalContext::system()` (god-mode). These are two independent breaks stacked on each other. **Reachability is bounded but real:** the MongoDB listener is an opt-in adapter, not on by default, but when enabled (`construction.rs:177-201` binds and serves) any TCP client gets full unauthenticated CRUD/DDL running with engine authz disabled. The DynamoDB sibling lane already does this correctly (per-request signed auth + loopback guard), so the fix has an in-repo template.

There are no findings indicating data-corruption-at-rest in the core storage engine on the happy path; the storage defects are an open-ended range-scan type leak (A2-1) and key-management hygiene gaps, all fixable without schema change.

---

## 2. Critical & High Findings

Ordered by severity, then blast radius. Auth/authz bypass leads.

### [CRITICAL] MongoDB `dispatch()` never checks `conn.authenticated` — SCRAM auth is decorative
**Where:** `crates/nimbus-mongodb/src/commands/mod.rs:29-66` (also D4-1 at `:43`)
**What/why:** `ConnectionState.authenticated` (`connection.rs:32`, init false) is set true only after SCRAM (`auth.rs:161`), but `dispatch()` routes insert/find/update/delete/findAndModify/count/distinct/aggregate/create/drop/createIndexes by command name alone. The only production reader of the field is `admin.rs:65` (cosmetic `connectionStatus` reporting). The listener (`nimbus-server/src/adapters/mongodb/listener.rs:75`) calls `dispatch()` for every OP_MSG with no gate. **Any TCP client runs all CRUD/DDL without authenticating.** Reachable when the (opt-in) listener is bound. Contrast DynamoDB's per-request signed auth + loopback guard (`construction.rs:209`).
**Fix:** Gate data/DDL arms on `conn.authenticated` (allow only handshake/auth/ping pre-auth), return Unauthorized (code 13) otherwise; make auth mandatory by default.
**Verification:** `confirmed` (D4-1 `downgraded` is the same defect re-filed from the server side — merge with E1-1).

### [CRITICAL] Every MongoDB command authorizes to the engine as `PrincipalContext::system()` — engine authz bypass
**Where:** `crates/nimbus-mongodb/src/commands/crud/filter.rs:149` (plus `crud/mod.rs`, `tenant.rs:24`, `aggregation/mod.rs`)
**What/why:** All MongoDB-originated engine calls pass `PrincipalContext::system()` — a fully-trusted internal principal (`nimbus-core/src/auth/mod.rs`, `authenticated:true`, `sub="system"`). `enforce_mutation_authorization` evaluates `AccessRule.allows()` (`nimbus-core/src/auth/access.rs:71-88`): `require_authenticated` passes trivially, and predicate rules evaluate against system's *synthetic* claims, not the real caller. **This is independent of E1-1** — even after adding an authn gate, every request still runs god-mode and silently misapplies predicate-based table policies.
**Fix:** Derive a real `PrincipalContext` from the authenticated SCRAM identity + tenant; thread it through every engine call. Reserve `system()` for genuinely internal ops.
**Verification:** `confirmed`

### [HIGH] Default MongoDB listener ships hard-coded `admin`/`admin` credentials, no loopback guard
**Where:** `crates/nimbus-mongodb/src/lib.rs:58-62` (E1-5; D4-3 `downgraded` is the same + bind-address angle)
**What/why:** `AuthConfig::default()` returns `admin`/`admin`; `run_listener()` wires exactly that (`listener.rs:16`). Unlike DynamoDB, no `guard_lookup_is_loopback_only` before binding (`construction.rs:177-201`). Combined with E1-2, an attacker who authenticates as `admin/admin` still drives the engine as system.
**Fix:** Remove the `Default` impl (or refuse to start); require explicit credentials; add a loopback guard mirroring DynamoDB.
**Verification:** `confirmed`

### [HIGH] Open-ended single-field range scan returns documents of other JSON types
**Where:** `crates/nimbus-storage/src/index/scan/range.rs:60-73`
**What/why:** `index_scan_range_in_read_txn` byte-compares `encoded_value.cmp(start/end)` with no type-tag check. Type tags sort `Null(0x00) < Bool(0x01) < Number(0x02) < String(0x03)`, so a one-sided numeric lower bound (`age >= 25`, no upper) byte-compares Greater for every string-valued doc. Empirically `encode_index_value("zzz") > encode_index_value(25)`. The planner removes the range filter from `residual_filters` (`planner/range.rs:56-73`), so it is **not re-checked** — cross-type docs leak into results. Two-sided ranges are safe (upper bound caps the type); the leak requires the open-ended bound the planner readily produces (`range.rs:52`).
**Fix:** Reject rows whose encoded value type tag differs from the bound's, or derive a type-bounded upper key when `end` is None. Same guard on the composite range field. (A2-4 fix — route through the bounded-seek helper — eliminates this class structurally.)
**Verification:** `confirmed`

### [HIGH] Direct mutation path bumps `applied_head` before invalidating the document cache (stale read-after-write)
**Where:** `crates/nimbus-engine/src/engine/mutations/direct/store.rs:21-24`
**What/why:** All four direct-path helpers call `mark_applied_head` *inside* the sequence guard but `invalidate_document_cache_for_commit` *after* the guard drops. Readers wait on `applied_head >= durable_head` then immediately read `get_cached_document` (`queries/documents.rs:184-196`). Between the watermark bump and cache invalidation, a reader observes the new watermark and returns a **stale cached document**. The journal path (`journal.rs:283-284`) and execution-unit path (`commit.rs:61-62`) invalidate *before* marking — only the direct path inverts the order. No test guards it.
**Fix:** Invalidate cache (and `materialized_reads`) before `mark_applied_head`, matching `commit.rs`/`journal.rs`. Publish the watermark last.
**Verification:** `confirmed`

### [HIGH] Execution-unit OCC conflict check runs outside the sequence lock — serialization gap
**Where:** `crates/nimbus-engine/src/engine/execution_units/commit.rs:42-55`
**What/why:** `commit()` runs `ensure_schema_unchanged` + `ensure_no_conflicts` (reads commit log from `snapshot_sequence+1`) **outside** `lock_mutation_sequence`; the guard is taken only to apply the batch (`:47`). Two units at the same snapshot touching the same predicate/range/missing-table — or both inserting docs matching the other's predicate — each pass their conflict scan before either appends, then serialize. Document-vs-document conflicts are caught by storage CAS (`store/write/batch.rs`), but predicate/range/insert (phantom) dependencies have **no storage backstop** — a phantom conflict commits silently, breaking OCC serializability.
**Fix:** Move the conflict scan inside the same `lock_mutation_sequence` critical section as the append (re-read commit log under the lock), making scan+append atomic w.r.t. other writers.
**Verification:** `confirmed`

### [HIGH] Pagination cursor signature is plan-dependent — spurious rejection when the plan flips
**Where:** `crates/nimbus-engine/src/evaluator/cursor.rs:87-94`
**What/why:** The cursor signature is `query_signature(query)` over whatever the evaluator receives — the *residual* query on the index path (`prepared.rs:310-322`) but the *full* merged query on the full-scan path (`prepared.rs:323-330`, since `FullScan` returns None at `planner/mod.rs:223`). Schemas/indexes are replaceable at runtime (`persistence/tenant/schema.rs:6`), so a cursor minted under `ExactIndex`/`RangeIndex` and replayed after the index is dropped resolves to `FullScan`, the signature differs, and `decode_cursor` returns `InvalidInput("invalid cursor")` (`cursor.rs:40-42`) for an *identical user query*.
**Fix:** Compute the signature from a plan-independent canonical form (authorized `planned_query` before residual reduction, or a table+order-only signature). (Combine with B2-2: also exclude principal-injected auth filters from the signature.)
**Verification:** `confirmed`

### [HIGH] Lost wakeup in `begin_delete_blocking` — `delete_tenant` can hang forever
**Where:** `crates/nimbus-engine/src/tenant/lifecycle.rs:43-62`
**What/why:** `begin_delete_blocking` waits on the `zero_active` Condvar holding `zero_active_lock`, but `release_operation` (`:43-48`) decrements `active_operations` and calls `zero_active.notify_all()` **without acquiring the lock** — violating the condvar contract. Window: deleter holds lock, reads `active_operations==1` (will wait); before it parks, the last op does `fetch_sub→0` + `notify_all()` with zero parked threads; deleter parks and is never woken. Reachable from `Engine::delete_tenant` (`engine/tenants.rs:125`) with a concurrent in-flight sync op. No test covers delete with concurrent in-flight work.
**Fix:** Acquire `zero_active_lock` in `release_operation` around the read-after-decrement check + notify, so the mutex guards both the predicate transition and the notify.
**Verification:** `confirmed`

### [HIGH] Trigger invocations stuck in `Running` after a crash are never re-enqueued — breaks at-least-once
**Where:** `crates/nimbus-engine/src/engine/mutations/commit_processing.rs:90-98`
**What/why:** The worker durably persists `Running` *before* `executor.execute_invocation`. A crash after that save but before terminal state leaves it `Running`. On restart, `bootstrap_trigger_execution` rebuilds the queue from `Pending`/`RetryPending` only — `Running` matches the `_ => None` arm and is **silently dropped**. The trigger never delivers and never reaches terminal state, breaking the durable at-least-once guarantee.
**Fix:** Treat `Running` (and mid-flight `RetryPending`) as recoverable: re-enqueue at `Timestamp(0)` like `recover_running_jobs` does for scheduled jobs. Owning write: `trigger_execution.rs:225-226`.
**Verification:** `confirmed`

### [HIGH] `matches_simple_filters` silently ignores `Gt/Gte/Lt/Lte` — wrong results on the `_id` fast path
**Where:** `crates/nimbus-mongodb/src/commands/crud/filter.rs:128-132`
**What/why:** Only `Eq`/`Neq` handled; catch-all `_ => true` (`:131`) makes range ops unconditionally match. The `_id` point-lookup fast path (`:151-176`) fetches by id then re-applies remaining filters via this matcher. `{"_id":"u1","age":{"$gt":100}}` returns the doc even when `age <= 100`. The non-`_id` path goes through `engine.query_documents` (correct), so behavior is silently inconsistent only when `_id` equality combines with a range predicate.
**Fix:** Implement `Gt/Gte/Lt/Lte` with a real comparator, or have the `_id` path delegate residual evaluation to engine query logic.
**Verification:** `confirmed`, **double: reconfirmed**

### [HIGH] Bun/JSC linked FFI path drops the watchdog and concurrency permit — no timeout enforcement
**Where:** `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:112-158`
**What/why:** `invoke_program_wrapper_json` destructures with `..`, discarding `watchdog`, `permit`, `context`, then runs the guest via a single blocking synchronous FFI call (`:146-158`) with no time bound. V8 threads `watchdog`/`permit` into `RuntimeInvocationExecution` (`v8/mod.rs:41-66`) so guests are killed on timeout and counted against concurrency. The Bun/JSC path honors only a pre-call cancellation snapshot — a CPU-bound guest that never calls back into the host runs unbounded. **Fail-open execution-limit parity gap** between backends for the same policy.
**Fix:** Hold the permit for the FFI call duration and enforce the timeout (run blocking FFI on a dedicated thread joined with a deadline). At minimum, fail closed by rejecting policies whose `execution_timeout` the linked backend cannot enforce.
**Verification:** `confirmed`

### [HIGH] HTTP-sourced machine image persisted as the bootable disk with no integrity verification
**Where:** `crates/nimbus-bin/src/machine/manager/image.rs:123-273`
**What/why:** `materialize_http_image` downloads a URL and persists it directly as `paths.materialized_image_path` with no size or digest check. The OCI path calls `verify_downloaded_oci_blob` (size + SHA-256) + `check_build_attestation`; the HTTP path has no equivalent and no expected-digest parameter. A corrupted or attacker-substituted download (especially plaintext — see I1-9) becomes the disk the outer VM boots.
**Fix:** Accept an expected SHA-256 (+ size) for HTTP image sources and verify the staged temp file before `persist()`, reusing `verify_downloaded_oci_blob`. Refuse plaintext HTTP for non-loopback hosts.
**Verification:** `confirmed`

> **Two High findings were downgraded on re-verification** — `D4-1` (duplicate of E1-1, kept as the critical) and `B4-2` / `H2-1` (now Medium; treat with the medium cohort). None became DISPUTED.

---

## 3. Medium Findings by Theme

Grouped across crates; duplicates merged.

### Theme: Adapter atomicity & correctness gaps
- **`$push/$pop/$mul/$bit` do read-modify-write into a Patch, losing atomicity (lost-update)** — `crates/nimbus-mongodb/src/commands/crud/update.rs:140-157`. Unlike `$addToSet/$pull` which emit atomic `FieldTransform`s. Express these as engine transforms. *(E1-6, confirmed)*
- **DynamoDB single-item & batch writes can commit data without a stream record** — `crates/nimbus-dynamodb/src/commands/item.rs:170`. Fold the stream-event write into the same `AtomicWriteBatch` as `transact.rs` already does. *(E3-1, downgraded)*
- **Firestore `update_time` write preconditions accepted by the gRPC adapter but rejected deep in the engine as "not executable yet"** — `crates/nimbus-engine/src/engine/execution_units/batch.rs:414-419`; lowered at `grpc/write_stream.rs:644-645`. Either enforce, or reject at the adapter boundary. *(B1-3, confirmed)*

### Theme: Concurrency / liveness in the engine
- **Concurrent overflow fallback can deliver an older snapshot after a newer one (subscription monotonicity)** — `crates/nimbus-engine/src/subscriptions/delivery.rs:100-134`. evaluate→recheck→send→record is not atomic across the worker and the sync fallback. Serialize per-subscription delivery, or gate `try_send` on a final CAS of `last_delivered_sequence`. *(B3-3, confirmed)*
- **Trigger candidate worker drops the rest of a batch on a transient error with no in-process retry** — `crates/nimbus-engine/src/tenant/trigger_candidates.rs:440-477`. Recovery relies solely on restart cursor replay; triggers stall silently for the process lifetime. Re-enqueue unprocessed commits with backoff. *(B3-4, confirmed)*
- **`process_due_jobs_async` abandons remaining claimed jobs when bookkeeping fails** — `crates/nimbus-engine/src/scheduler.rs:214-219`. `?`-propagation on `record_*`/`complete_*` strands the tail of an already-claimed batch. Per-job error isolation. *(B4-3, confirmed)*
- **`block_on` on async backend futures from async handlers (Tokio-worker starvation / panic)** — `crates/nimbus-services/src/manager/registry.rs:71` (+ `handles.rs:27`, `catalog.rs:35`). `teardown_tenant`/`resolve_service_binding`/`service_instances_for_tenant` run async `SandboxBackend` futures via `futures::executor::block_on` on a Tokio worker; the production `ForwardedMachineApiSandboxBackend` uses `spawn_blocking`. Make `teardown_tenant` async; never `block_on` a `spawn_blocking`-backed future on a worker. *(H2-1, downgraded)*

### Theme: Scheduler / cron arithmetic & bounds
- **`CronSchedule::next_after` uses unchecked u64 arithmetic — overflow** — `crates/nimbus-core/src/scheduled.rs:57-62`. `after.0 + seconds*1000` panics (debug) / wraps (release) → busy-loop or never-fires. Sibling already uses `saturating_add` (`scheduler/scheduled_jobs.rs:230`). Use `saturating_add`/`saturating_mul`. *(B4-2, downgraded)*
- **`claim_due_jobs` claims every due job with no batch cap (unbounded per-tenant fanout)** — `crates/nimbus-engine/src/engine/scheduler/scheduled_jobs.rs`. A backlog monopolizes a tenant tick. Add a max-claim batch size. *(B4-4, confirmed)*

### Theme: Runtime capability isolation & confinement
- **Process-global shared worker env map is cross-tenant mutable state** — `crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/env.rs:8`. `NIMBUS_SHARED_WORKER_ENV` static, gated only by name-shape validation, no tenant scoping or capability check; any isolate reads/overwrites another's values. Move into per-runtime `op_state` and gate on an env grant. *(C1-3, confirmed)*
- **`fs` stat(follow_symlink)/`readLink` confine via lexical-only path check, bypassing symlink resolution used by reads** — `crates/nimbus-runtime/.../runtime_local/fs.rs:285`. Metadata-only leak (size/mtime/mode, link target) for a symlink inside a root pointing outside it; inconsistent with canonicalizing read confinement. Canonicalize and re-check the resolved target. *(C1-1, downgraded)*
- **node-compat `require()` loader swallows the Deno permission denial inside Nimbus roots** — `crates/nimbus-runtime/src/node_compat.rs:144-158`. `Err(_) => Ok(Cow::Owned(canonical_path))` demotes the permission gate to advisory for CommonJS reads in the *production* loader. Propagate the `JsErrorBox` or grant the staged roots explicitly. *(C3-2, downgraded)*

### Theme: Crash-safety / key-management hygiene (CLI)
- **redb DEK rotation renames new DB over original before manifest rewrap — bricks on crash** — `crates/nimbus-bin/src/encryption/rotate.rs:475-489`. Crash between rename (`:475`) and manifest write (`:478`) leaves a DB encrypted under `new_dek` but a manifest wrapping `current_dek`. Write+fsync the rewrapped manifest before the rename, or stage both atomically with a recovery marker. *(I1-2, downgraded)*
- **Hand-rolled Unix-socket HTTP client treats read timeout as successful EOF** — `crates/nimbus-bin/src/machine/client.rs:417-424`. WouldBlock/TimedOut breaks the loop as if the body ended cleanly. Distinguish a true `Ok(0)` EOF from timeout and return an explicit error. *(I1-5, confirmed)*

### Theme: Constant-time secret comparison
- **Deploy admin bearer token compared with non-constant-time `!=`** — `crates/nimbus-operator/src/access_policy.rs:106` (filed twice: D2-1 confirmed, I2-1 downgraded — **same site, merge**). Reachable from `http/deploy.rs:32`, which gates staged-bundle execution. Inconsistent with the crate's own `ring::hmac::verify` use (`token.rs:42`, `access.rs:409`). Use `ring::constant_time::verify_slices_are_equal`. *(D2-1/I2-1)*

### Theme: Resource exhaustion / unbounded growth
- **Per-connection MongoDB `CursorStore`/`SessionStore` are unbounded** — `crates/nimbus-mongodb/src/connection.rs:35-36`. No cap or TTL; an (unauthenticated, per E1-1) client grows them for the connection lifetime. Bound counts and/or add idle TTL eviction. *(E1-9, confirmed)*

### Theme: Stringly-typed seam coupling
- **Missing-index detection parses a free-text engine error string by prefix** — `crates/nimbus-firebase/src/errors.rs:113-126`. `strip_prefix("structured query requires an index covering fields: ")` then `split(',')`. A reword silently degrades the violation; field names with commas mis-parse. Surface a typed `Error::MissingIndex { fields }`. *(E2-2, confirmed)*

### Theme: Aggregation efficiency (no pushdown)
- **MongoDB aggregation loads the entire collection with no filter/limit pushdown** — `crates/nimbus-mongodb/src/commands/aggregation/mod.rs:266`. O(n) per aggregate and amplifies the E1-2 god-mode exposure. Translate a leading `$match`/`$limit` into an engine Query. *(E1-7, confirmed)*

### Theme: Sandbox lifecycle parity & teardown robustness
- **Restart-on-failure implemented for krun but missing in the container backend** — `crates/nimbus-sandbox/src/backends/container/state.rs:199` hardcodes `restart_count: 0`; krun has real `restart_policy_allows_restart`/`restart_backoff_delay` (`krun/.../lifecycle.rs:274,288`). Same `SandboxLifecycleSpec`, divergent behavior. Lift restart policy into a shared lifecycle module. *(H1-3, confirmed)*
- **`teardown_tenant` aborts on first backend stop error — partially-stopped tenant, deletion wedged** — `crates/nimbus-services/src/manager/registry.rs:69`. `?`-early-return before any in-memory cleanup; `delete_tenant` calls it before `delete_tenant_async` (`http/tenants.rs:43-48`), so a single stuck sandbox blocks the whole tenant document deletion. Accumulate errors, best-effort stop all, clear succeeded state, return an aggregate. *(H2-2, confirmed)*

### Theme: Engine performance smell
- **`lock_tenant_load_gate_blocking` busy-waits with `try_lock` + `yield_now`** — `crates/nimbus-engine/src/engine/mod.rs:264-271`. Burns a CPU under contention instead of parking. Use a genuine blocking acquire. *(B4-5, confirmed)*

### Theme: Server-side safety posture
- **Route-family gate fails open (no audit) when `local_server_security` is unconfigured — `shutdown_system` unauthenticated** — `crates/nimbus-server/src/local_server/middleware.rs:117`. `shutdown_system` does not self-authorize; `rotate_local_admin_token` in the same module does (defense-in-depth). Fail closed for destructive route families when `None`, or self-authorize; at minimum audit the fail-open branch. *(D2-2, downgraded)*
- **Deploy artifacts staged in a predictable temp dir with default permissions** — `crates/nimbus-server/src/http/deploy.rs:339`. `temp_dir().join("nimbus-deploy-{pid}-{counter}")` via `create_dir_all` (umask perms), then attacker-influenced `bundle.mjs` written before load. Stage into a `0o700` random-suffix dir under a Nimbus-owned state dir. *(D2-4, unverified)*

### Theme: Modularity — duplicated logic & oversized files
- **Authz + permission-claim scaffolding duplicated near-verbatim across three HTTP modules** — `crates/nimbus-server/src/http/services.rs:1190-1202` (+ `sessions.rs`, `sandboxes.rs`). `PrincipalClass`, `principal_claim_string`, `format_millis_rfc3339`, operator-extraction, `authorize_operator_*_route` each 3×. Extract a shared `http::authz` module (`service_grants.rs` is the precedent). *(D1-2, confirmed)*
- **conmon lifecycle helpers duplicated nearly verbatim across container and krun backends** (~18 free functions) — `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs:320` vs `container/runtime.rs`. This is the structural cause of H1-3. Extract `backends/conmon/{lifecycle,spec_resolve}.rs`. *(H1-4, confirmed)*
- **`render.rs` is a 1630-line god-function building JS via nested `format!`** (incl. `eval(__nimbusEvalSource)` at `:209`) — `crates/nimbus-runtime/.../test_runtime/render.rs`. In the 1500-1999 band with no recorded justification. Decompose by concept or move to versioned template assets. *(C3-3, confirmed)*
- **`records.rs` is a 1062-line multi-domain switchboard over stringly-typed `&str` table sinks** — `crates/nimbus-system/src/records.rs:978`. Over the 1000-line comfort threshold; destination tables are untyped literals. Introduce a typed system-table enum; split into concept-owned modules. *(F2-2, downgraded)*

### Theme: Unenforced invariants on pub fields / unchecked buffers
- **CPU-usage ops write `out[0]`/`out[1]` with no length check on the `#[buffer]` slice (JS-reachable panic)** — `crates/nimbus-runtime/.../worker_threads.rs:350-375` (filed twice: C3-5 confirmed, C1-2 downgraded — **same site**). A short `Float64Array` from JS aborts the isolate. Guard `if out.len() < 2 { return; }`.

### Theme: Live index corruption handling
- **Live index key decode panics on storage corruption instead of returning a typed error** — `crates/nimbus-storage/src/index/keyspace.rs:70-101`. `.expect()` on every trailer part runs per row of every live index scan; the historical path maps the same corruption to `StorageErrorKind::Corruption`. Return `Result`, thread through scan loops. *(A2-3, confirmed)*

### Theme: Storage optimization & dead code
- **Single-field range scan does a full-index scan instead of a bounded seek** — `crates/nimbus-storage/src/index/scan/range.rs:49-84`. Siblings use `scan_documents_for_index_key_bounds_in_read_txn` with a real `range(start..end)`. Fixing this also closes A2-1's type leak. *(A2-4, confirmed)*
- **`rebuild_table_indexes`/`clear_table_indexes` are dead code with a non-atomic 3-transaction pattern** — `crates/nimbus-storage/src/index/maintenance/rebuild.rs:46-98`. Violates the storage-atomicity invariant; no callers. Delete (pre-launch posture). *(A2-6, confirmed)*

### Theme: Firestore admin test coverage
- **No tests cover Firestore admin runtime-extension dispatch or `database_id` validation** — `crates/nimbus-cloud-functions/.../firebase_admin/firestore.rs:79-414`. None of get/set/update/delete branches exercised; the `database_id` asymmetry (E4-4) is unguarded. *(E4-5, confirmed)*

---

## 4. Low-Hanging Quality Wins

Compact checklist of simplification / optimization / idiomatic-naming / dead-code items. Each is a small, isolated edit.

### Dead code / dead dependencies (delete — pre-launch posture favors removal)
- [ ] `crates/nimbus-cloud-functions/Cargo.toml:29` — remove unused `tokio` dependency. *(E4-1)*
- [ ] `crates/nimbus-convex/Cargo.toml:34` — move `tokio` to `[dev-dependencies]` (used only in one `#[tokio::test]`). *(E4-2)*
- [ ] `crates/nimbus-convex/Cargo.toml:37` — delete duplicate `tempfile` `[dev-dependencies]` entry. *(E4-3)*
- [ ] `crates/nimbus-bin/src/compose/mod.rs:81-164` — delete six `#[allow(dead_code)]` compose loader wrappers + `execution.rs:63`. *(I1-3)*
- [ ] `crates/nimbus-node/src/direct_process.rs:270` — delete never-constructed `DirectProcessStatusSnapshot` + lib.rs re-export. *(F2-5)*
- [ ] `crates/nimbus-node/src/direct_process.rs:199` — drop always-`none()` `HostPlatformDependencies` stub (or compute it). *(F2-6)*
- [ ] `crates/nimbus-tenant/src/operator_policy/validation.rs:78-94` — `OperatorRuntimePolicy::validate` is a no-op returning `Ok(())` on both branches with a dead `let _ = workload_key;`. Delete or give it real checks. *(F1-2)*
- [ ] `crates/nimbus-firebase/src/batch_write_request.rs:77` — `_labels` field never binds the `labels` key (camelCase renames to `Labels`); dead. Drop it or `#[serde(skip)]`. *(E2-3)*

### Duplication consolidation (single source of truth)
- [ ] `ensure_database_match` ×4 → shared helper — `nimbus-firebase` (`unary.rs:1318`, `write_stream.rs:615`, `batch_get_request.rs:145`, `commit_request.rs:377`). *(E2-4)*
- [ ] `parse_transaction` base64 decode ×3 → shared — `nimbus-firebase` (`commit_request.rs:347`, `batch_get_request.rs:139`, `run_query_request.rs:192`). *(E2-5)*
- [ ] `special_double_from_firestore` + `firestore_document_name` duplicated verbatim — `nimbus-firebase` (`serializer.rs:343`/`write_stream.rs:901`; `response.rs:108`/`batch_get_request.rs:159`). *(E2-6)*
- [ ] `render_command_failure` ×4 → one superset form — `nimbus-sandbox` (`oci/buildah/render.rs:35`, `container/runtime.rs:1378`, `krun/vm/lifecycle.rs:462`, `oci/network.rs:914`). *(H1-5)*
- [ ] `now_millis` ×3 + `next_*_version` pattern → `manager/clock.rs` — `nimbus-services` (`definitions.rs:550`, `sandboxes.rs:309`, `sessions.rs:387`). *(H2-3)*
- [ ] `map_join_error`/`map_permit_error` ×4 with divergent messages → shared mapper — `nimbus-storage` (`async_storage/helpers.rs:3`, `libsql/backend.rs:807`, `postgres/backend.rs:1424`, `mysql/backend.rs:288`). *(A4-4)*
- [ ] `storage_health_diagnostic_with_retention_config` ×5 → `impl_storage_health_diagnostic!` macro (mirror `impl_changefeed_journal!`) — `nimbus-storage/diagnostics.rs:628-815`. *(A3-6)*
- [ ] Order-preserving number transform duplicated live vs historical → one `order_preserving_number_bits` in `nimbus-core` — `nimbus-storage` (`encoding.rs:10-25`, `index_history.rs:21-26`). *(A2-9)*
- [ ] Subscription/trigger worker bodies duplicated `#[cfg(test)]` vs `#[cfg(not(test))]` → single body taking `pause: Option<...>` — `nimbus-engine` (`subscription_delivery/worker.rs:129-206`, `trigger_candidates.rs:417-530`). *(B3-5)*
- [ ] Durable-journal-suffix read loop duplicated → call the existing `read_durable_journal_suffix_to_sequence_async` — `nimbus-engine` (`verification.rs:25-44` vs `snapshot.rs:41-61`). *(B2-4)*
- [ ] Convex adapter reimplements bridge document capability helpers → route through `nimbus_bridge::capabilities::*` (as cloud-functions does) — `nimbus-server/.../convex/host_bridge/db_ops/documents.rs:20-120`. *(J-3)*

### API shape / idiomatic naming
- [ ] Replace `(Option<&Value>, bool inclusive)` pairs with `std::ops::Bound<&Value>` (removes 6 `too_many_arguments` allows, 30 occurrences) — `nimbus-storage/index/scan/adapters.rs:55-77`. *(A2-5)*
- [ ] `PlanCandidate::score()` returns an unlabeled 4-tuple → named `PlanScore` struct with explicit `Ord` — `nimbus-engine/.../planner/scoring.rs:15-22`. *(B2-7)*
- [ ] Empty-`Vec` sentinel for empty composite-range result → explicit enum/`Option` — `nimbus-storage/index/bounds.rs:9-29`. *(A2-8)*
- [ ] `$group` keys via `format!("{:?}")` → structured hashable key type — `nimbus-mongodb/.../aggregation/mod.rs:457`. *(E1-8)*
- [ ] Rename stale `service: &Engine` param to `engine` — `nimbus-bridge/.../intersection/commit.rs:6`. *(J-4)*
- [ ] Two distinct public `EmbeddedAsset` structs in `nimbus-assets` → `UiAsset`/`PackageAsset` — `js_packages.rs:65` vs `ui.rs:11`. *(I2-6)*
- [ ] Sibling parse methods return `String` vs `Error` → unify on domain `Error` — `nimbus-machine/lib.rs:155` vs `:200`. *(I2-7)*
- [ ] Redundant `RuntimeFlavor::CurrentThread | _` arm → bare `_` — `nimbus-runtime/executor/invoke.rs:41`. *(C2-4)*
- [ ] `finish_failed_start` is a thin pass-through → inline into its one caller — `nimbus-runtime/.../cooperative/execution.rs:69`. *(C2-6)*
- [ ] Rename `async_storage/helpers.rs` → `task_error.rs` (repo discourages `helpers.rs`) — *(A4-5)*

### Efficiency / simplification
- [ ] `existing_system_started_at_async` eager `unwrap_or(unix_time_millis()?)` always calls the clock → `unwrap_or_else` — `nimbus-system/records.rs:751`. *(F2-8)*
- [ ] Redundant `NEXT_SEQUENCE_KEY` re-write + non-saturating increment → single owner, `saturating_add` both — `nimbus-storage/store/journal.rs:666` & `:189`. *(A1-2)*
- [ ] Redundant document-read branch in subscription base-query synth (two identical broad pushes) → delete one — `nimbus-bridge/.../subscriptions.rs:83-89`. *(J-5)*
- [ ] Redundant `GeneratedDatabaseKey` zeroize attrs (`#[zeroize(drop)]` + `#[derive(ZeroizeOnDrop)]`) → drop the legacy one — `nimbus-storage/encryption/key.rs:14-25`. *(A3-7)*
- [ ] Dead readiness `||` branch (`service_binding_from_handle().is_some()` unreachable unless `Ready`) → key on `status == Ready` — `nimbus-services/.../activation.rs:60`. *(H2-5)*
- [ ] `canceled_invocations` aggregate omits disconnect/explicit cancellations → roll them up + add a sum test — `nimbus-runtime/metrics/global.rs:240-248`. *(C3-6)*
- [ ] Watchdog polls all external-cancellation registrations every 10ms (O(n)/tick) → event-driven notify — `nimbus-runtime/watchdog.rs:282`. *(C2-3)*
- [ ] `verify_scaffold_contract` runs a full lifecycle sim on every Bun/JSC invoke → once at pool construction — `nimbus-runtime/.../bun_jsc/mod.rs:95`. *(C3-9)*
- [ ] Per-read engine `table_id` lookup on every recorded read → cache `TableId` per `TableName` per invocation — `nimbus-bridge/lib.rs:225-233`. *(J-7)*
- [ ] O(n²) linear-scan dedup on read-set inserts → `HashSet`/`LinkedHashSet` if these grow — `nimbus-bridge/.../read_set.rs:108-172`. *(J-8)*
- [ ] AWS KMS sync/async bridge spawns a thread per call → async-end-to-end or `block_in_place`; propagate inner error — `nimbus-storage/encryption/aws_kms.rs:274-296`. *(A3-8)*

---

## 5. Test-Quality Observations

The codebase has strong test discipline in places (the Convex cancellation suite asserts behavior + empty-table state, D3-5; the storage simulation harness is genuinely assertion-heavy across model-vs-actual / PITR / CDC / crash-replay, A3-10). The gaps below are where coverage is absent or accidental:

- **No mixed-type or negative-number range-scan tests; the existing range test passes only by accident** — `crates/nimbus-storage/src/index/tests.rs:451-489`. `index_scan_range_on_numbers` inserts only numerics, so the open-ended scan returns the correct count solely because no string/bool/null docs exist — exactly what would have caught A2-1. Add a mixed-type single-field-index range test + a negative/positive span. *(A2-2, confirmed)*
- **No test covers cursor stability across a plan change (the B2-1 path)** — `crates/nimbus-engine/src/evaluator/tests.rs:572-620`. The existing test only asserts the *intended* rejection (order direction changes), never the *spurious* one (index dropped between pages). *(B2-5, confirmed)*
- **Overflow monotonicity test never exercises the concurrent fallback it names** — `crates/nimbus-engine/src/tenant/subscription_delivery/tests.rs:141-263`. The worker is paused, so the true seq-N-vs-seq-M race (B3-3) is never created. *(B3-6)*
- **MongoDB listener tests cover only ping/unknown/legacy-opcode — no data or auth coverage** — `crates/nimbus-server/src/adapters/mongodb/listener.rs:184`. The legacy-opcode test accepts Ok/Err/timeout interchangeably. No insert/find round-trip, no unauthenticated-rejection test — *this gap is why the E1-1 auth bypass went unnoticed*. *(D4-6, D2-3)*
- **`nimbus-machine` has zero tests despite owning parsing/env/fs logic** — `crates/nimbus-machine/src/lib.rs:154`. `MachineImageSource::parse`, `MachineVolume::parse`, XDG resolution, `ensure_directories` all untested at the crate level. *(I2-3, confirmed)*
- **MySql tenant-read `check_fault` is a no-op — silently disables fault injection on that backend** — `crates/nimbus-engine/src/persistence/tenant/reads.rs:10`. Reliability tests routed through MySql are false-green. *(B4-6)*
- **WS session/pending concurrency modules have no in-module unit tests** — `crates/nimbus-server/src/ws/socket/pending.rs:17-69`. The trickiest concurrency (link/finish/cancel state machine) is pure and trivially unit-testable but only touched by integration happy-paths. *(D1-7)*
- **No test asserts the external-policy worker is bounded under a hung backend** — `crates/nimbus-tenant/src/operator_policy/external.rs:338-352`. The natural place to lock in the F1-1 fix. *(F1-5)*
- **Range planner bound-subsumption and Neq-residual logic lack direct unit tests** — `crates/nimbus-engine/.../planner/range.rs:99-168`. *(B2-6)*
- **No negative/escape-path coverage for OCI layer materialization** (symlink escape, `..`, opaque-whiteout-over-symlink) — `crates/nimbus-sandbox/.../oci/materializer.rs:846`. This is the rootfs confinement boundary. *(H1-8)*

---

## 6. Per-Subsystem Scorecard

| Subsystem | Health | Top issue |
|---|---|---|
| **Adapters** | 🔴 Red | MongoDB auth + authz bypass with default `admin`/`admin` (E1-1/E1-2/E1-5); `_id`-fast-path wrong results (E1-3) |
| **Engine** | 🟡 Yellow | Stale read-after-write on direct path (B1-1) + OCC phantom-conflict gap (B1-2) + condvar lost-wakeup hang (B3-1) |
| **Storage** | 🟡 Yellow | Open-ended range scan leaks cross-type docs (A2-1); key-file collisions weaken per-subject isolation (A3-1) |
| **Runtime** | 🟡 Yellow | Bun/JSC FFI drops watchdog/permit — no timeout (C3-1); cross-tenant shared env map (C1-3) |
| **Server** | 🟢 Green (with notes) | Non-constant-time deploy bearer compare (D2-1); fail-open shutdown gate when security unconfigured (D2-2). Auth seams otherwise solid. |
| **Sandbox** | 🟡 Yellow | conmon lifecycle duplicated → restart-policy drift (H1-3/H1-4); teardown wedges tenant deletion (H2-2) |
| **Security** (nimbus-tenant/system/node) | 🟡 Yellow | External-policy worker thread leak under hung backend (F1-1); no-op `validate` misleads (F1-2) |
| **Trust** (nimbus-core/artifacts) | 🟢 Green | `Timestamp::now()` panics on pre-1970 clock (G-3); provenance gate no-ops when unconfigured (G-5, by design) |
| **CLI** (nimbus-bin/operator/machine) | 🟡 Yellow | HTTP machine image has no integrity check (I1-1) + loopback prefix-match downgrade to plaintext (I1-9) |
| **Misc** (bridge/testing) | 🟢 Green | Async vs sync atomic-write-batch behavior divergence (J-1); convex adapter reimplements bridge helpers (J-3) |

---

## 7. Recommended Remediation Order

Security → correctness → modularity/quality. Effort is net-new LoC + tests (repo SWAG convention).

1. **MongoDB auth/authz lockdown (E1-1, E1-2, E1-5, D4-3).** Gate `dispatch()` on `conn.authenticated`; thread a real `PrincipalContext` derived from SCRAM identity + tenant; remove the `admin`/`admin` default; add a loopback guard. Add the missing listener integration tests (D4-6). This is the headline and the only Red subsystem. **~400-600 LoC + ~150 LoC tests.**
2. **Engine correctness invariants (B1-1, B1-2, B3-1).** Reorder cache-invalidate-before-watermark on the direct path; move the OCC conflict scan inside the sequence lock; fix the condvar lost-wakeup. All three are silent-corruption / hang risks with no current test coverage. **~150 LoC + ~120 LoC tests** (the tests are the larger half — A2-2, B2-5, B3-6 patterns).
3. **Durability / at-least-once gaps (B4-1, E3-1, B3-4, B4-3).** Re-enqueue `Running` triggers on bootstrap; fold DynamoDB stream events into the data batch; per-job/per-commit error isolation in the scheduler and trigger workers. **~250 LoC + ~150 LoC tests.**
4. **Storage range-scan type leak (A2-1 + A2-4 together).** Route single-field range through the bounded-seek helper, which closes both the correctness leak and the full-scan inefficiency. Add mixed-type + negative-number tests. **~120 LoC + ~80 LoC tests.**
5. **Runtime execution-limit parity & isolation (C3-1, C1-3).** Enforce timeout/permit on the Bun/JSC FFI path (or fail closed); move the shared worker-env map into per-runtime state. **~200 LoC + ~80 LoC tests.**
6. **Constant-time secret compares + machine-image integrity (D2-1/I2-1, I1-1, I1-9).** Drop-in `ring::constant_time` for the deploy bearer; add expected-SHA-256 to the HTTP image source; exact-match loopback parsing. **~80 LoC + ~60 LoC tests.**
7. **Sandbox lifecycle consolidation (H1-4 → H1-3, H2-2).** Extract `backends/conmon/lifecycle.rs`, then make the container backend honor restart policy; make teardown best-effort. **~300 LoC moved/shared + ~120 LoC tests.**
8. **Remaining Medium modularity (D1-2, C3-3, F2-2) and the Low-hanging checklist (Section 4).** Mostly mechanical extraction and deletion; high value-per-line, low risk. Batch the dead-code deletions first (pre-launch posture). **~600-800 LoC net reduction across deletions + extractions.**

**Doc-drift note (Phase-1 carryover):** update `ARCHITECTURE.md` to cover the ~67% undocumented crates — the one outstanding architectural invariant failure, orthogonal to the code fixes above.

---

Relevant absolute paths for the highest-priority fixes:
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb/src/commands/mod.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb/src/commands/crud/filter.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb/src/lib.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine/src/engine/mutations/direct/store.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine/src/engine/execution_units/commit.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine/src/tenant/lifecycle.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-storage/src/index/scan/range.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/backends/bun_jsc/linked.rs`
- `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-bin/src/machine/manager/image.rs`

---

# Part II — Complete Findings Ledger

Every confirmed finding, all severities. IDs are stable (subsystem-prefixed). Use the index to navigate; each entry below has full reasoning, an exact location, a fix direction, and (for re-verified items) the evidence gathered on independent re-read.

## Index

| ID | Sev | Dim | Subsystem | Title | Location |
|---|---|---|---|---|---|
| `E1-1` | critical | safety | Adapters | dispatch() never checks conn.authenticated; SCRAM auth is purely advisory | `crates/nimbus-mongodb/src/commands/mod.rs:29-66` |
| `E1-2` | critical | seam | Adapters | Every command authorizes to the engine as PrincipalContext::system(), bypassing engine authz | `crates/nimbus-mongodb/src/commands/crud/filter.rs:149` |
| `E1-3` | high | bug | Adapters | matches_simple_filters silently ignores Gt/Gte/Lt/Lte, returning wrong results on the _id fast path | `crates/nimbus-mongodb/src/commands/crud/filter.rs:128-132` |
| `E1-5` | high | safety | Adapters | Default listener ships hard-coded admin/admin credentials | `crates/nimbus-mongodb/src/lib.rs:58-62` |
| `A2-1` | high | bug | Storage | Open-ended single-field range scan returns documents of other JSON types | `crates/nimbus-storage/src/index/scan/range.rs:60-73` |
| `A2-2` | high | test-quality | Storage | No mixed-type or negative-number range-scan tests; existing range test passes only by accident | `crates/nimbus-storage/src/index/tests.rs:451-489` |
| `B1-1` | high | bug | Engine | Direct mutation path bumps applied_head before invalidating the document cache (stale read-after-write) | `crates/nimbus-engine/src/engine/mutations/direct/store.rs:21-24` |
| `B1-2` | high | bug | Engine | Execution-unit OCC conflict check runs outside the sequence lock, leaving a serialization gap for predicate/range/insert dependencies | `crates/nimbus-engine/src/engine/execution_units/commit.rs:42-55` |
| `B2-1` | high | bug | Engine | Pagination cursor signature is plan-dependent, causing spurious rejection when the query plan flips | `crates/nimbus-engine/src/evaluator/cursor.rs:87-94` |
| `B3-1` | high | bug | Engine | Lost wakeup in begin_delete_blocking: condvar notified without holding the guarding mutex | `crates/nimbus-engine/src/tenant/lifecycle.rs:43-62` |
| `B4-1` | high | gap | Engine | Trigger invocations in Running state are never re-enqueued after a crash | `crates/nimbus-engine/src/engine/mutations/commit_processing.rs:90-98` |
| `C3-1` | high | gap | Runtime | Bun/JSC linked FFI path drops the watchdog and concurrency permit — no timeout enforcement | `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:112-158` |
| `D4-1` | high | safety | Server | MongoDB data plane has no authentication enforcement — SCRAM handshake is decorative | `crates/nimbus-mongodb/src/commands/mod.rs:43` |
| `I1-1` | high | safety | CLI | HTTP-sourced machine image is persisted as the bootable disk with no integrity verification | `crates/nimbus-bin/src/machine/manager/image.rs:123-273` |
| `E1-6` | medium | bug | Adapters | $push/$pop/$mul/$bit do read-modify-write into a Patch, losing atomicity under concurrency | `crates/nimbus-mongodb/src/commands/crud/update.rs:140-157` |
| `E1-7` | medium | optimization | Adapters | Aggregation loads the entire collection with no filter/limit pushdown | `crates/nimbus-mongodb/src/commands/aggregation/mod.rs:266` |
| `E1-9` | medium | safety | Adapters | Per-connection CursorStore and SessionStore are unbounded | `crates/nimbus-mongodb/src/connection.rs:35-36` |
| `E2-2` | medium | seam | Adapters | Missing-index detection parses a free-text engine error string by prefix | `crates/nimbus-firebase/src/errors.rs:113-126` |
| `E3-1` | medium | bug | Adapters | Single-item and batch writes can commit data without a stream record | `crates/nimbus-dynamodb/src/commands/item.rs:170` |
| `E3-2` | medium | code-smell | Adapters | Six more findings (sidecar, auth-docs, full-scan, 3 low smells) | `crates/nimbus-dynamodb/src/attribute_value.rs:5` |
| `E4-5` | medium | test-quality | Adapters | No tests cover Firestore admin runtime-extension dispatch or database_id validation | `crates/nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs:79-414` |
| `A2-3` | medium | safety | Storage | Live index key decode panics on storage corruption instead of returning a typed error | `crates/nimbus-storage/src/index/keyspace.rs:70-101` |
| `A2-4` | medium | optimization | Storage | Single-field range scan does a full-index scan instead of a bounded seek | `crates/nimbus-storage/src/index/scan/range.rs:49-84` |
| `A2-6` | medium | modularity | Storage | rebuild_table_indexes / clear_table_indexes are dead code with a non-atomic transaction pattern | `crates/nimbus-storage/src/index/maintenance/rebuild.rs:46-98` |
| `B1-3` | medium | gap | Engine | update_time write preconditions are accepted by the Firestore gRPC adapter but rejected as 'not executable yet' | `crates/nimbus-engine/src/engine/execution_units/batch.rs:414-419` |
| `B2-5` | medium | test-quality | Engine | No test covers cursor stability across a plan change (the B2-1 defect path) | `crates/nimbus-engine/src/evaluator/tests.rs:572-620` |
| `B3-3` | medium | bug | Engine | Concurrent overflow fallback can deliver an older snapshot after a newer one (monotonicity window) | `crates/nimbus-engine/src/subscriptions/delivery.rs:100-134` |
| `B3-4` | medium | gap | Engine | Trigger candidate worker drops the rest of a batch on a transient error with no in-process retry | `crates/nimbus-engine/src/tenant/trigger_candidates.rs:440-477` |
| `B4-2` | medium | bug | Engine | CronSchedule::next_after uses unchecked u64 arithmetic and can overflow | `crates/nimbus-core/src/scheduled.rs:57-62` |
| `B4-3` | medium | bug | Engine | process_due_jobs_async abandons remaining claimed jobs when bookkeeping fails | `crates/nimbus-engine/src/scheduler.rs:214-219` |
| `B4-4` | medium | optimization | Engine | claim_due_jobs claims every due job with no batch cap (unbounded per-tenant fanout) | `crates/nimbus-engine/src/engine/scheduler/scheduled_jobs.rs` |
| `B4-5` | medium | code-smell | Engine | lock_tenant_load_gate_blocking busy-waits with try_lock + yield_now | `crates/nimbus-engine/src/engine/mod.rs:264-271` |
| `C1-1` | medium | seam | Runtime | fs stat(follow_symlink) and readLink confine via lexical-only path check, bypassing symlink resolution used by reads | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/fs.rs:285` |
| `C1-3` | medium | seam | Runtime | Process-global shared worker env map is cross-tenant mutable state | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/env.rs:8` |
| `C3-2` | medium | safety | Runtime | node-compat require() loader silently bypasses the Deno permission check inside Nimbus-owned roots | `crates/nimbus-runtime/src/node_compat.rs:144-158` |
| `C3-3` | medium | modularity | Runtime | render.rs is a 1630-line single god-function building JS via nested format! macros | `crates/nimbus-runtime/src/runtime/bootstrap/ops/test_runtime/render.rs:1-1630` |
| `C3-5` | medium | bug | Runtime | CPU-usage ops write out[0]/out[1] with no length check on the #[buffer] slice | `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:350-375` |
| `D1-2` | medium | modularity | Server | Authorization + permission-claim scaffolding duplicated near-verbatim across three http modules | `crates/nimbus-server/src/http/services.rs:1190-1202` |
| `D2-1` | medium | safety | Server | Deploy admin bearer token uses non-constant-time comparison | `crates/nimbus-operator/src/access_policy.rs:106` |
| `D4-3` | medium | safety | Server | MongoDB ships weak default admin/admin credentials and no loopback guard on its bind address | `crates/nimbus-mongodb/src/lib.rs:58` |
| `H1-3` | medium | gap | Sandbox | Restart-on-failure implemented for krun backend but missing in container backend despite shared conmon lifecycle | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/container/state.rs:199` |
| `H1-4` | medium | modularity | Sandbox | conmon lifecycle helpers duplicated nearly verbatim across container and krun backends | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs:320` |
| `H2-1` | medium | bug | Sandbox | Sync registry/catalog methods call futures::executor::block_on on async backend futures, blocking a Tokio worker from an async handler | `crates/nimbus-services/src/manager/registry.rs:71` |
| `H2-2` | medium | gap | Sandbox | teardown_tenant aborts on first backend stop error, leaving partially-stopped tenant and blocking tenant deletion | `crates/nimbus-services/src/manager/registry.rs:69` |
| `I1-2` | medium | bug | CLI | redb DEK rotation renames new DB over original before manifest is rewrapped, bricking on crash | `crates/nimbus-bin/src/encryption/rotate.rs:475-489` |
| `I1-5` | medium | bug | CLI | Hand-rolled Unix-socket HTTP client treats read timeout as successful EOF | `crates/nimbus-bin/src/machine/client.rs:417-424` |
| `I1-7` | medium | code-smell | CLI | Triplicated persistence-config field boilerplate across three parallel structs | `crates/nimbus-bin/src/start/config.rs:99-160,571-709` |
| `I2-1` | medium | safety | CLI | Deploy admin bearer token compared with non-constant-time `!=` | `crates/nimbus-operator/src/access_policy.rs:106` |
| `I2-3` | medium | test-quality | CLI | nimbus-machine has zero tests despite owning parsing, env, and fs logic | `crates/nimbus-machine/src/lib.rs:154` |
| `E1-10` | low | bug | Adapters | compare_json_values collapses NaN and mixed numeric/type ordering to Equal | `crates/nimbus-mongodb/src/commands/crud/filter.rs:249-258` |
| `E1-11` | low | test-quality | Adapters | No test covers the _id-plus-range-operator leniency or the NaN update panic | `crates/nimbus-mongodb/src/commands/crud/tests.rs:1` |
| `E1-8` | low | code-smell | Adapters | $group keys are stringly-typed via format!("{:?}") | `crates/nimbus-mongodb/src/commands/aggregation/mod.rs:457` |
| `E2-1` | low | bug | Adapters | Timestamp precision is inconsistent: document/precondition times truncate to milliseconds while field-value timestamps keep nanoseconds | `crates/nimbus-firebase/src/grpc/write_stream.rs:920-942` |
| `E2-3` | low | bug | Adapters | BatchWrite `_labels` serde field is mis-cased and never binds the incoming `labels` key | `crates/nimbus-firebase/src/batch_write_request.rs:77` |
| `E2-4` | low | modularity | Adapters | ensure_database_match duplicated across four request/grpc modules | `crates/nimbus-firebase/src/grpc/unary.rs:1318` |
| `E2-5` | low | modularity | Adapters | parse_transaction (base64 transaction-token decode) duplicated across three request modules | `crates/nimbus-firebase/src/commit_request.rs:347` |
| `E2-6` | low | simplification | Adapters | special_double_from_firestore (with unreachable! arm) and firestore_document_name duplicated verbatim | `crates/nimbus-firebase/src/grpc/write_stream.rs:901-911` |
| `E2-7` | low | safety | Adapters | Mutex lock poisoning is converted to panic across the write and listen stream registries | `crates/nimbus-firebase/src/grpc/write_stream.rs:66` |
| `E4-1` | low | code-smell | Adapters | nimbus-cloud-functions declares tokio but never uses it | `crates/nimbus-cloud-functions/Cargo.toml:29` |
| `E4-2` | low | code-smell | Adapters | tokio is a production dependency of nimbus-convex but used only in one test | `crates/nimbus-convex/Cargo.toml:34` |
| `E4-4` | low | gap | Adapters | Firestore admin GET ignores database_id while writes validate it | `crates/nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs:203-225` |
| `E4-6` | low | bug | Adapters | Index read-bound narrowing always replaces on incomparable values | `crates/nimbus-convex/src/subscriptions/transforms/bounds.rs:35` |
| `E4-7` | low | bug | Adapters | Numeric index-bound comparison loses precision via as_f64 | `crates/nimbus-convex/src/subscriptions/transforms/bounds.rs:7-10` |
| `E4-8` | low | safety | Adapters | unreachable! in apply_builtin_transform is reachable via crate-public API | `crates/nimbus-convex/src/subscriptions/transforms/runtime_backed/builtins.rs:34-39` |
| `E4-9` | low | code-smell | Adapters | Lock-poison policy diverges within the convex crate (panic vs recover) | `crates/nimbus-convex/src/subscriptions/transforms/state.rs:12` |
| `A1-1` | low | safety | Storage | unsafe MoveFileExW block lacks a SAFETY comment, breaking the crate's own convention | `crates/nimbus-storage/src/encryption/manifest.rs:550` |
| `A1-2` | low | code-smell | Storage | redb sequence allocation: non-saturating increment plus a redundant NEXT_SEQUENCE re-write | `crates/nimbus-storage/src/store/journal.rs:666` |
| `A2-5` | low | code-smell | Storage | Boolean inclusive-flag pair should be std::ops::Bound | `crates/nimbus-storage/src/index/scan/adapters.rs:55-77` |
| `A2-7` | low | bug | Storage | Historical exclusive-start fallback inverts intent when prefix_end is None | `crates/nimbus-storage/src/index/history_scan.rs:195-202` |
| `A2-8` | low | code-smell | Storage | Empty-Vec sentinel for empty composite range result is fragile | `crates/nimbus-storage/src/index/bounds.rs:9-29` |
| `A2-9` | low | code-smell | Storage | Order-preserving number transform duplicated across live and historical encoders | `crates/nimbus-storage/src/index/encoding.rs:10-25` |
| `A3-1` | low | bug | Storage | KeyDirectoryProvider sanitization collides distinct key subjects onto one key file | `crates/nimbus-storage/src/encryption/key_directory.rs:61-65` |
| `A3-2` | low | bug | Storage | GeneratedDatabaseKey::into_wrapped zeroizes a copy, leaving the original plaintext un-zeroed | `crates/nimbus-storage/src/encryption/key.rs:48-60` |
| `A3-3` | low | safety | Storage | Plaintext DEK escapes zeroize protection as a bare [u8;32] at the runtime boundary | `crates/nimbus-storage/src/encryption/runtime.rs:241` |
| `A3-4` | low | safety | Storage | KeyDirectoryProvider wrapping key is never zeroized | `crates/nimbus-storage/src/encryption/key_directory.rs:68-92` |
| `A3-5` | low | bug | Storage | backend_feature_support ignores its backend argument; all backends report identical feature support | `crates/nimbus-storage/src/diagnostics.rs:364` |
| `A3-6` | low | modularity | Storage | Five copy-pasted storage_health_diagnostic_with_retention_config impls across store types | `crates/nimbus-storage/src/diagnostics.rs:628-815` |
| `A3-7` | low | code-smell | Storage | GeneratedDatabaseKey has redundant zeroize attributes | `crates/nimbus-storage/src/encryption/key.rs:14-25` |
| `A3-8` | low | code-smell | Storage | AWS KMS sync/async bridge spawns a thread to call block_on inside a tokio context | `crates/nimbus-storage/src/encryption/aws_kms.rs:274-296` |
| `A4-2` | low | seam | Storage | Scan pushdown probe hand-parses msgpack with a magic length gate coupled to Document serde layout | `crates/nimbus-storage/src/store/scan.rs:176` |
| `A4-3` | low | code-smell | Storage | execute_cancellable read path calls handle.abort() which is a no-op for in-flight blocking work and leaves the task running | `crates/nimbus-storage/src/async_storage/read.rs:117` |
| `A4-4` | low | modularity | Storage | map_join_error / map_permit_error duplicated across four storage backends with divergent messages | `crates/nimbus-storage/src/async_storage/helpers.rs:3` |
| `A4-6` | low | test-quality | Storage | Freshness barrier concurrency (wait_for_background_refresh + synchronous fallback) has no targeted unit test | `crates/nimbus-storage/src/libsql.rs:614` |
| `B1-4` | low | code-smell | Engine | Atomic-write apply calls engine.now() multiple times per write, producing inconsistent timestamps within one commit | `crates/nimbus-engine/src/engine/execution_units/batch.rs:142-189` |
| `B1-5` | low | seam | Engine | stage_write resolves table_id from live store while reads resolve from the execution-unit snapshot | `crates/nimbus-engine/src/engine/execution_units/staging.rs:174` |
| `B1-6` | low | safety | Engine | expect_immediate_result / expect_scheduled_applied panic via unreachable! on a wrong-variant result | `crates/nimbus-engine/src/engine/mutations/direct/types.rs:73-91` |
| `B1-7` | low | bug | Engine | Journal batch invalidates cache and dispatches subscription/trigger work for records beyond the actually-applied head on partial-apply error recovery | `crates/nimbus-engine/src/engine/mutations/journal.rs:276-284` |
| `B2-2` | low | seam | Engine | Cursor signature embeds principal-derived access filters, coupling pagination continuity to the principal | `crates/nimbus-engine/src/engine/queries/authorization.rs:37-45` |
| `B2-3` | low | safety | Engine | Sort comparators panic via .expect() relying on a non-local validation invariant | `crates/nimbus-engine/src/evaluator/ordering.rs:16` |
| `B2-4` | low | modularity | Engine | Duplicated durable-journal-suffix read loop across snapshot.rs and verification.rs | `crates/nimbus-engine/src/engine/queries/verification.rs:25-44` |
| `B2-6` | low | test-quality | Engine | Range planner bound-subsumption and Neq-residual logic lack direct unit tests | `crates/nimbus-engine/src/engine/queries/planner/range.rs:99-168` |
| `B2-7` | low | code-smell | Engine | PlanCandidate::score() returns an unlabeled 4-tuple that fights readability of the priority ordering | `crates/nimbus-engine/src/engine/queries/planner/scoring.rs:15-22` |
| `B3-5` | low | modularity | Engine | Subscription/trigger worker bodies duplicated between #[cfg(test)] and #[cfg(not(test))] | `crates/nimbus-engine/src/tenant/subscription_delivery/worker.rs:129-206` |
| `B3-6` | low | test-quality | Engine | Overflow monotonicity test never exercises the concurrent fallback it claims to guard | `crates/nimbus-engine/src/tenant/subscription_delivery/tests.rs:141-263` |
| `B3-7` | low | seam | Engine | affected_subscription_ids is fully public on a re-exported registry, leaking an internal dispatch hook | `crates/nimbus-engine/src/subscriptions/dependencies.rs:67` |
| `B4-6` | low | test-quality | Engine | MySql tenant-read check_fault is a no-op, silently disabling fault injection on that backend | `crates/nimbus-engine/src/persistence/tenant/reads.rs:10` |
| `C1-2` | low | bug | Runtime | CPU-usage ops index out[0]/out[1] with no buffer-length check (JS-reachable panic) | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:350` |
| `C1-4` | low | safety | Runtime | unsafe FFI blocks in worker CPU-usage helpers lack SAFETY comments | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:637` |
| `C1-6` | low | gap | Runtime | op_nimbus_runtime_exec_path returns host executable path unconditionally | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/bootstrap.rs:20` |
| `C2-1` | low | bug | Runtime | queued-invocation metric leaks on semaphore-closed error paths in permit acquire | `crates/nimbus-runtime/src/executor/admission/permit.rs:115` |
| `C2-2` | low | gap | Runtime | cooperative worker abandons in-flight/parked slots on shutdown without draining results | `crates/nimbus-runtime/src/worker_loop/cooperative/run.rs:58` |
| `C2-3` | low | optimization | Runtime | watchdog polls every external cancellation registration on a 10ms tick (O(n) per tick) | `crates/nimbus-runtime/src/watchdog.rs:282` |
| `C2-4` | low | code-smell | Runtime | redundant RuntimeFlavor match arm `CurrentThread \| _` | `crates/nimbus-runtime/src/executor/invoke.rs:41` |
| `C3-4` | low | safety | Runtime | worker_threads FFI unsafe blocks lack SAFETY comments, inconsistent with the rest of the subsystem | `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:639-759` |
| `C3-6` | low | bug | Runtime | canceled_invocations aggregate omits disconnect and explicit cancellations | `crates/nimbus-runtime/src/metrics/global.rs:240-248` |
| `C3-7` | low | safety | Runtime | Host-bridge FFI callback writes *output_len before the capacity check | `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:241-251` |
| `C3-8` | low | code-smell | Runtime | Raw post-message ops swallow channel send errors via .ok() | `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:137-139` |
| `D1-1` | low | bug | Server | HostCallCancellation::cancelled() has a missed-wakeup TOCTOU reachable from WS unsubscribe | `crates/nimbus-runtime/src/host.rs:641-646` |
| `D1-3` | low | safety | Server | Lock-poison policy diverges within the same crate (WS .expect vs AppState recover) | `crates/nimbus-server/src/ws/socket/pending.rs:20-21` |
| `D1-4` | low | bug | Server | Socket writer kills the whole connection on a single serialization failure | `crates/nimbus-server/src/ws/socket/transport.rs:102-105` |
| `D1-5` | low | code-smell | Server | AuthError and Authenticated server messages collapse into ambiguous V2 wire shapes | `crates/nimbus-server/src/protocol.rs:323-326` |
| `D1-6` | low | bug | Server | Deploy handler performs blocking std::fs I/O on the async runtime thread | `crates/nimbus-server/src/http/deploy.rs:337-415` |
| `D1-7` | low | test-quality | Server | WS session/pending concurrency modules have no in-module unit tests | `crates/nimbus-server/src/ws/socket/pending.rs:17-69` |
| `D2-2` | low | seam | Server | Route-family gate fails open (no audit) when local_server_security is unconfigured, leaving shutdown_system unauthenticated | `crates/nimbus-server/src/local_server/middleware.rs:117` |
| `D2-3` | low | test-quality | Server | No negative-auth or fail-open test for the destructive shutdown route | `crates/nimbus-server/src/tests/local_admin.rs:74` |
| `D2-4` | low | safety | Server | Deploy artifacts staged in a predictable temp dir with default permissions | `crates/nimbus-server/src/http/deploy.rs:339` |
| `D2-5` | low | safety | Server | validate_origin passes when Origin header is absent and never validates Host for non-UI routes | `crates/nimbus-operator/src/access_policy.rs:248` |
| `D3-1` | low | safety | Server | Pervasive lock-poison `.expect` is a deliberate-but-unguarded panic policy | `crates/nimbus-server/src/adapters/convex/host_bridge/read_tracking/builders.rs:8` |
| `D3-2` | low | code-smell | Server | Read-tracking is recorded pre-execution for plain queries but post-execution for paginated queries | `crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct/invocation.rs:251` |
| `D4-4` | low | code-smell | Server | Misleading Status::unimplemented("Not yet implemented") guard in Firestore gRPC adapter | `crates/nimbus-server/src/adapters/firebase/grpc/mod.rs:96` |
| `D4-5` | low | safety | Server | unsafe libc::gethostname FFI call lacks a SAFETY comment | `crates/nimbus-server/src/system/version_check.rs:359` |
| `F1-1` | low | safety | Security | External policy worker thread leaks on timeout / disconnect (never joined or cancelled) | `crates/nimbus-tenant/src/operator_policy/external.rs:326-352` |
| `F1-2` | low | code-smell | Security | OperatorRuntimePolicy::validate is a no-op that reads as if it enforces runtime policy | `crates/nimbus-tenant/src/operator_policy/validation.rs:78-94` |
| `F1-3` | low | seam | Security | Lenient vs strict principal-claim checks are an easy-to-misuse asymmetry on the tenant boundary | `crates/nimbus-tenant/src/context.rs:164-197` |
| `F1-4` | low | gap | Security | validate_host accepts embedded port / userinfo / brackets, only rejecting unspecified wildcards | `crates/nimbus-tenant/src/operator_policy/validation.rs:353-368` |
| `F2-1` | low | gap | Security | systemd job wait has no timeout and can hang forever | `crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs:90` |
| `F2-2` | low | modularity | Security | records.rs is a 1062-line multi-domain switchboard over stringly-typed table sinks | `crates/nimbus-system/src/records.rs:978` |
| `F2-3` | low | code-smell | Security | Three-way system-tenant guard asymmetry across subscription record functions | `crates/nimbus-system/src/records.rs:619` |
| `F2-4` | low | bug | Security | stable_key_segment can collide distinct identifiers into one document id | `crates/nimbus-system/src/keys.rs:3` |
| `F2-5` | low | code-smell | Security | DirectProcessStatusSnapshot is dead public surface | `crates/nimbus-node/src/direct_process.rs:270` |
| `F2-6` | low | code-smell | Security | HostPlatformDependencies is an always-none capability stub | `crates/nimbus-node/src/direct_process.rs:199` |
| `F2-7` | low | optimization | Security | delete-stale and port-cleanup paths do full O(n) table scans per operation | `crates/nimbus-system/src/records.rs:712` |
| `F2-8` | low | safety | Security | existing_system_started_at_async eagerly evaluates a fallible fallback inside unwrap_or | `crates/nimbus-system/src/records.rs:751` |
| `G-1` | low | safety | Trust | NumericValue::projected_json / into_stored_value panic on constructible non-finite Double and are dead public API | `crates/nimbus-core/src/typed_scalar.rs:194` |
| `G-2` | low | code-smell | Trust | Redundant two-branch provenance check is brittle even though it is currently correct | `crates/nimbus-artifacts/src/admission.rs:132` |
| `G-3` | low | safety | Trust | Timestamp::now() panics the process on a pre-1970 system clock | `crates/nimbus-core/src/types.rs:403` |
| `G-4` | low | gap | Trust | Verifier-output redaction is keyword-line based and leaks unlabeled secret values | `crates/nimbus-artifacts/src/lib.rs:559` |
| `H1-2` | low | bug | Sandbox | Bridge gateway computation overflows last octet (debug panic / release wraparound) for operator subnet ending in .255 | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/network.rs:523` |
| `H1-5` | low | code-smell | Sandbox | render_command_failure copied 4x across modules | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/buildah/render.rs:35` |
| `H1-6` | low | safety | Sandbox | IPv6 SSRF classification misses NAT64 (64:ff9b::/96) and IPv4-compatible (::a.b.c.d) embedded-IPv4 ranges | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/egress.rs:618` |
| `H1-8` | low | test-quality | Sandbox | No negative/escape-path coverage for OCI layer materialization (symlink escape, whiteout edge cases) | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/materializer.rs:846` |
| `H2-3` | low | code-smell | Sandbox | now_millis duplicated verbatim across three modules; next_*_version share an unextracted pattern | `crates/nimbus-services/src/manager/definitions.rs:550` |
| `H2-4` | low | modularity | Sandbox | Same-leaf-name module trap: production manager/definitions.rs vs test manager/tests/definitions.rs | `crates/nimbus-services/src/manager.rs:150` |
| `H2-5` | low | simplification | Sandbox | Dead readiness branch: service_binding_from_handle().is_some() can never be true unless status==Ready | `crates/nimbus-services/src/manager/activation.rs:60` |
| `H2-6` | low | seam | Sandbox | close_session/get_session are unscoped by tenant; correctness relies entirely on caller-side tenant checks | `crates/nimbus-services/src/manager/sessions.rs:149` |
| `H2-7` | low | bug | Sandbox | Force delete can stop the backend then fail the post-stop generation re-check, yielding a stopped-but-undeleted service | `crates/nimbus-services/src/manager/definitions.rs:289` |
| `I1-10` | low | code-smell | CLI | unreachable!() in duration_unit_nanos couples to a separately-maintained unit list | `crates/nimbus-bin/src/compose/file/parse.rs:279-289` |
| `I1-3` | low | modularity | CLI | Dead compose loader wrappers retained behind #[allow(dead_code)] | `crates/nimbus-bin/src/compose/mod.rs:81-164` |
| `I1-4` | low | safety | CLI | .expect() on the live per-tenant service-backend request path | `crates/nimbus-bin/src/compose/file/lower.rs:196-218` |
| `I1-6` | low | safety | CLI | DEK plaintext key material not zeroized in rotation paths | `crates/nimbus-bin/src/encryption/rotate.rs:465-466,515-516,699-711` |
| `I1-8` | low | safety | CLI | Missing SAFETY comments on production unsafe (libc kill, from_raw_fd) | `crates/nimbus-bin/src/machine/manager/stop.rs:428,439` |
| `I1-9` | low | bug | CLI | is_loopback_registry uses prefix match, downgrading lookalike hosts to plaintext HTTP | `crates/nimbus-bin/src/machine/manager/image.rs:487-491` |
| `I2-2` | low | safety | CLI | Session issuance panics on RNG/format failure inside an HTTP handler | `crates/nimbus-operator/src/access.rs:266` |
| `I2-4` | low | seam | CLI | "Record/contract" crate performs live env + fs I/O and ships a hard-coded /tmp fallback in a public constructor | `crates/nimbus-machine/src/lib.rs:49` |
| `I2-5` | low | bug | CLI | Session cookie validated against unsigned payload before signature verification | `crates/nimbus-operator/src/access.rs:198` |
| `I2-6` | low | idiomatic-naming | CLI | Two distinct public `EmbeddedAsset` structs in one crate | `crates/nimbus-assets/src/js_packages.rs:65` |
| `I2-7` | low | code-smell | CLI | Sibling parse methods return different error types (`String` vs `Error`) | `crates/nimbus-machine/src/lib.rs:200` |
| `J-1` | low | gap | Misc | Async atomic write batch fails closed where the sync path silently falls back to a fresh execution unit | `crates/nimbus-bridge/src/capabilities.rs:163-178` |
| `J-2` | low | gap | Misc | validate_host_call_session never binds the incoming token to the session — only rejects empty strings | `crates/nimbus-bridge/src/state.rs:54-68` |
| `J-3` | low | modularity | Misc | Convex adapter reimplements the bridge's document capability helpers instead of delegating | `crates/nimbus-server/src/adapters/convex/host_bridge/db_ops/documents.rs:20-120` |
| `J-4` | low | idiomatic-naming | Misc | Stale `service` parameter name for an `Engine` in commit-intersection helper | `crates/nimbus-bridge/src/read_tracking/intersection/commit.rs:6` |
| `J-5` | low | simplification | Misc | Redundant document-read branch in subscription base-query synthesis | `crates/nimbus-bridge/src/read_tracking/subscriptions.rs:83-89` |
| `J-6` | low | gap | Misc | Absent-document reads are not tracked as dependencies in the bridge get path | `crates/nimbus-bridge/src/capabilities.rs:96-100` |
| `J-7` | low | optimization | Misc | Per-read engine table_id lookup on every recorded document read | `crates/nimbus-bridge/src/lib.rs:225-233` |
| `J-9` | low | test-quality | Misc | WebSocket fixture conflates stream-closed and timeout, producing misleading panic messages | `crates/nimbus-testing/src/websocket_fixture.rs:143-149` |
| `E2-8` | info | idiomatic-naming | Adapters | Aggregation result encoder is asymmetric between gRPC and REST surfaces (parallel codecs, both correct) | `crates/nimbus-firebase/src/grpc/unary.rs:1030-1039` |
| `E2-9` | info | modularity | Adapters | unary.rs is a 1328-line composition-root switchboard mixing handler dispatch with many inline lowering helpers | `crates/nimbus-firebase/src/grpc/unary.rs:1041-1064` |
| `E4-3` | info | code-smell | Adapters | tempfile listed in both [dependencies] and [dev-dependencies] of nimbus-convex | `crates/nimbus-convex/Cargo.toml:37` |
| `A1-3` | info | seam | Storage | Schema cache refresh happens after COMMIT becomes visible (all SQL backends) | `crates/nimbus-storage/src/sqlite/write.rs:801` |
| `A1-4` | info | gap | Storage | Batch Update upserts but never removes a resource-path binding (by design — confirm with a test) | `crates/nimbus-storage/src/store/write/batch.rs:250` |
| `A3-10` | info | test-quality | Storage | Simulation harness is confirmed wired into real, assertion-heavy coverage | `crates/nimbus-storage/src/tests/generated_history.rs:1` |
| `A3-9` | info | code-smell | Storage | Encryption profiling uses eprintln + ad hoc env vars on the resolve path | `crates/nimbus-storage/src/encryption/runtime.rs:232-254` |
| `A4-5` | info | idiomatic-naming | Storage | async_storage/helpers.rs is a util-bucket name discouraged by repo modularity rules | `crates/nimbus-storage/src/async_storage/helpers.rs:1` |
| `C1-5` | info | safety | Runtime | Extension-transpile path panics/unwraps instead of returning an error | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/transpile.rs:163` |
| `C1-7` | info | seam | Runtime | Only CtxServiceLookup is grant-checked in-runtime; all other host ops trust host-side enforcement | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs:181` |
| `C2-5` | info | code-smell | Runtime | choose_worker reads load and last_assigned_sequence as a non-atomic tuple for tie-break | `crates/nimbus-runtime/src/executor/queue/router.rs:131` |
| `C2-6` | info | idiomatic-naming | Runtime | finish_failed_start is a thin pass-through to finish_invocation with no added behavior | `crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs:69` |
| `C3-9` | info | optimization | Runtime | verify_scaffold_contract runs a full lifecycle simulation on every Bun/JSC invocation | `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs:95` |
| `D2-6` | info | code-smell | Server | LocalAdminTokenRecord::authorize uses an HMAC-of-self construction where a direct constant-time compare would be clearer | `crates/nimbus-operator/src/token.rs:39` |
| `D3-3` | info | code-smell | Server | Subscription key builders panic via `.expect` on serde serialization of internally-built values | `crates/nimbus-server/src/adapters/convex/subscriptions/socket/mod.rs:284` |
| `D3-4` | info | seam | Server | Raw query/mutation HTTP surface accepts arbitrary client-supplied queries/mutations gated only by engine access policy | `crates/nimbus-server/src/adapters/convex/handlers/function_routes/queries.rs:75` |
| `D3-5` | info | test-quality | Server | Cancellation tests assert observable no-op behavior, a good pattern worth preserving | `crates/nimbus-server/src/adapters/convex/tests/cancellation.rs:47` |
| `D4-6` | info | test-quality | Server | MongoDB listener tests cover only ping/unknown/legacy-opcode — no data or auth coverage | `crates/nimbus-server/src/adapters/mongodb/listener.rs:184` |
| `D4-7` | info | optimization | Server | Artifact-verifier process runner writes all stdin before reading stdout (latent pipe deadlock pattern) | `crates/nimbus-server/src/artifact_verifier_effects/process.rs:42` |
| `F1-5` | info | test-quality | Security | No test asserts the external-policy worker is bounded under a hung backend | `crates/nimbus-tenant/src/operator_policy/external.rs:338-352` |
| `F2-10` | info | safety | Security | emulator_principal_from_bearer trusts unsigned JSON claims (intentional, document the gate) | `crates/nimbus-auth/src/lib.rs:69` |
| `F2-9` | info | safety | Security | object_fields panics via unreachable!() on non-object payloads | `crates/nimbus-system/src/records.rs:1026` |
| `G-5` | info | seam | Trust | Provenance gate silently no-ops when no RuntimeBundleProvenanceConfig is configured | `crates/nimbus-server/src/execution/invocations/provenance.rs:21` |
| `G-6` | info | idiomatic-naming | Trust | Binary subtype field is captured but silently dropped in projected_json and never validated | `crates/nimbus-core/src/typed_scalar.rs:42` |
| `H1-7` | info | safety | Sandbox | process.rs unsafe FFI liveness blocks lack SAFETY comments | `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/process.rs:1` |
| `I1-11` | info | gap | CLI | sandbox_supervisor is a documented validation-only stub with packet enforcement hardcoded off | `crates/nimbus-bin/src/sandbox_supervisor.rs:86` |
| `I2-8` | info | idiomatic-naming | CLI | `nimbus-operator` name overloads the Kubernetes "operator" term | `crates/nimbus-operator/src/lib.rs:1` |
| `J-8` | info | optimization | Misc | O(n^2) linear-scan dedup on read-set vector inserts | `crates/nimbus-bridge/src/read_tracking/read_set.rs:108-172` |

## Critical (2)

#### `E1-1` — dispatch() never checks conn.authenticated; SCRAM auth is purely advisory
**Severity:** critical · **Dimension:** safety · **Subsystem:** Adapters · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-mongodb/src/commands/mod.rs:29-66`

**Finding.** ConnectionState carries an `authenticated: bool` (connection.rs:32, initialized false at connection.rs:44, set true only in auth.rs after successful SCRAM). But dispatch() (commands/mod.rs:29-66) routes insert/find/update/delete/findAndModify/count/distinct/aggregate/create/drop/createIndexes/etc. by command name alone, with no read of `conn.authenticated`. The only reader of that field is admin.rs (reporting). The server listener (nimbus-server/src/adapters/mongodb/listener.rs:75) calls dispatch() for every parsed OP_MSG with no auth gate. Net result: any client that can open a TCP connection can run all CRUD/DDL without ever issuing saslStart/saslContinue. Authentication is decorative.

**Fix direction.** Gate the data/DDL arms of dispatch() on conn.authenticated (allowing only handshake/auth/ping pre-auth), returning Unauthorized (code 13) otherwise. Pre-launch: make auth mandatory by default.

**Verification evidence.** Re-read commands/mod.rs:20-67 (dispatch) — routes by command_name alone; insert/find/update/delete/findAndModify/count/distinct/aggregate/create/drop/createIndexes are reachable with no read of conn.authenticated. Re-read connection.rs:32,44 (authenticated:bool, init false) and grepped readers: the field is read ONLY in admin.rs:65 (connection_status reporting) and set true only in auth.rs:161. Re-read listener.rs:64-88 (handle_connection) — calls commands::dispatch for every parsed OP_MSG with no auth gate. SCRAM is fully implemented but never enforced as a precondition for data commands. Any TCP client can run all CRUD/DDL without saslStart. Critical authentication bypass confirmed.

#### `E1-2` — Every command authorizes to the engine as PrincipalContext::system(), bypassing engine authz
**Severity:** critical · **Dimension:** seam · **Subsystem:** Adapters · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-mongodb/src/commands/crud/filter.rs:149`

**Finding.** All MongoDB-originated engine calls pass PrincipalContext::system() as the principal: query_documents (crud/filter.rs:149), the CRUD writers in crud/mod.rs and update/delete, tenant resolution (tenant.rs:24), aggregation loads (aggregation/mod.rs load_initial_documents), and session/transaction paths. PrincipalContext::system() is defined in nimbus-core/src/auth/mod.rs as a fully-trusted internal principal (authenticated:true, claims sub="system"). The engine's enforce_mutation_authorization evaluates AccessRule.allows() (nimbus-core/src/auth/access.rs:71-88): require_authenticated passes trivially for system, and any claim-predicate rules are evaluated against system's synthetic claims rather than the real caller. So external MongoDB traffic both bypasses require_authenticated and silently misapplies predicate-based table access policies. This is an authorization bypass independent of E1-1 — even after adding an authn gate, every request still runs with engine god-mode.

**Fix direction.** Derive a real PrincipalContext from the authenticated SCRAM identity (and tenant) and thread it through every engine call instead of PrincipalContext::system(). Reserve system() for genuinely internal operations.

**Verification evidence.** Re-read crud/filter.rs:149 (let principal = PrincipalContext::system()) used by query_documents, and aggregation/mod.rs:272 (same in load_initial_documents). Re-read nimbus-core/src/auth/mod.rs:35-43 — system() returns authenticated:true with synthetic claims {sub:"system"}. Re-read access.rs:71-88 (AccessRule::allows): require_authenticated && !principal.authenticated is false for system (passes trivially), and predicates evaluate against system's synthetic claims. Confirmed the engine actually enforces these: enforce_mutation_authorization (mutations/authorization.rs:15-36) and query authorization.rs:53 both call rule.allows. Net: every MongoDB-originated request runs with engine god-mode, bypassing require_authenticated and misapplying predicate policies. Independent of E1-1 and equally severe.


## High (12)

#### `E1-3` — matches_simple_filters silently ignores Gt/Gte/Lt/Lte, returning wrong results on the _id fast path
**Severity:** high · **Dimension:** bug · **Subsystem:** Adapters · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-mongodb/src/commands/crud/filter.rs:128-132`

**Finding.** matches_simple_filters (crud/filter.rs:123-138) handles only Eq and Neq; the catch-all arm `_ => true` (line 131) makes Gt/Gte/Lt/Lte unconditionally match. This is reachable: query_documents takes an `_id` point-lookup fast path (filter.rs:151-176) that fetches the document by id, then re-applies the remaining (non-_id) filters via matches_simple_filters at line 172. A query like `{ "_id": "u1", "age": { "$gt": 100 } }` returns the document even when age <= 100, because the $gt predicate is treated as always-true. The same filters on the non-_id general path go through engine.query_documents (which honors them), so behavior is inconsistent and silently wrong only when an _id equality is combined with a range predicate.

**Fix direction.** Implement Gt/Gte/Lt/Lte (and unsupported ops) in matches_simple_filters using a real comparator, or have the _id fast path delegate residual-filter evaluation to the same engine query logic instead of a divergent local matcher.

**Verification evidence.** Re-read crud/filter.rs:123-138 (matches_simple_filters): only Eq (line 129) and Neq (line 130) are handled; catch-all `_ => true` (line 131) makes Gt/Gte/Lt/Lte unconditionally match. Re-read the _id fast path (filter.rs:151-183): when _id is a non-operator point value, the doc is fetched by id then non-_id filters are re-applied via matches_simple_filters at line 172. translate_filter_excluding_id DOES translate $gt/$gte/$lt/$lte into Filter ops (filter.rs:67-83), so a range predicate survives into matches_simple_filters and is then treated as always-true. The general (non-_id) path uses engine.query_documents which honors range ops, so behavior is inconsistent and silently wrong only for {_id: <eq>, field: {$gt: ...}}. Confirmed.

#### `E1-5` — Default listener ships hard-coded admin/admin credentials
**Severity:** high · **Dimension:** safety · **Subsystem:** Adapters · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-mongodb/src/lib.rs:58-62`

**Finding.** AuthConfig::default() returns username/password "admin"/"admin" (lib.rs:58-62), and run_listener() wires exactly that default (nimbus-server/src/adapters/mongodb/listener.rs:16). Even once an authn gate exists (E1-1), the out-of-box deployment accepts a trivially-guessable credential. Combined with E1-2, an attacker who authenticates as admin/admin still drives the engine as the system principal.

**Fix direction.** Remove the Default impl (or have it refuse to start), and require explicit credentials/secret provisioning before the listener accepts connections. Pre-launch: no legacy fallback needed.

**Verification evidence.** Re-read lib.rs:58-62 — AuthConfig::default() = Self::new("admin".into(), "admin".into()). Re-read listener.rs:16 — run_listener wraps AuthConfig::default() and passes it to run_listener_with_auth. So the out-of-box listener ships guessable admin/admin credentials. Severity high is justified for a default-credential posture; it is one notch below E1-1/E1-2 because it presupposes the auth gate is actually enforced (which today it is not, per E1-1) and an operator can override via run_listener_with_auth. Confirmed at high.

#### `A2-1` — Open-ended single-field range scan returns documents of other JSON types
**Severity:** high · **Dimension:** bug · **Subsystem:** Storage · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-storage/src/index/scan/range.rs:60-73`

**Finding.** index_scan_range_in_read_txn iterates the entire index prefix and filters each row with a raw byte-wise encoded_value.cmp(start/end) (range.rs:50-73), with no JSON type-tag check. Encoded type tags sort Null(0x00) < Bool(0x01) < Number(0x02) < String(0x03), so a one-sided numeric lower bound (e.g. age >= 25 with no upper bound) byte-compares Greater for every string-valued document and includes them. Verified empirically: encode_index_value("zzz") > encode_index_value(25) is true. The planner (engine/queries/planner/range.rs:56-73, mod.rs:192-201) marks the range filter as satisfied and removes it from residual_filters, so it is NOT re-checked after the store scan, and the cross-type documents leak into final query results. Two-sided numeric ranges are safe because the upper bound caps at 0x02..., so the leak requires an open-ended bound — which the planner readily produces (range.rs:52 only rejects when both bounds are absent).

**Fix direction.** Reject rows whose encoded value does not share the bound's leading type tag (compare value[0] to start/end[0]), or seek with proper type-bounded keys. Equivalently, derive a type-bounded upper key from the start's type tag when end is None. Add the same guard to the composite range field. Confirm the planner cannot rely on the store filter alone for type correctness.

**Verification evidence.** Re-read index/scan/range.rs:50-73 (embedded redb path): it ranges from prefix.. and byte-compares encoded_value against start/end with no JSON type-tag gate. encoding.rs:5-42 confirms tags Null=0x00 < Bool=0x01 < Number=0x02 < String=0x03; empirically verified enc("zzz")>enc(25) and even enc("a")>enc(25) are True. For an open-ended lower bound (end=None at range.rs:67) every string-valued doc compares Greater and is included. Traced the planner (engine/queries/planner/range.rs:56-73): the satisfied range filter is removed from residual_filters; prepared.rs:270-291 re-evaluates store results only against residual_query.filters via evaluate_query_with_docs_cancellable_and_predicate -> filter_documents_cancellable (evaluator/query.rs:71), so the dropped range filter is never re-checked and cross-type docs leak into final results, then get sorted/truncated as real results. Two-sided numeric ranges are capped at 0x02.. so safe; the leak requires the open-ended bound the planner readily emits. SQL backends (sqlite/read.rs:577-622 etc.) use typed columns so they don't share the leak, but the cited default embedded backend does. Wrong-results correctness bug on the default backend; high is justified.

#### `A2-2` — No mixed-type or negative-number range-scan tests; existing range test passes only by accident
**Severity:** high · **Dimension:** test-quality · **Subsystem:** Storage · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-storage/src/index/tests.rs:451-489`

**Finding.** index_scan_range_on_numbers (tests.rs:452-489) inserts only numeric documents, so the open-ended scan at line 473 returns the correct count purely because no string/bool/null documents exist for that index. There is no test that inserts mixed-type documents into a ranged index, which is exactly what would have caught A2-1. There is also no end-to-end range scan over negative numbers (only an isolated encode_index_value sort assertion at 262-287), so the negative-number ordering is never exercised through the actual scan path.

**Fix direction.** Add a range-scan test that inserts number, string, bool, and null documents under one single-field index and asserts an open-ended numeric range returns only numeric documents. Add a range scan spanning negative and positive numbers asserting correct membership and ordering.

**Verification evidence.** Re-read index/tests.rs:451-489: index_scan_range_on_numbers inserts only numeric docs (ages 20/30/40/50), so the open-ended over_25 scan at line 473 returns 3 purely because no string/bool/null docs exist for the index. Grepped the whole subsystem: no test inserts mixed-type documents into a ranged index, which is exactly the gap that masks A2-1. Negative-number ordering is only asserted in isolation via encode_index_value sort at tests.rs:262-287, never through index_scan_range. The test-quality gap is the direct cause that a high-severity correctness bug shipped untested; keeping severity high consistent with A2-1.

#### `B1-1` — Direct mutation path bumps applied_head before invalidating the document cache (stale read-after-write)
**Severity:** high · **Dimension:** bug · **Subsystem:** Engine · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-engine/src/engine/mutations/direct/store.rs:21-24`

**Finding.** In all four direct-path helpers, mark_applied_head is called INSIDE the sequence guard while invalidate_document_cache_for_commit is called AFTER the guard is dropped. Readers wait on applied_head >= durable_head then immediately read get_cached_document (queries/documents.rs:184-196 sync and :302-321 async). Between the watermark bump and the cache invalidation a reader can observe the new watermark and return a STALE cached document for an updated/deleted doc that was previously read into the cache. The journal path (journal.rs:283-284) and execution-unit path (commit.rs:61-62) both invalidate the cache BEFORE marking applied_head; only the direct path inverts the order. No test guards this ordering.

**Fix direction.** Invalidate the document cache (and materialized_reads) before calling mark_applied_head, matching commit.rs/journal.rs ordering. Move invalidate_document_cache_for_commit inside the guarded block before mark_applied_head, or restructure so the watermark is the last thing published.

**Verification evidence.** Re-read store.rs:17-97 (all four direct helpers): mark_applied_head is called inside the _sequence_guard block (lines 21/42/66/87) while invalidate_document_cache_for_commit is called AFTER the guard is dropped (lines 24/49/69/94). mark_applied_head (tenant/mutation/journal.rs:157-167) notify_all/notify_waiters BEFORE the cache is invalidated. Readers (queries/documents.rs:184 sync via wait_for_latest_applied_visibility_blocking -> wait_for_applied_sequence_blocking; :301-304 async) wait on applied_head>=durable_head then immediately read get_cached_document (:192, :312). document_cache.rs get() returns whatever entry is present and invalidate_commit() removes by (table,doc_id); so a reader woken in the window between mark_applied_head and invalidate can return a previously-cached, now-stale doc. Confirmed the inversion is unique to the direct path: journal.rs:283-284 invalidates BEFORE mark_applied_head, commit.rs:61-62 invalidates BEFORE mark_applied_head, and provider_hints.rs:203 invalidates before sync_mutation_journal_progress(->mark_applied_head). The direct path is live (execution.rs:134-399 calls all four helpers for insert/update/delete). No test guards the ordering. Window is narrow and the stale read is transient, but it violates read-after-write monotonicity that every other path explicitly upholds. High is justified.

#### `B1-2` — Execution-unit OCC conflict check runs outside the sequence lock, leaving a serialization gap for predicate/range/insert dependencies
**Severity:** high · **Dimension:** bug · **Subsystem:** Engine · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-engine/src/engine/execution_units/commit.rs:42-55`

**Finding.** commit() calls ensure_schema_unchanged and ensure_no_conflicts (which reads the commit log from snapshot_sequence+1) OUTSIDE the lock_mutation_sequence guard; the guard is only taken to apply the batch (commit.rs:47). Two execution units starting at the same snapshot that touch the same predicate/range/missing-table or both INSERT documents matching the other's query predicate can each pass their conflict scan before either appends, then serialize their appends. Document-vs-document conflicts are caught by the storage CAS in nimbus-storage/src/store/write/batch.rs (apply_update/apply_delete: 'changed before transaction commit'), but predicate/range/insert dependencies have NO storage backstop, so a phantom/predicate conflict can be silently committed and break OCC serializability.

**Fix direction.** Perform ensure_schema_unchanged and ensure_no_conflicts inside the same lock_mutation_sequence critical section as apply_execution_unit_batch_with_origin (re-read commit log from snapshot_sequence+1 under the lock), so the conflict scan and the append are atomic with respect to other writers.

**Verification evidence.** Re-read commit.rs:42-55: the result closure calls ensure_schema_unchanged and ensure_no_conflicts (lines 43-44) OUTSIDE the lock; _sequence_guard is taken only in the nested block at lines 46-53 wrapping apply_execution_unit_batch_with_origin. ensure_no_conflicts (87-106) scans read_commit_log_from(snapshot_sequence+1) with no lock held. lock_mutation_sequence is a bare Mutex<()> gate (mutation_facade.rs:85-87). Each unit takes its own snapshot at begin_mutation_execution_unit with no lock (mod.rs:38-49), so two units can share/overlap snapshots and run concurrently. Storage backstop in sqlite/write.rs apply_resolved_write (Insert 656-661, Update 678-689, Delete 727-738) is strictly document-id-keyed CAS ('changed before transaction commit') — it does NOT re-validate predicate/range/missing-table/phantom-insert dependencies. Execution units genuinely record these (reads.rs:248 record_missing_predicate, :259 record_predicate, :290/:353 paginated_window, plus index_ranges) and ensure_no_conflicts/commit_intersects_dependency_set (dependency.rs:294-360) checks them. So two units at the same snapshot can each pass the scan, then serialize their appends, with a newer commit's predicate/range/insert conflict never re-checked. Real, reachable OCC serializability hole; the fix is to move ensure_no_conflicts inside the sequence lock. High is justified. (Note: finding cited storage path nimbus-storage/src/store/write/batch.rs; the actual CAS lives in sqlite/write.rs and postgres/write.rs — citation imprecise but the described behavior is correct.)

#### `B2-1` — Pagination cursor signature is plan-dependent, causing spurious rejection when the query plan flips
**Severity:** high · **Dimension:** bug · **Subsystem:** Engine · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-engine/src/evaluator/cursor.rs:87-94`

**Finding.** The cursor signature is computed by `query_signature(query)` over whatever query the pagination evaluator receives. In the index-plan path the evaluator receives the *residual* query (filters reduced to those not satisfied by the index) — see prepared.rs:310-322 building `residual_paginated` from `plan.residual_query(...)`. In the full-scan path it receives the *full* merged query — load_query_plan_documents_from_docs returns None for FullScan (planner/mod.rs:223) so the else branch passes `prepared.planned_paginated` with all filters (prepared.rs:323-330). Because schemas/indexes are replaceable at runtime (persistence/tenant/schema.rs:6 replace_table_schema), a cursor minted while a query resolved to ExactIndex/RangeIndex is replayed after an index is dropped/added: the plan now resolves to FullScan (or a different residual set), `query_signature` differs, and decode_cursor returns Error::InvalidInput("invalid cursor") (cursor.rs:40-42) even though the user's query shape is identical. The signature should be derived from the stable user/authorized query, not the post-plan residual.

**Fix direction.** Compute the cursor signature from a plan-independent canonical form of the query (e.g. the authorized `planned_query`/`planned_paginated.query` before residual reduction, or a normalized table+order-only signature). Pass that canonical query into encode/decode rather than the residual `unbounded_query` used for sorting, so cursor validity is invariant under plan selection.

**Verification evidence.** Re-read cursor.rs:34-94 (query_signature normalizes only limit, then serializes the whole Query incl. filters), pagination.rs:90-153 (encode_cursor/decode_cursor both derive from paginated.query via unbounded_query), prepared.rs:102-132 and 293-331 (index path passes residual_paginated with plan.residual_query; FullScan/else path passes the full planned_paginated), planner/mod.rs:24-40 (residual_query REPLACES filters with the strict-subset residual_filters for ExactIndex/RangeIndex, returns the full query unchanged for FullScan) and 160/223 (load_query_plan_documents_* return None for FullScan, forcing the full-query else branch). query_api.rs:362-393 confirms each page request re-prepares against the live runtime.schema(); materialized.rs:205-216 confirms the FullScan branch feeds the full planned_paginated through paginate_documents_for_docs_prepared. replace_table_schema exists across all backends (persistence/tenant/schema.rs:6; storage index/tests.rs:38 rebuilds indexes). Net: a cursor minted when the query resolved to an index plan carries the residual-filter signature; replayed after an index add/drop flips the plan to FullScan (or a different residual set), the signature differs, and decode_cursor returns InvalidInput("invalid cursor") for an unchanged user query. Within a single request encode/decode are symmetric, so the break is strictly cross-request when the plan flips. Reachable via a supported runtime op and it defeats the subsystem's documented headline invariant (cursor stability across schema/query-shape changes). Graceful error rather than corruption, but high is justified given it silently breaks the stated contract.

#### `B3-1` — Lost wakeup in begin_delete_blocking: condvar notified without holding the guarding mutex
**Severity:** high · **Dimension:** bug · **Subsystem:** Engine · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-engine/src/tenant/lifecycle.rs:43-62`

**Finding.** TenantLifecycle::begin_delete_blocking (crates/nimbus-engine/src/tenant/lifecycle.rs:50-62) waits on the `zero_active` Condvar while holding `zero_active_lock`, checking the `active_operations` atomic predicate. But release_operation (lines 43-48) decrements `active_operations` and calls `zero_active.notify_all()` WITHOUT ever acquiring `zero_active_lock`. This violates the condvar contract (predicate mutation + notify must be synchronized through the same mutex the waiter holds). Window: the deleter holds the lock, reads active_operations==1 (true, so it will wait), and before it parks in `.wait()`, the last in-flight operation runs fetch_sub->0 then notify_all() with zero threads parked. The deleter then parks and is never woken => delete_tenant hangs permanently. Reachable from Engine::delete_tenant (engine/tenants.rs:125) whenever a concurrent sync tenant operation finishes in that window. No test covers begin_delete with a concurrent in-flight op (existing delete tests delete idle tenants).

**Fix direction.** Acquire zero_active_lock in release_operation around the (read-after-decrement) check + notify, or perform the decrement under the lock when it may reach zero. The mutex must guard both the predicate transition and the notify so the waiter cannot miss it.

**Verification evidence.** Re-read crates/nimbus-engine/src/tenant/lifecycle.rs:43-62. zero_active is std::sync::Condvar (line 2 import, line 16). begin_delete_blocking holds zero_active_lock across the predicate read (line 56) and wait() (line 59), but release_operation (lines 43-48) mutates the predicate via active_operations.fetch_sub and calls zero_active.notify_all() WITHOUT ever acquiring zero_active_lock. std::sync::Condvar stores no permit, so a notify_all() firing after the deleter reads active_operations==1 and before it parks in wait() is lost permanently. Reachability confirmed: sync ops (insert_document/list_documents) call enter_operation/release_operation on caller threads; delete_tenant (engine/tenants.rs:120-135) removes the runtime from the registry, but an op already holding Arc<TenantRuntime> can release concurrently, and store.rs:17-25 scopes lock_mutation_sequence to the mutate block so ops run unserialized vs delete. Result: delete_tenant thread hangs forever. Unlike begin_delete_async, the std::sync::Condvar path has no counter to save it. No sync concurrent-in-flight delete test exists (tests/subscriptions/lifecycle.rs:4 deletes an idle tenant; line 43 tests the async path and releases only after delete is confirmed parked, never exercising the race window). High justified.

#### `B4-1` — Trigger invocations in Running state are never re-enqueued after a crash
**Severity:** high · **Dimension:** gap · **Subsystem:** Engine · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-engine/src/engine/mutations/commit_processing.rs:90-98`

**Finding.** The trigger worker durably persists a Running record (record.begin_attempt + save_trigger_invocation) BEFORE calling executor.execute_invocation. If the process crashes after that save but before Completed/RetryPending/Terminal is written, the invocation is stuck in Running. On restart, bootstrap_trigger_execution rebuilds the in-memory queue by filtering list_trigger_invocations() to only Pending => Timestamp(0) and RetryPending => next_attempt_at, with `_ => None`. Running records match the `_` arm and are silently dropped, so the at-least-once trigger is never delivered and the record never reaches a terminal state. This breaks the durable at-least-once guarantee the engine claims to own.

**Fix direction.** Treat Running (and RetryPending-mid-flight) records as recoverable in bootstrap_trigger_execution: re-enqueue Running invocations at Timestamp(0) (idempotent re-attempt) the same way recover_running_jobs handles scheduled jobs. The owning write is trigger_execution.rs:225-226.

**Verification evidence.** Re-read trigger_execution.rs:215-254 (worker durably saves Running via begin_attempt+save_trigger_invocation at lines 225-226 BEFORE executor.execute_invocation at 227; terminal save at 248). Re-read commit_processing.rs:78-102: bootstrap_trigger_execution is the ONLY function that repopulates the in-memory trigger queue from durable state, and its filter_map handles only Pending and RetryPending with `_ => None`, dropping Running (confirmed via trigger.rs:332-353 enum). Grepped all callers of bootstrap_trigger_execution (mod.rs:295/325, tenants.rs:76/305, coordination.rs:198) and searched for any trigger-specific recovery: none exists — unlike scheduled jobs which have recover_running_jobs (scheduler/recovery.rs). A crash between the Running save (226) and terminal save (248) strands the invocation in Running forever, silently breaking the at-least-once guarantee. The scheduled-job sibling having an explicit recovery sweep proves this is an unintended gap, not design. High justified.

#### `C3-1` — Bun/JSC linked FFI path drops the watchdog and concurrency permit — no timeout enforcement
**Severity:** high · **Dimension:** gap · **Subsystem:** Runtime · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:112-158`

**Finding.** invoke_program_wrapper_json destructures the invocation as `let RuntimeBackendInvocation { policy, bundle, request, cancellation, host, .. } = invocation;` (linked.rs:112-119), explicitly discarding `watchdog`, `permit`, and `context` via `..`. The guest is then executed through a single blocking synchronous FFI call (linked.rs:146-158) with no time bound. By contrast V8RuntimeBackend::invoke threads `watchdog` and `permit` into RuntimeInvocationExecution (v8/mod.rs:41-66) so V8 guests are killed on timeout and counted against the concurrency budget. The Bun/JSC path only honors a pre-call cancellation snapshot (linked.rs:121-126) and a cooperative cancellation token inside host-bridge calls; a CPU-bound or tight-loop guest that never calls back into the host can run unbounded. This is a fail-open execution-limit parity gap between the two backends for the same policy.

**Fix direction.** Thread the watchdog/permit through the FFI path: hold the permit for the FFI call's duration, and enforce the execution timeout (e.g. run the blocking FFI on a dedicated thread joined with a deadline, or pass a deadline the embedder honors and have it return a timeout status). At minimum, fail closed by rejecting policies whose execution_timeout cannot be enforced by the linked backend rather than silently ignoring it.

**Verification evidence.** Re-read backends/bun_jsc/linked.rs:112-158 — invocation is destructured `{ policy, bundle, request, cancellation, host, .. }`, dropping `watchdog` and `permit`; the guest runs via a single blocking synchronous FFI call (invoke_program_wrapper_json_with_host_bridge) with no time bound and no interruption path. By contrast backends/v8/mod.rs:41-66 threads `watchdog` into RuntimeInvocationExecution, and cooperative.rs:206-230 wires it into the driver where the watchdog cancel callback terminates the V8 isolate. worker_loop/run_to_completion.rs has no wall-clock timeout other than the WatchdogTimer, so a CPU-bound Bun/JSC guest runs unbounded — a genuine fail-open timeout-parity gap. The selectable Bun/JSC product route is explicitly InProcessUntrusted (axes.rs:580-585), and contract.rs:41 even lists an embedder `..._probe_timeout_and_cancel` export that is never wired into the production call, so the capability exists but is unused. One sub-claim is REFUTED: 'not counted against the concurrency budget' is wrong — run_to_completion.rs:111-155 acquires SharedInvocationPermit and holds it across the backend.invoke closure via run_invocation_lifecycle, so the invocation is counted regardless of the backend dropping the `permit` field. Reachability is gated (opt-in `bun-jsc-linked-adapter` feature, external linked .dylib/.so, non-default policy; default backend is V8 per resources.rs:433). The substantive timeout fail-open on the untrusted route holds, so high stands; the concurrency portion of the writeup is inaccurate.

#### `D4-1` — MongoDB data plane has no authentication enforcement — SCRAM handshake is decorative
**Severity:** high · **Dimension:** safety · **Subsystem:** Server · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-mongodb/src/commands/mod.rs:43`

**Finding.** The MongoDB adapter implements a full SCRAM-SHA-256 handshake that sets conn.authenticated=true (crates/nimbus-mongodb/src/auth.rs:161), but commands::dispatch never checks conn.authenticated before serving data commands. The dispatch match (crates/nimbus-mongodb/src/commands/mod.rs:43-64) routes insert/find/update/delete/drop/aggregate/createIndexes/findAndModify directly to handlers with no auth gate. A grep confirms conn.authenticated is read in non-test production code ONLY by connectionStatus for cosmetic reporting (crates/nimbus-mongodb/src/commands/admin.rs:65) — never to deny a command. This is reachable in production: construction.rs:177-201 binds the listener, spawns run_listener_with_auth, and records the listener as 'listening'. Any TCP client can issue full CRUD + DDL against tenant data without ever authenticating. Contrast the sibling DynamoDB lane, which enforces per-request signed auth and a loopback guard (construction.rs:209). This is the headline defect.

**Fix direction.** Gate all non-handshake/non-auth/non-ping commands on conn.authenticated in dispatch (return MongoError::Unauthorized when an AuthConfig is bound and the connection has not completed SCRAM). Add a connection-level test in listener.rs that asserts an unauthenticated insert is rejected. Remove the dead_code allow (D4-2) so the unused field stops compiling once enforcement is wired.

**Verification evidence.** Re-read commands/mod.rs:20-67 (dispatch), connection.rs:29-51, auth.rs:15-172, listener.rs:53-89, and construction.rs:177-201. The code-level defect is real and precisely described: sasl_continue sets conn.authenticated=true (auth.rs:161) but dispatch routes insert/find/update/delete/drop/aggregate/createIndexes/findAndModify directly to handlers with no auth gate, and handle_connection (listener.rs:75) calls dispatch unconditionally. grep confirms conn.authenticated is read in production ONLY at admin.rs:65 (connectionStatus, cosmetic) and conn.auth_user is never read in production. The DynamoDB contrast (per-request auth + guard_lookup_is_loopback_only at listener.rs:64-68) is accurate. HOWEVER the explicit claim 'This is reachable in production' is overstated: with_mongodb (construction.rs:66, the only setter of mongodb_config=Some) has ZERO callers anywhere in the repo, and the production boot path start/boot.rs:156-173 never wires it, so mongodb_config stays None and the listener is never spawned in any shipping binary. run_listener is invoked only from tests. This is a genuine, well-located latent security defect that activates the instant someone wires the listener, but it is not currently exploitable in a production deploy, so critical is not justified; high.

#### `I1-1` — HTTP-sourced machine image is persisted as the bootable disk with no integrity verification
**Severity:** high · **Dimension:** safety · **Subsystem:** CLI · **Double-verified:** reconfirmed (0 of 14 crit/high disputed)

**Location:** `crates/nimbus-bin/src/machine/manager/image.rs:123-273`

**Finding.** materialize_http_image downloads a URL and persists it directly as paths.materialized_image_path with no size or digest check. The OCI path (pull_oci_artifact_to_cache) calls verify_downloaded_oci_blob (size + SHA-256) and check_build_attestation, but the HTTP path has no equivalent: a corrupted or attacker-substituted download (especially over plaintext, see I1-9) becomes the disk the outer VM boots from. There is no expected-digest parameter on the HTTP image source at all.

**Fix direction.** Accept an expected SHA-256 (and optionally size) for HTTP image sources and verify the staged temp file before persist(); reuse the existing verify_downloaded_oci_blob digest helper. At minimum, refuse plaintext HTTP for non-loopback hosts.

**Verification evidence.** Re-read materialize_http_image (image.rs:123-273): the download is io::copy'd to a temp file and persist()'d directly to paths.materialized_image_path with no size or digest check anywhere. Contrast verify_downloaded_oci_blob (image.rs:605-631) which checks metadata.len()==layer.size and compute_sha256==expected, plus check_build_attestation (675+). The enum (nimbus-machine/src/lib.rs:148-152) confirms HttpUrl { url: String } carries no expected-digest field, so verification is impossible by construction. resolve_bootable_image_path (image.rs:82-110) returns this file as the bootable disk, and MachineImageSource::parse routes any http(s):// string here from handlers.rs:265 (machine create). The asymmetry and the absence of a digest parameter are real. The disk the outer VM boots is unverified. URL is operator-supplied so the worst-case requires MITM/compromised mirror, but the integrity-verification gap relative to the OCI path is a genuine safety defect; high is justified.


## Medium (35)

#### `E1-6` — $push/$pop/$mul/$bit do read-modify-write into a Patch, losing atomicity under concurrency
**Severity:** medium · **Dimension:** bug · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-mongodb/src/commands/crud/update.rs:140-157`

**Finding.** Unlike $addToSet/$pull/$pullAll (which emit FieldTransform operations the engine applies atomically), $mul (update.rs:114-124), $push (140-157), $pop (182-200), and $bit (202-229) read current_doc, compute the new value in adapter memory, and emit a plain field_patch/Patch (the Patch branches at update.rs:248-262). The read and the write are not a single atomic operation, so two concurrent $push operations against the same document can lose an element (classic lost-update). The transform-based operators in the same function show the atomic pattern that these four diverge from.

**Fix direction.** Express these as engine FieldTransform operations (append/pop/multiply/bitwise) so the engine performs them atomically, matching the $addToSet/$pull path, instead of read-modify-write in the adapter.

**Verification evidence.** Re-read update.rs build_operator_write: $mul (114-124), $push (140-157), $pop (182-200), $bit (202-229) all read current_doc, compute the new value in adapter memory, then field_patch.insert + mask.push, landing in an AtomicWrite::Patch (lines 247-263). By contrast $addToSet/$pull/$pullAll/$inc/$min/$max/$currentDate emit FieldTransform operations (lines 81-138,159-181) applied atomically by the engine (batch.rs apply_field_transform). The read for the four patch operators is not part of the engine write transaction, so concurrent $push on the same doc can lose-update. Real medium-severity correctness-under-concurrency gap; the atomic pattern in the same function confirms the divergence.

#### `E1-7` — Aggregation loads the entire collection with no filter/limit pushdown
**Severity:** medium · **Dimension:** optimization · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-mongodb/src/commands/aggregation/mod.rs:266`

**Finding.** load_initial_documents in the aggregation module loads ALL documents for the collection (full scan) regardless of a leading $match or $limit stage, then filters in memory. For any non-trivial collection this is O(n) memory and time per aggregate, and amplifies the system-principal exposure (E1-2) since the full table is materialized. A leading $match (and $limit) should be pushed into an engine Query.

**Fix direction.** Detect a leading $match/$limit prefix and translate it into engine Query filters/limit before materializing, falling back to full load only when the first stage isn't pushable.

**Verification evidence.** Re-read aggregation/mod.rs:266-282 load_initial_documents: builds Query{ filters: vec![], order: None, limit: None } and ignores the _stages argument entirely (note the underscore-prefixed parameter), then query_documents_with_principal loads the whole collection; filtering/limiting happens in-memory via execute_stage (Match/Limit at 289,291). No leading-$match/$limit pushdown. O(n) memory/time per aggregate and amplifies the system-principal full-table materialization. Confirmed medium (optimization/scalability).

#### `E1-9` — Per-connection CursorStore and SessionStore are unbounded
**Severity:** medium · **Dimension:** safety · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-mongodb/src/connection.rs:35-36`

**Finding.** ConnectionState owns a CursorStore and SessionStore (connection.rs:35-36) with no cap on entries or TTL eviction. A client that opens many cursors (find/aggregate returning cursorless ids) or startSession calls without killing/ending them grows these maps for the life of the connection, an unauthenticated memory-growth vector (compounded by E1-1). SessionStore in particular retains transaction state.

**Fix direction.** Bound cursor/session counts per connection and/or add idle TTL eviction; reject new cursors/sessions past the cap with the appropriate Mongo error code.

**Verification evidence.** Re-read connection.rs:35-36 (ConnectionState owns cursor_store + session_store). Re-read cursor.rs:20-56 — CursorStore is HashMap<i64,StoredCursor> with create() inserting unconditionally; no cap, no TTL eviction. Re-read session.rs:203-216 — SessionStore is HashMap<Vec<u8>,SessionState> with create_session inserting unboundedly; entries only removed via explicit end_session. Both grow for the connection lifetime; combined with the missing auth gate (E1-1) this is an unauthenticated per-connection memory-growth vector, and SessionStore retains transaction state. Confirmed medium.

#### `E2-2` — Missing-index detection parses a free-text engine error string by prefix
**Severity:** medium · **Dimension:** seam · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-firebase/src/errors.rs:113-126`

**Finding.** missing_index_fields downcasts Error::InvalidInput and then does message.strip_prefix("structured query requires an index covering fields: ") to recover the field list that is reformatted into a google.rpc.PreconditionFailure detail (errors.rs:113-126, consumed by missing_index_details at errors.rs:128). This couples the Firebase adapter's REST error shape to the exact wording of an engine-produced string with no compile-time link between producer and consumer. A benign reword of the engine message silently degrades the FIRESTORE_QUERY_INDEX violation into a generic error, and the field list (split on ',') will mis-parse if any field name legitimately contains a comma.

**Fix direction.** Have the engine surface a typed error variant (e.g. Error::MissingIndex { fields: Vec<String> }) instead of a formatted string, and match on that variant here. If a typed variant is out of scope, at least centralize the prefix as a shared const owned by the producing crate and add a test that fails if the producer string drifts.

**Verification evidence.** Re-read errors.rs:113-137 (missing_index_fields strip_prefix on the literal 'structured query requires an index covering fields: ' then split(',')/trim; missing_index_details builds the FIRESTORE_QUERY_INDEX PreconditionFailure), errors.rs:139-142 (firestore_grpc_code returns FailedPrecondition only if the prefix matches, else falls through to Code::InvalidArgument for InvalidInput at line 155-157), and the producer at prepare.rs:578-581 in nimbus-engine which formats Error::InvalidInput with that exact wording, joining fields with ', '. The cross-crate coupling to a free-text engine string with no compile-time link is accurate. A benign engine reword silently degrades FIRESTORE_QUERY_INDEX into a generic InvalidArgument — and the firebase test at errors.rs:342-360 hardcodes its own copy of the string rather than calling the engine producer, so it would stay green while production degraded (false confidence, confirming the brittleness). The split(',') vs producer join(', ') mismatch would also mis-parse any field name containing a comma. Currently not a live failure since the strings match, so medium (not high) is correct.

#### `E3-1` — Single-item and batch writes can commit data without a stream record
**Severity:** medium · **Dimension:** bug · **Subsystem:** Adapters · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-dynamodb/src/commands/item.rs:170`

**Finding.** put/delete/update item and batch_write_item commit the data write then call capture_event in a separate transaction; a crash between leaves the change applied with no stream record.

**Fix direction.** Fold the stream-event write into the same AtomicWriteBatch as the data write, as transact.rs already does.

**Verification evidence.** Confirmed real. Re-read item.rs put_item (atomic_overwrite L155 then a SEPARATE capture_event L170), delete_item (atomic_delete L279 then capture_event L288), update_item (atomic_overwrite L367 then capture_event L382), and batch.rs batch_write_item (store_item L129 then capture_event L131; remove_item L151 then capture_event L156). capture_event (stream.rs:349-382) opens its own begin_mutation_execution_unit().execute_atomic_write_batch(), so the data commit and the stream-event commit are two distinct storage transactions — a crash between them leaves the row written/deleted with no stream record. This is NOT how the transactional path works: transact.rs:181-204/237-269 folds stream events into the SAME AtomicWriteBatch as the data ('so the events commit atomically with the data (F3)'). The hardening plan's D-Atomic decision (dynamodb-adapter-hardening-plan.md:93-98) and H3 completion gate (L113) explicitly require single-item AND BatchWriteItem to fold stream capture into the same AtomicWriteBatch; H3 is marked done but the code does not do this for the single-item/batch paths — so this is also an unmet 'done' gate. Downgraded from high to medium: the original F3 (zero records emitted, rated High) is fixed (events ARE emitted on the happy path); the residual gap is only a process-crash window between two already-durable commits (low probability), and DynamoDB Streams are themselves asynchronous, at-least-once, and not transactionally coupled to base writes — so real-world CDC impact is bounded. It remains a genuine violation of the project's explicit atomic-stream-capture invariant, hence above info/low.

#### `E3-2` — Six more findings (sidecar, auth-docs, full-scan, 3 low smells)
**Severity:** medium · **Dimension:** code-smell · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-dynamodb/src/attribute_value.rs:5`

**Finding.** attribute_value.rs:5 sidecar codec unused (lib.rs:31-34, live=wire-JSON 96-103); dispatch.rs:158 docs claim no-verify default but Strict default (tenant.rs:30-36); query.rs:393 full scan per query (also stream.rs:391, ttl.rs:212); stream.rs:232 event-name->INSERT; stream.rs:552 reclaim/poll; ttl.rs:155 TOCTOU; stream.rs:1 1500+ files.

**Fix direction.** Address each cited file:line: delete/wire sidecar, fix auth docs, add range read, Result event-name, batch reclaim, atomic TTL, split files.

**Verification evidence.** Bundle of six smells, all verified real. (1) Sidecar codec unused: grep shows attribute_value_to_stored/stored_to_attribute_value/item_to_stored/stored_to_item are called only inside #[cfg(test)] (expression.rs:326/354 is in mod tests); the live persistence path is wire-JSON via item_to_fields/fields_to_item (attribute_value.rs:96-137), exactly as the module doc (L96-103) states — dead public API kept alive only by re-export (lib.rs:31-34). (2) auth-docs: dispatch.rs:157-158 says 'LookupOnly (the default) skips signature verification' but tenant.rs:30-36 marks Strict with #[default]; the comment is stale/wrong, though harmless since the secure mode is in fact the default. (3) full-scan per query: query.rs:89 query() calls enumerate() (L388-408) which runs StructuredQuery::default() over the whole table then filters one partition in memory, despite the file header (L4) claiming 'A Query selects one partition'; same full-scan pattern in stream.rs:391 read_events and ttl.rs:212 sweep_table — a real O(N) performance smell. (4) stream.rs:232 event_name_from_str maps any unknown string to INSERT — real but unreachable for well-formed data (only INSERT/MODIFY/REMOVE are ever written by event_name_str L224-230). (5) ttl.rs:147-169 upsert_ttl_state does get_document then update/insert non-atomically (TOCTOU on the TTL config doc) — real, admin-only, low impact. (6) stream.rs is 1530 lines and query.rs 1511 lines, both over the 1,500-line justification threshold in CLAUDE.md. Each item is individually low, but the bundle includes a genuine cross-cutting performance issue (full-scan-per-Query) plus multiple confirmed smells, so medium as an aggregate is justified.

#### `E4-5` — No tests cover Firestore admin runtime-extension dispatch or database_id validation
**Severity:** medium · **Dimension:** test-quality · **Subsystem:** Adapters · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs:79-414`

**Finding.** firestore.rs contains no #[test]/#[tokio::test] module, and a workspace grep for FirestoreAdminGetDocumentPayload/FirestoreAdminSetDocumentPayload/invoke_firebase_admin_*/dispatch_firestore_admin_* in test/assert context returned nothing. None of the get/set/update/delete sync, cancellable, or async branches are exercised, and the database_id validation asymmetry (E4-4) is therefore unguarded by any regression test. The crate's strong manifest/lib.rs tests do not reach this surface.

**Fix direction.** Add a tests module with a fake RuntimeCapabilityHost asserting: get/set round-trip values, non-default database_id is rejected symmetrically on read and write, update() rejects empty patches (firestore.rs:464), and unsupported operations return Contract errors.

**Verification evidence.** Workspace grep for FirestoreAdminGetDocumentPayload/SetDocumentPayload/invoke_firebase_admin_*/dispatch_firestore_admin_* returns only firestore.rs itself plus the extension.rs dispatch wiring (lines 19/33/52). The firebase_admin dir contains only firestore.rs + mod.rs with no test module; the only #[cfg(test)] modules in nimbus-cloud-functions are host_bridge.rs, app_contract.rs, registry.rs, lib.rs, none of which reach dispatch_firestore_admin_runtime_extension or the invoke_* functions (their firestore/firebase references are manifest/registry/import-resolution contracts). No tests/ integration tests and no JS/TS tests exercise the runtime operations. None of the get/set/update/delete sync/cancellable/async branches, nor the database_id validation asymmetry, are guarded by any regression test. Medium is justified for a fully-untested multi-branch runtime surface that also gates on validate_runtime_capability_access.

#### `A2-3` — Live index key decode panics on storage corruption instead of returning a typed error
**Severity:** medium · **Dimension:** safety · **Subsystem:** Storage · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-storage/src/index/keyspace.rs:70-101`

**Finding.** doc_id_from_index_key and encoded_value_end (keyspace.rs:70-101) use .expect() on every part of the trailer parse: non-UTF-8 doc id, invalid DocumentId, missing/short length trailer, and a doc_id_length that underflows the key. These run on every row of every live index scan over data read back from redb, so a corrupted index entry turns a recoverable read into a process panic. The historical index path treats the identical corruption class as a typed StorageErrorKind::Corruption error (store/index_versions.rs:597-626, sqlite/index_versions.rs:377-393), so the live path is inconsistent with the established corruption idiom.

**Fix direction.** Make doc_id_from_index_key / encoded_value_end return Result and map malformed trailers to Error::storage(StorageErrorKind::Corruption, ...), threading the Result through scan/read.rs and scan/range.rs row loops, matching the historical path.

**Verification evidence.** Re-read keyspace.rs:70-101: doc_id_from_index_key and encoded_value_end use .expect() on non-UTF-8 doc id (line 74), invalid DocumentId (line 75), missing/short length trailer (lines 90-93, 94-96), and a doc_id_length underflow (lines 98-100). doc_id_from_index_key is called per row in both scan/range.rs:75 and scan/read.rs:78/94, so a corrupted live index entry panics a recoverable read. Confirmed the historical path treats the same corruption class as a typed StorageErrorKind::Corruption error: store/index_versions.rs:598-604 and index_version_base_key at 623-631 (checked_sub(8).ok_or_else(Corruption)). The inconsistency and panic-on-corruption are real; requires actual on-disk corruption (keys are written by index_key in the same module and redb is checksummed), so medium/safety is appropriately scoped.

#### `A2-4` — Single-field range scan does a full-index scan instead of a bounded seek
**Severity:** medium · **Dimension:** optimization · **Subsystem:** Storage · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-storage/src/index/scan/range.rs:49-84`

**Finding.** index_scan_range_in_read_txn opens index_table.range(prefix.as_slice()..) (range.rs:50-53), i.e. it begins at the index prefix and linearly filters every entry, even for a tight bounded range. Its siblings do the right thing: composite range, eq, and prefix all use scan_documents_for_index_key_bounds_in_read_txn with a real range(start_key..end_key) seek (scan/read.rs:68-71). For a large index with a selective range (e.g. age BETWEEN 999000 AND 999100) this scans the whole index. The full-scan approach is also what enables the A2-1 type leak.

**Fix direction.** Compute bounded start/end keys (as composite_range_scan_bounds does for the composite case) and route single-field range through scan_documents_for_index_key_bounds_in_read_txn, eliminating the per-row byte comparison loop.

**Verification evidence.** Re-read range.rs:50-73: index_scan_range_in_read_txn opens index_table.range(prefix.as_slice()..) and only `continue`s past out-of-range rows, breaking solely when starts_with(prefix) fails (line 56-58) -- it never breaks once past the upper bound, so a tight bounded single-field range still scans the entire index prefix. Confirmed siblings do a real bounded seek: composite path delegates to scan_documents_for_index_key_bounds_in_read_txn which uses range(start_key..end_key) (scan/read.rs:68-71). Real performance defect; medium/optimization justified. Also corroborates that the unbounded full-prefix scan is the structural enabler of the A2-1 leak.

#### `A2-6` — rebuild_table_indexes / clear_table_indexes are dead code with a non-atomic transaction pattern
**Severity:** medium · **Dimension:** modularity · **Subsystem:** Storage · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-storage/src/index/maintenance/rebuild.rs:46-98`

**Finding.** Neither TenantStore::rebuild_table_indexes nor clear_table_indexes (rebuild.rs:48-98) is called anywhere in the workspace (grep across crates finds only the definitions; the production schema path uses transaction.replace_table_schema within a single write txn, schema_store.rs:157-158). Beyond being dead code (against the repo's no-dead-code / pre-launch breaking-change posture), they violate the storage-atomicity invariant from CLAUDE.md: clear_table_indexes reads keys in one read_txn then deletes them in a separate write_txn (rebuild.rs:49-66), and rebuild_table_indexes runs clear (txn 1), scan_table (txn 2), and an insert write_txn (txn 3) — a concurrent writer between phases yields an inconsistent index.

**Fix direction.** Delete both methods (and the file if it leaves the module empty). If a standalone rebuild is genuinely needed later, reintroduce it as a single write transaction (collect, clear, and reinsert under one begin_write).

**Verification evidence.** Re-read maintenance/rebuild.rs:46-98. Grepped the entire repo (crates+packages, .rs and .md): rebuild_table_indexes and clear_table_indexes have zero callers, tests, or doc references -- genuinely dead code, against the pre-launch no-dead-code posture. Confirmed the production schema path uses transaction.replace_table_schema (schema_store.rs:157-158). The non-atomic pattern is real: clear_table_indexes reads keys in begin_read (line 49) then deletes in a separate begin_write (line 59); rebuild_table_indexes runs clear, scan_table, and an insert write across three+ transactions, so a concurrent writer between phases yields an inconsistent index -- violating the CLAUDE.md storage-atomicity invariant. The atomicity hazard is latent (unreachable while dead), but the dead-code finding plus a documented-invariant violation if revived together support medium.

#### `B1-3` — update_time write preconditions are accepted by the Firestore gRPC adapter but rejected as 'not executable yet'
**Severity:** medium · **Dimension:** gap · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/engine/execution_units/batch.rs:414-419`

**Finding.** ensure_write_precondition returns Error::InvalidInput("update-time preconditions are modeled but not executable yet") for any precondition.update_time. This is reachable: the Firebase gRPC write stream lowers ConditionType::UpdateTime into WritePrecondition::update_time (grpc/write_stream.rs:644-645) and commit_request.rs:253-260 does the same, both flowing into the execution-unit atomic-write path. A Firestore client using an updateTime optimistic-concurrency precondition gets a confusing accept-then-reject instead of real enforcement, and the OCC guarantee the client expects is absent.

**Fix direction.** Either implement update_time precondition enforcement (compare existing.update_time against precondition.update_time) or reject it at the adapter boundary with a clear unsupported-feature error rather than silently lowering it and failing deep in the engine.

**Verification evidence.** Re-read batch.rs:408-419: ensure_write_precondition returns Error::InvalidInput('update-time preconditions are modeled but not executable yet') for any precondition.update_time, and it is called by every atomic-write applier (apply_set_write:119, apply_patch_write:207, apply_delete_write:268, apply_verify_write:323, apply_transform_write:347). Reachability confirmed: grpc/write_stream.rs lower_precondition (644-645) maps ConditionType::UpdateTime -> WritePrecondition::update_time; commit_request.rs lower_precondition (253-260) maps update_time JSON into WritePrecondition. Both call precondition.validate() (write_batch.rs:77-84), which only rejects setting BOTH exists and update_time — a standalone update_time passes validate, so it is ACCEPTED at the gRPC/REST boundary and only rejected later at execution. Test at commit_request.rs:490-499 proves an updateTime precondition parses into AtomicWrite::Verify{precondition:{update_time:Some(_)}}, which flows into apply_verify_write -> ensure_write_precondition -> rejection. So a Firestore client's updateTime optimistic-concurrency precondition gets accept-then-reject with a confusing message and no real OCC enforcement. It fails closed (rejects, not silently ignores), so it is a functional gap rather than data corruption; medium/gap is appropriate.

#### `B2-5` — No test covers cursor stability across a plan change (the B2-1 defect path)
**Severity:** medium · **Dimension:** test-quality · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/evaluator/tests.rs:572-620`

**Finding.** paginate_rejects_cursor_for_different_query_shape (tests.rs:572-620) only asserts rejection when the order direction actually changes — i.e. it tests the intended rejection, not the spurious one. There is no test that mints a cursor under an index plan, then replays it after the index set changes (or vice versa) with an otherwise-identical query, which is precisely the scenario B2-1 breaks. The prepared.rs sqlite tests (lines 557-602) paginate only within a single fixed schema, so the residual-vs-full signature divergence is never exercised. The subsystem's headline focus (cursor encode/decode stability across schema/query-shape changes) is therefore unverified.

**Fix direction.** Add an engine-level test: paginate page 1 of an indexed query, replace the table schema to drop the index, then request page 2 with the prior cursor and the same user query; assert the cursor is still accepted and continuation is correct (this should fail today, demonstrating B2-1).

**Verification evidence.** Re-read tests.rs:572-620: paginate_rejects_cursor_for_different_query_shape changes only OrderDirection Asc->Desc, a genuine query-shape change and the intended rejection — it does not exercise the B2-1 spurious-rejection path. Grepped tests.rs for after:/next_cursor/replace_table_schema/residual: every evaluator pagination test calls evaluate_paginated with the full query directly (store-scan path, no planner/residual split), so the residual-vs-full signature divergence is never hit. prepared.rs:557-602 paginates within one fixed schema, never flipping the plan between pages. The gap the finding names (mint cursor under an index plan, replay after the index set changes with an identical query) is genuinely uncovered. Medium is appropriate: a missing-coverage gap on the subsystem's headline behavior, not a runtime defect itself.

#### `B3-3` — Concurrent overflow fallback can deliver an older snapshot after a newer one (monotonicity window)
**Severity:** medium · **Dimension:** bug · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/subscriptions/delivery.rs:100-134`

**Finding.** On queue overflow, dispatch_or_enqueue_subscription_work (crates/nimbus-engine/src/engine/mutations/commit_processing.rs:104-118) runs dispatch_subscription_work synchronously on the caller thread while the dedicated delivery worker thread is also running dispatch_subscription_work for the same tenant. In dispatch_subscription_work (crates/nimbus-engine/src/subscriptions/delivery.rs:87-134) the sequence (evaluate -> recheck is_stale -> try_send -> record_delivery) is not atomic: thread A evaluates at seq N, passes the recheck at line 100, then thread B (newer seq M>N) evaluates, sends M, and records M; thread A then sends N at line 128. The receiver observes N after M — an older result after a newer one. The atomic last_delivered watermark (mark_delivered/record_delivery uses fetch_max-style CAS) protects the stored watermark but not channel ordering, because send and watermark update are separate steps under no shared lock.

**Fix direction.** Serialize per-subscription delivery so evaluate+send+record is mutually exclusive across the worker and the sync-fallback (e.g. a per-subscription delivery mutex, or route the fallback through the same single worker instead of dispatching inline); alternatively gate the try_send on a final compare_exchange of last_delivered_sequence so a lost race re-checks staleness atomically with publication.

**Verification evidence.** Re-read commit_processing.rs:104-118 (overflow runs dispatch_subscription_work synchronously on caller thread), worker.rs:166/203 (worker runs it for same tenant), delivery.rs:78-157 (no lock across evaluate -> is_stale recheck line 100 -> try_send line 128 -> record_delivery line 131). Mutations are unserialized at dispatch: store.rs:17-25 releases lock_mutation_sequence before process_commit, so commits seq N and M>N can be concurrently in dispatch_or_enqueue_subscription_work; under queue overflow (cap 256, queue.rs:39) both take the sync fallback or one races the worker. Thread A(N) can pass its stale recheck while watermark<N, Thread B(M) fully sends+records M, then Thread A sends N -> receiver observes N after M. record_delivery (registry.rs:197-209) uses plain store; is_stale_for_sequence guards the watermark but not channel send ordering since send and watermark update are separate unlocked steps. Real monotonicity window. Held at medium: needs queue saturation + concurrent same-subscription mutations + tight interleaving; inversion is transient and snapshot carries covered_sequence (subscription.rs:28) for consumer defense.

#### `B3-4` — Trigger candidate worker drops the rest of a batch on a transient error with no in-process retry
**Severity:** medium · **Dimension:** gap · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/tenant/trigger_candidates.rs:440-477`

**Finding.** run_trigger_candidate_worker (crates/nimbus-engine/src/tenant/trigger_candidates.rs:440-477 test path, 497-528 prod path) wraps per-commit materialize_trigger_invocations_and_sync in a closure that returns on the first Err and only logs a warn (lines 472-474 / 526-528). The commits already popped from the in-process queue for that and subsequent batch entries are discarded; the worker loops to pop_next for NEW commits and never retries the failed/unprocessed ones. Recovery relies entirely on a process restart re-reading trigger_delivery_cursor (bootstrap_trigger_candidate_feed, commit_processing.rs:64-76). So a transient storage error on materialize_trigger_invocations causes trigger invocations to stall silently until the next restart — a liveness/durability gap for the trigger path within a process lifetime.

**Fix direction.** On Err, re-enqueue the unprocessed commits (or the failing commit onward) for retry with backoff instead of dropping them, and/or record a worker_failure metric so the stall is observable. Do not depend solely on restart-time cursor replay for transient errors.

**Verification evidence.** Re-read trigger_candidates.rs:479-530 (prod) and 416-477 (test); identical behavior. The closure (lines 497-525) iterates ready_batches with ? on build_trigger_commit_candidates/build_trigger_invocation_records/materialize_trigger_invocations_and_sync; first Err exits the whole closure, abandoning remaining commits in the current batch and all subsequent batches, only logging warn! (line 527). Those commits were already popped (pop_next + drain_ready_batches, lines 486-492) into the local Vec, dropped on error. materialize_trigger_invocations_and_sync (lines 532-543) is the only thing advancing trigger_delivery_cursor and only on success, so the cursor stays at the last successful commit. Confirmed bootstrap_trigger_candidate_feed (commit_processing.rs:64-76) is the only replay path and runs ONLY at bootstrap/load (engine/mod.rs:294, tenants.rs:75/304, scheduler/coordination.rs:197), never in the worker loop. So a transient storage error stalls trigger invocations for unprocessed commits until process restart with no in-process retry. Genuine liveness/durability gap. Medium (not high): a warn! is emitted (not fully silent) and restart durably recovers via the unadvanced cursor; fits the gap dimension.

#### `B4-2` — CronSchedule::next_after uses unchecked u64 arithmetic and can overflow
**Severity:** medium · **Dimension:** bug · **Subsystem:** Engine · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-core/src/scheduled.rs:57-62`

**Finding.** Interval schedule advancement computes Timestamp(after.0 + (seconds * 1000)) with no overflow guard. Both `seconds * 1000` and the subsequent add are unchecked u64 ops. A large interval (attacker- or misconfig-controlled via create_cron_job) overflows in debug (panic) or wraps in release, producing a next_run in the past that makes the cron fire every tick (busy loop) or far in the future (never fires). The sibling scheduled-job path already uses saturating_add (scheduled_job_from_request, engine/scheduler/scheduled_jobs.rs:230), so this is an inconsistent, fixable gap.

**Fix direction.** Use after.0.saturating_add(seconds.saturating_mul(1000)). Optionally validate/clamp the interval at create_cron_job time.

**Verification evidence.** Re-read scheduled.rs:57-62: `Timestamp(after.0 + (seconds * 1000))` is genuinely unchecked u64 arithmetic; Timestamp is `pub u64` (types.rs:394) and CronSchedule::Interval{seconds:u64} (scheduled.rs:52) is deserialized straight from HTTP JSON (http/scheduling.rs:76-89) with zero validation in cron.rs:124-135 or the handler. Confirmed sibling scheduled_job_from_request uses saturating_add (scheduled_jobs.rs:228). next_after is called both at creation (cron.rs:125) and every fire (scheduler.rs:244,275). So the bug is real and the fix is trivially consistent. Downgrading from high: overflow requires a pathological interval (~1.8e16 seconds since after.0 is a ~1.7e12 ms epoch), impact is self-scoped to the misconfiguring tenant (debug panic in its tick/handler, or release-wrap busy-loop firing every tick), and there is no data corruption or cross-tenant blast radius. A real bug worth fixing, but 'high' overstates exploitability and reach.

#### `B4-3` — process_due_jobs_async abandons remaining claimed jobs when bookkeeping fails
**Severity:** medium · **Dimension:** bug · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/scheduler.rs:214-219`

**Finding.** After executing a claimed job, the loop calls record_scheduled_job_result_async(...).await? and complete_scheduled_job_async(...).await? with `?`. If either bookkeeping op errors mid-batch (transient storage error, contention), the `?` returns from the whole function, leaving every subsequent already-claimed job in the batch un-executed and un-completed for this tick. Those jobs were claimed-to-running, so they rely on recover_running_jobs to be retried later, but the per-tick batch makes no forward progress on the tail and one flaky job can starve the rest. The loop is serial per tenant, amplifying the impact.

**Fix direction.** Per-job error isolation: log/record the bookkeeping failure for the offending job and continue the loop instead of `?`-propagating, so the remaining claimed jobs in the batch still complete this tick.

**Verification evidence.** Re-read scheduler.rs:169-222: process_due_jobs_async loops serially over the claimed batch and uses `?` on both record_scheduled_job_result_async (214-216) and complete_scheduled_job_async (217-219); a mid-batch error returns from the whole function, leaving the already-claimed-to-running tail un-executed and un-completed for the tick. The finding says these 'rely on recover_running_jobs to be retried later' — re-reading coordination.rs:199-201 shows recovery is explicitly restricted to startup/unloaded-tenant activation ('Once a tenant is already loaded, the live scheduler owns claim state and provider wake paths must not requeue in-flight jobs'), so for an already-loaded tenant the stranded tail is NOT recovered until process restart or tenant unload/reload. Impact is thus at least as bad as described. Held at medium: the trigger is a storage error mid-batch (uncommon), the next tick still makes progress on newly-due jobs, and impact is per-tenant.

#### `B4-4` — claim_due_jobs claims every due job with no batch cap (unbounded per-tenant fanout)
**Severity:** medium · **Dimension:** optimization · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/engine/scheduler/scheduled_jobs.rs`

**Finding.** The scheduled-job claim path claims ALL jobs whose run_at <= now for a tenant in a single tick, then process_due_jobs_async executes them serially in one tenant tick. A backlog (after downtime, or a tenant scheduling a large burst at the same timestamp) produces an unbounded in-memory claimed batch and a single tenant monopolizing its tick for an arbitrarily long span, blocking that tenant's later ticks and inflating tail latency. The per-tenant fanout has tick-level parallelism across tenants (scheduler_tenant_tick_parallelism) but no per-tenant batch bound.

**Fix direction.** Add a max-claim batch size (e.g. claim up to N due jobs per tenant per tick) so a backlog drains across ticks with bounded memory and bounded per-tick hold time; remaining due jobs are picked up on the next tick.

**Verification evidence.** Re-read the engine entrypoint scheduled_jobs.rs:73-93 (claim_due_jobs/_async just forward to store.claim_due_jobs) and the backend implementations: redb scheduler/jobs.rs:19-68 ranges `..=upper` with no cap, and postgres write.rs:735-786 uses `WHERE run_at <= $1 ORDER BY run_at, id` with no LIMIT (same pattern across mysql/libsql/sqlite per grep). process_due_jobs_async (scheduler.rs:169-222) then executes the entire claimed set serially within one tenant tick. So a backlog or same-timestamp burst yields an unbounded claimed batch monopolizing that tenant's tick. Real concern; medium/optimization is fair given cross-tenant parallelism (scheduler_tenant_tick_parallelism, scheduler.rs:98-104) bounds global impact and the harm is self-scoped to the affected tenant.

#### `B4-5` — lock_tenant_load_gate_blocking busy-waits with try_lock + yield_now
**Severity:** medium · **Dimension:** code-smell · **Subsystem:** Engine · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-engine/src/engine/mod.rs:264-271`

**Finding.** The blocking tenant-load gate spins: it repeatedly try_lock()s the gate and calls yield_now() on failure rather than blocking on the mutex/condvar. Under contention (many concurrent loads of the same tenant) this burns a CPU in a hot spin loop instead of parking the thread, wasting cycles and degrading under load. A real blocking acquire (std Mutex::lock or a parking primitive) would be both simpler and cheaper.

**Fix direction.** Replace the try_lock+yield_now spin with a genuine blocking acquire (Mutex::lock / Condvar park), or document why a bounded spin is required here if the gate is held only for trivially short critical sections.

**Verification evidence.** Re-read mod.rs:264-271: lock_tenant_load_gate_blocking spins with try_lock() + std::thread::yield_now() on the tokio::sync::Mutex tenant_load_gate (declared AsyncMutex at mod.rs:62). This exists because sync callers (tenants.rs:27,113,188 create_tenant/get-or-load paths) cannot .await the async mutex's lock(); async callers correctly use .lock().await (tenants.rs:48,140,236; coordination.rs:145). The spin is a genuine code smell that burns CPU under simultaneous tenant-load contention, though yield_now mitigates a pure busy-wait. Held at medium/code-smell: the gate is only touched on infrequent tenant-lifecycle operations (not a hot mutation path) and is held briefly, so sustained contention is unlikely; the cleaner fix is a sync primitive or running the load off a blocking task.

#### `C1-1` — fs stat(follow_symlink) and readLink confine via lexical-only path check, bypassing symlink resolution used by reads
**Severity:** medium · **Dimension:** seam · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/fs.rs:285`

**Finding.** op_nimbus_runtime_stat / _sync (fs.rs:285-342) and op_nimbus_runtime_read_link / _sync (fs.rs:769-816) resolve the path with ensure_read_metadata_path -> ensure_read_path_lexical (runtime_capabilities.rs:195-208), which only normalizes '..' lexically and never canonicalizes. ensure_within_roots (runtime_capabilities.rs:230) then checks the symlink's own in-root path and passes. With follow_symlink:true, op_nimbus_runtime_stat calls tokio::fs::metadata / std::fs::metadata (fs.rs:300,329), which follows a symlink that lives inside an allowed root but points outside it, returning metadata about an out-of-root target; read_link returns the raw out-of-root target string. This is inconsistent with the canonicalizing confinement used for actual reads/opens (ensure_module_read_path at runtime_capabilities.rs:189 and check_open). The leak is metadata-only (size/mtime/mode, link target path), not file contents, but it is a real confinement asymmetry across two adjacent code paths.

**Fix direction.** Make the follow_symlink:true stat path and readLink canonicalize and re-check the resolved target against read_roots (or, for stat, fall back to symlink_metadata when the canonicalized target escapes), so metadata/link-target visibility matches the same root confinement as content reads.

**Verification evidence.** Re-read fs.rs:285-342 (stat/_sync) and 767-816 (read_link/_sync): both resolve via ensure_read_metadata_path/ensure_read_path_lexical. Re-read runtime_capabilities.rs:189-208: ensure_read_path_lexical calls normalize_absolute_path_lexically (735-770), which only handles '.'/'..' lexically and never touches the filesystem, then ensure_within_roots (230) does a prefix check on that lexical path. ensure_module_read_path (189) and write/symlink paths use canonicalize_preserving_missing_suffix(_from_base) (772-817) which DO realpath through symlinks. Confirmed the read-file op (fs.rs:38) goes through deno permissions check_open, and the deno fork's CheckedPath (permissions/lib.rs:304-343) carries a canonicalized flag / realpath-aware resolution. So a symlink living inside an allowed root but pointing outside it passes the lexical stat/readLink guard and metadata()/read_link() follow it out-of-root, while the content-read path is canonicalized. Real confinement asymmetry across adjacent ops; the only existing test (1192-1213) covers '..' lexical denial, not symlink following, so it is not guarded elsewhere. Downgraded from high to medium: the leak is strictly metadata (size/mtime/mode) plus the raw link-target string, never file contents or write access, so impact is bounded.

#### `C1-3` — Process-global shared worker env map is cross-tenant mutable state
**Severity:** medium · **Dimension:** seam · **Subsystem:** Runtime · **Verification:** confirmed on independent re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/env.rs:8`

**Finding.** NIMBUS_SHARED_WORKER_ENV is a static LazyLock<Mutex<BTreeMap<String,String>>> (env.rs:8). op_nimbus_runtime_shared_env_seed/get/set/delete/snapshot (env.rs:43-118) read and mutate this single process-wide map, gated only by ascii-name validation (is_valid_shared_env_name) with no tenant/runtime scoping and no capability (grant) check. Any isolate running in the process can read and overwrite values another isolate seeded. Distinct from op_nimbus_runtime_env_get (env.rs:13), which is correctly allowlist+capability gated through InstalledRuntimeCapabilityPolicy. If multiple tenants ever share one host process this is a confidentiality/integrity cross-tenant channel; even single-tenant, it is unscoped ambient mutable state in a crate whose whole design is per-invocation capability isolation.

**Fix direction.** Move the shared worker-env map into per-runtime op_state (keyed/owned by the runtime instance) instead of a process static, and gate access on an env capability/grant rather than name-shape validation alone.

**Verification evidence.** Re-read env.rs:1-119: NIMBUS_SHARED_WORKER_ENV is a process-global static LazyLock<Mutex<BTreeMap>> (8-9); shared_env_seed/get/set/delete/snapshot (43-118) read/mutate it gated only by is_valid_shared_env_name (36-41), with no tenant/runtime/grant scoping - unlike op_nimbus_runtime_env_get (13-25) which is capability-gated via permissions.check_env. The multi-tenant-in-one-process premise is substantiated: warm_pool.rs defines RuntimePoolPartitionKey with exact_service_grants (25-31), i.e. isolates carrying different service grants are pooled in the same OS process, while the shared env map is keyed by nothing. JS side (node22_runtime_bootstrap.js:3295-3327, 3710-3717) shows this implements Node SHARE_ENV semantics and seed replaces the whole global map (env.rs:55-57). Confirmed as real unscoped ambient cross-isolate mutable state. Held at medium (not higher): exploitation requires SHARE_ENV opt-in and co-resident isolates, and the channel only exposes env values an isolate itself chose to place in shared env.

#### `C3-2` — node-compat require() loader silently bypasses the Deno permission check inside Nimbus-owned roots
**Severity:** medium · **Dimension:** safety · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-runtime/src/node_compat.rs:144-158`

**Finding.** ScopedNodeRequireLoader::ensure_read_permission first runs the embedder path policy (`ensure_module_read_path`, node_compat.rs:140-143), then calls `permissions.check_open(...)`. On the Err arm it discards the permission denial and returns the canonicalized path anyway: `Err(_) => { Ok(Cow::Owned(canonical_path)) }` (node_compat.rs:150-157). This demotes the Deno PermissionsContainer from an enforced gate to advisory for CommonJS reads — any path the embedder path policy canonicalizes is readable regardless of the runtime permission snapshot. The comment frames this as a compat-harness staging concession, but the loader is the production require() loader, so the bypass is not test-only. The two gates are meant to be defense-in-depth; collapsing to a single gate on the Err path weakens the trust boundary.

**Fix direction.** Do not swallow the permission Err. Either propagate the JsErrorBox so the Deno permission snapshot remains authoritative, or replace the snapshot-vs-path-policy seam with an explicit, narrowly-scoped allow-list of staged roots that the permission container itself is granted at construction time, so there is a single source of truth instead of an Err-arm escape hatch.

**Verification evidence.** Re-read node_compat.rs:134-159: ensure_read_permission runs path_policy.ensure_module_read_path() then permissions.check_open(); the Err arm returns Ok(Cow::Owned(canonical_path)), swallowing the Deno permission denial. Confirmed this is the production require() loader (build_node_init_services wired unconditionally for node-compat at extensions.rs:94-98, not test-gated). The bypass has real effect: runtime_capabilities.rs:474-492 derives Deno allow_read from the same read_roots only when ambient authority is allowed, and allows_configured_ambient_authority() is true ONLY for Action (runtime_capabilities.rs:38-40) — so under Query/Mutation profiles allow_read is None (Deno denies all reads) while the Err arm still permits reads. However, the claim that this makes the gate 'advisory' for arbitrary paths is overstated: ensure_module_read_path (runtime_capabilities.rs:189-193 -> ensure_within_roots:230-248) is itself an ENFORCED capability boundary returning CapabilityDenied outside read_roots. The bypass is therefore bounded to read_roots, demoting the second defense-in-depth layer rather than removing capability enforcement. Real safety weakening but not an unbounded-filesystem bypass; high overstates it.

#### `C3-3` — render.rs is a 1630-line single god-function building JS via nested format! macros
**Severity:** medium · **Dimension:** modularity · **Subsystem:** Runtime · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-runtime/src/runtime/bootstrap/ops/test_runtime/render.rs:1-1630`

**Finding.** render_runtime_test_spawn_bundle_source spans essentially the entire 1630-line file, assembling a large JS bundle through deeply nested format! string concatenation (including an `eval(__nimbusEvalSource)` site at render.rs:209). At 1630 lines the file is in the 1500-1999 band that CLAUDE.md requires to carry an explicit justification in the owning active plan, and there is no such justification recorded. A single monolithic string-builder of this size is hard to review for correct JS escaping/interpolation and resists targeted change.

**Fix direction.** Decompose by concept (parent harness scaffold, child-process bootstrap, message-port wiring, eval shim) into named sub-renderers returning composed fragments, or move the static JS into versioned template assets interpolated with a small typed context. Record an ownership justification in the owning plan if it must stay unsplit.

**Verification evidence.** wc -l reports exactly 1630 lines. Re-read render.rs: the only top-level Rust fn (grep for `^fn`/`^pub.. fn`) is render_runtime_test_spawn_bundle_source at line 10, which spans essentially the whole file building a JS bundle via nested format! concatenation; the apparent extra `fn`/`const` lines from a naive grep are JS inside raw string templates. The eval site is real (an `eval(__nimbusEvalSource)` JS template at render.rs:209, wrapping the user's own test bundle source in the spawned test subprocess). 1630 is in CLAUDE.md's 1500-1999 band that requires an explicit justification in the owning active plan; no such justification was located. Modularity/medium is consistent with the repo's own framing.

#### `C3-5` — CPU-usage ops write out[0]/out[1] with no length check on the #[buffer] slice
**Severity:** medium · **Dimension:** bug · **Subsystem:** Runtime · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:350-375`

**Finding.** op_host_get_worker_cpu_usage (worker_threads.rs:350-367) and op_current_thread_cpu_usage (369-375) both unconditionally index `out[0]` and `out[1]` on a `#[buffer] out: &mut [f64]` supplied by JS, with no `out.len() >= 2` guard. If the JS caller ever passes a shorter (or zero-length) typed array, this panics inside the op, which aborts/poisons the isolate rather than returning a clean error. The values are also written non-atomically relative to the surrounding TypedArray, but the immediate defect is the unchecked index.

**Fix direction.** Guard with `if out.len() < 2 { return; }` (or return a Result error) before writing, and ideally pin the contract by asserting the expected length once at the binding layer that allocates the buffer.

**Verification evidence.** Re-read worker_threads.rs:350-375: op_host_get_worker_cpu_usage and op_current_thread_cpu_usage both unconditionally write out[0]/out[1] on a `#[buffer] out: &mut [f64]` with no `out.len() >= 2` guard. #[op2(fast)] buffer params can be empty/detached, and a Rust panic on the fast-op path propagates across the V8/FFI boundary rather than becoming a clean JS error — a real robustness defect. Reachability caveat: the JS shim that allocates the buffer for process.threadCpuUsage()/worker.cpuUsage() is not in-tree (lives in the nimbus/deno fork) and under normal operation supplies a fixed length-2 Float64Array, so this is a latent/defensive defect rather than something I could prove is directly guest-triggerable from in-repo evidence (could not locate any in-tree caller passing a short buffer, nor a definitive op-scrubbing step for untrusted guests). The unchecked index on an ABI buffer param is a genuine in-scope bug and the fix is trivial; medium is defensible.

#### `D1-2` — Authorization + permission-claim scaffolding duplicated near-verbatim across three http modules
**Severity:** medium · **Dimension:** modularity · **Subsystem:** Server · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-server/src/http/services.rs:1190-1202`

**Finding.** services.rs, sessions.rs, and sandboxes.rs each independently define their own PrincipalClass enum (identical 3 variants + as_str), principal_claim_string (byte-identical), *_principal_class_from_principal (identical match incl. operator->forbidden), *_permission_values, *_permission_actions_allow, *_permission_scope_allows, format_millis_rfc3339 (byte-identical), and authorize_operator_*_route (identical Authorized/Missing/Invalid-local_admin/Revoked/Expired/Invalid match arms differing only in audit strings). principal_claim_string alone appears 3x; format_millis_rfc3339 appears 3x; the operator-extraction match appears 3x. This is hundreds of lines of copy-forwarded logic, so a policy fix (e.g. a new ExtractedServerAccessStatus arm) must be made in three places and can silently drift between resource families.

**Fix direction.** Extract a shared http::authz module: one PrincipalClass, one principal_claim_string, one format_millis_rfc3339, one operator-credential extraction helper returning a normalized outcome, and a generic permission-scope evaluator parameterized over claim-name sets. service_grants.rs is the existing precedent for a shared helper module.

**Verification evidence.** Verified the duplication directly. principal_claim_string is byte-identical across services.rs:1190-1202, sessions.rs:974-986, sandboxes.rs:638-650. format_millis_rfc3339 is byte-identical across services.rs:1348-1356, sessions.rs:1096-1104, sandboxes.rs:729-737. The PrincipalClass enum (3 variants Operator/Tenant/SpawnedWorkload + identical as_str mapping) is replicated as PrincipalClass/SessionPrincipalClass/SandboxPrincipalClass (services.rs:25-39, sessions.rs:23-37, sandboxes.rs:24-30). *_principal_class_from_principal is logic-identical incl. the operator->forbidden arm and the four claim-name aliases (services.rs:1162-1188 vs sandboxes.rs:610-636), differing only in enum name and one error-message word. authorize_operator_*_route share the same ExtractedServerAccessStatus match skeleton (Authorized/Missing/Invalid-local_admin/Revoked/Expired/Invalid) across services.rs:1072-1140 and sandboxes.rs:458-527, plus a sessions variant. The claim 'differing only in audit strings' is slightly overstated (they also differ in return struct shape and which record_*_authorization_audit helper is called), but the drift-prone control-flow skeleton is genuinely triplicated. The finding's core risk holds: a policy fix such as a new ExtractedServerAccessStatus arm must be made in three places and can silently drift between resource families. Notably http/service_grants.rs already factors a shared pub(super) authorization helper, proving shared extraction is an accepted pattern here, so this is a missed factoring rather than an intentional boundary. Medium is appropriate for duplicated authorization scaffolding in a trust-critical path.

#### `D2-1` — Deploy admin bearer token uses non-constant-time comparison
**Severity:** medium · **Dimension:** safety · **Subsystem:** Server · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-operator/src/access_policy.rs:106`

**Finding.** authorize_deploy_admin_bearer compares the supplied deploy bearer against the expected NIMBUS_DEPLOY_TOKEN with a plain `if token != expected` (byte-by-byte short-circuit) rather than a constant-time comparison. This is a timing side channel on a high-value admin secret that gates the deploy route (deploy_app loads and executes a staged bundle.mjs). It is also internally inconsistent with the local-admin path, which correctly uses ring's constant-time hmac::verify in LocalAdminTokenRecord::authorize. Because the deploy token is a shared secret compared directly, the constant-time path is straightforward.

**Fix direction.** Replace `if token != expected` with a constant-time equality (e.g. ring::constant_time::verify_slices_are_equal, or the same hmac::verify construction used by LocalAdminTokenRecord::authorize) after a length-independent check.

**Verification evidence.** Re-read access_policy.rs:106 — authorize_deploy_admin_bearer uses `if token != expected`, a non-constant-time short-circuiting byte comparison on &str. token.rs:38-43 (LocalAdminTokenRecord::authorize) and access.rs:124-130 (authorize_bearer) both use ring hmac::verify, the documented constant-time path (token.rs:488-493 even has a test asserting hmac::verify usage). The deploy route is reachable: router.rs:703 registers POST /api/admin/deploy -> http::deploy_app, deploy.rs:32 calls authorize_deploy_admin_bearer, and deploy.rs:362-363 writes a staged bundle.mjs for execution. The deploy token comes from env NIMBUS_DEPLOY_TOKEN (router.rs:244), arbitrary user entropy. This is a genuine timing side channel on an admin secret gating code deployment, and is internally inconsistent with the established constant-time pattern; the fix (use a constant-time compare) is trivial. Remote exploitability is hard (network jitter dominates), but medium is justified for an admin-execution secret with an inconsistent insecure comparison.

#### `D4-3` — MongoDB ships weak default admin/admin credentials and no loopback guard on its bind address
**Severity:** medium · **Dimension:** safety · **Subsystem:** Server · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-mongodb/src/lib.rs:58`

**Finding.** AuthConfig::default() returns Self::new("admin".into(), "admin".into()) (crates/nimbus-mongodb/src/lib.rs:58-62), and MongoDbConfig::new / Default use that default (crates/nimbus-server/src/adapters/mongodb/mod.rs:19,31). Unlike the DynamoDB lane — which calls guard_lookup_is_loopback_only before serving to refuse exposing an unauthenticated surface on a network-reachable address (construction.rs:206-212) — the MongoDB wiring (construction.rs:177-201) performs no loopback/address check before binding. with_auth changes credentials but never the bind address. Even after D4-1 is fixed, the trivial default password plus the lack of a loopback guard mean a misconfigured deploy exposes a guessable-credential database to the network.

**Fix direction.** Drop the admin/admin default (require explicit credentials, mirroring DynamoDB's Strict-by-default posture) and add a loopback guard in construction.rs that refuses a non-loopback MongoDB bind unless real credentials are configured, parallel to guard_lookup_is_loopback_only.

**Verification evidence.** Re-read lib.rs:58-62 (AuthConfig::default => new("admin","admin")), adapters/mongodb/mod.rs:15-33 (MongoDbConfig::new and Default both use AuthConfig::default), and construction.rs:177-201 vs the DynamoDB guard at construction.rs:209-212 + listener.rs:64-73. All facts are accurate: weak default admin/admin credentials, and no loopback/address check before binding the MongoDB listener, unlike DynamoDB. But severity hinges on the same reachability gap as D4-1: with_mongodb has zero callers so the listener is never wired into any binary, and MongoDbConfig::new defaults bind_addr to 127.0.0.1 (mod.rs:18). Network exposure of guessable creds requires BOTH a future with_mongodb wiring AND an explicit non-loopback bind_addr override. This is a real latent hardening gap worth fixing alongside the auth gate, but it is not a currently-exposed network surface, so high is not justified; medium.

#### `H1-3` — Restart-on-failure implemented for krun backend but missing in container backend despite shared conmon lifecycle
**Severity:** medium · **Dimension:** gap · **Subsystem:** Sandbox · **Verification:** confirmed on independent re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/container/state.rs:199`

**Finding.** krun's lifecycle.rs implements restart_policy_allows_restart (line 274) and restart_backoff_delay (line 288) and persists/reports a real restart_count (krun/state.rs:199,239). The container backend (container/runtime.rs) has no restart-policy or backoff logic at all and its state summary hardcodes `restart_count: 0` (container/state.rs:199). Both backends drive the same conmon create/start/state/delete lifecycle and accept the same SandboxLifecycleSpec restart_policy, so a SandboxSpec configured with restart_policy != never is silently honored under krun and silently ignored under the container backend. This is a real behavioral divergence, not just duplicated helpers, and is exactly the process-control divergence risk the duplication invites.

**Fix direction.** Lift restart-policy evaluation, backoff, and restart_count accounting into one shared lifecycle module consumed by both backends (the natural home alongside the shared conmon helpers), then have the container backend honor restart_policy and report a real restart_count. Add a per-backend test asserting restart-on-failure occurs (and counts) when restart_policy=on-failure.

**Verification evidence.** Re-read both backends. krun/vm/lifecycle.rs has restart_policy_allows_restart (274), restart_backoff_delay (288), and an active maybe_restart_after_exit (123-152) that reads exit code, checks the spec's lifecycle.restart_policy, applies backoff, increments restart_count, and relaunches; krun/state.rs:199 reports the real self.manifest.restart_count. container/runtime.rs has no restart-policy/backoff logic: detect_runtime_status (501-507) just returns SandboxStatus::Failed on a nonzero exit and never restarts, and container/state.rs:199,213 hardcode restart_count: 0. Both consume the same SandboxLifecycleSpec.restart_policy (spec.rs:618; container/state.rs:160,221). A spec with restart_policy != Never is honored under krun and silently ignored under the container backend -- a genuine, silent cross-backend behavioral divergence. Medium is justified (functional gap, not security).

#### `H1-4` — conmon lifecycle helpers duplicated nearly verbatim across container and krun backends
**Severity:** medium · **Dimension:** modularity · **Subsystem:** Sandbox · **Verification:** confirmed on independent re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs:320`

**Finding.** spawn_background, run_status_checked, runtime_state, wait_for_runtime_state, signal_process, read_pid, wait_for_path, read_exit_code, ensure_linux_host, configured_stop_signal, configured_stop_timeout, detect_runtime_status, slugify, merge_env_overrides, env_key, resolve_root_spec, resolve_process_spec are defined as free functions in both container/runtime.rs (lines 972-1378, 1001-1115) and krun/vm/lifecycle.rs (lines 91-462) + krun/vm/launch.rs (lines 356-624), with the same signatures and essentially the same bodies. This is the structural cause of H1-3 (the two copies have already drifted on restart handling). runtime.rs is also 1388 lines (within threshold but trending high) largely because it re-hosts this shared logic.

**Fix direction.** Extract a concept-owned shared module (e.g. backends/conmon/lifecycle.rs and backends/conmon/spec_resolve.rs) holding the process-control, runtime-state polling, pidfile/exit-code reading, and env/root/process resolution helpers; have both backends call it. This removes the drift surface and is the right home for the unified restart policy from H1-3.

**Verification evidence.** Verified all 17 named helpers exist as free functions in both container/runtime.rs and the krun vm modules (lifecycle.rs/launch.rs). Diffed representative bodies: spawn_background is byte-identical; signal_process and wait_for_runtime_state differ only by a single blank line; read_exit_code differs only trivially in return phrasing -- confirming 'near-verbatim' rather than merely shared names. runtime.rs is 1388 lines (within the <1500 acceptable band, 'trending high' as stated). The duplication is the structural cause of H1-3's restart-handling drift (the logic was added to one copy only). For a pre-launch enterprise-grade codebase that explicitly calls for concept-owned shared helpers, medium is defensible.

#### `H2-1` — Sync registry/catalog methods call futures::executor::block_on on async backend futures, blocking a Tokio worker from an async handler
**Severity:** medium · **Dimension:** bug · **Subsystem:** Sandbox · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-services/src/manager/registry.rs:71`

**Finding.** RuntimeServiceRegistry::teardown_tenant (manager/registry.rs:71,77,81,86), RuntimeServiceRegistry::resolve_service_binding -> ServiceManager::refresh_handle (manager/handles.rs:27), and the ServiceInstanceCatalog::service_instances_for_tenant loop (manager/catalog.rs:35 -> refresh_handle) all run async SandboxBackend futures via futures::executor::block_on. The production CLI backend ForwardedMachineApiSandboxBackend (crates/nimbus-bin/src/machine/backend.rs:57-89) implements inspect/stop/start as `tokio::task::spawn_blocking(...).await`. block_on parks the calling thread on a private executor; the async delete_tenant Axum handler (crates/nimbus-server/src/http/tenants.rs:43-45) calls teardown_tenant directly on a Tokio worker thread, so block_on synchronously blocks a runtime worker while spawn_blocking offloads — starving the async runtime under load. With no entered runtime, spawn_blocking panics outright. Fully async siblings already exist (refresh_handle_async, start_service_async, the ensure_service_binding_async override), so the sync block_on path is avoidable.

**Fix direction.** Make teardown_tenant async on the trait (add teardown_tenant_async and route delete_tenant through .await) and serve resolve_service_binding / service_instances_for_tenant via the async refresh path. Reserve a synchronous, snapshot-only fast path (no backend I/O) for callers that genuinely cannot await; never run a spawn_blocking-backed future under futures::executor::block_on on a Tokio worker.

**Verification evidence.** Re-read manager/registry.rs:61-94 (teardown_tenant uses futures::executor::block_on on sandbox_backend.stop/.remove_tenant_artifacts and record_service_handle), handles.rs:20-50 (refresh_handle block_on of inspect), catalog.rs:33-46 (service_instances_for_tenant loops refresh_handle), nimbus-bin/src/machine/backend.rs:72-89 (spawn_machine_api_operation wraps tokio::task::spawn_blocking(...).await), and nimbus-server/src/http/tenants.rs:37-49 (async delete_tenant calls the sync teardown_tenant directly, before delete_tenant_async). router.rs:48-62 confirms a configured ServiceManager IS the runtime_service_registry, so the block_on impl is the production path. The blocking-in-async smell is REAL: futures::executor::block_on parks the calling Tokio worker thread for the whole teardown, an anti-pattern that can degrade throughput under concurrent deletes, and fully-async siblings (refresh_handle_async, start_service_async, ensure_service_binding_async) already exist while teardown_tenant has no async sibling on the trait (registry.rs:67). HOWEVER the high-severity framing overstates impact: futures::executor::block_on runs the future on the current thread WITHOUT leaving the Tokio runtime context, so spawn_blocking finds the entered runtime via thread-local Handle and does NOT 'panic outright' on the cited Axum path, and it does not deadlock (Tokio's blocking pool is separate from worker threads). The 'no entered runtime, spawn_blocking panics' claim is only true for a thread with no runtime context, which the cited reachable path never is. Real bug, but mischaracterized failure mode and over-severe; medium.

#### `H2-2` — teardown_tenant aborts on first backend stop error, leaving partially-stopped tenant and blocking tenant deletion
**Severity:** medium · **Dimension:** gap · **Subsystem:** Sandbox · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-services/src/manager/registry.rs:69`

**Finding.** teardown_tenant iterates tenant handles and standalone sandboxes, propagating the first stop/remove_tenant_artifacts error via `?` (manager/registry.rs:71-94) before any in-memory state is cleared (state mutation only happens at lines 96-113 after all backend calls succeed). A single sandbox that fails to stop aborts the whole teardown: earlier sandboxes are already stopped in the backend, none of the tenant's in-memory handles/definitions/sessions are cleared, and because delete_tenant calls teardown_tenant before delete_tenant_async (crates/nimbus-server/src/http/tenants.rs:43-48), the tenant document deletion never runs. There is no best-effort accumulation or partial-progress recording.

**Fix direction.** Accumulate per-sandbox stop errors instead of early-returning, attempt every stop + artifact removal, clear in-memory state for successfully-stopped resources, and return an aggregate error only after best-effort cleanup so a single stuck sandbox cannot wedge tenant deletion.

**Verification evidence.** Re-read manager/registry.rs:61-115. The two backend-stop loops and remove_tenant_artifacts (lines 69-94) all use `?` to propagate the first SandboxError before any in-memory state is mutated (state lock + removals only at lines 96-113). A single sandbox that fails to stop aborts the whole teardown with earlier sandboxes already stopped in the backend, none of the tenant's handles/definitions/sandbox_resources/sessions cleared, and (per nimbus-server/src/http/tenants.rs:43-48, where teardown_tenant precedes delete_tenant_async) the tenant document deletion is skipped. There is no best-effort accumulation or partial-progress recording. Real partial-failure/correctness gap. Medium is appropriate: it requires a backend stop error to trigger and the operation can be retried, so not high; but it leaves the tenant in an inconsistent, undeletable state with no recovery, so not low.

#### `I1-2` — redb DEK rotation renames new DB over original before manifest is rewrapped, bricking on crash
**Severity:** medium · **Dimension:** bug · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bin/src/encryption/rotate.rs:475-489`

**Finding.** rotate_redb_dek renames the freshly re-encrypted temp DB over the live path (line 475) BEFORE writing the rewrapped key manifest (line 478). A crash or power loss in that window leaves a database encrypted under new_dek while the on-disk manifest still wraps current_dek — the DB is unopenable. The backup restore only runs inside the manifest-write Err branch (481-487); it does not cover a process death between the rename and the manifest write.

**Fix direction.** Write the rewrapped manifest to a temp path and fsync it BEFORE renaming the DB into place, or stage both and rename atomically (DB then manifest) with a recovery marker; ensure any crash leaves either fully-old or fully-new consistent state.

**Verification evidence.** Re-read rotate_redb_dek (rotate.rs:433-492). Ordering is exactly as claimed: std::fs::rename(temp_path -> path) at 475 makes the on-disk DB encrypted under new_dek, then write_rotated_manifest at 478 rewraps the manifest. A crash between 476 and the manifest's write_for (rotate.rs:548-550) leaves DB@new_dek with manifest wrapping current_dek, which is unopenable. The restore is only inside the manifest-write Err branch (481-487) and does not cover process death in the rename->manifest gap. reencrypt_redb_pages sync_all's the temp before rename (rotate.rs:735-737), so the temp is durable but the manifest is not yet updated. Downgrade rationale: this is an explicit operator-run `encryption rotate-dek` command (reachable via rotate.rs:150), the window is sub-second, a .bak backup is created by default unless --skip-backup (backup_single_file 554-559), enabling manual recovery, and the identical rekey-then-manifest ordering exists in the SQLite path (379-431) so it is a consistent design relying on operator backups, not a redb-specific oversight. Real durability gap but not high.

#### `I1-5` — Hand-rolled Unix-socket HTTP client treats read timeout as successful EOF
**Severity:** medium · **Dimension:** bug · **Subsystem:** CLI · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-bin/src/machine/client.rs:417-424`

**Finding.** read_unix_http_request's read loop breaks out of the loop on WouldBlock/TimedOut and then proceeds as if the response body ended cleanly, rather than surfacing a timeout error. A slow or stalled machine-api response is silently parsed as a truncated-but-complete reply, which can yield a misleading success or a confusing parse error instead of an explicit timeout.

**Fix direction.** Distinguish a true 0-byte EOF (Ok(0)) from a WouldBlock/TimedOut read and return an explicit timeout Error in the latter case rather than breaking the loop as if complete.

**Verification evidence.** Re-read read_unix_http_request (client.rs:366-444). The read loop (413-433) matches WouldBlock|TimedOut (417-421) and breaks (423), then falls through to the same path as Ok(0) clean EOF; the only post-loop guard is response.is_empty() (435). With HTTP/1.0 no-keep-alive (request built at 386), normal termination is server-close (Ok(0)); a stall fires the socket read timeout (set ~380-384) and the partially-accumulated buffer is handed to parse_http_json_body (446+), which will either find no \r\n\r\n and error generically, or parse a truncated-but-superficially-complete body. A timeout is silently conflated with success/clean-EOF rather than surfaced as a timeout error. Confirmed; medium is right since this is a localhost machine-api control socket affecting reliability/diagnostics, not a security boundary.

#### `I1-7` — Triplicated persistence-config field boilerplate across three parallel structs
**Severity:** medium · **Dimension:** code-smell · **Subsystem:** CLI · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-bin/src/start/config.rs:99-160,571-709`

**Finding.** PersistenceFileConfig, PersistenceEnv, and ResolvedPersistenceInputs each redeclare the same field set, and the merge logic hand-threads every field through command>env>file .or() chains. Adding or renaming a persistence field requires synchronized edits in three places plus the merge, an error-prone duplication. (The PoolConfig overwrite-with-None was verified to be a harmless no-op, not a bug.)

**Fix direction.** Collapse the three parallel structs into one field set parameterized by source, or generate the merge with a macro/helper so each field is declared once and merged uniformly.

**Verification evidence.** Re-read config.rs:99-160 (PersistenceFileConfig and PersistenceEnv declare the identical ~28-field set) and ResolvedPersistenceInputs::from_sources (571-710), which hand-threads every field through command.or(env).or(file) / or_else clones. A third struct (ResolvedPersistenceInputs) repeats the same field names again. Adding/renaming a persistence field requires synchronized edits across all three structs plus the merge, an error-prone duplication exactly as described. The structs do serve distinct roles (serde Deserialize with deny_unknown_fields vs manual env load vs non-optional resolved), so they cannot collapse trivially, but the field-by-field merge could be macro/derive-driven. Real, sizable maintainability smell; medium for a code-smell dimension is defensible given the breadth (~28 fields x 3).

#### `I2-1` — Deploy admin bearer token compared with non-constant-time `!=`
**Severity:** medium · **Dimension:** safety · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-operator/src/access_policy.rs:106`

**Finding.** `authorize_deploy_admin_bearer` rejects a mismatch via `if token != expected` (a short-circuiting String comparison) for a security-sensitive deploy bearer secret. This is reachable from the real HTTP deploy endpoint (`crates/nimbus-server/src/http/deploy.rs:32`) that guards deploy/code-push operations. It contradicts the rest of the crate, which deliberately uses `ring::hmac::verify` for constant-time comparison of the local admin token (`token.rs:42`) and session signatures (`access.rs:409`). A timing side-channel on the deploy admin token is a credential-disclosure risk.

**Fix direction.** Compare the candidate against `expected` in constant time (e.g. `ring::constant_time::verify_slices_are_equal` after a length-normalizing step, or the same double-HMAC pattern used by `LocalAdminTokenRecord::authorize`).

**Verification evidence.** Re-read access_policy.rs:106 (`if token != expected`) — a non-constant-time, short-circuiting `&str` comparison of the deploy bearer. Confirmed reachable from the real HTTP deploy endpoint: deploy.rs:32 calls `authorize_deploy_admin_bearer(state.deploy_admin_token.as_deref(), &headers)`, and the token is wired through production config (NIMBUS_DEPLOY_TOKEN / --token / with_deploy_admin_token, confirmed via grep). The contrast is genuine: token.rs:39-43 (`hmac::verify`) and access.rs:409 (`hmac::verify`) deliberately use constant-time comparison. So the inconsistency and reachability are real. However, 'high' overstates impact: a byte-level short-circuit timing difference on a static secret over an HTTP boundary (with request-parsing jitter) is effectively unmeasurable remotely and offers no amplification. Legitimate consistency/defense-in-depth hardening, not a high-severity disclosure vector. Adjusted to medium.

#### `I2-3` — nimbus-machine has zero tests despite owning parsing, env, and fs logic
**Severity:** medium · **Dimension:** test-quality · **Subsystem:** CLI · **Verification:** confirmed on independent re-read

**Location:** `crates/nimbus-machine/src/lib.rs:154`

**Finding.** The 555-line `nimbus-machine` crate has no `#[cfg(test)]` module at all. It contains non-trivial untested branching: `MachineImageSource::parse` (http/docker/absolute-path/implicit-docker classification, lib.rs:154-184), `MachineVolume::parse` (empty/relative-path rejection, lib.rs:199-223), XDG env resolution with platform fallbacks (lib.rs:489-555), and `MachinePaths::ensure_directories` (fs I/O, lib.rs:429-487). The only coverage lives downstream in `nimbus-bin` tests, leaving the crate's own contract unverified and fragile to refactor.

**Fix direction.** Add a `tests` module covering both parsers' success and error branches, the XDG/HOME resolution fallbacks (using injected env rather than process env), and the shared-parent vs fallback behavior of `MachineRootLayout::new`.

**Verification evidence.** Verified via grep: the entire nimbus-machine crate is a single 555-line lib.rs with zero `#[cfg(test)]`/`#[test]` occurrences. The cited untested logic is real and non-trivial: MachineImageSource::parse (lib.rs:154-184, four-way http/docker/absolute/implicit-docker classification), MachineVolume::parse (lib.rs:199-223, empty/relative-path rejection), XDG resolution with HOME/USERPROFILE/HOMEDRIVE fallbacks (lib.rs:489-549), and MachinePaths::ensure_directories (lib.rs:429-487 fs I/O). Coverage exists only downstream in nimbus-bin/nimbus-system/nimbus-server test modules. Medium is appropriate for a test-quality gap on a crate that owns parsing and platform-conditional logic with no in-crate verification.


## Low (116)

#### `E1-10` — compare_json_values collapses NaN and mixed numeric/type ordering to Equal
**Severity:** low · **Dimension:** bug · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-mongodb/src/commands/crud/filter.rs:249-258`

**Finding.** compare_json_values (crud/filter.rs:229-259), used by compound sort, does fa.partial_cmp(&fb).unwrap_or(Ordering::Equal) (line 253) so any NaN sorts as Equal, and na.as_f64().unwrap_or(0.0) (line 251-252) silently coerces an un-representable number to 0.0. The final catch-all arm (line 257) returns Equal for any pair the explicit arms didn't cover. This yields unstable/non-total ordering for documents containing NaN or heterogeneous values in a sort key, producing non-deterministic multi-key sort results.

**Fix direction.** Define a total order with explicit NaN placement (Mongo orders types, then values) and avoid unwrap_or(0.0); reuse the same comparator the engine/BSON sort uses rather than an ad-hoc f64 path.

#### `E1-11` — No test covers the _id-plus-range-operator leniency or the NaN update panic
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Adapters · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-mongodb/src/commands/crud/tests.rs:1`

**Finding.** tests/find.rs exercises $gt/$gte/$lt/$ne only via the general query path; crud/tests.rs seeds _id values u1/u2/u3 but never combines an _id equality with a range predicate, so the E1-3 wrong-result fast path is entirely untested. Likewise there is no test feeding NaN/Infinity to a numeric update operator (E1-4), and no test asserting that an unauthenticated connection is rejected from a data command (E1-1). These are the highest-risk behaviors and have zero coverage.

**Fix direction.** Add tests: (1) find with {"_id":"u1", field:{"$gt":N}} asserting non-matching docs are excluded; (2) $inc/$mul with NaN asserting a BadValue error, not a panic; (3) a data command on an unauthenticated ConnectionState asserting Unauthorized.

**Verification evidence.** Verified the coverage gaps: grep -F '$gt' across crates/nimbus-mongodb/src/commands/crud/tests/ shows range ops only on non-_id fields (find.rs:45,61,77,95,111 on age/name; update.rs:121, delete.rs:36, distinct.rs:54, count.rs:22) — and a grep for any filter containing both '_id' and a '$' operator returns nothing, so the E1-3 _id+range leniency fast path is genuinely untested. No NaN/Infinity appears in any crud test (only in bson_bridge.rs round-trip tests). No test asserts an unauthenticated connection is rejected from a data command (authenticated is referenced in tests only via admin.rs connection_status reporting). The gaps are real, but two sub-claims are weakened: the find.rs location cited is actually crud/tests/find.rs (minor), and the 'NaN update panic' it says is uncovered describes a non-bug (E1-4 is a false positive — the engine returns InvalidInput, so the missing test would assert graceful rejection, not prevent a panic). Real test-quality gap but narrower and lower-stakes than stated; downgrade to low.

#### `E1-8` — $group keys are stringly-typed via format!("{:?}")
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-mongodb/src/commands/aggregation/mod.rs:457`

**Finding.** execute_group builds its grouping HashMap key with format!("{group_key:?}") (aggregation/mod.rs:457), i.e. the Rust Debug rendering of the group-key value is used as the identity. Distinct values whose Debug strings collide (or that should be distinguished by BSON type) can be merged or split incorrectly, and the key is opaque/fragile to Debug-format changes. Group identity should be a structured/typed key, not a Debug string.

**Fix direction.** Use a structured, hashable key type (e.g. an enum over the BSON/JSON value variants) for the group map instead of the Debug-formatted string.

#### `E2-1` — Timestamp precision is inconsistent: document/precondition times truncate to milliseconds while field-value timestamps keep nanoseconds
**Severity:** low · **Dimension:** bug · **Subsystem:** Adapters · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-firebase/src/grpc/write_stream.rs:920-942`

**Finding.** Two distinct timestamp codecs coexist. core_timestamp_from_prost / prost_timestamp_from_core truncate to milliseconds (nanos -> nanos/1_000_000 and back via millis*1_000_000), and they back creation_time, update_time, commit_time, read_time, and WritePrecondition::update_time (write_stream.rs:645, listen_stream.rs:740, plus the many prost_timestamp_from_core callers in unary.rs and listen_stream.rs). Meanwhile field-value TimestampValue encode/decode goes through format_prost_timestamp / parse_rfc3339_timestamp (write_stream.rs:805, 857) which preserve full nanoseconds. The root cause is nimbus_core::Timestamp(pub u64) being millisecond-only (types.rs:394). Consequence: a client that reads a document update_time carrying nanoseconds and round-trips it into an update_time precondition gets silently truncated to ms before comparison, so an optimistic-concurrency precondition can spuriously mismatch (or spuriously match a different version); and a timestamp stored as a field has finer resolution than the document metadata it sits beside. Firestore guarantees microsecond-precision timestamps, so this is a real compatibility/correctness gap, not cosmetic.

**Fix direction.** Pick one precision model end-to-end. Either widen nimbus_core::Timestamp to microsecond (or nanosecond) resolution and make core_timestamp_from_prost / prost_timestamp_from_core lossless to match the field-value path, or (pre-launch, breaking-change-preferred) standardize all six metadata paths and field timestamps on the same sub-second granularity. At minimum, document and test the precondition round-trip at sub-millisecond resolution.

**Verification evidence.** Re-read write_stream.rs:920-965 (both codecs), types.rs:393-394 (Timestamp(pub u64) = 'Milliseconds since Unix epoch'), unary.rs:1308-1309 (create_time/update_time emitted via prost_timestamp_from_core), write_stream.rs:644-645 (UpdateTime precondition lowered via core_timestamp_from_prost), write_batch.rs:51-85 (WritePrecondition stores Timestamp millis on both sides of the comparison), and serializer.rs:14-19 (FirestoreValue::Timestamp(String)). The two-codec asymmetry is real: document/precondition timestamps are millisecond-only while field-value TimestampValue round-trips as an RFC3339 string preserving nanos. BUT the high-severity 'bug' premise is refuted. Every document update_time/create_time the client can read originates from nimbus_core::Timestamp (millis), so prost_timestamp_from_core emits nanos = millis*1_000_000 — always a multiple of 1,000,000 ns, i.e. ms-aligned. Round-tripping such a value back through core_timestamp_from_prost (seconds*1000 + nanos/1_000_000) is therefore LOSSLESS, and the precondition comparison is millis-vs-millis end to end (write_batch.rs). No optimistic-concurrency precondition can 'spuriously mismatch' for any server-issued timestamp; truncation only affects a client-fabricated sub-ms precondition that could never equal any stored ms-aligned update_time anyway. The field-value path stores timestamps as lossless RFC3339 strings, so there is no data-loss there either. What remains is a true-but-cosmetic fidelity/compatibility gap (metadata is ms-precision while Firestore documents microsecond precision, and field timestamps carry finer resolution than the metadata beside them). Real, but not a correctness bug and not high — downgraded to low.

#### `E2-3` — BatchWrite `_labels` serde field is mis-cased and never binds the incoming `labels` key
**Severity:** low · **Dimension:** bug · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/batch_write_request.rs:77`

**Finding.** BatchWriteRequestJson derives #[serde(rename_all = "camelCase")] and declares the field `_labels` (batch_write_request.rs:71-77). camelCase rename turns `_labels` into the JSON key `Labels` (the leading underscore is treated as a word boundary, capitalizing the next segment), so the real Firestore key `labels` is never bound to this field and is simply dropped by serde. The accompanying test parses_writes_and_ignores_labels (batch_write_request.rs:96-122) only asserts database and writes.len(), so it cannot detect that the field is dead. The intent is to ignore labels, so this is not a data-loss correctness bug, but the field is misleading dead code: it implies labels are captured when they are not, and the mis-cased rename is a latent trap if anyone later tries to read parsed labels.

**Fix direction.** If labels are intentionally ignored, drop the field entirely (serde already ignores unknown keys) or mark it #[serde(skip)] / use #[serde(rename = "labels")] with a non-underscore name. Strengthen the test to assert the actual ignore/capture contract rather than only counting writes.

#### `E2-4` — ensure_database_match duplicated across four request/grpc modules
**Severity:** low · **Dimension:** modularity · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/grpc/unary.rs:1318`

**Finding.** Four near-identical ensure_database_match implementations exist (batch_get_request.rs:145, commit_request.rs:377, grpc/unary.rs:1318, grpc/write_stream.rs:615), each validating that a request's database name matches the resolved context. Divergence risk: a fix or tightening (e.g. stricter (default) enforcement) applied to one copy can silently miss the others, producing inconsistent cross-surface behavior between REST and gRPC.

**Fix direction.** Extract a single ensure_database_match (returning a crate-internal error that each surface maps to its own REST/gRPC status) into a shared module (e.g. resource_names.rs or firestore_model.rs) and have all four call sites delegate.

#### `E2-5` — parse_transaction (base64 transaction-token decode) duplicated across three request modules
**Severity:** low · **Dimension:** modularity · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/commit_request.rs:347`

**Finding.** Three copies of parse_transaction perform the same base64 transaction-token decode, differing only in their error enum type (batch_get_request.rs:139, run_query_request.rs:192, commit_request.rs:347). The decode policy (alphabet, padding, error handling) is replicated, so a change to token format would need three synchronized edits.

**Fix direction.** Hoist the decode into one shared helper returning Result<Vec<u8>, TransactionTokenError> and let each module map that into its local error variant via From/map_err.

#### `E2-6` — special_double_from_firestore (with unreachable! arm) and firestore_document_name duplicated verbatim
**Severity:** low · **Dimension:** simplification · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/grpc/write_stream.rs:901-911`

**Finding.** special_double_from_firestore is defined identically in serializer.rs:343-353 and grpc/write_stream.rs:901-911, including the same unreachable!("finite doubles should not map to special doubles") arm. firestore_document_name is defined in both response.rs:108 and batch_get_request.rs:159. These are exact-copy helpers within one crate; the duplicated unreachable! in particular doubles the surface where a future refactor could turn an invariant violation into a panic.

**Fix direction.** Keep one special_double_from_firestore in serializer.rs and re-use it from write_stream.rs; consolidate firestore_document_name into response.rs (or resource_names.rs) and import it in batch_get_request.rs.

#### `E2-7` — Mutex lock poisoning is converted to panic across the write and listen stream registries
**Severity:** low · **Dimension:** safety · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/grpc/write_stream.rs:66`

**Finding.** The std::sync::Mutex guards in the write-stream and listen-target registries unwrap poison with .expect(...) (write_stream.rs:66, write_stream.rs:89, listen_stream.rs:80, listen_stream.rs:98, listen_stream.rs:110). If any thread panics while holding one of these locks, every subsequent stream operation panics instead of degrading, which on a multi-tenant gRPC surface turns a single poisoned registry into a cascading availability failure for all streams sharing it.

**Fix direction.** Decide a poison policy deliberately: either recover via lock().unwrap_or_else(|e| e.into_inner()) since the guarded state is plain registries that remain consistent, or switch to parking_lot::Mutex (no poisoning). Avoid expect on locks that span fallible per-request work.

#### `E4-1` — nimbus-cloud-functions declares tokio but never uses it
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-cloud-functions/Cargo.toml:29`

**Finding.** tokio.workspace = true is in [dependencies] (Cargo.toml:29), but a grep for `tokio` across crates/nimbus-cloud-functions/src and tests returns zero matches. The crate's async fns use bare async/.await driven by the caller's runtime and need no direct tokio dependency. This is a dead dependency that inflates the build graph.

**Fix direction.** Remove the tokio line from [dependencies]; if a future test needs it, add it to [dev-dependencies].

#### `E4-2` — tokio is a production dependency of nimbus-convex but used only in one test
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-convex/Cargo.toml:34`

**Finding.** tokio.workspace = true sits in [dependencies] (Cargo.toml:34). The only tokio reference in the crate is a single #[tokio::test] at auth/verifier/metadata.rs:115; there is no tokio::runtime/spawn/block_on/Handle anywhere in src (grep confirmed). The crate's async fns are runtime-agnostic (they only .await reqwest/engine futures). This unnecessarily pulls the full tokio runtime into the production dependency graph of nimbus-convex.

**Fix direction.** Move tokio from [dependencies] to [dev-dependencies] (next to tempfile at line 37).

#### `E4-4` — Firestore admin GET ignores database_id while writes validate it
**Severity:** low · **Dimension:** gap · **Subsystem:** Adapters · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs:203-225`

**Finding.** Write paths route through firebase_admin_bound_key (firestore.rs:426) which calls validate_default_database_id, rejecting any non-`(default)` database (nimbus-firebase/src/firestore_model.rs:22-29). The read path invoke_firebase_admin_firestore_get_document_cancellable (firestore.rs:203-225) deserializes payload.database_id (struct field at firestore.rs:25) but resolves the target solely via firebase_admin_resolve_document_target (firestore.rs:435), which never validates database_id. A guest passing a non-default database_id is rejected on set/update/delete but silently accepted on get. There is no cross-tenant leak (the locator derives from document_path, which does not encode the database), but the contract is asymmetric and the read-path field is dead.

**Fix direction.** In the get document handler(s), call validate_default_database_id(&payload.database_id, "firebase-admin/firestore database id") before resolving the target, mirroring firebase_admin_bound_key, so reads and writes share one validation contract.

**Verification evidence.** Re-read firestore.rs:203-250 (get sync+async) and 263-414 (set/update/delete) plus firebase_admin_bound_key (426-433), firebase_admin_resolve_document_target (435-441), and firestore_model.rs:22-29. Confirmed the asymmetry: writes call validate_default_database_id via firebase_admin_bound_key; the get path deserializes payload.database_id (field at line 25) but resolves only via document_path and never validates it, so a non-(default) database_id is rejected on set/update/delete but silently accepted on get, and the read-path field is effectively dead. However the finding's own detail concedes there is NO cross-tenant leak (the locator derives from document_path, which does not encode the database). The consequence is a contract inconsistency / dead field on a nonsensical input, with no data-exposure or correctness impact, so medium is over-severe; low is appropriate.

#### `E4-6` — Index read-bound narrowing always replaces on incomparable values
**Severity:** low · **Dimension:** bug · **Subsystem:** Adapters · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-convex/src/subscriptions/transforms/bounds.rs:35`

**Finding.** compare_index_values returns None for cross-type scalar pairs (e.g. Number vs String, firestore.rs n/a; bounds.rs:3-14). Both should_replace_lower_bound and should_replace_upper_bound treat None as `true` (bounds.rs:35,54), so an incomparable candidate always overwrites the current bound. These feed read-set index range derivation in nimbus-server read_tracking/indexes.rs:55-91 (derive_index_read). is_scalar_filter_value (bounds.rs:16) admits null/bool/number/string, so a single index field carrying mixed scalar types can produce a bound replacement that does not actually narrow the range, yielding an incorrect tracked read range and potential missed or spurious subscription invalidations.

**Fix direction.** Treat None (incomparable) as `false` so an incomparable candidate never silently replaces an existing comparable bound, or widen the tracked range conservatively; add unit tests for mixed-type candidates against an existing bound.

**Verification evidence.** Re-read bounds.rs:3-56 (compare_index_values None for cross-type pairs; should_replace_*_bound treat None as true) and is_scalar_filter_value (16-18) — the mechanical claim is accurate. But the correctness/bug impact is largely refuted. (1) Trigger: derive_index_read (indexes.rs:16-116) loops over filters for ONE field; producing the None-replacement requires two range filters of incomparable scalar types on the same field in one query, which is nonsensical — evaluator filtering.rs compare_values (54-72) returns Err for any document whose typed field is compared against the wrong-typed filter value, so such a query errors rather than returning results. (2) Backstop: record_query_read (recording/queries.rs:4-13) records the FULL predicate filter set as a separate predicate dependency ALONGSIDE the index range; read_set.rs dependency_set (37-66) and dependency.rs writes_intersect_dependency_set (289-292) make invalidation a UNION (any dependency match triggers), so a wrong index bound cannot suppress invalidation the predicate dependency would catch. (3) document_matches_predicate_dependency (dependency.rs:456) uses unwrap_or(true), erring toward invalidation on comparison errors. A missed invalidation (the dangerous outcome) is therefore not realistically reachable; worst case is an over-broad bound that over-invalidates. Real but minor; downgrade to low.

#### `E4-7` — Numeric index-bound comparison loses precision via as_f64
**Severity:** low · **Dimension:** bug · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-convex/src/subscriptions/transforms/bounds.rs:7-10`

**Finding.** compare_index_values compares two JSON numbers by mapping both through as_f64 and partial_cmp (bounds.rs:7-10). i64/u64 magnitudes beyond 2^53 are not exactly representable as f64, so two distinct large integer index keys can compare Equal (or order incorrectly), feeding the same read-bound narrowing in nimbus-server read_tracking/indexes.rs:55-91. Combined with E4-6 this affects correctness of tracked read ranges for large-integer index fields.

**Fix direction.** Compare integers via as_i64/as_u64 first and fall back to f64 only for genuine floats (handling mixed i64/f64 explicitly), so large integer bounds order exactly.

#### `E4-8` — unreachable! in apply_builtin_transform is reachable via crate-public API
**Severity:** low · **Dimension:** safety · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-convex/src/subscriptions/transforms/runtime_backed/builtins.rs:34-39`

**Finding.** apply_builtin_transform is re-exported at the crate root (lib.rs:67) and from subscriptions/mod.rs:7 and transforms/mod.rs:8, so it is callable by any sibling crate, not just the internal router. Passing ConvexSubscriptionTransform::RuntimeNamedQuery/RuntimeNamedPaginatedQuery hits unreachable!() (builtins.rs:36-38), panicking instead of returning the function's existing Result<_, String> error channel. The invariant ('runtime transforms are routed elsewhere') is enforced only by convention, not the type system.

**Fix direction.** Return Err("runtime transforms must be resolved before builtin handling") instead of unreachable!(), or restrict apply_builtin_transform to pub(crate)/pub(in subscriptions) and split the runtime variants into a separate non-public type so the panic arm cannot be constructed by callers.

#### `E4-9` — Lock-poison policy diverges within the convex crate (panic vs recover)
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-convex/src/subscriptions/transforms/state.rs:12`

**Finding.** The auth metadata cache recovers from a poisoned RwLock with unwrap_or_else(|poisoned| poisoned.into_inner()) (auth/verifier/metadata.rs:92,103), keeping auth available after an unrelated panic. The subscription-transform state takes the opposite policy, panicking via .expect("...should not be poisoned") on every write (state.rs:12,25,36,47,59) and in resolve_subscription_transform (runtime_backed/selection.rs:12). Both guard simple in-memory maps with no invariant that a poisoned lock would corrupt, so the divergence is unjustified and the panic path can cascade a single transform-handler panic into killing subsequent subscription routing.

**Fix direction.** Pick one policy for non-invariant in-memory maps. Prefer poison recovery (into_inner) consistent with metadata.rs for the transform state and selection paths, or document why subscription state intentionally fails closed.

#### `A1-1` — unsafe MoveFileExW block lacks a SAFETY comment, breaking the crate's own convention
**Severity:** low · **Dimension:** safety · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/encryption/manifest.rs:550`

**Finding.** The Windows variant of move_temp_file_into_place calls MoveFileExW inside an unsafe block with no SAFETY justification (only a higher-level durability rationale elsewhere). This is inconsistent with the documented convention used right next door in encryption/key.rs, where into_wrapped()'s unsafe std::ptr::read carries an explicit '// Safety:' note explaining the ptr::read + mem::forget reasoning. The call dereferences two raw NUL-terminated UTF-16 pointers and relies on both `encode()` results outliving the FFI call; that contract should be documented at the unsafe site. Note this is hygiene, not a hard CI error: undocumented_unsafe_blocks lives in clippy's `restriction` group, not the `all = deny` set the workspace enables.

**Fix direction.** Add a SAFETY comment above the `unsafe { MoveFileExW(...) }` block stating that source/destination are valid NUL-terminated wide-string buffers that outlive the call and that the return value is checked. Mirror the style at crates/nimbus-storage/src/encryption/key.rs:53. Optionally do the same for the test-only env::set_var/remove_var unsafe blocks in encryption/aws_kms.rs.

#### `A1-2` — redb sequence allocation: non-saturating increment plus a redundant NEXT_SEQUENCE re-write
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/store/journal.rs:666`

**Finding.** next_sequence() reads `current`, writes `current + 1` (non-saturating) into NEXT_SEQUENCE_KEY, and returns `current`. The subsequent append_tenant_event_record() then re-writes NEXT_SEQUENCE_KEY to `sequence.0.saturating_add(1)`, which equals the same value next_sequence already stored — a redundant second write of an identical key within the same transaction, and an inconsistent overflow posture (`current + 1` vs `saturating_add(1)`). There is no practical overflow risk on a u64 sequence, so this is purely a clarity/redundancy smell, but the duplicated source-of-truth for the next-sequence cursor invites future drift if one site is edited without the other.

**Fix direction.** Pick a single owner for advancing NEXT_SEQUENCE_KEY. Either have next_sequence reserve-and-return only (and drop the re-write at journal.rs:189), or have only append_tenant_event_record advance it. Use saturating_add consistently at both sites while consolidating.

#### `A2-5` — Boolean inclusive-flag pair should be std::ops::Bound
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/index/scan/adapters.rs:55-77`

**Finding.** The range API threads start_inclusive: bool / end_inclusive: bool (30 occurrences across the subsystem) through composite_range_scan_bounds, index_scan_range, index_scan_composite_range, all four TenantStore/TenantReadSnapshot cancellable adapters, and the HistoricalIndexScanPlan constructors, requiring 6 #[allow(clippy::too_many_arguments)] suppressions (1 in bounds.rs, 4 in range.rs, 11 inclusive-flag/too_many sites in adapters.rs, 6 in history_scan.rs). Adjacent Option<&Value> bound + bool inclusive pairs are exactly std::ops::Bound<&Value> (Included/Excluded/Unbounded), which would collapse two arguments into one self-describing type and remove the too_many_arguments allows.

**Fix direction.** Replace each (Option<&Value> bound, bool inclusive) pair with std::ops::Bound<&Value> (or a small RangeBound newtype). Map at the planner boundary. This removes the boolean params and the too_many_arguments suppressions.

**Verification evidence.** Conceptual claim is valid: composite_range_scan_bounds (bounds.rs:11-19) and the adapters/range signatures thread Option<&Value> start/end + start_inclusive/end_inclusive bool pairs that map exactly onto two std::ops::Bound<&Value>, and no std::ops::Bound is used anywhere in nimbus-storage today. But the quantitative evidence is materially wrong: claimed '6 allows (1 in bounds.rs, 4 in range.rs, 11 sites in adapters.rs, 6 in history_scan.rs)'; actual via grep is 7 total too_many_arguments allows -- 2 in range.rs, 5 in adapters.rs, 0 in bounds.rs, 0 in history_scan.rs. The '30 occurrences' figure is also unsubstantiated (63 raw matches of the flag names). This is a subjective style refactor with inaccurate supporting metrics; downgrade from medium to low.

#### `A2-7` — Historical exclusive-start fallback inverts intent when prefix_end is None
**Severity:** low · **Dimension:** bug · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/index/history_scan.rs:195-202`

**Finding.** historical_range_start_key returns prefix_end(start).or_else(|| Some(Vec::new())) for an exclusive start (history_scan.rs:200). When prefix_end returns None (an all-0xFF encoded value), the correct semantics for an exclusive lower bound is 'no key is strictly greater' -> empty seek range, but the fallback yields Vec::new() = 'scan from the very beginning'. The live composite path handles the analogous case correctly by returning an empty-result sentinel (bounds.rs:28-29). This is currently unreachable because every encoded value begins with a type tag byte < 0xFF, and the byte key is only a coarse seek bound (the provider SQL tuple_bounds is the real gate), so results stay correct — but the start-key intent is inverted and inconsistent with the live path.

**Fix direction.** On the None branch return an empty-range sentinel (or None plus a caller-side short-circuit) so an exclusive start past the keyspace maximum yields no rows, mirroring bounds.rs.

#### `A2-8` — Empty-Vec sentinel for empty composite range result is fragile
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/index/bounds.rs:9-29`

**Finding.** composite_range_scan_bounds signals 'no rows' by returning (match_prefix, Vec::new(), Some(Vec::new())) (bounds.rs:29), and index_scan_composite_range_in_read_txn detects it via start_key.is_empty() (range.rs:112). An empty Vec is an in-band magic value that conflates 'empty start key' with 'empty result', and the type CompositeRangeScanBounds (bounds.rs:9) gives no hint of this contract. A reader adding a new caller of composite_range_scan_bounds could miss the is_empty() check.

**Fix direction.** Return an explicit enum (e.g. enum CompositeRangeScan { Empty, Bounds { match_prefix, start_key, end_key } }) or Option<CompositeRangeScanBounds> = None for the empty case, so the empty result is type-level and cannot be silently ignored.

#### `A2-9` — Order-preserving number transform duplicated across live and historical encoders
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/index/encoding.rs:10-25`

**Finding.** The float-to-order-preserving-bits transform (if value.is_sign_positive() || value == 0.0 { XOR sign bit } else { invert all bits }) is implemented twice: encoding.rs:14-21 produces a [u8;8] for the live byte key, and index_history.rs:21-26 produces a u64 for the historical key. They are algorithmically identical (including the shared -0.0-sorts-as-+0.0 quirk and int/float unification via as_f64), so live and historical index ordering agree today — but the duplication means a future change to one ordering rule could silently desync the two index representations.

**Fix direction.** Extract one canonical fn order_preserving_number_bits(value: f64) -> u64 in nimbus-core and have both encoders consume it (the byte encoder via .to_be_bytes()), so the two index paths cannot diverge.

#### `A3-1` — KeyDirectoryProvider sanitization collides distinct key subjects onto one key file
**Severity:** low · **Dimension:** bug · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/encryption/key_directory.rs:61-65`

**Finding.** key_file_path replaces both ':' and '/' with '_' (`descriptor.replace([':', '/'], "_")`). The subject descriptor is `{kind_tag}:{tenant}:{logical_name}`, so two structurally different subjects map to the same key file whenever a tenant or logical name legitimately contains '_' or '/'. Examples: descriptor `db:acme:a_b` and `db:acme:a:b` both sanitize to `db_acme_a_b.key`; `db:a_b:c` and `db:a:b_c` and `db:a:b:c` all collide too. A collision means two different databases are wrapped/unwrapped under the same wrapping key, silently weakening the per-subject isolation the directory provider is supposed to give, and (worse) one subject can decrypt another's DEK. This is a correctness/safety break in a key-management path, not just an aesthetic concern.

**Fix direction.** Use a collision-free encoding for the on-disk file name instead of lossy character replacement — e.g. hex/base32-encode the full descriptor bytes, or hash it (SHA-256) to a fixed-width name. Bind the original descriptor into the manifest AAD (already partly done via header) so a mismatched file is rejected on unwrap.

**Verification evidence.** Re-read key_directory.rs:61-65 (key_file_path), subject.rs (descriptor/derivation_context), types.rs:414-433 (validate_logical_name), manifest.rs (to_aad + KeyManifest sidecar-per-path), and runtime.rs:113-147 (validate_manifest). The KEK FILE collision is real and reachable: TenantId/logical_name permit '_' ([a-zA-Z0-9_-]), and logical_name is a filename, so tenant 'acme'+file 'a_b.sqlite3' and tenant 'acme_a'+file 'b.sqlite3' both sanitize to db_sqlite_tenant_acme_a_b.sqlite3.key. The MasterKeyFileProvider deliberately uses NUL-separated derivation_context() for HKDF to avoid exactly this, while KeyDirectory uses the naive colon-join + replace — a genuine deviation from the documented per-subject isolation. BUT the headline claims are refuted: (1) each database has its own random DEK and its own sidecar manifest (<path>.nimbus-enc), so colliding subjects do NOT share a DEK; (2) wrap/unwrap AAD = header.to_aad() which includes the FULL unsanitized subject_descriptor (manifest.rs:209-211), which differs between two colliding subjects ('...:acme:a_b...' vs '...:acme_a:b...'), so AES-GCM-SIV authentication fails on cross-unwrap; (3) validate_manifest (runtime.rs:121) rejects any descriptor mismatch before unwrap. So 'one subject can decrypt another's DEK' / 'correctness/safety break in key management' is false. Residual impact is weakened KEK-file isolation + an availability/operational hazard (two subjects depending on one physical key file), not a confidentiality break — low.

#### `A3-2` — GeneratedDatabaseKey::into_wrapped zeroizes a copy, leaving the original plaintext un-zeroed
**Severity:** low · **Dimension:** bug · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/encryption/key.rs:48-60`

**Finding.** into_wrapped does `let mut plaintext_copy = self.plaintext;` which, because `[u8;32]` is Copy, duplicates the key material. It then `std::mem::forget(self)` (so the ZeroizeOnDrop Drop impl never runs on the original `self.plaintext`) and only calls `plaintext_copy.zeroize()`. The result is the opposite of the doc comment 'The plaintext is zeroed as part of the drop': the forgotten original copy is leaked un-zeroed on the stack/heap of the consumed value, and only the throwaway copy is scrubbed. The unsafe ptr::read + mem::forget dance is also unnecessary.

**Fix direction.** Drop the manual unsafe path. Destructure via a safe pattern that moves `wrapped` out and explicitly zeroizes `plaintext` before returning — e.g. take `mut self`, copy wrapped with a safe move, `self.plaintext.zeroize()`, then `mem::forget`/return; or restructure the type so `wrapped` can be moved out without ptr::read. Ensure the *field that is forgotten* is the one that gets zeroized.

**Verification evidence.** Re-read key.rs:14-61. The mechanism described is accurate: plaintext is [u8;32] (Copy); `let mut plaintext_copy = self.plaintext` makes a bitwise copy; `std::mem::forget(self)` suppresses the ZeroizeOnDrop Drop impl on the original; only `plaintext_copy.zeroize()` scrubs the throwaway. So the doc comment 'The plaintext is zeroed as part of the drop' is wrong — drop is forgotten, and the original self.plaintext bytes are left un-scrubbed in the stack slot backing the consumed value. Real defect in a security primitive with a misleading comment. However grep shows into_wrapped is invoked only from crates/nimbus-storage/src/encryption/tests.rs (lines 125,182,291,352,399) — never on a production path — and the leaked bytes are short-lived stack memory reclaimed by subsequent frames. Defense-in-depth gap, not an exploitable leak; severity over-stated at medium, low is justified.

#### `A3-3` — Plaintext DEK escapes zeroize protection as a bare [u8;32] at the runtime boundary
**Severity:** low · **Dimension:** safety · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/encryption/runtime.rs:241`

**Finding.** resolve/generate paths return `*generated.plaintext()` — a Copy of the 32-byte DEK — as a plain `[u8;32]`. Once copied out of GeneratedDatabaseKey, the value is no longer covered by ZeroizeOnDrop and will sit in caller stack frames / move temporaries until overwritten, defeating the careful zeroize work done inside GeneratedDatabaseKey, MasterKeyFileProvider, and the KMS provider. The whole LocalKeyProvider trait surface (unwrap_* methods) also returns bare `[u8;32]`, so this leakage is structural, not a single call site.

**Fix direction.** Return a zeroizing wrapper (e.g. `zeroize::Zeroizing<[u8;32]>` or a dedicated `DataEncryptionKey` newtype with ZeroizeOnDrop) from the provider trait and from resolve/generate, so the plaintext is scrubbed when the storage engine handoff completes. Thread that type through the cipher-construction call sites.

**Verification evidence.** Re-read runtime.rs:241 (Ok(*generated.plaintext())), provider.rs:199-232 (LocalKeyProvider trait returns bare [u8;32] for unwrap), and a representative consumer async_storage/sqlite.rs:203-215 (let dek = resolve_database_encryption_key(...); &dek passed to open_encrypted_*, dek never zeroized at the call site). The structural claim is correct: once the DEK leaves GeneratedDatabaseKey it is a plain Copy [u8;32] dropped without scrubbing across all consumers. But this is an inherent design tension, not a discrete bug: storage engines (SQLCipher/redb/libsql) require the raw 32-byte DEK to operate, so it must escape the wrapper, and any attacker who can read this process memory already has access to the engine's own key buffers. It is a defense-in-depth hardening gap (caller-side dek zeroize is missing), not an active or exploitable leak path. Medium over-states it; low.

#### `A3-4` — KeyDirectoryProvider wrapping key is never zeroized
**Severity:** low · **Dimension:** safety · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/encryption/key_directory.rs:68-92`

**Finding.** read_wrapping_key copies the 32-byte wrapping key into a `[u8;32]` and returns it; wrap_key/unwrap_key bind it into a cipher and let it drop normally. Unlike MasterKeyFileProvider (which explicitly zeroizes its wrapping_key and the intermediate Vec), this provider leaves long-lived key material un-scrubbed on the stack. Lower severity than the DEK leak because it's the wrapping key rather than per-database DEK, but it's an inconsistency in the same module's own secret-hygiene contract.

**Fix direction.** Return `Zeroizing<[u8;32]>` from read_wrapping_key (and zeroize the intermediate `bytes` Vec read from disk), matching MasterKeyFileProvider's pattern, so wrap_key/unwrap_key scrub the wrapping key on drop.

#### `A3-5` — backend_feature_support ignores its backend argument; all backends report identical feature support
**Severity:** low · **Dimension:** bug · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/diagnostics.rs:364`

**Finding.** The function signature is `fn backend_feature_support(_backend: &str)` and the parameter is unused — every backend (redb/TenantStore, sqlite, postgres, mysql, libsql-replica) gets the same hardcoded feature list claiming full support for HistoricalDocumentReads, HistoricalIndexReads, PointInTimeRestore, and Changefeed. This makes the operator-facing capability/feature matrix inaccurate for any backend whose real support differs (e.g. an embedded replica's PITR or changefeed semantics). A diagnostic that always reports 'EnterpriseComplete' regardless of backend is misleading exactly where operators rely on it to reason about a deployment.

**Fix direction.** Branch on `backend` to report per-backend feature support honestly, or remove the parameter and rename the function if support truly is uniform — but verify each backend actually implements the four conditional features before claiming uniformity.

**Verification evidence.** Re-read diagnostics.rs:270-391 and 600-815, changefeed.rs:154-186 (impl_changefeed_journal! applied to TenantStore, SqliteTenantStore, PostgresTenantStore, MySqlTenantStore, LibsqlReplicaTenantStore), and confirmed libsql/read.rs:126-140 + libsql/document_versions.rs implement stream_durable_journal / export_durable_journal_bootstrap / journal_progress / document_version_storage_diagnostic. The factual part is true: backend_feature_support's `_backend` param is unused and every backend gets the same Supported list for Historical*/PITR/Changefeed. BUT the severity claim — that this makes the operator feature matrix INACCURATE/misleading — is unsubstantiated by the code: all five backends genuinely implement these features on the SAME shared tenant-event journal abstraction (the changefeed macro and the identical per-store diagnostic bodies prove it; libsql replica implements every underlying primitive). So reporting uniform support is accurate, not misleading. What remains is a dead-parameter / latent-fragility smell (a future backend that diverges would be mis-reported), not a current diagnostic-accuracy bug. Downgrade to low.

#### `A3-6` — Five copy-pasted storage_health_diagnostic_with_retention_config impls across store types
**Severity:** low · **Dimension:** modularity · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/diagnostics.rs:628-815`

**Finding.** The same ~20-line body (journal_progress / load_schema / table_identity_diagnostics / document+index version diagnostics / gc_watermarks / build StorageHealthDiagnosticInput) is duplicated verbatim for TenantStore, SqliteTenantStore, PostgresTenantStore, MySqlTenantStore, and LibsqlReplicaTenantStore (the storage_health_diagnostic shim is also duplicated five times). This is the same store set the changefeed module handles cleanly with a macro (impl_changefeed_journal!). Each future change to the diagnostic shape must be made in five places, inviting drift.

**Fix direction.** Extract the shared body behind a trait method or a `impl_storage_health_diagnostic!` macro over the five store types, mirroring the existing impl_changefeed_journal! pattern in changefeed.rs.

**Verification evidence.** Re-read diagnostics.rs:628-815. Confirmed verbatim duplication: storage_health_diagnostic (one-line shim) and storage_health_diagnostic_with_retention_config (~20-line body: journal_progress / load_schema / table_identity_diagnostics / document+index version diagnostics / gc_watermarks / build StorageHealthDiagnosticInput) are repeated identically for TenantStore (632-654), SqliteTenantStore (675-697), PostgresTenantStore (714-736), MySqlTenantStore (753-775), and LibsqlReplicaTenantStore (792-814); the only per-store difference is self.storage_capabilities(). The changefeed module already demonstrates the cleaner macro pattern for the exact same store set, so a macro would consolidate this. Real maintainability/drift concern, but it is a pure modularity/quality issue with zero behavioral impact — medium over-states a 5-way copy of an identical ~20-line shim. Low is appropriate.

#### `A3-7` — GeneratedDatabaseKey has redundant zeroize attributes
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/encryption/key.rs:14-25`

**Finding.** The struct carries both `#[derive(ZeroizeOnDrop)]` on the type and a legacy `#[zeroize(drop)]` on the plaintext field. With ZeroizeOnDrop derived, the per-field `#[zeroize(drop)]` is redundant (and `#[zeroize(drop)]` is the older API form). It's harmless today but obscures intent and can confuse the next reader about which mechanism actually runs on drop — directly relevant given the into_wrapped bug (A3-2) where drop behavior matters.

**Fix direction.** Keep `#[derive(ZeroizeOnDrop)]` and `#[zeroize(skip)]` on wrapped; drop the redundant `#[zeroize(drop)]` on plaintext (it's zeroized by the derive).

#### `A3-8` — AWS KMS sync/async bridge spawns a thread to call block_on inside a tokio context
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/encryption/aws_kms.rs:274-296`

**Finding.** block_on_future detects a current tokio runtime and, to avoid the 'block_on within a runtime' panic, spawns an OS thread per call that re-enters `handle.block_on(future)`. This works but is a thread-per-KMS-call cost on the hot key-resolution path and a fragile sync-in-async bridge. The panic-to-error mapping ('aws kms bridge thread panicked') also flattens any real error context from the spawned future.

**Fix direction.** Prefer making the provider trait async end-to-end, or use `tokio::task::block_in_place` where applicable, instead of spawning a fresh thread per call. At minimum, propagate the inner future's error rather than collapsing it to a generic bridge-panic message.

#### `A4-2` — Scan pushdown probe hand-parses msgpack with a magic length gate coupled to Document serde layout
**Severity:** low · **Dimension:** seam · **Subsystem:** Storage · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-storage/src/store/scan.rs:176`

**Finding.** probe_document_fields_from_msgpack (store/scan.rs:166-204) assumes Document serializes as a msgpack array of exactly 5 or 6 elements (array_len != 5 && array_len != 6 -> None) with the fields map at positional index 4 after skipping the first four values. This layout is implied solely by the derived Serialize on Document (nimbus-core/src/document.rs:11-19, where typed_fields is skipped when empty). There is no shared constant, no assertion tying the probe to document_codec, and no compile-time link: if a field is added/reordered or rmp_serde struct encoding changes, the probe silently returns None and pushdown stops rejecting (correctness is preserved but the optimization silently dies, and a more dangerous reorder could read the wrong positional value). The only guard is a single pushdown test that would catch total breakage but not a subtle positional shift among same-typed fields.

**Fix direction.** Centralize the document msgpack layout (element count + fields index) as named constants colocated with document_codec, and add a probe-vs-full-decode equivalence test that fuzzes documents (extra/missing/typed fields, numeric edge cases) asserting rejects_document_bytes never rejects a row matches_filters keeps.

**Verification evidence.** Re-read scan.rs:166-204 (probe), nimbus-core/document.rs:10-19 (Document derives Serialize; typed_fields skip_serializing_if empty -> 5 or 6 elements; fields at positional index 4 after id/table/creation_time/update_time), and document_codec.rs (rmp_serde::to_vec, which uses compact positional array struct encoding). The coupling is real: the probe hard-codes array_len==5||6 (scan.rs:176) and skips exactly four leading values to reach the fields map, with no shared constant or compile-time tie to document_codec/Document. Harm is correctly bounded by the finding itself: on layout drift the probe returns None, rejects_document_bytes returns false (scan.rs:106-108), the row falls through to full decode — correctness preserved, optimization silently lost. The 'dangerous reorder reads wrong positional value' sub-claim is overstated: fields is consumed by map-key lookup (read_map_len + per-key match), not positionally, and a reorder among the four skipped scalars would most likely make index-4 not-a-map -> read_map_len errs -> None (safe), not a silent wrong read. Guarding tests in store/scan/tests.rs assert exact pushdown_rejected_rows counts (506/90/98), catching total breakage though not every same-typed positional shuffle. Given zero correctness impact, a self-healing failure mode, and an existing regression guard, medium is over-severe; low fits the maintainability/seam-coupling risk.

#### `A4-3` — execute_cancellable read path calls handle.abort() which is a no-op for in-flight blocking work and leaves the task running
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/async_storage/read.rs:117`

**Finding.** In BlockingReadExecutor::execute_cancellable (async_storage/read.rs:114-122) and the libsql equivalent (libsql/storage.rs:83-91), the cancel arm sets the cancelled flag, calls handle.abort(), and immediately returns Err(Cancelled). For a spawn_blocking task that has already started, abort() cannot interrupt the running closure; the actual interruption is the cooperative check_cancel polled per row (store/read.rs:271,294). So abort() is dead/misleading and the blocking task continues to hold its semaphore permit until it next polls check_cancel and unwinds. The behavior is correct (reads have no commit point) but the abort() call implies an interruption guarantee that does not exist, and a non-cooperative task (e.g. a single get() with no row loop) keeps the permit until natural completion.

**Fix direction.** Drop the misleading handle.abort() (or replace with a comment that cancellation is cooperative via the AtomicBool/check_cancel), and document that the permit is released only when the cooperative cancel is observed.

#### `A4-4` — map_join_error / map_permit_error duplicated across four storage backends with divergent messages
**Severity:** low · **Dimension:** modularity · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/async_storage/helpers.rs:3`

**Finding.** Identical-shaped JoinError/AcquireError mappers are redefined in async_storage/helpers.rs:3-9, libsql/backend.rs:807-813, postgres/backend.rs:1424-1430, and mysql/backend.rs:288-300. The bodies differ only in their hard-coded message strings ('blocking storage task failed' vs 'libsql replica read task failed' vs the libsql 'libsql replica executor unexpectedly closed'), so the same async-boundary failure surfaces with four inconsistent operator-facing messages. This is a small but real cross-module duplication in the exact seam this review covers.

**Fix direction.** Hoist a single pair of mappers (parameterized by a short context label) into a shared module and have each backend call it, so the join/permit failure taxonomy is consistent.

#### `A4-6` — Freshness barrier concurrency (wait_for_background_refresh + synchronous fallback) has no targeted unit test
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/libsql.rs:614`

**Finding.** The libsql freshness state machine (ensure_local_cache_current, schedule_background_refresh, run_background_refresh_loop, wait_for_background_refresh in libsql.rs:338-626) is only exercised by network-gated end-to-end libsql_provider tests that assert post-condition metrics (tests/libsql_provider.rs:316-367). There is no test that drives the race-prone interleavings directly: a read barrier created while a background refresh is in flight, the synchronous refresh_local_cache fallback at libsql.rs:351 when the background pass does not reach required_cache_sequence, and the should_reschedule self-respawn at libsql.rs:590-598. The Notify pattern is correct (verified against tokio 1.51 notify_waiters semantics), but the absence of a deterministic concurrency test means a future regression in this barrier would only be caught by the slow, environment-dependent lane.

**Fix direction.** Add a deterministic test (injectable refresh hook / controllable clock and a fake remote) that forces (a) a barrier wait that resolves via background completion, (b) a barrier that must fall through to the synchronous refresh_local_cache, and (c) a required-sequence bump arriving mid-refresh that triggers reschedule, asserting required_sequence is met and no thread blocks.

#### `B1-4` — Atomic-write apply calls engine.now() multiple times per write, producing inconsistent timestamps within one commit
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/execution_units/batch.rs:142-189`

**Finding.** apply_set_write (and the patch/transform variants) call self.engine.now() independently for document construction/overwrite, field transforms, preserve_document_lifecycle_times, and the reported PendingAtomicWriteResult.update_time. Under a wall clock these calls can return different timestamps, so a single document's update_time, its ServerTimestamp transform field, and the reported outcome update_time can disagree, and differ from the actual CommitEntry.timestamp. Firestore semantics expect one server timestamp per commit.

**Fix direction.** Capture a single now() at the start of prepare_atomic_write_batch (or per write) and thread that one Timestamp through all transform/lifecycle/outcome computations.

#### `B1-5` — stage_write resolves table_id from live store while reads resolve from the execution-unit snapshot
**Severity:** low · **Dimension:** seam · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/execution_units/staging.rs:174`

**Finding.** stage_write (staging.rs:174) resolves table identity via self.runtime.store().table_id(&table) against current store state, whereas read paths (reads.rs:33, batch.rs:393, load_batch_document) resolve via self.snapshot.table_id against the captured snapshot. Mixing live and snapshotted table identity when recording write_dependencies can record a table_id that differs from the snapshot-era one used for read dependencies, weakening dependency-set intersection in the OCC check if a table is (re)created concurrently during the unit's lifetime.

**Fix direction.** Resolve table_id from self.snapshot in stage_write to keep read and write dependency identities consistent with the unit's snapshot, or document why live resolution is intentional.

#### `B1-6` — expect_immediate_result / expect_scheduled_applied panic via unreachable! on a wrong-variant result
**Severity:** low · **Dimension:** safety · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/mutations/direct/types.rs:73-91`

**Finding.** These helpers use unreachable!("{message}") when the MutationExecutionResult variant does not match the caller's expectation. A logic regression (e.g. a path returning Scheduled where Immediate is expected) would panic the worker/request thread rather than surfacing an Error::Internal. Every other invariant violation in this subsystem is modeled as Error::Internal (see expect_immediate_document_id).

**Fix direction.** Return Result and map the unexpected variant to Error::Internal(message) instead of panicking, consistent with expect_immediate_document_id/expect_immediate_unit.

#### `B1-7` — Journal batch invalidates cache and dispatches subscription/trigger work for records beyond the actually-applied head on partial-apply error recovery
**Severity:** low · **Dimension:** bug · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/mutations/journal.rs:276-284`

**Finding.** After apply_durable_records_batch errors, the code recovers via recover_durable_journal and uses progress.applied_head (journal.rs:278-282), but then invalidate_document_cache_for_commits(applied.iter()) (line 283) and process_applied_commit_batch(&batch_result.applied, ...) (line 96) still process the FULL records vec. Because the batch apply is atomic per transaction and recover replays the tail, the final state converges, so this is over-eager rather than corrupting; subscriptions can fire before the corresponding writes are visible at the watermark. Cache over-invalidation is harmless (forces re-read).

**Fix direction.** Gate cache invalidation and process_applied_commit_batch on records whose sequence <= applied_head, so downstream dispatch never runs ahead of the published applied watermark.

#### `B2-2` — Cursor signature embeds principal-derived access filters, coupling pagination continuity to the principal
**Severity:** low · **Dimension:** seam · **Subsystem:** Engine · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-engine/src/engine/queries/authorization.rs:37-45`

**Finding.** ReadAuthorization::merge_query appends principal-derived `planner_filters` into the query (authorization.rs:42-44), and these filters are principal-dependent (compile_read_filters in nimbus-core/src/auth/access.rs:90-116 derives filters from the principal). The merged query is what flows into the pagination evaluator and thus into `query_signature`. Consequently the cursor's signature encodes principal-specific access-filter values. If the principal's identity/claims change between page requests (token refresh producing different owner-scoped filter values), or if a cursor minted for one principal is presented under another, decode_cursor rejects it with "invalid cursor". Pagination continuity should depend on the user-supplied query shape, not on internally-injected per-principal authorization predicates.

**Fix direction.** Exclude authorization-injected planner_filters from the cursor signature. Combined with B2-1, sign only the stable user query (table, user filters, order), keeping per-principal authorization filters out of the opaque cursor identity.

**Verification evidence.** Re-read authorization.rs:37-45 (merge_query appends planner_filters into query.filters) and nimbus-core/src/auth/access.rs:90-116 + 168-207 (compile_read_filter resolves a PrincipalClaim to a concrete constant and emits Filter{field,op,value:<principal-derived value>}). So the merged query — and thus query_signature — does encode principal-specific filter values; the mechanism is real. But severity is over-stated: (1) same root cause as B2-1 (signature taken over the post-merge/post-plan query rather than the stable user query), not an independent defect; (2) for the dominant owner-by-subject case the principal subject is stable across a same-user token refresh, so continuity does NOT break in the common case; (3) cross-principal cursor rejection is arguably a desirable security posture, not a bug. Realistic harm (a mutable claim feeding a filter changing value mid-pagination) is narrow. Low.

#### `B2-3` — Sort comparators panic via .expect() relying on a non-local validation invariant
**Severity:** low · **Dimension:** safety · **Subsystem:** Engine · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-engine/src/evaluator/ordering.rs:16`

**Finding.** sort_documents (ordering.rs:14-22) and sort_documents_for_structured_query (finalize.rs:99-119) call `compare_order_field`/`compare_structured_order_values` inside the sort closure and `.expect("ordering inputs should be validated before sorting")`. Correctness depends entirely on `validate_order_domain` / `validate_structured_order_domains` having rejected every value that could make compare_values return Err. Today that holds (validation rejects non-string/non-number and mixed kinds; serde_json numbers are always finite so partial_cmp never returns None), but the guarantee is fragile: any future relaxation of the validators, a new comparable value kind, or a refactor that sorts without first validating turns a data-shape edge case into a process panic on a reachable query path rather than a returned Error.

**Fix direction.** Make the sort total/infallible by construction: precompute a validated sort key (e.g. an enum SortKey { Number(OrderedFloat), Str(String), Absent } with a derived Ord) per document once, then sort by that key, eliminating the fallible compare inside the closure and the .expect().

**Verification evidence.** Re-read ordering.rs:8-37 and structured/finalize.rs:15-121: both sort closures call .expect("ordering inputs should be validated before sorting") and both functions invoke validate_order_domain / validate_structured_order_domains BEFORE the sort_by in the same function (ordering.rs:13, finalize.rs:98). compare_values (filtering.rs:54-72) and compare_structured_order_values only return Err for non-string/non-number kinds or non-f64 numbers; validators reject exactly those kinds first, and serde_json numbers are finite so partial_cmp never yields None. No reachable sort-without-validate path exists today, so there is NO active panic — the finding itself concedes "Today that holds." This is a defensive/maintainability concern about future fragility, not a current safety bug; medium overstates a latent issue. Low.

#### `B2-4` — Duplicated durable-journal-suffix read loop across snapshot.rs and verification.rs
**Severity:** low · **Dimension:** modularity · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/queries/verification.rs:25-44`

**Finding.** The while-loop that pages the durable journal up to `bootstrap.bootstrap_cut` (stream 256 records, take_while sequence <= cut, advance `after`, error on no-progress) is implemented twice almost verbatim: snapshot.rs read_durable_journal_suffix_to_sequence_async (lines 41-61) and verification.rs build_shadow_materializer_async (lines 25-44). verification.rs::verify_consistency_async already calls the snapshot.rs helper, so the inline copy in build_shadow_materializer_async is redundant divergence risk (e.g. one could get a no-progress message fix the other misses).

**Fix direction.** Have build_shadow_materializer_async call read_durable_journal_suffix_to_sequence_async (already pub(super)) instead of re-implementing the suffix read loop.

#### `B2-6` — Range planner bound-subsumption and Neq-residual logic lack direct unit tests
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/queries/planner/range.rs:99-168`

**Finding.** range.rs contains non-trivial logic: tightest-bound selection (update_lower_bound/update_upper_bound, lines 146-168), inclusive/exclusive tie-breaking in compare_lower_bounds/compare_upper_bounds (170-194), and residual computation that drops a looser duplicate range filter as already-satisfied while keeping Neq (range_bound_from_filter returns None for Neq at line 128, so Neq stays residual). The planner test module (planner/mod.rs:287-567) tests single lower+upper selection but never asserts that two competing lower bounds (e.g. Gt 2 and Gte 5) select the tighter one and leave/drop the correct residual, nor that a Neq on the range field is preserved in residual_filters. A wrong subsumption here would silently widen index scans or drop a filter from residual without re-application.

**Fix direction.** Add unit tests for: (a) two lower bounds of differing tightness selecting the tighter and producing the expected residual, (b) inclusive-vs-exclusive bound tie-breaking, and (c) a Neq filter on the range field remaining in residual_filters.

#### `B2-7` — PlanCandidate::score() returns an unlabeled 4-tuple that fights readability of the priority ordering
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/engine/queries/planner/scoring.rs:15-22`

**Finding.** Plan selection priority is encoded as a positional tuple `(consumed_fields, supports_requested_order, exact_prefix_len, prefer_exact)` (scoring.rs:15-22) compared with `>` in choose_better_plan (scoring.rs:25-32) and in plan_query_inner via `range.score() > exact.score()` (planner/mod.rs:85). The semantics (more consumed fields beats order-support beats longer exact prefix beats exact-over-range) are entirely implicit in field order; a reordering or added field silently changes plan selection for every query with no compile-time signal. bool fields participating in ordering (false<true) is also non-obvious.

**Fix direction.** Introduce a named `PlanScore` struct with a documented `Ord`/explicit cmp, or comment each tuple position with its ranking intent, so the priority contract is explicit and refactor-safe.

#### `B3-5` — Subscription/trigger worker bodies duplicated between #[cfg(test)] and #[cfg(not(test))]
**Severity:** low · **Dimension:** modularity · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/tenant/subscription_delivery/worker.rs:129-206`

**Finding.** run_delivery_worker is written twice (crates/nimbus-engine/src/tenant/subscription_delivery/worker.rs:129-169 and 171-206) differing only in the optional `pause` arg and the `pause.wait_if_armed()` call. The same near-identical duplication exists for start_inner (lines 48-99) and for the trigger candidate worker (trigger_candidates.rs:417-477 vs 479-530, plus start/start_inner 170-232). Each pair is ~40-90 lines of copy with one inserted line; divergence risk is real (a fix to the prod loop can silently skip the test loop, masking bugs in CI).

**Fix direction.** Collapse to a single body taking `pause: Option<Arc<...>>` (cfg-gated only at the type/param boundary) so the loop logic exists once; the test build supplies Some(pause), prod supplies None. Same for the trigger candidate worker.

#### `B3-6` — Overflow monotonicity test never exercises the concurrent fallback it claims to guard
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/tenant/subscription_delivery/tests.rs:141-263`

**Finding.** subscription_delivery_queue_overflow_falls_back_without_regressing_monotonicity (crates/nimbus-engine/src/tenant/subscription_delivery/tests.rs:141-290) pauses the worker, then performs the overflow update so the sync fallback effectively runs while the worker is parked at the pause point — it asserts the final state and that older deliveries are skipped, but it does not create the true concurrent race (worker actively dispatching seq N while the caller thread dispatches seq M for the same subscription). The name asserts a guarantee (B3-3) that the test does not actually cover.

**Fix direction.** Add a test that lets the worker run a slow/instrumented evaluation for seq N concurrently with a sync-fallback dispatch of seq M>N for the same subscription, and assert the receiver never observes a decreasing snapshot sequence — i.e. exercise the send/record interleaving directly.

#### `B3-7` — affected_subscription_ids is fully public on a re-exported registry, leaking an internal dispatch hook
**Severity:** low · **Dimension:** seam · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/subscriptions/dependencies.rs:67`

**Finding.** SubscriptionRegistry is re-exported as pub from subscriptions.rs:10, and affected_subscription_ids is declared `pub fn` (crates/nimbus-engine/src/subscriptions/dependencies.rs:67), while its batch sibling affected_subscription_ids_for_batch is correctly pub(crate). This exposes a commit-matching/dispatch internal (taking CommitEntry + candidate documents) on the public embedder surface with no apparent external consumer, widening the API seam beyond intent.

**Fix direction.** Demote affected_subscription_ids to pub(crate) to match affected_subscription_ids_for_batch unless an external embedder genuinely needs it; keep the commit-dispatch matching internal to the engine.

#### `B4-6` — MySql tenant-read check_fault is a no-op, silently disabling fault injection on that backend
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Engine · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-engine/src/persistence/tenant/reads.rs:10`

**Finding.** The persistence tenant-read fault hook returns Ok(()) unconditionally for the MySql backend while every other backend delegates to the real fault-injection check. Any reliability/fault test routed through the MySql provider therefore exercises a path where injected faults never fire, producing false-green coverage and an inconsistent cross-backend test contract. Because all backends are supposed to behave identically under fault injection, this is a latent test-quality hole rather than intended behavior.

**Fix direction.** Delegate the MySql arm to the same check_fault implementation as the other backends (or, if MySql intentionally has no fault store yet, make that explicit and assert it in a test) so cross-backend fault parity holds.

#### `C1-2` — CPU-usage ops index out[0]/out[1] with no buffer-length check (JS-reachable panic)
**Severity:** low · **Dimension:** bug · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:350`

**Finding.** op_host_get_worker_cpu_usage (worker_threads.rs:350-367) and op_current_thread_cpu_usage (worker_threads.rs:369-375) write out[0] and out[1] on a caller-supplied #[buffer] out: &mut [f64] without verifying out.len() >= 2. If the JS bootstrap (or any reachable JS) passes a shorter Float64Array, the op panics with an out-of-bounds slice index, which aborts the isolate. Repo policy forbids panics on reachable non-test paths; op inputs from JS are attacker-influenced.

**Fix direction.** Guard with `if out.len() < 2 { return; }` (or return a JsErrorBox) before writing out[0]/out[1] in both ops, or take a fixed-size [f64; 2] return value instead of an out-buffer.

**Verification evidence.** Re-read worker_threads.rs:350-375: confirmed out[0]/out[1] are written with no out.len() check. But the reachability premise is refuted. The only callers are trusted internal polyfills in the nimbus/deno fork: ext/node/polyfills/worker_threads.ts:149 (new Float64Array(2)) and process.ts:299 (new Float64Array(2)) - both module-private fixed-length-2 buffers, never user-supplied. These ops are imported from ext:core/ops into extension modules; user code cannot reach them: source.rs:758 deletes globalThis.Deno (capturing __nimbusCoreOps privately at line 5), and the retained node22 path binds a curated ext_node_denoGlobals (node22_internal_bootstrap.js:2), not raw Deno.core.ops. The code is also a near-verbatim port of upstream deno (deno fork runtime/ops/worker_host.rs:685-710, same unchecked out[0]/out[1]), which ships it in production. Not a JS-reachable isolate abort from untrusted code; at most a defensive-coding nit. Downgraded high to low.

#### `C1-4` — unsafe FFI blocks in worker CPU-usage helpers lack SAFETY comments
**Severity:** low · **Dimension:** safety · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:637`

**Finding.** worker_threads.rs has multiple unsafe blocks with no SAFETY justification: mach_thread_self() (639), std::mem::zeroed::<ThreadBasicInfo>() + thread_info() (645-654), the macOS `unsafe extern "C"` declarations (684-692), libc::syscall(SYS_gettid) (696), libc::sysconf (709), and the Windows OpenThread/GetThreadTimes/CloseHandle/assume_init sequence (736-759). Repo policy requires every unsafe block to carry a SAFETY comment explaining the invariants. The calls look individually sound, but the missing rationale is a maintainability/auditability gap exactly where it matters most (raw OS handles, zeroed repr(C) structs, MaybeUninit::assume_init).

**Fix direction.** Add a // SAFETY: comment above each unsafe block stating the precondition (valid thread handle, FFI signature matches the platform ABI, struct is fully initialized by thread_info/GetThreadTimes before reads, handle closed exactly once).

**Verification evidence.** Re-read worker_threads.rs:637-765: confirmed every unsafe block lacks a SAFETY comment - mach_thread_self (639), mem::zeroed::<ThreadBasicInfo>() (645), thread_info (647-654), unsafe extern C decls (684-692), libc::syscall SYS_gettid (696), libc::sysconf (709), GetCurrentThreadId (725), OpenThread (736), GetThreadTimes (745), CloseHandle (754), assume_init (758-759). The upstream deno fork (runtime/ops/worker_host.rs:712-840) documents each with // SAFETY, so the port dropped them - a real auditability regression. However the 'repo policy requires every unsafe block to carry a SAFETY comment' claim is not backed by enforcement: Cargo.toml workspace lints (40-44) set only rust unused=deny and clippy all=deny; clippy::undocumented_unsafe_blocks is a restriction-group lint not in 'all', and CLAUDE.md Execution Quality does not mandate SAFETY comments. So this is a maintainability/auditability gap, not an enforced-policy or memory-safety defect. Downgraded medium to low.

#### `C1-6` — op_nimbus_runtime_exec_path returns host executable path unconditionally
**Severity:** low · **Dimension:** gap · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local/bootstrap.rs:20`

**Finding.** op_nimbus_runtime_exec_path (bootstrap.rs:20-28) returns std::env::current_exe() to JS with no capability/grant check, unlike the sibling stdio ops op_set_raw and op_http_start (bootstrap.rs:36-57) which fail closed with capability_denied_error. This leaks the host binary's absolute filesystem path (deployment layout / username in the path) to every runtime regardless of run/sys grants. Low severity because it is a path string, not an authority, but it is an unscoped host-info disclosure inside an otherwise fail-closed op surface.

**Fix direction.** Gate exec_path behind a grant (e.g. the same run/sys grant family that authorizes process info) or return a stable synthetic path, rather than disclosing the real current_exe() unconditionally.

#### `C2-1` — queued-invocation metric leaks on semaphore-closed error paths in permit acquire
**Severity:** low · **Dimension:** bug · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/executor/admission/permit.rs:115`

**Finding.** In acquire_initial (permit.rs:98 increments queued, line 124 decrements only on success) and complete_async_host_call (line 211 increments, line 236 decrements only on success), an Err from acquire_active_permit().await? (line 101 / 214) or runtime_instance_semaphore().acquire_owned() (line 115 / 227) propagates via `?` WITHOUT decrementing the queued-invocations gauge. The cancellation branches handle the decrement, but the semaphore-closed Err branch does not. Reachable only at executor shutdown (semaphore closed), so impact is a small terminal metric skew, not a runtime leak.

**Fix direction.** Wrap the acquire in a guard that decrements queued_invocations on any early return, or decrement explicitly before mapping the acquire error, mirroring the cancellation branches.

#### `C2-2` — cooperative worker abandons in-flight/parked slots on shutdown without draining results
**Severity:** low · **Dimension:** gap · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/worker_loop/cooperative/run.rs:58`

**Finding.** CooperativeWorkerLoop::run uses `while !shutdown.is_cancelled()` (run.rs:58); once shutdown fires the loop body never runs again, so any runnable/parked/in-flight slots in the scheduler are dropped without calling finish_invocation or queue.complete_job. Callers still observe Err(Contract "dropped an invocation result") because the oneshot sender is dropped (invoke.rs:199), so no hang occurs, and admission/semaphore state is reclaimed when the executor Drop runs. But the run-to-completion loop drains via recv-until-None (run_to_completion.rs:107) and is therefore strictly cleaner; the two models have asymmetric shutdown semantics and the cooperative path emits a Contract error rather than Cancelled for jobs that were genuinely mid-flight.

**Fix direction.** On shutdown, drain the scheduler: finish each remaining slot with Err(Cancelled) and complete_job it (or document that abandoned cooperative results are expected to surface as Contract errors). Aligning with the run-to-completion drain behavior would make shutdown deterministic.

#### `C2-3` — watchdog polls every external cancellation registration on a 10ms tick (O(n) per tick)
**Severity:** low · **Dimension:** optimization · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/watchdog.rs:282`

**Finding.** fire_cancelled_registrations iterates the entire cancellation_handlers map every EXTERNAL_CANCELLATION_POLL_INTERVAL (10ms) checking is_cancelled() (watchdog.rs:282-296, next_wait forces a 10ms wake whenever any cancellation handler is registered). With many concurrent in-flight invocations carrying external cancellations this is O(n) work every 10ms on a single watchdog thread, and also wakes the thread continuously even when nothing is cancelled. Timeouts are already event-driven via the BinaryHeap; only the external-cancellation path resorts to polling.

**Fix direction.** Have HostCallCancellation expose a wake/notify channel the watchdog can select on (event-driven) instead of polling, or at minimum batch-poll only registrations whose cancellation has a cheap already-fired fast path; the current design caps watchdog throughput under high concurrent-cancellation load.

#### `C2-4` — redundant RuntimeFlavor match arm `CurrentThread | _`
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/executor/invoke.rs:41`

**Finding.** bridge_blocking_invocation matches `RuntimeFlavor::CurrentThread | _ =>` (invoke.rs:41). RuntimeFlavor is #[non_exhaustive], so the `_` already covers CurrentThread and every future variant; listing CurrentThread explicitly is dead and misleads the reader into thinking only those two cases route to the thread::scope fallback. The intent (everything that is not MultiThread spawns a scoped fallback thread) is clearer as a bare `_`.

**Fix direction.** Replace the arm with `_ =>` (or add a comment that all non-MultiThread flavors fall back to a scoped thread), removing the redundant CurrentThread token.

#### `C3-4` — worker_threads FFI unsafe blocks lack SAFETY comments, inconsistent with the rest of the subsystem
**Severity:** low · **Dimension:** safety · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:639-759`

**Finding.** The per-OS thread CPU-usage helpers contain numerous unsafe blocks with no `// SAFETY:` justification: mach_thread_self() (worker_threads.rs:639), zeroed ThreadBasicInfo + thread_info() (645,647), libc::syscall(SYS_gettid) (696), libc::sysconf (709), GetCurrentThreadId (725), OpenThread/GetThreadTimes (736,745), CloseHandle (754), and assume_init on the FILETIME values (758-759). This is inconsistent with bun_jsc/linked.rs, where every unsafe block in the same crate carries a SAFETY comment (e.g. linked.rs:201-206,247-251). Undocumented FFI/raw-pointer unsafe is exactly the class CLAUDE.md's enterprise bar targets, and the assume_init calls in particular depend on GetThreadTimes having succeeded (the success check is at 745-756) — an invariant worth stating explicitly.

**Fix direction.** Add a `// SAFETY:` comment to each unsafe block stating the upheld invariant (valid thread handle, correctly-sized info struct, output initialized only after a checked success return), matching the discipline already used in bun_jsc/linked.rs.

**Verification evidence.** Re-read worker_threads.rs:637-764: confirmed the per-OS helpers contain unsafe blocks with zero SAFETY comments (grep counts 0 SAFETY / 12 unsafe in the file) — mach_thread_self (639), thread_info + zeroed ThreadBasicInfo (645,647), libc::syscall SYS_gettid (696), libc::sysconf (709), GetCurrentThreadId (725), OpenThread/GetThreadTimes (736,745), CloseHandle (754), assume_init on FILETIME (758-759). The contrast with bun_jsc/linked.rs:201-251 (every unsafe documented) is accurate. But this is a documentation/consistency nit, not a demonstrated soundness defect — the FFI calls themselves check kr/null/ret before use. undocumented_unsafe_blocks is a clippy restriction lint NOT included in the workspace `all = deny` config (Cargo.toml:43-44), so CI does not enforce it. Real and worth fixing, but medium overstates a missing-comment finding with no behavioral impact.

#### `C3-6` — canceled_invocations aggregate omits disconnect and explicit cancellations
**Severity:** low · **Dimension:** bug · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/metrics/global.rs:240-248`

**Finding.** record_queued_canceled_invocation (global.rs:228-232) and record_in_flight_canceled_invocation (234-238) both increment their specific counter AND call record_canceled_invocation() to roll into the `canceled_invocations` aggregate. But record_disconnect_canceled_invocation (240-243) and record_explicit_canceled_invocation (245-248) increment only their specific counters and do NOT roll up. Since canceled_invocations is exposed as a public snapshot field (global.rs:75) alongside the four sub-counters (77-80) and is the natural total, it under-reports by the number of disconnect/explicit cancellations — making the aggregate inconsistent with the sum of its parts. (The host-op variants at 250-265 are internally consistent, but the precanceled/in_flight_canceled host-op roll-ups suggest the same intended aggregate semantics, underscoring the invocation gap as an oversight rather than intent.)

**Fix direction.** Either call record_canceled_invocation() in the disconnect and explicit recorders so the aggregate sums all four causes, or remove the roll-up entirely and compute the total at read time from the four sub-counters so the contract is uniform. Add a test asserting canceled_invocations == queued+in_flight+disconnect+explicit.

#### `C3-7` — Host-bridge FFI callback writes *output_len before the capacity check
**Severity:** low · **Dimension:** safety · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:241-251`

**Finding.** bun_jsc_host_bridge_call_json writes the full response length into the caller's out-param first (`*output_len = response.len();`, linked.rs:241-243) and only afterward checks `if response.len() > output_cap { return 307; }` (244-245). The copy itself is correctly gated, so this is not a memory-safety bug, but on the overflow path the embedder receives a written output_len that exceeds output_cap with status 307. That is the contract the Rust caller relies on to print the size (linked.rs:160-165), so it is currently consistent — but the ordering is fragile: any future embedder that reads output_len before checking status would see an out-of-bounds length. Writing the length only on the success path (or only after the capacity check) is the safer invariant.

**Fix direction.** Perform the `response.len() > output_cap` check before writing *output_len, and on overflow write the required length explicitly as a documented 'needed capacity' signal, so the out-param is never set to a value the buffer cannot hold without an accompanying error status the embedder is required to honor.

#### `C3-8` — Raw post-message ops swallow channel send errors via .ok()
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/runtime/bootstrap/ops/worker_threads.rs:137-139`

**Finding.** op_nimbus_worker_parent_post_message_raw (worker_threads.rs:137-139) and op_host_post_message_raw / the detached-port send (344-346) discard the result of `tx.send(...)` with `.ok()`. A failed send means the receiving worker/port is gone and the message is silently dropped, which can mask lifecycle races (message posted to an already-torn-down worker) with no diagnostic. Given the subsystem's otherwise careful cancellation/ack accounting, silently dropping cross-thread messages is an observability gap.

**Fix direction.** Distinguish 'port intentionally closed' from 'send failed unexpectedly': on Err either return a JS-visible error or record a metric/trace so dropped messages are observable, rather than collapsing both to .ok().

#### `D1-1` — HostCallCancellation::cancelled() has a missed-wakeup TOCTOU reachable from WS unsubscribe
**Severity:** low · **Dimension:** bug · **Subsystem:** Server · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-runtime/src/host.rs:641-646`

**Finding.** cancelled() checks is_cancelled(), and if false awaits notify.notified(). cancel_with_cause() sets the atomic then calls notify_waiters(), which (unlike notify_one) buffers NO permit for future waiters. If cancel() (e.g. pending.rs:57 cancel_subscription, called from the session task) races into the window after the worker's is_cancelled() returns false but before notified() registers, the awaiting bootstrap worker (session.rs:148-171, awaiting subscription_wait.cancelled()) misses the wakeup. The cancel-wait branch never fires; cancellation then only takes effect at the next check_cancel atomic re-read, degrading immediate cancellation to checkpoint-latency cancellation. This is the disconnect/unsubscribe cancellation path the subsystem depends on. Fix is the standard tokio pattern: build the Notified future and register it (pin + poll once) BEFORE re-checking the flag, or hold a Notified across the is_cancelled() check.

**Fix direction.** Reorder cancelled() to create/register the Notified future first, then re-check is_cancelled(): let notified = self.inner.notify.notified(); if self.is_cancelled() { return; } notified.await; (pin the Notified so registration happens before the second flag check).

**Verification evidence.** The TOCTOU is real: host.rs:641-646 cancelled() does `if is_cancelled() return; notify.notified().await;` and cancel_with_cause (host.rs:628-635) calls notify_waiters() (which buffers no permit for future waiters), so a cancel landing between the flag read and waiter registration is missed. pending.rs:57 cancel_subscription -> cancellation.cancel() is the unsubscribe trigger, as cited. HOWEVER every consumer of cancelled() is paired with synchronous atomic re-reads that backstop a missed wakeup, so impact is bounded to latency, not a hang or correctness defect: (1) execute_cancellable (async_storage/read.rs:103-121) runs the blocking scan whose combined_cancel re-reads is_cancelled() per document (filtering.rs:14, query.rs:81/83) and self-terminates so the handle completes the select even if cancel_wait missed; (2) subscribe_async_cancellable_with_principal re-checks check_cancel() synchronously after bootstrap (subscriptions.rs:283); (3) the watchdog cancellation path polls is_cancelled() on EXTERNAL_CANCELLATION_POLL_INTERVAL (watchdog.rs:253-263, 282-296), never relying on the Notify wakeup at all. The finding itself characterizes the worst case as 'checkpoint-latency cancellation.' Citations are also imprecise: the cancelled() awaits are at runtime shared.rs:148 and invocation.rs:159, and the session.rs:148-171 block is the combined wait+check builder (the synchronous check at 162 is itself a backstop), not the await site. A latency-only degradation in a narrow race window that is independently backstopped on three layers does not warrant medium; downgraded to low.

#### `D1-3` — Lock-poison policy diverges within the same crate (WS .expect vs AppState recover)
**Severity:** low · **Dimension:** safety · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/ws/socket/pending.rs:20-21`

**Finding.** PendingBootstrapCancellationRegistry .expect("...should not be poisoned") on every Mutex lock (5 sites), which panics the WS task if the lock is ever poisoned. AppState::ActiveDeployment chose the opposite policy: .unwrap_or_else(|poisoned| poisoned.into_inner()) to recover. Poison is unlikely here (tiny HashMap critical sections; cancel() under lock won't panic), but the inconsistent posture means one code path treats poison as fatal and another as recoverable. Pick one policy crate-wide. Recovery is the more defensible default for a long-lived server: a poisoned cancellation registry should not take down an otherwise-healthy connection.

**Fix direction.** Standardize on lock().unwrap_or_else(|poisoned| poisoned.into_inner()) in pending.rs to match ActiveDeployment, or factor a tiny poison-recovering lock helper used by both.

#### `D1-4` — Socket writer kills the whole connection on a single serialization failure
**Severity:** low · **Dimension:** bug · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/ws/socket/transport.rs:102-105`

**Finding.** spawn_socket_writer breaks out of its loop (terminating the writer task and thus the connection) when message.to_text(protocol) returns Err. A serde_json failure on one outbound ServerMessage (e.g. a pathological document payload that fails to serialize) takes down all subscriptions on that socket rather than skipping/erroring the single frame. Given to_text serializes server-controlled JSON it is rare, but a per-message failure should not be connection-fatal.

**Fix direction.** On to_text error, log and continue (or emit a session_error frame) instead of break; reserve break for socket send failures only.

#### `D1-5` — AuthError and Authenticated server messages collapse into ambiguous V2 wire shapes
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/protocol.rs:323-326`

**Finding.** to_v2_json maps Authenticated -> {"type":"authenticated"} but AuthError -> {"type":"error", ...} (not a distinct auth type), while request-scoped Error -> {"type":"op.error"}. An auth failure and a generic session error are therefore indistinguishable by message type on the wire; clients must inspect error.code to tell them apart. The typed ServerMessage::AuthError variant implies a distinct frame that the serializer does not actually produce.

**Fix direction.** Either emit a distinct type (e.g. "auth.error") for AuthError, or fold AuthError into Error if the distinction is not intended, so the enum variants and wire shapes stay in 1:1 correspondence.

#### `D1-6` — Deploy handler performs blocking std::fs I/O on the async runtime thread
**Severity:** low · **Dimension:** bug · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/http/deploy.rs:337-415`

**Finding.** deploy_app -> stage_deploy_artifacts uses synchronous std::fs::create_dir_all/write (and StagedDeployArtifacts::Drop calls std::fs::remove_dir_all) directly inside the async handler, blocking a tokio worker for the duration of disk staging of potentially large bundles. Deploy is an infrequent admin op so impact is limited, but it still parks a runtime thread under load.

**Fix direction.** Wrap the staging + cleanup filesystem work in tokio::task::spawn_blocking, or use tokio::fs, so disk I/O does not block the async executor.

#### `D1-7` — WS session/pending concurrency modules have no in-module unit tests
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/ws/socket/pending.rs:17-69`

**Finding.** session.rs (subscription registration race, cancelled_pending_subscriptions bookkeeping, handle_pending_event drop-on-cancel) and pending.rs (track/link/finish/cancel/clear registry transitions) carry the trickiest concurrency in the subsystem but have zero #[test]/#[tokio::test] beside them. Coverage exists only via tests/reactive_loop integration tests, which exercise the happy paths and two cancel-before-bootstrap cases but not, e.g., link_subscription racing finish_request, or double-unsubscribe. The registry state machine in pending.rs is pure and trivially unit-testable.

**Fix direction.** Add focused unit tests for PendingBootstrapCancellationRegistry transitions (track->link->finish ordering, cancel_subscription on a linked vs unlinked id, clear) and for handle_pending_event dropping a registration whose id is already in cancelled_pending_subscriptions.

#### `D2-2` — Route-family gate fails open (no audit) when local_server_security is unconfigured, leaving shutdown_system unauthenticated
**Severity:** low · **Dimension:** seam · **Subsystem:** Server · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-server/src/local_server/middleware.rs:117`

**Finding.** route_family_gate_middleware returns next.run(request).await immediately when policy.app_state.local_server_security.is_none(), with no audit record. The destructive shutdown_system handler does NOT self-authorize (it only records an audit event and calls request_server_shutdown), so it relies entirely on this middleware. When server-access auth is unconfigured, /api/system/shutdown is reachable unauthenticated. This is inconsistent with rotate_local_admin_token in the same module, which defends in depth by re-checking local_server_security presence and calling authorize_bearer itself before acting. The fail-open branch also emits no audit event, so unauthenticated access in this mode is invisible.

**Fix direction.** Decide a single posture: either make local_server_security mandatory (fail closed for admin/destructive route families when None), or have shutdown_system self-authorize like rotate_local_admin_token does (local_admin.rs:28-39). At minimum emit an audit event on the fail-open branch so the unconfigured-security mode is observable.

**Verification evidence.** Confirmed the code: middleware.rs:117 returns next.run(request).await with no audit when local_server_security.is_none(), and shutdown_system (local_admin.rs:70-100) does not self-authorize — it only records an audit event and calls request_server_shutdown (line 98). So in the None mode the shutdown route is unauthenticated. HOWEVER, the None fail-open is unreachable in any shipped nimbus binary: boot.rs:106-170 and first_boot.rs:225-236 unconditionally call with_local_server_security on every production start path. The is_none()=>fail-open is a deliberate, repo-wide opt-in design (authorize_standard_server_access access_policy.rs:118, extract_server_access:147, middleware.rs:117 all treat unconfigured security as not-enforced). The real residue is the defense-in-depth asymmetry the finding cites: rotate_local_admin_token (local_admin.rs:28-32) returns 401 when security is None while shutdown_system does not — a genuine inconsistency worth fixing. But because it is only reachable by an embedder that intentionally omits security configuration (documented as auth-not-configured), medium overstates impact; low is appropriate.

#### `D2-3` — No negative-auth or fail-open test for the destructive shutdown route
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Server · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-server/src/tests/local_admin.rs:74`

**Finding.** system_shutdown_endpoint_stops_live_server sends /api/system/shutdown with a valid bearer (bearer_auth(&token.token)) and asserts 200 — happy path only. There is no test that calls shutdown with a missing/invalid token and asserts 401 (the only thing standing between an attacker and shutdown is the middleware, since shutdown_system does not self-authorize), and the entire local_server_security integration suite always constructs the app with_local_server_security configured, so the fail-open `local_server_security.is_none()` branch in route_family_gate_middleware is never exercised. A destructive, middleware-only-gated route must have explicit negative-auth coverage.

**Fix direction.** Add a test asserting /api/system/shutdown returns 401 with a missing/invalid token while security is configured, and a test covering the local_server_security == None path (whichever posture is chosen in D2-2) so the fail-open behavior is pinned by a test.

**Verification evidence.** Confirmed: local_admin.rs:74-127 (system_shutdown_endpoint_stops_live_server) is happy-path only — it sends with a valid bearer_auth(&token.token) and asserts 200. grep over crates/nimbus-server/src/tests confirms the only /api/system/shutdown HTTP test is this one (other shutdown matches are unrelated scheduler watch channels). The only UNAUTHORIZED assertion in the file (line 70) covers the rotate route, which self-authorizes (local_admin.rs:37) — not shutdown. No test exercises a missing/invalid token against shutdown, and none exercises the fail-open None branch of route_family_gate_middleware. This is a real test-quality gap for a destructive, middleware-only-gated route. But severity is mitigated: the route IS correctly gated in all production wiring (boot/first_boot always configure security), so this is missing coverage of an already-protected path rather than an untested live vulnerability; low fits a test-quality gap of this exposure better than medium.

#### `D2-4` — Deploy artifacts staged in a predictable temp dir with default permissions
**Severity:** low · **Dimension:** safety · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/http/deploy.rs:339`

**Finding.** stage_deploy_artifacts builds the staging path as std::env::temp_dir().join("nimbus-deploy-{pid}-{counter}") and creates it with create_dir_all (inherits process umask, not a restricted mode), then writes attacker-influenced content (bundle.mjs, JSON manifests) into it before the bundle is loaded. The path is predictable to any local user and the directory is not created 0o700, so on a shared host this is a local race/observability surface for a path that feeds code execution. The deploy route is already admin-gated, which bounds severity, but the staging itself is a defense-in-depth gap.

**Fix direction.** Stage into a per-invocation unpredictable directory created with 0o700 (e.g. tempfile::Builder with a random suffix, or create the dir then set restrictive perms) under a Nimbus-owned state dir rather than the world-readable system temp dir.

#### `D2-5` — validate_origin passes when Origin header is absent and never validates Host for non-UI routes
**Severity:** low · **Dimension:** safety · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-operator/src/access_policy.rs:248`

**Finding.** validate_origin returns Ok(()) immediately when no Origin header is present, and for all non-UI route families only checks that a present Origin is loopback — the Host header is validated only for the Ui/UiAuthSession families. Native-API and CLI clients with no Origin therefore bypass origin validation entirely, leaning solely on the loopback bind plus server-access auth. This is a reasonable posture for non-browser clients, but the absence of any Host-header check on the native API surface removes a layer of DNS-rebinding defense-in-depth that the UI routes have.

**Fix direction.** Consider validating the Host header against the expected loopback host:port for native-API route families too (defense-in-depth against DNS rebinding), or document explicitly why Origin-absent + server-access auth is the accepted boundary for non-browser clients.

#### `D3-1` — Pervasive lock-poison `.expect` is a deliberate-but-unguarded panic policy
**Severity:** low · **Dimension:** safety · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/convex/host_bridge/read_tracking/builders.rs:8`

**Finding.** Every Mutex/RwLock acquisition in the adapter unwraps with `.expect("... lock should not be poisoned")` (e.g. the query-builder registry in builders.rs and the subscription status maps in socket/mod.rs). This is an internally-consistent policy: a poisoned lock means a prior holder panicked while mutating shared state, so fail-fast is defensible. The residual risk is that a single panic inside any host-call handler that holds one of these locks converts into a cascading panic on the next acquisition rather than a recoverable per-request error, and the per-connection task could take down shared bridge state. It is a low-severity safety/operability note, not a bug, because the invariant (no panics while holding these short-lived locks) currently holds.

**Fix direction.** Keep the fail-fast policy but centralize it behind one helper (e.g. `lock_or_poison(&self.query_builders)`) so the message and recovery posture are defined in exactly one place, and document the no-panic-while-holding invariant beside that helper.

#### `D3-2` — Read-tracking is recorded pre-execution for plain queries but post-execution for paginated queries
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct/invocation.rs:251`

**Finding.** Non-paginated direct queries record the read intent BEFORE execution (`record_executable_query_read` at invocation.rs:13 and :40), so the read set is captured even when the query errors or returns empty. Paginated queries record the window read only AFTER a successful execution, inside `finalize_paginated_runtime_response` via `.and_then` (invocation.rs:72,107 -> primitives.rs:259), and the builder-based collect/paginate pair shows the same split (collection.rs:15 vs :80,124). On a query error the paginated path records no read. This is NOT a live-subscription invalidation gap: subscription bootstrap requires a successful invocation (`?` at execution/runtime_backed/subscriptions.rs:52), so an errored paginated query establishes no subscription and there is nothing to miss invalidations. The window read genuinely needs the result page to define its bounds, so the asymmetry is intentional; flagging it only as a consistency/maintainability note so a future reader does not mistake the missing pre-record for a bug.

**Fix direction.** Add a short comment on `finalize_paginated_runtime_response` (and the builder paginate arms) explaining that window reads are intentionally post-execution because the page boundaries are required, and that subscription bootstrap's success-gate makes the error path benign.

#### `D4-4` — Misleading Status::unimplemented("Not yet implemented") guard in Firestore gRPC adapter
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/firebase/grpc/mod.rs:96`

**Finding.** FirestoreGrpc::app_state returns Status::unimplemented("Not yet implemented") when self.state is None (crates/nimbus-server/src/adapters/firebase/grpc/mod.rs:93-97). The feature IS implemented — production always constructs the server via from_state (router.rs), so state is Some. The None branch is reachable only through the stateless new()/Default constructors, and a test (firestore_grpc_unary_stub_returns_unimplemented) asserts this misleading 'unimplemented' response. The message tells operators/clients a working feature is unimplemented; it also pins a confusing contract in a test.

**Fix direction.** Either make state non-optional (remove the stateless constructors so app_state is infallible) or change the error to Status::internal("Firestore gRPC server constructed without application state"), which honestly describes a misconfiguration rather than an unimplemented feature. Update the test to assert the corrected behavior.

#### `D4-5` — unsafe libc::gethostname FFI call lacks a SAFETY comment
**Severity:** low · **Dimension:** safety · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/system/version_check.rs:359`

**Finding.** version_check.rs calls `unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) }` (crates/nimbus-server/src/system/version_check.rs:359) with no // SAFETY: comment documenting why the call is sound (fixed 256-byte stack buffer, length passed matches buffer, NUL-termination handled, rc checked). The call itself is correct, but every unsafe block in enterprise Rust should carry a SAFETY justification so the invariants are auditable and a future buffer-size change can't silently break them.

**Fix direction.** Add a // SAFETY: comment above the unsafe block explaining the buffer/length/NUL invariants, or replace with a safe crate (e.g. hostname/gethostname) to eliminate the unsafe block entirely.

#### `F1-1` — External policy worker thread leaks on timeout / disconnect (never joined or cancelled)
**Severity:** low · **Dimension:** safety · **Subsystem:** Security · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-tenant/src/operator_policy/external.rs:326-352`

**Finding.** evaluate_external_policy_backend spawns a detached worker thread that runs backend.evaluate(...) and sends the result over a sync_channel(1). On recv_timeout returning Timeout or Disconnected the function returns an error immediately (lines 340-351) but the spawned thread is neither joined nor cancelled and keeps running until backend.evaluate eventually returns. Because this is on the per-workload admission hot path and external backends are exactly the ones that can be slow/hung, a misbehaving or unreachable backend converts every admission into a permanently leaked OS thread (and a retained Arc<backend> clone). Under sustained load against a wedged backend this is an unbounded thread/FD/memory growth vector and a denial-of-service amplifier on the very fail-closed path meant to protect the tenant boundary. The fail-closed decision itself is correct; the resource lifecycle is not.

**Fix direction.** Bound the worker. Either run evaluation with a join handle and a hard deadline (e.g. a bounded thread pool with cancellation, or pass the deadline into backend.evaluate so it can abort), or document and enforce that backends must be cancellation-aware; at minimum cap concurrent in-flight workers so a hung backend cannot spawn unbounded threads. Keep the fail-closed error, but stop orphaning the thread.

**Verification evidence.** Re-read external.rs:317-376. The mechanism is real: thread::Builder::spawn's JoinHandle is consumed by .map_err(...)? (line 326-336), so no handle is retained; on RecvTimeoutError::Timeout (340-345) and Disconnected (346-351) the fn returns an Err immediately. Rust threads cannot be cancelled, so a wedged backend.evaluate keeps the detached worker (plus its cloned Arc<backend> at 324 and cloned request at 325) alive until it returns. So the resource-lifecycle leak is genuine and the fail-closed Err is correct (corroborated by the timeout test at operator_policy/tests.rs:593-618 which asserts <500ms latency, call_count==1). However the medium severity overstates reachability/impact: I grepped the whole repo for OperatorExternalPolicyBackend impls and evaluate_with_external_policy(Some(...)) callers — the ONLY backend impl is the test fake (operator_policy/tests.rs:323) and the ONLY Some(engine) callers are unit tests. The public OperatorPolicyDocument::evaluate() always passes None (evaluation.rs:70), and nimbus-server merely re-exports the types (lib.rs:81,84) without wiring them into any admission path. So the 'per-workload admission hot path' / 'DoS amplifier' / 'unbounded thread/FD growth under sustained load' framing assumes a production caller that does not exist today. Real defect on a not-yet-wired surface, exercised only by tests -> low.

#### `F1-2` — OperatorRuntimePolicy::validate is a no-op that reads as if it enforces runtime policy
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Security · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-tenant/src/operator_policy/validation.rs:78-94`

**Finding.** Both control-flow branches of OperatorRuntimePolicy::validate return Ok(()): the matched Node-profile + InProcessUntrusted case returns Ok(()) with an explanatory comment, and the fall-through path executes a dead `let _ = workload_key;` and also returns Ok(()). The method therefore validates nothing for any profile/tier combination, yet it is called from the operator-policy validate() chain (validation.rs:71-73 sibling calls do real work) and its name plus the explanatory comment strongly imply runtime-grant enforcement happens here. The actual runtime grant enforcement lives in admit_runtime_policy -> validate_production_in_process_untrusted_policy (context.rs:158) and is enforced downstream by nimbus-bridge, so this is NOT a missing-enforcement security hole, but it is misleading dead code that invites a future maintainer to assume operator-policy load-time rejects bad runtime tiers when it does not.

**Fix direction.** Either delete OperatorRuntimePolicy::validate and its call site (and drop the workload_key param), or give it real load-time checks (e.g. reject operator-declared InProcessUntrusted for non-Node profiles, or anything else that is statically known-bad) so the name matches behavior. Remove the dead `let _ = workload_key;`.

**Verification evidence.** Re-read validation.rs:78-94. Confirmed both paths return Ok(()): the Node{20,22,24}+InProcessUntrusted match returns Ok at line 89 with an explanatory comment, and the fall-through runs the dead `let _ = workload_key;` (91) then returns Ok at 92. The method enforces nothing for any profile/tier and is invoked from the sibling validate chain at validation.rs:61 alongside peers that do real work, so the naming/comment are indeed misleading. The finding's own claim that this is NOT a security hole checks out: actual runtime grant enforcement is admit_runtime_policy (context.rs:146-162) delegating to runtime_admission::validate_production_in_process_untrusted_policy (runtime_admission.rs:185), reached from policy_input.rs:319 via with_runtime_policy in evaluation.rs:145 — enforcement is present and downstream. The behavior is exactly as described, but medium overstates a purely cosmetic, inert dead-code clarity issue with no functional or security impact; low is appropriate.

#### `F1-3` — Lenient vs strict principal-claim checks are an easy-to-misuse asymmetry on the tenant boundary
**Severity:** low · **Dimension:** seam · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-tenant/src/context.rs:164-197`

**Finding.** validate_principal_claim_if_present (context.rs:164-178) admits when an Application principal has NO tenant claim (returns Ok at line 169), while require_matching_principal_claim (context.rs:180-197) denies the no-claim case. The two differ only in the absent-claim branch and have nearly identical signatures/bodies, so a caller can trivially pick the permissive one by accident. The lenient variant is the one wired into the decision build path (decision.rs admit). tests.rs:663-688 confirms this split is intentional and the strict variant is exercised at control-plane routes, so this is by-design and not a live bypass; the risk is purely future-misuse on a security-critical seam where the safe default for a tenant check should arguably be deny-on-missing-claim.

**Fix direction.** Make the asymmetry explicit at the type/name level rather than two look-alike methods: e.g. take an enum (RequireClaim vs AdmitIfAbsent) so the call site must state intent, or rename to admit_if_principal_claim_absent_or_matching to make the permissive semantics unmissable. Add a doc note that ambient (no-claim) admission is only sound when an upstream layer has already bound the principal to the tenant.

#### `F1-4` — validate_host accepts embedded port / userinfo / brackets, only rejecting unspecified wildcards
**Severity:** low · **Dimension:** gap · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-tenant/src/operator_policy/validation.rs:353-368`

**Finding.** validate_host rejects empty, '*', '0.0.0.0', '::', '[::]', and any IP that parses as unspecified, but performs no structural validation of the host string otherwise. A value like 'example.com:8080', 'user:pass@example.com', or a string containing a path/scheme passes, because the function never asserts the host is a bare hostname/IP. Since this feeds admitted egress endpoints (operator network policy), a malformed or surprising host could be admitted and later interpreted inconsistently by the egress PEP. host_port is validated separately (validate_port), so an embedded port in host is redundant-but-accepted ambiguity rather than an outright bypass.

**Fix direction.** Tighten validate_host to require a bare hostname or IP literal: reject ':' (except inside a valid bracketed IPv6 literal), '@', '/', whitespace, and scheme markers, mirroring the strictness already applied in validate_spiffe_trust_domain (identity.rs:286-301).

#### `F2-1` — systemd job wait has no timeout and can hang forever
**Severity:** low · **Dimension:** gap · **Subsystem:** Security · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs:90`

**Finding.** wait_for_job drains the JobRemoved signal stream with `while let Some(signal) = job_removed.next().await` and only returns when a matching job object path arrives or the stream ends. There is no timeout. If systemd accepts the StartTransientUnit/StopUnit job but never emits a JobRemoved for it (e.g. a wedged manager, a job that is merged/collected without a removal signal for the returned path, or a dropped-but-not-closed stream), the await blocks indefinitely. This is the trust-critical NDB3 completion path; an unbounded hang here stalls the calling reconcile loop with no diagnostic. The stream-ended branch is handled, but the never-arrives branch is not.

**Fix direction.** Wrap the wait in tokio::time::timeout with a bounded budget (configurable, default a few seconds beyond systemd's own job timeout) and map elapsed to a Failed/Internal outcome so a stuck job surfaces as an error instead of an indefinite hang.

**Verification evidence.** Re-read crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs:90-105: wait_for_job loops `while let Some(signal) = job_removed.next().await` and only returns on a matching job_path or on stream end (Error::Internal). There is genuinely no tokio::time::timeout / deadline anywhere in the path. Confirmed by grepping the whole systemd_transient/ subtree and zbus_client/mod.rs:196-243 (callers wrap the await with no timeout). So the never-arrives branch blocks indefinitely as claimed — the technical core of the finding is accurate. However the severity is over-stated on two counts: (1) reachability — per CLAUDE.md and code inspection, no production caller wires NodeWorkloadReconciler to drive ZbusSystemdClient (TSB14 deferred the production reconcile loop), so the 'stalls the calling reconcile loop' impact has no current production path; (2) the NDB3 plan deliverable (docs/plans/archive/node-dbus-client-binding-plan.md:128) specifies the subscribe/receive_job_removed/correlate flow but never lists a timeout as a gate item, so this is an absent hardening rather than a violated completion gate. Real reliability gap, but no live blast radius today → low.

#### `F2-2` — records.rs is a 1062-line multi-domain switchboard over stringly-typed table sinks
**Severity:** low · **Dimension:** modularity · **Subsystem:** Security · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-system/src/records.rs:978`

**Finding.** records.rs owns projection logic for at least a dozen unrelated system domains (system_status, services, machines, listeners, ports, events, workload_status, tables, bundles/functions, runs, scheduled_jobs, cron_jobs, subscriptions) and funnels them all through upsert_system_document_async(engine, table: &str, ...) and delete_system_document_if_exists_async(engine, table: &str, ...). The destination table is an untyped &str literal at every call site ('services', 'subscriptions', 'ports', etc.), so a typo or schema drift is only caught at runtime via TableName::new, and there is no compiler link between a record_*_async function and the schema it writes. At 1062 lines this is over the repo's 1000-line comfort threshold and is a composition-root that keeps accreting inline domain logic rather than delegating to concept-owned children.

**Fix direction.** Replace the &str table parameter with a typed system-table enum (or per-domain newtype) shared with schema.rs so destinations are exhaustively checked, and split records.rs into concept-owned modules (services.rs, subscriptions.rs, scheduler.rs, listeners.rs, tables.rs) behind a thin re-export root.

**Verification evidence.** Re-read records.rs (1062 lines confirmed via wc), the upsert_system_document_async/delete_system_document_if_exists_async signatures at lines 978-1021 and 695, and ~28 call sites — all pass an untyped &str table literal validated only at runtime via TableName::new (line 985). That stringly-typed observation is factually correct. But the severity rests on a false premise: the finding cites a '1000-line comfort threshold' that does not exist. CLAUDE.md 'Modularity thresholds' (lines 368-376) sets the bar at 1,500 lines ('usually acceptable when they keep one coherent ownership story'); 1,500-1,999 needs justification; 2,000+ must be decomposed. At 1062 lines the file is comfortably within 'usually acceptable'. It also keeps a coherent ownership story (10 system_tenant_id() uses; every function projects system-tenant documents) and the module is already split into concept-owned siblings (identity.rs/inventory.rs/keys.rs/projection.rs/schema.rs). The residual stringly-typed nit is minor: table names are in-file literals (not external input), validated at runtime, and seeded from same-module system_table_schemas(). Not a medium modularity violation → low maintainability nit.

#### `F2-3` — Three-way system-tenant guard asymmetry across subscription record functions
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-system/src/records.rs:619`

**Finding.** record_subscription_delivery_async early-returns Ok(()) when is_system_tenant_id(tenant_id) before delegating to record_subscription_state_async, but the primary writer record_subscription_state_async (called directly from socket/mod.rs:189 with ctx.tenant_id) has no such guard, and record_subscription_error_async also has no guard. Net effect: a subscription opened on the _nimbus system tenant would have its state document and error updates written into _nimbus.subscriptions, while only its delivery updates are filtered. The guard intent (don't let _nimbus record its own subscription churn) is applied inconsistently, leaving a stale, partially-updated subscription document for the system tenant. Largely theoretical today because _nimbus does not open Convex websocket subscriptions, hence low severity, but the inconsistency is a latent correctness trap.

**Fix direction.** Apply the is_system_tenant_id skip consistently: guard record_subscription_state_async and record_subscription_error_async the same way as record_subscription_delivery_async (or hoist the guard to the single shared writer), so the system tenant is uniformly excluded from all three subscription record paths.

#### `F2-4` — stable_key_segment can collide distinct identifiers into one document id
**Severity:** low · **Dimension:** bug · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-system/src/keys.rs:3`

**Finding.** stable_key_segment maps every non-alphanumeric character to '-' and then trims leading/trailing '-'. As a result inputs like 'foo.bar', 'foo bar', 'foo-bar', and 'foo/bar' all collapse to the same segment 'foo-bar'. These segments are joined into document ids for services, tables, cron jobs, listeners, ports, subscriptions, bundles, and functions (keys.rs). Two genuinely distinct names (e.g. two service names, or two cron job names) that differ only by separator characters would map to the same _nimbus document id and silently overwrite each other in the system projection. The projection is a status mirror (not the source of truth), so this is observability corruption rather than primary-data loss, hence low severity, but it produces wrong system tables.

**Fix direction.** Make the segment injective for the inputs that matter: either percent/hex-encode non-alphanumerics instead of folding them all to '-', or append a short stable hash of the original value to the slug so distinct inputs cannot collide.

#### `F2-5` — DirectProcessStatusSnapshot is dead public surface
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-node/src/direct_process.rs:270`

**Finding.** pub struct DirectProcessStatusSnapshot is declared with status()/evidence()/logs() accessors and re-exported from lib.rs, but it is never constructed anywhere in the workspace. A grep across all crate .rs files shows only the declaration (direct_process.rs:270) and the re-export (lib.rs:20); there is no DirectProcessStatusSnapshot { .. } literal and no factory. It is dead public API that implies a status-snapshot capability the backend does not actually produce.

**Fix direction.** Either wire DirectProcessBackend to produce a DirectProcessStatusSnapshot on its status path, or delete the struct and its lib.rs re-export. Pre-launch posture favors deleting the unused surface.

#### `F2-6` — HostPlatformDependencies is an always-none capability stub
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-node/src/direct_process.rs:199`

**Finding.** HostPlatformDependencies exposes five capability flags (requires_pid1/dbus/podman/conmon/kvm) and is embedded in DirectProcessEvidence, but DirectProcessEvidence::from_plan always sets it to HostPlatformDependencies::none() (line 199). The only constructor used is ::none(), so every accessor always returns false. The type advertises platform-dependency tracking that the direct-process backend never actually computes, which is misleading evidence on a security/lifecycle path. Severity low because it is currently honest-but-empty (false, not wrong), not an active misreport.

**Fix direction.** Either compute the real dependency flags from the HostLifecyclePlan when building evidence, or drop HostPlatformDependencies from DirectProcessEvidence until the direct-process backend has real platform requirements to report.

#### `F2-7` — delete-stale and port-cleanup paths do full O(n) table scans per operation
**Severity:** low · **Dimension:** optimization · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-system/src/records.rs:712`

**Finding.** delete_service_port_documents_async (712), delete_stale_deployment_documents_async (757), and delete_stale_scheduler_documents_async (796) each call engine.list_documents_async over an entire system table and then filter in memory by field equality (serviceId / status / tenantId). On a busy server these run per service-stop and per deployment/scheduler sync, so the projection cost grows linearly with total _nimbus document count regardless of how few documents actually match. This is the system status mirror, so it is a scalability smell rather than a hot user-facing path, hence low severity.

**Fix direction.** Drive these deletions from indexed lookups (e.g. an index on serviceId / tenantId+status) or from the known active id set rather than full-table list-then-filter, so cleanup cost scales with matches not with table size.

#### `F2-8` — existing_system_started_at_async eagerly evaluates a fallible fallback inside unwrap_or
**Severity:** low · **Dimension:** safety · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-system/src/records.rs:751`

**Finding.** The success arm returns document.fields.get("startedAt").and_then(Value::as_u64).unwrap_or(unix_time_millis()?). Because unwrap_or takes its argument by value, unix_time_millis() is always called (and its `?` always evaluated) even when startedAt is present and parses, doing needless work and propagating a clock error on a path that should not need the clock at all. Functionally near-harmless but it is the classic eager-unwrap_or smell on a fallible expression.

**Fix direction.** Use unwrap_or_else with a closure, or restructure to compute the fallback time only when startedAt is absent: e.g. match on the parsed value and call unix_time_millis() only in the None branch.

#### `G-1` — NumericValue::projected_json / into_stored_value panic on constructible non-finite Double and are dead public API
**Severity:** low · **Dimension:** safety · **Subsystem:** Trust · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-core/src/typed_scalar.rs:194`

**Finding.** NumericValue::Double { value: f64 } is a public struct-variant with no construction-time finiteness invariant, so callers can build NumericValue::Double { value: f64::NAN | INFINITY } (mongo update.rs:283 and firebase serializer.rs:57 / write_stream.rs:753 already construct Double directly from arbitrary BSON/Firestore doubles). Both NumericValue::projected_json (line 194) and NumericValue::into_stored_value (line 207) then call Number::from_f64(value).expect("finite numeric transform doubles should serialize"), which returns None for NaN/Inf and would panic the process. The only saving grace is that these two methods have ZERO callers in the workspace (verified by grep) — the engine converts to FiniteNumericTransformValue / ComparableNumericValue first, both of which guard with is_finite() (batch.rs:611,665). So today this is dead, unsound public API: it advertises a total method that panics for a representable input. Either enforce the invariant at construction (private field + checked constructor returning Result, rejecting non-finite Double) or make these methods return Result, or delete them since nothing calls them.

**Fix direction.** Delete the unused NumericValue::projected_json/into_stored_value, OR give NumericValue a checked constructor that routes non-finite doubles into SpecialDouble and keep the variant data private so a non-finite Double is unrepresentable.

**Verification evidence.** Re-read typed_scalar.rs:184-215 (NumericValue, projected_json, into_stored_value), lib.rs:76 (NumericValue is public API), batch.rs:601-754 (FiniteNumericTransformValue/ComparableNumericValue guards), and construction sites mongo update.rs:283 / firebase serializer.rs:57 / write_stream.rs:753. Core claims CORRECT: NumericValue::Double{value:f64} is a public struct-variant with no finiteness invariant; projected_json (line 194) and into_stored_value (line 207) both call Number::from_f64(value).expect(...) which panics on NaN/Inf; and these two methods have ZERO callers. I verified zero-callers independently: the only two `.into_stored_value()` sites (batch.rs:793,798) operate on ComparableNumericValue (transform_extreme returns Result<ComparableNumericValue>, batch.rs:891), NOT NumericValue; grep for qualified NumericValue::projected_json/into_stored_value returns nothing. All live flows route NumericValue through ComparableNumericValue::from_operand (batch.rs:665-668) or FiniteNumericTransformValue::from_operand (batch.rs:611-616), both guarding is_finite() and rejecting non-finite as Error::InvalidInput before any conversion. The panic is thus provably UNREACHABLE today — dead, unsound public API, as the finding itself concedes. With no reachable path and no production crash risk, 'medium' safety overstates impact; this is a latent API-soundness/maintainability wart (a total method that panics on a representable input). Downgrade to low. Fix still warranted: delete the unused methods, return Result, or enforce finiteness at construction.

#### `G-2` — Redundant two-branch provenance check is brittle even though it is currently correct
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Trust · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-artifacts/src/admission.rs:132`

**Finding.** ensure_artifact_policy_evidence splits provenance validation into (a) an empty-predicate_types branch that checks builder_id + source_uri only (lines 132-141), and (b) a per-predicate loop that re-checks builder_id + source_uri + predicate_type (lines 142-152). I traced the M3 lead: there is NO bypass — when predicate_types is non-empty the loop still enforces builder and source on every iteration, and a wrong builder fails each .any() check. However, the duplicated builder/source matching across two branches is exactly the shape that invites a future regression (e.g. someone 'simplifies' the loop and forgets the builder check). The two branches share a candidate predicate that could be unified: require at least one attestation matching builder+source, and additionally require each demanded predicate_type.

**Fix direction.** Compute the builder+source match once (filter attestations to matching builder/source into a slice), reject if empty, then assert every required predicate_type is present in that filtered slice. Single source of truth for the builder/source constraint.

#### `G-3` — Timestamp::now() panics the process on a pre-1970 system clock
**Severity:** low · **Dimension:** safety · **Subsystem:** Trust · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-core/src/types.rs:403`

**Finding.** Timestamp::now() does SystemTime::now().duration_since(UNIX_EPOCH).expect("system time should be after unix epoch"). duration_since returns Err whenever the wall clock is set before 1970 (misconfigured container/VM, dead RTC battery, deliberate clock skew). Because this is used on the hot document-creation path (document.rs:33, dependency.rs:657-672), a single bad clock turns into a process panic rather than a degraded timestamp. nimbus-core is the zero-I/O core crate, so a panic here aborts every embedder. saturating to 0 (or returning Self(0)) is strictly safer and still monotonic-ish for the degenerate case.

**Fix direction.** Replace the expect with .map(|d| d.as_millis() as u64).unwrap_or(0) (or unwrap_or_default) so a pre-epoch clock yields Timestamp(0) instead of aborting the process.

#### `G-4` — Verifier-output redaction is keyword-line based and leaks unlabeled secret values
**Severity:** low · **Dimension:** gap · **Subsystem:** Trust · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-artifacts/src/lib.rs:559`

**Finding.** redact_artifact_verifier_output redacts a whole line only if that line (lowercased) contains one of SENSITIVE_OUTPUT_FRAGMENTS (token/secret/bearer/...). A secret whose surrounding text carries no keyword — e.g. a bare JWT, a base64 registry-auth blob printed without the word 'auth', or an AWS key id without 'credential' — passes through verbatim. This is defense-in-depth for verifier subprocess stderr, and the inputs are semi-controlled, so severity is low, but the redactor advertises stronger guarantees than it delivers. Worth at least a doc-comment stating it is keyword-only, or adding value-shape patterns (long base64/hex runs, eyJ JWT prefix).

**Fix direction.** Document the keyword-only limitation in the function doc, and consider augmenting with value-shape heuristics (JWT eyJ prefix, >=40-char base64/hex runs) so unlabeled secrets are also masked.

#### `H1-2` — Bridge gateway computation overflows last octet (debug panic / release wraparound) for operator subnet ending in .255
**Severity:** low · **Dimension:** bug · **Subsystem:** Sandbox · **Verification:** real but severity-adjusted on re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/network.rs:523`

**Finding.** parse_ipv4_subnet_and_gateway derives the gateway as `octets[3] + 1` on a u8. When the configured subnet base address has last octet 255 (e.g. network_subnet = "10.0.0.255/24"), this panics in debug builds (attempt to add with overflow) and silently wraps to .0 in release, producing a wrong/unusable gateway. network_subnet is operator-configurable (OciNetworkConfig.network_subnet, default 10.x but overridable) and feeds three call sites (lines 138, 468, 652). Inputs like base octet 255, or a base that is the broadcast/network address of the prefix, are not validated.

**Fix direction.** Parse the address into a u32, validate it against the prefix (reject network/broadcast addresses), and compute gateway = (base | 1) or base+1 using checked/saturating arithmetic with an explicit InvalidSpec error when the subnet has no room for a gateway host. Add unit tests for base .255 and a /31 edge case.

**Verification evidence.** Re-read network.rs:489-525: gateway is built with `octets[3] + 1` on u8 with no bounds check, so a base last-octet of 255 overflows (debug panic / release wrap to .0). Confirmed `network_subnet` is a pub field on the public ContainerSandboxBackendConfig (runtime.rs:78) and OciNetworkConfig (network.rs:100), feeding three call sites (138,468,652); no validation rejects .255 or broadcast/network-address bases (grep for validate/broadcast/255 found none). However, the default DEFAULT_NETWORK_SUBNET='10.89.0.0/24' (line 33) is safe, and triggering requires an operator to set a degenerate subnet whose base octet is 255 (e.g. 10.0.0.255/24, itself the /24 broadcast address). This is an operator-self-inflicted input-validation/robustness defect, not a tenant- or attacker-reachable path, so medium overstates it; low is appropriate.

#### `H1-5` — render_command_failure copied 4x across modules
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/buildah/render.rs:35`

**Finding.** Identical single-arg `fn render_command_failure(stderr: &[u8]) -> String` appears in container/runtime.rs:1378, krun/vm/lifecycle.rs:462, and oci/network.rs:914; a closely related two-arg variant `render_command_failure(stdout, stderr)` lives in oci/buildah/render.rs:35. All do the same stderr-then-stdout-then-empty fallback. Pure duplication with no ownership justification.

**Fix direction.** Keep a single shared `render_command_failure(stdout: &[u8], stderr: &[u8])` (the buildah/render.rs form is the superset) in a shared command/diagnostics module and delete the three stderr-only copies; callers that only have stderr pass an empty stdout.

#### `H1-6` — IPv6 SSRF classification misses NAT64 (64:ff9b::/96) and IPv4-compatible (::a.b.c.d) embedded-IPv4 ranges
**Severity:** low · **Dimension:** safety · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/egress.rs:618`

**Finding.** is_non_global_or_internal_ipv6 maps IPv4-mapped (::ffff:0:0/96) addresses through the IPv4 classifier and blocks loopback/unspecified/ULA/link-local/multicast/2001:db8, but does not handle the well-known NAT64 prefix 64:ff9b::/96 nor deprecated IPv4-compatible addresses (::0.0.x.x / ::a.b.c.d). On a host with a NAT64 gateway, an allowed resolver answer of 64:ff9b::169.254.169.254 would be classified global and could reach link-local metadata; ::a.b.c.d similarly carries an embedded IPv4 that bypasses the IPv4 internal checks. Narrow (requires NAT64 deployment and a permitted-host resolution to such an address) but it is a real classification gap in the SSRF gate.

**Fix direction.** After to_ipv4_mapped, also extract and IPv4-classify the embedded address for NAT64 (segments[0..3] == [0x0064,0xff9b,0]) and for IPv4-compatible (first 96 bits zero), and block accordingly; add unit tests for 64:ff9b::169.254.169.254 and ::169.254.169.254.

#### `H1-8` — No negative/escape-path coverage for OCI layer materialization (symlink escape, whiteout edge cases)
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/backends/oci/materializer.rs:846`

**Finding.** materializer.rs tests cover happy-path gzip layer unpack and blob verification, but there is no test asserting that a layer containing a symlink-then-write-through (the H1-1 vector) or a path with `..` is rejected/confined, and no test for the `.wh..wh..opq` opaque-dir whiteout against a symlinked parent. Given this code is the rootfs confinement boundary, the absence of adversarial-layer tests is a meaningful coverage gap.

**Fix direction.** Add layer-fixture tests: (a) `evil -> /` symlink followed by `evil/x` asserts no host write; (b) entry path with `../` asserts sanitize_archive_path error; (c) opaque whiteout where the parent is a symlink asserts the symlink is removed via symlink_metadata, not followed.

#### `H2-3` — now_millis duplicated verbatim across three modules; next_*_version share an unextracted pattern
**Severity:** low · **Dimension:** code-smell · **Subsystem:** Sandbox · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-services/src/manager/definitions.rs:550`

**Finding.** Identical `now_millis()` (SystemTime::now().duration_since(UNIX_EPOCH)... unwrap_or(0)) is copy-pasted in definitions.rs:550, sandboxes.rs:309, and sessions.rs:387. The version generators next_definition_version (definitions.rs:545), next_sandbox_resource_version (sandboxes.rs:304), and next_session_version (sessions.rs:378) all share the same `*next = next.saturating_add(1).max(1)` body differing only in the format prefix. This is exactly the kind of cross-module duplication the repo guidelines call out; a clock helper drift (e.g. fixing overflow/test-injectable time) must be applied in three places.

**Fix direction.** Extract a single now_millis (and a `next_version(next, prefix)` helper) into a concept-owned module such as manager/clock.rs or manager/types.rs and call it from all three sites; this also gives one seam for injecting a deterministic clock in tests.

**Verification evidence.** Re-read definitions.rs:545-555, sandboxes.rs:304-314, sessions.rs:378-392. Confirmed verbatim: now_millis() is byte-identical in all three modules (SystemTime::now().duration_since(UNIX_EPOCH).map(...as_millis().try_into().unwrap_or(u64::MAX)).unwrap_or(0)), and next_definition_version/next_sandbox_resource_version/next_session_version share the identical body `*next = next.saturating_add(1).max(1)` differing only by format prefix. This matches the repo's stated aversion to cross-module duplication, and a clock-helper change would need to be applied in three places. But these are trivial pure-helper duplications with no behavioral risk; severity is maintainability-only, so low rather than medium.

#### `H2-4` — Same-leaf-name module trap: production manager/definitions.rs vs test manager/tests/definitions.rs
**Severity:** low · **Dimension:** modularity · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-services/src/manager.rs:150`

**Finding.** `mod definitions;` appears twice in manager.rs — once at line 9 (production module crates/nimbus-services/src/manager/definitions.rs) and once at line 150 inside `#[cfg(test)] mod tests` (test module crates/nimbus-services/src/manager/tests/definitions.rs). Two modules with the same leaf name in the same parent file is a navigation/grep hazard; the test file also reaches back into production internals via `super::super::types::TenantServiceKey` (manager/tests/definitions.rs:203,259), tightening coupling.

**Fix direction.** Rename the test submodule to something disambiguating (e.g. mod definition_lifecycle;) or co-locate definition tests in a #[cfg(test)] block inside manager/definitions.rs so the two 'definitions' names no longer collide.

#### `H2-5` — Dead readiness branch: service_binding_from_handle().is_some() can never be true unless status==Ready
**Severity:** low · **Dimension:** simplification · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-services/src/manager/activation.rs:60`

**Finding.** In wait_for_ready_handle_async the readiness check is `handle.status == SandboxStatus::Ready || service_binding_from_handle(&handle).is_some()` (manager/activation.rs:60-61). service_binding_from_handle returns None for any status other than Ready (registry.rs:118-119), so the right-hand side of the `||` is unreachable as a distinct condition and adds a misleading impression that a non-Ready handle could be considered ready.

**Fix direction.** Drop the redundant `|| service_binding_from_handle(&handle).is_some()` and key readiness solely on status == Ready, or, if a non-Ready-but-bindable state is intended, make service_binding_from_handle the single source of truth and document why.

#### `H2-6` — close_session/get_session are unscoped by tenant; correctness relies entirely on caller-side tenant checks
**Severity:** low · **Dimension:** seam · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-services/src/manager/sessions.rs:149`

**Finding.** ServiceManager::get_session and close_session key only on session_id with no tenant argument (manager/sessions.rs:133-196). Cross-tenant safety is enforced one layer up by ensure_session_lookup_tenant_matches in the HTTP handlers (crates/nimbus-server/src/http/sessions.rs:343,381). close_session then mutates by id without re-checking the tenant, leaving a (low-risk, ULID-guarded) TOCTOU and a footgun for any future caller that forgets the external check. Session IDs being unguessable ULIDs keeps real exposure low.

**Fix direction.** Thread tenant_id through get_session/close_session and filter on it inside the manager so the resource API is fail-closed by construction rather than depending on every caller re-checking tenancy.

#### `H2-7` — Force delete can stop the backend then fail the post-stop generation re-check, yielding a stopped-but-undeleted service
**Severity:** low · **Dimension:** bug · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-services/src/manager/definitions.rs:289`

**Finding.** delete_service_definition_async checks expected_generation before claiming the slot (definitions.rs:210), stops the running backend when force (definitions.rs:253-268), then re-reads and re-checks generation under the state lock (definitions.rs:289-296). If a concurrent update bumps the generation between the first check and the second, the function returns PreconditionFailed after the backend was already stopped and a stopped handle recorded — the definition survives but its service is down, an inconsistent observable outcome for the caller.

**Fix direction.** Re-validate generation under the activation slot/state lock before performing the irreversible backend stop, or treat the post-stop generation mismatch as a recoverable state (e.g. proceed with delete, or restart) so a lost generation race cannot strand a service in stopped-but-present.

#### `I1-10` — unreachable!() in duration_unit_nanos couples to a separately-maintained unit list
**Severity:** low · **Dimension:** code-smell · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bin/src/compose/file/parse.rs:279-289`

**Finding.** duration_unit_nanos ends in unreachable!() and depends on the parse loop's unit list staying exactly in sync. The two lists match today, but the unreachable!() turns a future divergence (a unit accepted by the parser but not mapped here) into a panic on a parse path rather than a returned parse error.

**Fix direction.** Return a parse Error for an unknown unit, or drive both the accepted-unit list and the nanos mapping from a single source so divergence is impossible.

#### `I1-3` — Dead compose loader wrappers retained behind #[allow(dead_code)]
**Severity:** low · **Dimension:** modularity · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bin/src/compose/mod.rs:81-164`

**Finding.** Six loader functions in compose/mod.rs (load_service_definition_catalog, _for_selection, load_service_manager, _for_selection, load_host_backed_service_manager_for_selection, _with_admission) are all #[allow(dead_code)]. Production routes through load_host_backed_service_manager_for_selection_with_isolation_mode (line 166); the dead wrappers' only callers are each other or tests. compose/execution.rs:63 load_host_backed_service_manager_for_platform_selection is likewise dead (its only caller, execution.rs:55, is #[cfg(test)]). Pre-launch policy prefers deletion over keeping dormant paths.

**Fix direction.** Delete the unused loader wrappers and the dead execution.rs:63 variant; have any tests call the live isolation-mode/admission entry points directly.

**Verification evidence.** Re-read compose/mod.rs:81-164 and execution.rs:48-77. All six named functions carry #[allow(dead_code)]. Grep for callers confirms: load_service_definition_catalog has none; load_service_definition_catalog_for_selection is called only by the two dead wrappers; compose::load_service_manager is called only by a test (start/tests/krun.rs:57) and is distinct from the production boot.rs:257 load_service_manager used at boot.rs:78; load_service_manager_for_selection is called only by the dead load_service_manager; load_host_backed_service_manager_for_selection has zero callers; _with_admission is called only by that zero-caller wrapper. execution.rs load_host_backed_service_manager_for_platform_selection (63) is reached only via the #[cfg(test)] _for_platform (49), itself used only by compose/tests/forwarded_api.rs:69. So the claim is accurate. Downgrade: several wrappers retain genuine test-support value (not strictly dead), and this is pure modularity/cleanup debt with no runtime impact; medium overstates it.

#### `I1-4` — .expect() on the live per-tenant service-backend request path
**Severity:** low · **Dimension:** safety · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bin/src/compose/file/lower.rs:196-218`

**Finding.** The ServiceDefinitionCatalog impl service_backend_for_tenant / service_backends_for_tenant call .expect() on lowering results. Validation is performed once up-front in into_service_catalog using a fixed CONFIG_VALIDATION_TENANT_ID; the per-request path then re-lowers and panics if it ever fails. Today to_service_backend/to_sandbox_spec failure is tenant-independent so the pre-validation covers it, but the coupling is implicit — any future tenant-dependent lowering failure becomes a server-side panic on a request path rather than a returned error.

**Fix direction.** Return Result from the per-tenant methods (or precompute and store the lowered backends so the request path is infallible), instead of .expect() that relies on an invariant established with a different tenant id.

**Verification evidence.** Re-read lower.rs:190-219 (the .expect() calls in service_backend_for_tenant / service_backends_for_tenant) and into_service_catalog (139-146), which up-front lowers every service with TenantId::new(CONFIG_VALIDATION_TENANT_ID) and propagates Err. Re-read to_service_backend (331-338) -> to_sandbox_spec (340-383): the only fallible work (egress policy, lifecycle, process spec) is tenant-independent; tenant_id is used solely as a clone passed into SandboxSpec::new. So pre-validation provably covers the same failure modes and the .expect() is currently unreachable. The finding itself concedes this and frames it as latent/implicit-coupling risk for future tenant-dependent lowering. A genuine but presently-guarded panic-on-request-path; medium is too high for a currently-unreachable path, low is appropriate.

#### `I1-6` — DEK plaintext key material not zeroized in rotation paths
**Severity:** low · **Dimension:** safety · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bin/src/encryption/rotate.rs:465-466,515-516,699-711`

**Finding.** new_dek and current_dek are bare [u8;32] arrays filled with OsRng/unwrapped key bytes and never zeroized; they live across re-encryption, rename, and manifest-write for the longest of any code in the CLI. Per-page plaintext in reencrypt_redb_pages is likewise un-zeroized. The underlying storage trait returns bare [u8;32], so this is partly systemic, but the rotation CLI is where the secret is resident longest.

**Fix direction.** Wrap DEK arrays in zeroize::Zeroizing (or a ZeroizeOnDrop newtype) and zeroize per-page plaintext buffers after use throughout the rotation/re-encryption paths.

**Verification evidence.** Re-read rotate.rs:465-466, 515-516, 699-711. new_dek/current_dek are bare [u8;32] filled by OsRng/unwrap and never zeroized; per-page plaintext from old_cipher.decrypt (699-711) is a Vec<u8> also left un-zeroized. Confirmed the trait returns bare arrays (provider.rs:215-232 unwrap/rewrap_database_key -> [u8;32]; runtime.rs:178-189 unwrap_database_manifest_key -> Result<[u8;32]>), so it is systemic. nimbus-storage already has ZeroizeOnDrop wrappers (encryption/key.rs:14-61) and zeroize is a workspace dep, but nimbus-bin does not use it here. Downgrade: this is a defense-in-depth gap in a short-lived CLI process, the un-zeroized [u8;32] return is the trait contract everywhere (a full fix is systemic, not CLI-local), and there is no proven exfiltration path; medium overstates it for a single-shot maintenance binary.

#### `I1-8` — Missing SAFETY comments on production unsafe (libc kill, from_raw_fd)
**Severity:** low · **Dimension:** safety · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bin/src/machine/manager/stop.rs:428,439`

**Finding.** send_signal/pid_is_alive call unsafe { kill(...) } and the machine API listener calls unsafe { from_raw_fd(fd) } with no SAFETY comment justifying the invariant (unlike boot.rs's systemd path, which validates LISTEN_FDS==1 and documents it). The repo has ~39 unsafe blocks but only ~6 SAFETY comments; these are reachable in production.

**Fix direction.** Add SAFETY comments documenting the invariants (kill with a validated pid and signal; from_raw_fd taking sole ownership of an fd validated as the single inherited socket), and audit the remaining production unsafe blocks for the same.

#### `I1-9` — is_loopback_registry uses prefix match, downgrading lookalike hosts to plaintext HTTP
**Severity:** low · **Dimension:** bug · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bin/src/machine/manager/image.rs:487-491`

**Finding.** is_loopback_registry uses host.starts_with("localhost")/"127.0.0.1"/"[::1]", so hosts like localhost.evil.com or 127.0.0.1.attacker.tld match and build_oci_client switches to plaintext ClientProtocol::Http. Combined with I1-1 (no digest verification), a plaintext fetch of a bootable image is attacker-influenceable. start/network_bind.rs:49-57 already does this correctly (parses the IP, case-folds an exact localhost) and is the model to follow.

**Fix direction.** Parse the host and require an exact match against localhost / a loopback Ip address (reuse the host_is_loopback logic from network_bind.rs) instead of starts_with.

#### `I2-2` — Session issuance panics on RNG/format failure inside an HTTP handler
**Severity:** low · **Dimension:** safety · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-operator/src/access.rs:266`

**Finding.** `create_session` uses `.expect("session id should generate")` on `generate_prefixed_token` (which calls `SystemRandom::fill` and can return `io::Error`) plus three more `.expect()` on RFC3339 formatting and cookie serialization. This path is reachable on every UI auth-session request (`crates/nimbus-server/src/http/ui.rs:169,304,309`). A transient system-RNG failure would panic the request task instead of returning an error. The sibling `mint_launch_ticket` (access.rs:146-159) already propagates the same RNG error as `io::Result`, so the inconsistency is gratuitous.

**Fix direction.** Make `create_session` return `io::Result<IssuedSessionCookie>` and propagate the RNG/format/serialize errors with `?`, matching `mint_launch_ticket`; update the two HTTP callers to map the error to a 500.

**Verification evidence.** Re-read access.rs:257-297: create_session uses .expect() on generate_prefixed_token (line 266, which propagates SystemRandom::fill failure as io::Error), and three more .expect() on Rfc3339 formatting (269,272) and sign_session_cookie serialization (280). Confirmed reachable from HTTP handlers: ui.rs:304 (create_session_for_local_admin_token) and ui.rs:309 (create_session_for_launch_ticket) both funnel into create_session. The inconsistency claim is true: mint_launch_ticket (access.rs:146-159) propagates the same RNG error as io::Result. But severity is overstated: a panic in an axum handler aborts only that single request task (tokio catches it), not the server; OffsetDateTime/Rfc3339 formatting of a valid timestamp and serde_json serialization of a fixed struct effectively never fail; SystemRandom::fill failure is near-impossible. Real robustness/consistency defect but extremely low triggerability and blast radius. Adjusted to low.

#### `I2-4` — "Record/contract" crate performs live env + fs I/O and ships a hard-coded /tmp fallback in a public constructor
**Severity:** low · **Dimension:** seam · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-machine/src/lib.rs:49`

**Finding.** nimbus-machine is documented as owning "the render-independent machine model" / "machine record and provider contracts", but `MachineRootLayout::resolve()` reads process env (`env::var_os` of XDG_*/HOME, lib.rs:489-555) and `MachinePaths::ensure_directories`/`ensure_runtime_directories` create directories (lib.rs:429-487). Worse, the public `MachineRootLayout::new` silently substitutes `/tmp/nimbus-test-data` and `/tmp/nimbus-test-cache` (lib.rs:64,68) when the three roots don't share a common parent. Every production caller of `new` is in fact a test module; `new` is effectively a test fixture exposed as production API, and the /tmp paths would be a silent, insecure data-placement bug if ever hit outside tests.

**Fix direction.** Either keep nimbus-machine pure-data and move env/fs effects into the consuming crate, or rename/document the crate honestly as a layout+IO helper. Replace the silent /tmp fallback in `new` with an explicit `data_root`/`cache_root` parameter (or a `#[cfg(test)]`-gated test constructor) so production paths can never collapse to /tmp.

**Verification evidence.** Confirmed the factual core: resolve() reads process env (lib.rs:489-555) and ensure_directories/ensure_runtime_directories create dirs (lib.rs:429-487), which sits oddly with the crate's documented 'render-independent machine model' framing. Crucially verified the 'every production caller of new is a test' claim: all callers of MachineRootLayout::new (guest_config.rs:836, bootstrap.rs:276, local_server.rs:359/414, plus the test files) are inside `#[cfg(test)]` modules (mod tests at guest_config.rs:793, bootstrap.rs:233, local_server.rs:225); production code uses resolve()/guest_api_default(). The /tmp/nimbus-test-* fallback (lib.rs:64,68) is genuinely a test-fixture behavior on a public constructor. However the 'silent insecure data-placement bug if ever hit' is unreachable in practice (every caller passes shared-parent paths so the fallback never fires) and this is a pre-launch repo where breaking API cleanup is preferred. A valid seam/design smell, but the security framing is hypothetical. Adjusted to low.

#### `I2-5` — Session cookie validated against unsigned payload before signature verification
**Severity:** low · **Dimension:** bug · **Subsystem:** CLI · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-operator/src/access.rs:198`

**Finding.** `authorize_session_cookie` reads `payload.generation` and `payload.session_id` from the un-verified cookie payload and uses them to return `Revoked` (line 198), look up the stored session (line 201), and return `Invalid`/`Expired` (lines 202-208) — all before the HMAC signature is checked at line 210. Because `sessions` is keyed by a 256-bit server-generated random id, this is not an auth bypass, but it is a defense-in-depth defect and a state oracle: the distinct return values (Revoked vs Invalid vs Expired) leak server-side session/generation state derived purely from attacker-controlled, unsigned bytes. Canonical order is verify-then-trust.

**Fix direction.** Verify `verify_session_signature` first; only after the signature is valid should the code trust `payload.generation`/`payload.session_id` for the generation, lookup, and expiry checks.

**Verification evidence.** Re-read access.rs:182-217: confirmed the ordering — payload.generation (line 198 → Revoked), guard.sessions.get(payload.session_id) (line 201 → Invalid), stored.generation (204 → Revoked), stored.expires_at (207 → Expired) are all evaluated BEFORE verify_session_signature at line 210. So values from the unsigned base64 payload drive the return discriminant before the HMAC check; canonical order is verify-then-trust. The distinct statuses are observable (access_policy.rs:128-133 maps them to distinct messages auth.token_revoked / auth.session_expired / generic). However the finding's own text concedes it is not an auth bypass, and the actual oracle leakage is negligible: session_id is a 256-bit server-random key (not enumerable, so the Invalid-vs-found distinction reveals nothing useful), and generation is a small non-secret monotonic counter. A legitimate verify-then-trust hygiene defect, but 'bug'/medium overstates it given near-zero exploitable information. Adjusted to low.

#### `I2-6` — Two distinct public `EmbeddedAsset` structs in one crate
**Severity:** low · **Dimension:** idiomatic-naming · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-assets/src/js_packages.rs:65`

**Finding.** `ui::EmbeddedAsset { data: Cow<'static,[u8]> }` (ui.rs:11) and `js_packages::EmbeddedAsset { data: Vec<u8> }` (js_packages.rs:65) are both `pub` in nimbus-assets with the same name but different field types and ownership semantics. They are currently only used intra-crate, but as the public surface grows this collision invites confusing imports (`use nimbus_assets::{ui, js_packages}` then ambiguous `EmbeddedAsset`).

**Fix direction.** Rename to module-specific types (e.g. `UiAsset` / `PackageAsset`) or unify on one representation, given both wrap embedded bytes.

#### `I2-7` — Sibling parse methods return different error types (`String` vs `Error`)
**Severity:** low · **Dimension:** code-smell · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-machine/src/lib.rs:200`

**Finding.** On the same crate, `MachineVolume::parse` returns `Result<Self, String>` (lib.rs:200) while `MachineImageSource::parse` returns `Result<Self, nimbus_core::Error>` (lib.rs:155). The `String` shape exists only to satisfy the clap value-parser at `nimbus-bin/src/machine/mod.rs:222`, but a data/contract crate shaping one method's errors for a CLI framework while the neighbor uses the domain error type is an inconsistent, leaky abstraction.

**Fix direction.** Return the domain `Error` from both and let the CLI layer adapt to clap's `String` at the boundary, or document why the volume parser intentionally diverges.

#### `J-1` — Async atomic write batch fails closed where the sync path silently falls back to a fresh execution unit
**Severity:** low · **Dimension:** gap · **Subsystem:** Misc · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bridge/src/capabilities.rs:163-178`

**Finding.** execute_atomic_write_batch (sync) with no mutation execution unit creates one on the engine and runs the batch; execute_atomic_write_batch_async (async) with no execution unit instead returns Error::InvalidInput("async atomic write batch execution requires an active mutation execution unit"). The same logical operation behaves differently by sync/async. This is reachable: cloud-functions async Firestore writes (nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs:304/360/412) route Admin SDK set/update/delete through execute_atomic_write_batch_async, and an action context (InvocationKind != Mutation) has no execution unit, so async Admin writes from an action error out while the sync equivalents succeed. Either both should fall back to the engine, or both should fail closed with the same error.

**Fix direction.** Make the async branch mirror the sync fallback: begin a mutation execution unit on the engine and stage/execute the batch (respecting cancellation), or deliberately fail closed in both and document why async cannot create an ad-hoc execution unit.

**Verification evidence.** Re-read capabilities.rs:144-178: the source-level inconsistency is REAL — sync execute_atomic_write_batch falls back to engine.begin_mutation_execution_unit() when host.mutation_execution_unit() is None (lines 153-160), while execute_atomic_write_batch_async returns Error::InvalidInput in the same case (lines 174-177). However the finding's reachability/exploitation claim is REFUTED. The bridge async-batch helper is consumed ONLY by cloud-functions firestore.rs (grep over crates/ shows execute_atomic_write_batch_async has exactly the 3 callers at firestore.rs:304/360/412). Every cloud-functions invocation entry point is constructed with InvocationKind::Mutation (http/invocation.rs:99,116 and trigger_executor.rs:104,121 — the only non-test InvocationKind:: uses in the crate). Per lib.rs:51, execution_unit is Some iff InvocationKind::Mutation, so for cloud functions the execution unit is ALWAYS Some and the async helper always takes the stage_atomic_write_batch branch (line 172); the InvalidInput branch is currently dead code. Cloud functions have no 'action' invocation kind, so the claimed scenario ('async Admin writes from an action error out while the sync equivalents succeed') cannot occur. Convex does not call this helper. The divergent fail-closed-vs-fallback branch is a latent inconsistency worth aligning, not a reachable medium-severity gap. Low.

#### `J-2` — validate_host_call_session never binds the incoming token to the session — only rejects empty strings
**Severity:** low · **Dimension:** gap · **Subsystem:** Misc · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-bridge/src/state.rs:54-68`

**Finding.** RuntimeHostState generates a unique host_call_session_id (prefix + global AtomicU64) and exposes host_call_session_id(), but validate_host_call_session only errors when the caller passes Some(""). It never compares the incoming token to the session's generated id, and no caller anywhere in the workspace compares them (grep for `host_call_session_id ==` is empty). The error message ("runtime host-call token must not be empty for tenant ...") implies a security/session-binding intent that is not enforced: a host call carrying any non-empty token (or None) is accepted regardless of which session it belongs to. Either the token model is vestigial and should be removed, or it should actually validate token == self.host_call_session_id().

**Fix direction.** Decide the contract: if tokens must bind a call to its session, compare host_call_session_id against self.host_call_session_id (and the tenant). If they are not a security boundary, delete the accessor/validation and the per-payload host_call_session_id fields to avoid implying a guarantee that does not exist.

**Verification evidence.** Confirmed the mechanics: RuntimeHostState::new generates host_call_session_id = prefix + AtomicU64 (state.rs:24-30) and exposes host_call_session_id() (state.rs:43-45), but validate_host_call_session (state.rs:54-68) only errors when Some("") is passed — it never compares the incoming token to self.host_call_session_id. grep 'host_call_session_id ==' across the workspace is empty; all three validate_host_call_session impls (lib.rs:192/202, convex bridge.rs:201/217, state.rs:54) delegate to the same is_empty()-only check. The getter is used only to INJECT the id into outgoing payloads (async_bridge dispatch.rs, read_tracking/builders.rs, tests) and tracing spans, never to verify an inbound token. The error message ('runtime host-call token must not be empty') implies a binding contract that is not enforced — a real vestigial/misleading-validation finding. But 'medium' overstates impact: the session id is Nimbus-generated and round-trips through the trusted host/runtime ABI, not an attacker-controlled external auth boundary, and there is no nonce store it could gate against. Dead/misleading validation, not an exploitable bypass. Low.

#### `J-3` — Convex adapter reimplements the bridge's document capability helpers instead of delegating
**Severity:** low · **Dimension:** modularity · **Subsystem:** Misc · **Verification:** real but severity-adjusted on re-read

**Location:** `crates/nimbus-server/src/adapters/convex/host_bridge/db_ops/documents.rs:20-120`

**Finding.** The bridge exposes canonical capabilities::{get,insert,update,delete}_document plus their async/cancellable variants that encapsulate the execution-unit-vs-engine branching and read recording. nimbus-cloud-functions correctly imports and uses these (firestore.rs:9-12). The convex adapter, however, hand-rolls the identical branching in host_bridge/db_ops/documents.rs (e.g. invoke_ctx_db_get_cancellable at lines 82-120 duplicates capabilities::get_document's mutation_execution_unit().map_or_else(engine fallback) + record-on-Some logic), while only implementing the RuntimeCapabilityHost trait. This is the duplication the bridge seam was created to remove, and it forks behavior maintenance across two crates (e.g. the absent-read recording choice now lives in two places).

**Fix direction.** Route convex ctx.db get/insert/patch/delete through nimbus_bridge::capabilities::* (as cloud-functions does), keeping only convex-specific JSON envelope shaping (document_to_convex_json / ConvexRuntimeResponseEnvelope) at the edges.

**Verification evidence.** Confirmed the duplication is real. documents.rs:98-114 (invoke_ctx_db_get_cancellable) re-implements mutation_execution_unit().map_or_else(engine get_document_with_principal fallback, execution_unit.get_document) plus record_document_read-on-found (line 116) — the same shape as bridge capabilities::get_document (capabilities.rs:78-101). Insert/patch/delete (documents.rs:139-162, 222-245, 307-329) likewise inline the execution-unit-vs-engine branching that capabilities::{insert,update,delete}_document encapsulate. ConvexHostBridge already implements RuntimeCapabilityHost in full (bridge.rs:216-243), and cloud-functions firestore.rs demonstrates the seam supports adapter-specific encoding while delegating the branching. So convex could delegate and keep only its Convex-specific id-resolution / JSON-envelope wrapping, but instead forks it — a legitimate modularity/seam-bypass finding. Downgrade rationale: behaviors currently match (both skip read-recording on absent docs), there is no functional bug, and git history shows the convex inline path and the bridge helpers have coexisted since baseline (not a fresh regression) — pre-existing maintainability debt. Low.

#### `J-4` — Stale `service` parameter name for an `Engine` in commit-intersection helper
**Severity:** low · **Dimension:** idiomatic-naming · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bridge/src/read_tracking/intersection/commit.rs:6`

**Finding.** commit_intersects_runtime_read_set takes `service: &nimbus_engine::Engine` and calls `service.get_document(...)`. The rest of the bridge crate consistently uses `engine` (state.rs, capabilities.rs, lib.rs), and CLAUDE.md routing lists the engine `Service` -> `Engine` rename as active work. This is the lone stale `service`-for-Engine spot in the subsystem.

**Fix direction.** Rename the parameter to `engine` and update the `service.get_document` call accordingly.

#### `J-5` — Redundant document-read branch in subscription base-query synthesis
**Severity:** low · **Dimension:** simplification · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bridge/src/read_tracking/subscriptions.rs:83-89`

**Finding.** In synthesize_runtime_subscription_base_queries_for_table, after the predicate/index/window loops, there are two consecutive blocks: `if queries.is_empty() && documents.any(table) { push broad }` immediately followed by `if queries.is_empty() { push broad }`. The first block is fully subsumed by the second — both push the exact same broad_runtime_subscription_query, so the document-existence distinction has no observable effect. Either the document case was meant to produce a different (e.g. document-scoped) query, or the first block is dead.

**Fix direction.** Delete the redundant document-specific block, or if document-only reads were meant to synthesize a narrower subscription, implement that distinct query instead of falling through to the broad query twice.

#### `J-6` — Absent-document reads are not tracked as dependencies in the bridge get path
**Severity:** low · **Dimension:** gap · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bridge/src/capabilities.rs:96-100`

**Finding.** capabilities::get_document / get_document_async record the read only `if document.is_some()` (inspect closures at lines 96-100 and 137-141), and the RuntimeHostContext::record_document_read callback is only invoked on a present document. By contrast the engine's own execution-unit get_document records the document dependency unconditionally (nimbus-engine .../execution_units/reads.rs records before returning, even when absent). For query/subscription contexts (no execution unit) the bridge path is the only recorder, so reading a missing id and later inserting it will not invalidate the subscription. This matches the convex adapter's existing behavior (so it is consistent, not a regression), but it is a real reactive-correctness gap relative to the execution-unit model and should be an explicit, documented decision.

**Fix direction.** Either record the document read unconditionally (so absent reads create insert-invalidation dependencies, matching the execution unit) or add a code comment documenting the deliberate absent-read non-tracking and its reactive implications.

#### `J-7` — Per-read engine table_id lookup on every recorded document read
**Severity:** low · **Dimension:** optimization · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bridge/src/lib.rs:225-233`

**Finding.** RuntimeHostContext::record_document_read calls self.engine.table_id(&self.tenant_id, &locator.table) for every recorded read; engine.table_id does a tenant-registry get_existing_tenant + store lookup (query_api.rs:37-40). On read-heavy runtime invocations this repeats the same tenant/table resolution for every document of the same table.

**Fix direction.** Cache resolved TableId per TableName within the host context/state for the lifetime of an invocation, or resolve table ids once when first touching a table.

#### `J-9` — WebSocket fixture conflates stream-closed and timeout, producing misleading panic messages
**Severity:** low · **Dimension:** test-quality · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-testing/src/websocket_fixture.rs:143-149`

**Finding.** next_message_with_timeout returns None for both a timeout (Err(_)) and a cleanly closed stream (Ok(None)). Callers next_message/next_json/next_binary then panic with .expect("timed out waiting for websocket message"). A test where the server closes the socket early will therefore fail with a 'timed out' message that points away from the real cause (premature close), costing debugging time in the very reactive-loop tests this fixture serves.

**Fix direction.** Distinguish the two cases (e.g. return a Result or an enum, or have the next_* wrappers emit different panic messages for closed-before-message vs timeout) so failures name the real condition.


## Info / by-design / positives (28)

#### `E2-8` — Aggregation result encoder is asymmetric between gRPC and REST surfaces (parallel codecs, both correct)
**Severity:** info · **Dimension:** idiomatic-naming · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/grpc/unary.rs:1030-1039`

**Finding.** RunAggregationQuery on the gRPC path encodes aggregate fields with encode_nimbus_value_to_grpc (unary.rs:1035), while the REST path encodes them with serializer::encode_proto_json_value (response.rs:64). This is correct (each targets its own wire format: proto Value vs proto-JSON), so it is not a bug. It is flagged only because the two parallel codecs are easy to mistake for an inconsistency and any future value-type addition must be mirrored in both, with no shared test guaranteeing parity.

**Fix direction.** No behavior change needed. Optionally add a parity test that a representative aggregate value encodes equivalently through both codecs, and/or a comment cross-referencing the two encoders so a future contributor knows both must be updated together.

#### `E2-9` — unary.rs is a 1328-line composition-root switchboard mixing handler dispatch with many inline lowering helpers
**Severity:** info · **Dimension:** modularity · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-firebase/src/grpc/unary.rs:1041-1064`

**Finding.** grpc/unary.rs (1328 lines) is the composition root for every unary Firestore handler (commit, get, batch_get, batch_write, list_documents, list_collection_ids, create, update, delete, begin/rollback transaction, run_query, run_aggregation) and also hosts numerous inline lower_* helpers (lower_collection_selector, lower_projection, lower_query_filter, proto_aggregation_result, etc., e.g. lines 1030-1064). It is under the repo's 1,500-line soft threshold so it needs no justification today, but per the repo's composition-root guidance new logic should accrue in concept-owned children (e.g. a query-lowering module) rather than back into this switchboard.

**Fix direction.** When this file next grows toward 1,500 lines, extract the structured-query / aggregation lowering helpers into a concept-owned module (e.g. grpc/query_lowering.rs) and keep unary.rs as thin handler dispatch.

#### `E4-3` — tempfile listed in both [dependencies] and [dev-dependencies] of nimbus-convex
**Severity:** info · **Dimension:** code-smell · **Subsystem:** Adapters · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-convex/Cargo.toml:37`

**Finding.** tempfile.workspace = true appears at Cargo.toml:33 ([dependencies]) and again at Cargo.toml:37 ([dev-dependencies]). tempfile is legitimately a production dependency (lib.rs:166 artifact_guard: Option<Arc<tempfile::TempDir>>; registry/loading.rs:51-53 tempfile::Builder), so the [dev-dependencies] entry is redundant — the regular dependency already makes tempfile available to tests.

**Fix direction.** Delete the duplicate tempfile entry from [dev-dependencies] (line 37); the [dependencies] entry at line 33 already covers tests.

#### `A1-3` — Schema cache refresh happens after COMMIT becomes visible (all SQL backends)
**Severity:** info · **Dimension:** seam · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/sqlite/write.rs:801`

**Finding.** In the SQL write commit paths the schema cache is refreshed strictly after the transaction is made visible: SqliteWriteTransaction::commit reloads load_schema_from_conn after `COMMIT` (write.rs:800-808), and PostgresWriteTransaction::commit invalidates the schema cache handle after `COMMIT` (write.rs:1170-1173). This opens a sub-millisecond window where committed data is visible to readers while the in-process schema cache is momentarily stale. In practice writers are serialized per tenant (Postgres pg_advisory_xact_lock, MySQL FOR UPDATE, libsql Immediate, redb single write_txn), so a concurrent writer cannot interleave, and reads tolerate eventual cache refresh; hence this is documented as informational, not a defect. Flagged so a future reader does not mistake the post-commit ordering for a missed in-transaction step.

**Fix direction.** No change required; if tightened later, document the post-commit cache-refresh ordering and its reliance on per-tenant write serialization at the commit sites so the window is intentional and visible.

#### `A1-4` — Batch Update upserts but never removes a resource-path binding (by design — confirm with a test)
**Severity:** info · **Dimension:** gap · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/store/write/batch.rs:250`

**Finding.** apply_update only calls upsert_resource_path_binding_in_write_txn when the binding is Some (batch.rs:250-252); it never removes a binding when the update carries None. This is correct under the current model: a resource-path binding is keyed by the document's locator (table+id), is stable for the document's lifetime, and is removed only on delete (direct.rs:127 and batch.rs:305 both call remove_resource_path_binding_in_write_txn). An update therefore cannot legitimately 'drop' a binding while keeping the document, so the never-remove-on-update behavior is intended, not a leak. The risk is latent: if a future change ever allows a bound document's path to be rescinded via update, this path would silently leave a stale binding. Recorded as info so the invariant is captured.

**Fix direction.** Add a brief comment at the upsert site noting that bindings are locator-stable and only removed on delete, and add a regression test asserting that updating a bound document leaves its binding intact (and that no API path can rescind a binding via update). If path-rescind-on-update is ever introduced, add a remove branch here.

#### `A3-10` — Simulation harness is confirmed wired into real, assertion-heavy coverage
**Severity:** info · **Dimension:** test-quality · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/tests/generated_history.rs:1`

**Finding.** Positive finding (refutes the 'is the sim harness dead code?' lead). The DeterministicHarness, FaultInjector family, ManualClock, ScenarioSignal/SignalRegistry, and GeneratedTaskHistory are exercised by tests that assert concrete outcomes — model-vs-actual document state, PITR archive round-trip, CDC sequence parity, redb/sqlite cross-backend parity, and a crash-replay recovery campaign — not just 'did not panic'. ScenarioSignal::wait correctly creates the notified() future before checking the triggered flag, avoiding a lost-wakeup. Retention gc_watermarks (saturating window floor, pinned_floor.min(window_floor)) and changefeed cursor monotonicity (next_cursor = last sequence, floor = first-key-minus-one, ensure_cursor_at_or_above_floor on every transition) were probed and are correct. No action required; recorded so the orchestrator can mark these focus areas validated.

**Fix direction.** No change needed. Keep the harness as the canonical determinism/fault-injection seam for new storage invariants.

#### `A3-9` — Encryption profiling uses eprintln + ad hoc env vars on the resolve path
**Severity:** info · **Dimension:** code-smell · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/encryption/runtime.rs:232-254`

**Finding.** generate_database_encryption_key emits profiling via eprintln! gated on NIMBUS_ENCRYPTION_PROFILE / NIMBUS_PROFILE_ONLY_COLD_SAMPLES (string-substring 'cold-sample' path matching). This is fine for ad hoc perf work but is unstructured logging on a security-sensitive path and a stringly-typed scope filter; it bypasses whatever tracing/log facility the rest of the crate uses.

**Fix direction.** Route profiling through the crate's tracing infrastructure (e.g. a `tracing::debug!`/span) rather than eprintln, and ensure no path/secret-adjacent data is logged; consider gating behind a feature flag rather than runtime env substring matching.

#### `A4-5` — async_storage/helpers.rs is a util-bucket name discouraged by repo modularity rules
**Severity:** info · **Dimension:** idiomatic-naming · **Subsystem:** Storage · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-storage/src/async_storage/helpers.rs:1`

**Finding.** CLAUDE.md explicitly prefers concept-owned names over helpers.rs/common.rs/misc.rs/utils.rs unless ownership is truly shared and obvious. async_storage/helpers.rs holds only the two async-task error mappers (map_join_error, map_permit_error). The content is cohesive (async-task error mapping) and would read better under a concept name. Minor and only flagged because the repo sets this convention itself.

**Fix direction.** Rename to a concept-owned module such as task_error.rs (or fold the mappers into the shared mapper module from A4-4) once that consolidation happens.

#### `C1-5` — Extension-transpile path panics/unwraps instead of returning an error
**Severity:** info · **Dimension:** safety · **Subsystem:** Runtime · **Verification:** real but severity-adjusted on re-read

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/transpile.rs:163`

**Finding.** maybe_transpile_source (transpile.rs) calls deno_core::url::Url::parse(&name).unwrap() (transpile.rs:169) and panic!(...) for an unsupported media type (transpile.rs:163-165). The surrounding function already returns Result<_, JsErrorBox> and uses ? elsewhere, so these are the only two non-recoverable exits. `name` is an internal ext:/node: specifier today, so reachability is low, but a malformed specifier or an unexpected media type aborts the process rather than surfacing a JS error, contrary to repo policy on reachable panics.

**Fix direction.** Replace the panic! with `return Err(JsErrorBox::generic(...))` and the .unwrap() with `.map_err(|e| JsErrorBox::generic(...))?`, keeping the function total.

**Verification evidence.** Re-read transpile.rs:146-201: confirmed panic! at 163-165 for unsupported media type and Url::parse(&name).unwrap() at 169, both inside a Result-returning fn. But this function is wired only as the extension_transpiler (142, extension_transpiler_for_target), consumed by snapshot/bootstrap/construction (startup.rs:91,154; construction.rs:158) for built-in ext:/node: extension modules whose specifiers the embedder fully controls at build/snapshot time - it is NOT the user module loader. media_type is forced to TypeScript for node: names (154-158); the panic arm requires a non-JS/TS/Mjs extension module, an embedder/build mistake. It is a verbatim copy of upstream deno (nimbus/deno and denoland/deno runtime/transpile.rs:24-46, identical unwrap + panic), which ships it. The finding itself concedes 'reachability is low.' name is never user-influenced, so an untrusted-JS-triggered process abort is effectively unreachable. Downgraded medium to info.

#### `C1-7` — Only CtxServiceLookup is grant-checked in-runtime; all other host ops trust host-side enforcement
**Severity:** info · **Dimension:** seam · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs:181`

**Finding.** enforce_host_call_grants (shared.rs:181-207) returns Ok early for every operation except HostCallOperation::CtxServiceLookup, deferring all other authorization to the HostBridge implementation. This is a deliberate seam (the runtime is the untrusted side and the host re-validates), and it is consistent with the documented adapter-boundary trust model, so it is not a defect. Flagging only so the boundary is explicit: the runtime must never be relied on as the authorization point for db/storage/identity host calls; the host bridge is the sole enforcement seam for those.

**Fix direction.** No code change required; keep host-side enforcement authoritative. Optionally add a comment at enforce_host_call_grants stating that non-service ops are intentionally host-enforced, so a future reader does not mistake the early-return for a missing check.

#### `C2-5` — choose_worker reads load and last_assigned_sequence as a non-atomic tuple for tie-break
**Severity:** info · **Dimension:** code-smell · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/executor/queue/router.rs:131`

**Finding.** In RuntimeWorkerRouter::choose_worker the min_by_key closure reads worker.load and worker.last_assigned_sequence as two separate SeqCst loads (router.rs:136-140), and again compares affinity_load vs least_loaded_load with independent loads (lines 158-162). Under concurrent dispatch these snapshots can be mutually inconsistent, so the tie-break/affinity-vs-least-loaded decision is best-effort rather than exact. This is acceptable for a routing heuristic (no correctness impact: note_assignment/complete_worker_job keep the counters themselves consistent), but the inexactness is undocumented and could surprise a future reader reasoning about load-balancing guarantees.

**Fix direction.** Add a short comment that routing reads are an eventually-consistent snapshot used only as a heuristic, so future maintainers do not assume an atomic (load, sequence) pair.

#### `C2-6` — finish_failed_start is a thin pass-through to finish_invocation with no added behavior
**Severity:** info · **Dimension:** idiomatic-naming · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs:69`

**Finding.** CooperativeWorkerLoop::finish_failed_start (execution.rs:69-92) only reorders parameters and forwards directly to finish_invocation with identical semantics; it adds no failed-start-specific logic. The separate name implies distinct handling that does not exist, and the parameter reorder (cancellation_for_metrics before execution_started_at) invites a transposition bug at the single call site.

**Fix direction.** Inline finish_failed_start into its one caller (admit_job, execution.rs:175) calling finish_invocation directly, or drop the wrapper and keep a single ordering for the metrics/started-at args.

#### `C3-9` — verify_scaffold_contract runs a full lifecycle simulation on every Bun/JSC invocation
**Severity:** info · **Dimension:** optimization · **Subsystem:** Runtime · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs:95`

**Finding.** BunJscRuntimeBackend::invoke calls BunJscPool::verify_scaffold_contract() (bun_jsc/mod.rs:95) on every invocation. The scaffold contract walks the full ack-driven lifecycle state machine as a self-check; running it per-invocation pays a fixed simulation cost on the hot path for an invariant that does not change between calls within a process. This is correctness-preserving and only an efficiency note.

**Fix direction.** Run the scaffold-contract self-check once at pool construction (or behind a debug-assert / first-use Once) rather than on every invoke, keeping a per-invocation cheap assertion if a runtime guard is still desired.

#### `D2-6` — LocalAdminTokenRecord::authorize uses an HMAC-of-self construction where a direct constant-time compare would be clearer
**Severity:** info · **Dimension:** code-smell · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-operator/src/token.rs:39`

**Finding.** authorize derives an HMAC key from self.token and then signs self.token with it, producing expected = HMAC(token, token), and verifies the candidate against that. The result is correct and constant-time (only candidate == self.token verifies), but HMAC-keyed-and-messaged on the same secret is an indirect way to express 'constant-time compare candidate to self.token'. A reader has to reason about why key and message are identical. A direct constant-time byte comparison (ring::constant_time::verify_slices_are_equal) of candidate vs self.token expresses the intent plainly and is equivalent.

**Fix direction.** Replace the HMAC-of-self with a length-checked constant-time slice comparison of the candidate against self.token, or add a comment documenting that the HMAC construction is intentionally a constant-time equality primitive.

#### `D3-3` — Subscription key builders panic via `.expect` on serde serialization of internally-built values
**Severity:** info · **Dimension:** code-smell · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/convex/subscriptions/socket/mod.rs:284`

**Finding.** `plain_subscription_query_key` and `named_subscription_query_key` call `serde_json::to_string(...).expect("... key should serialize")` on a `&Query` and on a freshly-built `serde_json::json!` object (socket/mod.rs:284 and :294-301). These inputs are server-side structs / json! literals whose serialization cannot realistically fail, so the panic is effectively unreachable. It is an info-level smell because it embeds an infallible-by-construction assumption as a runtime panic rather than encoding it in the type or returning a structured error.

**Fix direction.** Either leave as-is (acceptable for infallible serialization) or have these helpers return `Result<String, Error>` and propagate, consistent with the rest of the adapter's error-as-value style.

#### `D3-4` — Raw query/mutation HTTP surface accepts arbitrary client-supplied queries/mutations gated only by engine access policy
**Severity:** info · **Dimension:** seam · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/convex/handlers/function_routes/queries.rs:75`

**Finding.** `ConvexQueryRequest::Raw { query }`, `ConvexPaginatedQueryRequest::Raw { query }`, and `ConvexMutationRequest::Raw { mutation }` let a client submit an arbitrary structured query/mutation that bypasses the function registry entirely (queries.rs:75-88, :175-189; mutations.rs:67-78). These paths do route through the same principal-bearing engine entrypoints as named functions (`execute_query_result_async` / `dispatch_convex_mutation_async` / `paginate_documents_async_cancellable_with_principal`, all carrying `auth` / `normalize_principal_context(auth)`), so they honor the engine access policy and do not constitute a write-path bypass. The seam note is that on a table with no configured access policy these endpoints grant arbitrary read/write to any authenticated (or anonymous, per deployment config) caller. This matches the Convex client protocol and is by-design, but it makes per-table access policy the sole authorization boundary for the Raw surface.

**Fix direction.** No code change required; ensure docs/adapters/convex make explicit that the Raw query/mutation endpoints rely entirely on engine access policy for authorization, so operators do not assume the function registry is the access boundary.

#### `D3-5` — Cancellation tests assert observable no-op behavior, a good pattern worth preserving
**Severity:** info · **Dimension:** test-quality · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/convex/tests/cancellation.rs:47`

**Finding.** The cancellation suite verifies real outcomes rather than mere non-panic: after a pre-cancelled action/HTTP-route dispatch it asserts both `Err(Cancelled)` AND that the target table is still empty (cancellation.rs:32-44 and :125-137), proving the mutation never reached storage. This is the correct enterprise-grade test shape (behavior, not compilation). Recorded as an info-level positive so the pattern is retained; the one small gap is that `runtime_cancellable_db_get_short_circuits_before_dispatch` only asserts the error and does not assert the read-tracking state was left untouched, which would strengthen the short-circuit guarantee for read paths.

**Fix direction.** Optionally extend the db-get cancellation test to assert no read was recorded into RuntimeHostState, mirroring the empty-table assertion the mutation tests already use.

#### `D4-6` — MongoDB listener tests cover only ping/unknown/legacy-opcode — no data or auth coverage
**Severity:** info · **Dimension:** test-quality · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/adapters/mongodb/listener.rs:184`

**Finding.** The listener test module (crates/nimbus-server/src/adapters/mongodb/listener.rs:91-209) asserts only ping success, unknown-command rejection, and legacy-opcode handling; the legacy-opcode test even accepts Ok/Err/timeout interchangeably (lines 203-207), so it asserts almost nothing. There is no end-to-end test that an insert/find round-trips, and critically no test that an unauthenticated data command is refused — the exact behavior that is silently broken in D4-1. The test gap is why the auth bypass went unnoticed.

**Fix direction.** Add a connection-level integration test that performs an insert/find round-trip, and (after D4-1) a test asserting an insert before SCRAM completion is rejected. Tighten listener_rejects_legacy_opcode to assert the connection actually drops rather than accepting any of three outcomes.

#### `D4-7` — Artifact-verifier process runner writes all stdin before reading stdout (latent pipe deadlock pattern)
**Severity:** info · **Dimension:** optimization · **Subsystem:** Server · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/artifact_verifier_effects/process.rs:42`

**Finding.** ProcessArtifactVerifierCommandRunner writes the entire stdin payload synchronously (crates/nimbus-server/src/artifact_verifier_effects/process.rs:42-47) before entering the stdout read loop (lines 50-84). If a future caller passes large stdin to a child that also produces large stdout before draining stdin, both pipe buffers can fill and deadlock. Today this is harmless: the only stdin-using caller (ArtifactVerifierCommandBackend, artifact_verifier_effects.rs:233) sends a small serialized request, and the cosign/slsa/sbom runners all use stdin: None. Flagging it as a latent pattern, not a live bug.

**Fix direction.** If stdin payloads ever grow, move the stdin write onto a separate thread/task (or use a concurrent read+write strategy) so the parent drains stdout while feeding stdin, eliminating the pipe-buffer deadlock window. No change needed while payloads stay small — document the invariant.

#### `F1-5` — No test asserts the external-policy worker is bounded under a hung backend
**Severity:** info · **Dimension:** test-quality · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-tenant/src/operator_policy/external.rs:338-352`

**Finding.** The external-policy backend has tests for the Deny outcome and for timeout producing a fail-closed error, but there is no test that exercises a backend which never returns (or returns after the timeout) and asserts the worker does not accumulate. This is the natural place to lock in the fix for F1-1: a regression test that spawns N admissions against a deliberately-blocking backend and asserts a bounded number of live workers / no unbounded growth.

**Fix direction.** Add a test with a fake backend that blocks on a barrier; drive several timeouts and assert worker count stays bounded (or that workers are joined/cancelled) once F1-1 is addressed.

#### `F2-10` — emulator_principal_from_bearer trusts unsigned JSON claims (intentional, document the gate)
**Severity:** info · **Dimension:** safety · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-auth/src/lib.rs:69`

**Finding.** emulator_principal_from_bearer parses the bearer token as raw JSON and returns a PrincipalContext with authenticated:true and the parsed claims, performing no signature verification. This is by design: the call site in nimbus-server/src/application_auth.rs is gated behind firebase_emulator_mock_auth_enabled, matching the Firebase Auth emulator contract where tokens are unsigned. Confirmed not reachable in non-emulator configuration. Recorded as info so the security boundary stays explicit: the safety of this function depends entirely on the caller-side feature gate, and any new caller must preserve that gate.

**Fix direction.** Keep the function name/docs unambiguous that it is emulator-only (e.g. a doc comment stating it MUST only be called when the emulator mock flag is enabled) so no future caller wires it into a signed-token path.

#### `F2-9` — object_fields panics via unreachable!() on non-object payloads
**Severity:** info · **Dimension:** safety · **Subsystem:** Security · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-system/src/records.rs:1026`

**Finding.** object_fields matches a serde_json::Value and falls through to unreachable!("system document seed payload must be an object") for any non-object input. All current callers pass json!({...}) object literals, so the invariant holds today, but the function is a private helper invoked from many record_* sites and a future caller passing a non-object would panic the projection task rather than return an error. On a status-projection path this is acceptable as an internal invariant; flagged as info so a future refactor keeps it total.

**Fix direction.** If object_fields ever takes externally-shaped input, return Result and map the non-object case to Error::Internal instead of unreachable!; otherwise leave as-is with the documented invariant.

#### `G-5` — Provenance gate silently no-ops when no RuntimeBundleProvenanceConfig is configured
**Severity:** info · **Dimension:** seam · **Subsystem:** Trust · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-server/src/execution/invocations/provenance.rs:21`

**Finding.** admit_runtime_bundle_artifact returns Ok(None) when provenance_gate is None (provenance.rs:21-23), i.e. an unconfigured registry skips bundle provenance verification entirely. This is the intended opt-in shape and is fine for pre-launch, but it means the security posture depends on every production registry path actually setting a gate; the artifacts/provenance crates themselves cannot enforce that. Flagging so the wiring (set_runtime_bundle_provenance in nimbus-convex/nimbus-cloud-functions) is treated as the real trust boundary and gets a default-on or explicit-opt-out decision before launch.

**Fix direction.** Track a launch-gate decision (default-on vs explicit opt-out) for whether a missing provenance config should be allowed in production, and assert the gate is present on trust-critical runtime lanes.

#### `G-6` — Binary subtype field is captured but silently dropped in projected_json and never validated
**Severity:** info · **Dimension:** idiomatic-naming · **Subsystem:** Trust · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-core/src/typed_scalar.rs:42`

**Finding.** TypedScalarValue::Binary { subtype: u8, data } carries a subtype (BSON binary subtype) that is preserved by serde round-trip but completely ignored by projected_json (line 42 destructures `Binary { data, .. }`). That is acceptable for the lossy clean-JSON view, but there is no validation that subtype is a known/sane value on construction (it is a public field accepting any u8), and a reader of projected_json cannot tell two different-subtype binaries apart. Low impact today; noting because Binary is the one typed scalar whose discriminating metadata is dropped in projection without a comment explaining the intent.

**Fix direction.** Add a short comment that subtype is intentionally not surfaced in the lossy JSON projection (the typed sidecar remains authoritative), mirroring the existing Number-precision comment.

#### `H1-7` — process.rs unsafe FFI liveness blocks lack SAFETY comments
**Severity:** info · **Dimension:** safety · **Subsystem:** Sandbox · **Triage:** structural pass (not individually re-verified)

**Location:** `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox/src/process.rs:1`

**Finding.** pid_is_alive uses `unsafe { libc::kill(pid as i32, 0) }` (unix) and OpenProcess/GetExitCodeProcess (windows) with no `// SAFETY:` justification, unlike the network.rs unshare/mount/umount2 blocks which are documented. The calls are trivially sound, but the inconsistency weakens the unsafe-audit story; additionally `kill(pid,0)` is subject to PID reuse, so as a liveness signal it can report a recycled PID as alive (acceptable for best-effort, worth noting where it gates readiness/restart decisions).

**Fix direction.** Add `// SAFETY:` comments on each unsafe block stating the FFI invariants, and document the PID-reuse caveat at the call sites that use pid_is_alive for lifecycle gating (prefer correlating with the conmon exit-status file where authoritative).

#### `I1-11` — sandbox_supervisor is a documented validation-only stub with packet enforcement hardcoded off
**Severity:** info · **Dimension:** gap · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bin/src/sandbox_supervisor.rs:86`

**Finding.** The hidden sandbox-supervisor command hardcodes packet_enforcement_active: false and validates the spec twice (once at 65, once at 75) without performing enforcement. This is documented and tested as a validation-only stub, so it is informational rather than a defect — flagged so the gap between the command surface and actual enforcement is tracked.

**Fix direction.** Track the enforcement implementation in the owning sandbox plan; remove the redundant double validate() call and the dead hardcoded flag if enforcement remains out of scope for this binary.

#### `I2-8` — `nimbus-operator` name overloads the Kubernetes "operator" term
**Severity:** info · **Dimension:** idiomatic-naming · **Subsystem:** CLI · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-operator/src/lib.rs:1`

**Finding.** The crate is named/documented as the "Local and deploy operator security model" (lib.rs:1) — "operator" meaning the human operating the local/deploy server. This is a deliberate, documented extraction (docs/plans/proof/server-crate-extraction-completion/fce4-operator.md), so it is not a bug, but in a backend that already speaks Convex/Firebase/k8s-adjacent vocabulary, "operator" strongly connotes a controller/reconciler. New readers may expect a control-loop, not loopback-origin/session/token policy. Phase-1 lead is otherwise refuted: the name is intentional and the role matches.

**Fix direction.** Optional: a one-line module doc clarifying "operator = the administrator operating this host's server, not a controller/reconciler" would remove the ambiguity at zero cost.

#### `J-8` — O(n^2) linear-scan dedup on read-set vector inserts
**Severity:** info · **Dimension:** optimization · **Subsystem:** Misc · **Triage:** structural pass (not individually re-verified)

**Location:** `crates/nimbus-bridge/src/read_tracking/read_set.rs:108-172`

**Finding.** record_index_range, record_predicate, and record_paginated_window each dedup with self.<vec>.iter().any(|existing| existing == &read) before pushing, making N inserts O(N^2). Read sets are normally small so impact is negligible, but a runtime function that reads many distinct predicates/windows would scale quadratically.

**Fix direction.** If these grow large in practice, derive Hash/Eq and store in a HashSet (or LinkedHashSet to preserve order) as is already done for tables/documents.


---

# Appendix — Methodology & verification

**Pipeline.** Phase 1 mapped all 27 crates and validated 8 documented architecture invariants; the only failure was documentation drift (see Part I §4 carryover). Phase 2 ran one deep finder per subsystem across ten dimensions (bug, gap, safety, seam, modularity, code-smell, optimization, simplification, idiomatic-naming, test-quality), then an adversarial verifier re-read every medium-or-higher claim and tried to refute it (verdicts: confirmed / downgraded / false_positive). Every surviving critical and high finding was then handed to an independent second skeptic with no prior context for a fresh read.

**Trust signals for a codex executor.** Critical/high findings are the most reliable — two independent agents substantiated each by reading the code. Medium findings marked *confirmed* or *severity-adjusted* were re-read once. Low/info findings come from the structural pass and were **not** individually re-verified — treat them as high-quality leads, but confirm the exact lines before refactoring, as line numbers may drift. Always re-open the cited file before editing.

**Run stats.** Phase 1: 36 agents. Phase 2: 66 agents, ~5.1M tokens, 27 subsystem targets, 193 findings survived verification, 14 double-verified, 0 disputed.
