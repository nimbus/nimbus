# Nimbus Full Codebase Review & Audit — 2026-06-16

**Scope:** entire fresh whole-repo audit of the current working-tree state — all
27 Rust crates (~413K LOC), 7 JS packages, plus docs/website/CI — followed by a
focused review of the uncommitted in-flight refactor, then regression-verification
of the prior (2026-06-09) critical/high findings.

**Method:** two multi-agent workflows. The main audit fanned out across 21
subsystem slices (find → adversarial-verify → synthesize → completeness-critic),
32 agents. The in-flight follow-up reviewed the 51-file uncommitted refactor across
5 clusters (find → adversarial-verify → synthesize), 6 agents. Every medium+
finding was adversarially verified (refute-by-default); refuted candidates were
dropped, not reported.

- **Baseline:** `HEAD 0d7ab207e`, working tree `51 files changed, +1053/-1127`.
- **Gates already green** at audit time: `cargo fmt --all --check`, `make clippy`,
  `make deny` all pass — no formatting/lint/dependency findings reported.
- **Harnesses (local-only, untracked):** `.claude/audit-full.mjs`,
  `.claude/audit-inflight.mjs`.

---

## Verdict

**Healthy for a pre-launch codebase. Zero critical, zero high.** Every audited
slice lands YELLOW — real but bounded medium-severity defects plus low/info polish,
no architecture-breaking or readily-exploitable issues.

| Severity | Main audit | Critic extras | In-flight | Total |
| --- | --- | --- | --- | --- |
| Critical | 0 | 0 | 0 | **0** |
| High | 0 | 0 | 0 | **0** |
| Medium | 8 | 1 | 0 | **9** |
| Low | 10 | 1 | 1 | **12** |
| Info | 1 | 0 | 0 | **1** |

- **All 27 crates + 7 JS packages reviewed.** No architecture invariant is violated
  (single mutation path, storage atomicity, `nimbus-core` zero-I/O,
  `nimbus-runtime` zero-workspace-deps, fail-closed egress/admission, isolates ≠
  sandboxes, no banned durable names in prod code).
- **The in-flight refactor is clean.** The 51-file uncommitted `Launch→Start`
  sandbox/services refactor preserves behavior; its three intentional behavior
  changes are all *improvements* (see below). Only residual: one low naming-drift.
- **Prior criticals are fixed.** The 2026-06-09 H1 ("mongodb adapter has no auth
  gate") is **resolved** for the auth-gate question — but the tenant-isolation half
  is an open, documented, loopback-gated design constraint (M9 below).

The nine mediums cluster into two themes: **(1) silent correctness/integrity
false-negatives** where a guard exists on paper but one path bypasses it, and
**(2) cross-backend / cross-path asymmetry** where equivalent code paths disagree.
None are excused by pre-launch posture — each is a genuine internal inconsistency,
not missing-backcompat noise. Several are *latent* (gated behind a not-yet-driven
lifecycle or a privileged path), so today's blast radius is low.

---

## Medium findings

### M1 — Encrypted-redb RMW silently swallows AES-GCM-SIV integrity failures
`security` · `storage-core` · `crates/nimbus-storage/src/encrypted_redb.rs:433-435` (file backend) & `:715-717` (memory backend)

Partial-page writes do read-modify-write via
`self.read_encrypted_page(...).unwrap_or([0u8; LOGICAL_PAGE_SIZE])`.
`read_encrypted_page` returns `InvalidData` ("decryption failed: wrong key or
corrupted page") on AES-256-GCM-SIV tag failure. Because both write paths
bounds-check `offset+len > logical_len` *first*, the RMW only runs on a page
guaranteed present — so the **only reachable failure `unwrap_or` catches is the
integrity failure itself**. Every other call site propagates with `?` (incl.
`set_len`, which does the same RMW correctly), proving the write path is the
anomaly. A partial write over a tampered page zero-fills and re-encrypts,
destroying the corruption evidence and surrounding plaintext.

**Fix:** match the error — zero-fill only on `UnexpectedEof` / "read beyond end of
buffer", propagate `InvalidData` out of `write`. Add a corruption-during-partial-
write test.

### M2 — SQLite/MySQL index DDL doesn't escape the single-quote delimiter (vs PostgreSQL)
`security` · `storage-providers` · `crates/nimbus-storage/src/sqlite/schema.rs:317-322` & `crates/nimbus-storage/src/mysql/query_helpers.rs:346-350`

SQLite `json_extract_expr` escapes only `"`; MySQL `mysql_generated_column_expr`
escapes `\` and `"` — neither escapes the surrounding `'...'` string-literal
delimiter. PostgreSQL routes the field through `postgres_string_literal` (doubles
`'`) and is safe. `validate_logical_name` (ASCII alnum + `_`/`-`) is applied to
tenant/table/index ids but **never to `FieldSchema.name`**, so a field named
`a' || (subquery) || '` breaks out of the JSON-path literal on SQLite/MySQL — and
the SQLite expr flows into `execute_batch` (multi-statement). Reachable only via the
privileged schema-authoring path (hence medium, not high), but a real latent
injection plus a cross-backend correctness divergence that violates the documented
multi-backend hardening invariant.

**Fix:** route field names through `validate_logical_name` in `nimbus-core` **and**
make SQLite/MySQL double `'` like PostgreSQL (defense in depth). Add a negative test
on all three backends.

### M3 — Egress proxy truncates plain-HTTP request bodies in the ForwardHttp path
`correctness` · `sandbox` · `crates/nimbus-sandbox/src/egress_proxy.rs:316-323` (with `read_http_headers` at `:330-357`)

`read_http_headers()` returns `Ok(())` the instant `find_header_end` matches — it
never reads the body. The `ForwardHttp` branch forwards only the co-buffered bytes
then immediately relays the response, and never reads the client socket again.
`tunnel_connect` (CONNECT/HTTPS) does bidirectional `io::copy`, so the asymmetry
confirms defect vs intent. Live in production via
`backends/container/runtime.rs:731`. Any allowed POST/PUT whose body doesn't fully
arrive in the same `read()` as the headers loses the remainder and stalls on the
response until the 10s timeout. HTTPS-via-CONNECT is unaffected.

**Fix:** after writing the buffered prefix, fully relay the client body to upstream
(second `io::copy` thread with `shutdown(Write)`, or drain by
Content-Length/Transfer-Encoding) before reading the response. Regression test with
a body larger than the header buffer.

### M4 — Consistency verifier digests `table_identities` but never field-compares them
`correctness` · `engine` · `crates/nimbus-engine/src/verification.rs:100-222` & `:291-305`; driven by `engine/queries/verification.rs:83`

`canonicalize` includes `table_identities` and `snapshot_fingerprint` hashes the
whole struct, so identity drift *does* change the digest. But
`compare_materialized_journal_snapshots` field-compares only version /
applied_sequence / durable_head / schema / documents / scheduled_execution_ids —
there is **no branch comparing `table_identities`**. `ok = mismatches.is_empty()`.
Since `table_identities` is a real mutable per-table field (Active/Hidden/Deleting
transitions), the authoritative/shadow/replica triad can genuinely diverge: two
snapshots differing *only* in stable table identity (the exact invariant this
verifier protects) yield different digests yet report `ok=true` with an empty
mismatch list — the human diff and the integrity flag silently disagree.

**Fix:** add a `table_identities` comparison branch (sorted `CanonicalTableIdentity`
vectors, emit a `table_identities` mismatch), or derive `ok` from fingerprint-digest
equality so the flag can never disagree with the hash. Test divergence on identity
only.

### M5 — Index-state maintenance inconsistent across write paths
`correctness` · `storage-core` · `store/write/batch.rs:170-176,236-251,301-307`; `store/schema_rewrite.rs:70-75`; `store/journal.rs:467-521`

Batch + journal-replay paths iterate `for index in indexes` with **no state
filter**; the interactive path filters `.is_maintained()` and the history path uses
`maintained_indexes()` (= Backfilling|Enabled). So Pending/Deleting indexes are
handled differently across equivalent paths. **Latent today** (IndexState defaults
to Enabled; every non-Enabled construction is `#[cfg(test)]`), but
`reconcile_index_metadata` preserves incoming state, so the lifecycle structurally
supports non-Enabled states reaching `table_schema.indexes`. Once the backfill
lifecycle ships, batch/journal paths will populate physical entries the interactive
path and version history deliberately omit — silent index drift, activating with no
error.

**Fix:** route every live-index write through `maintained_indexes()`/`is_maintained()`
in batch.rs, schema_rewrite.rs, journal.rs. Test that a staged Pending index writes
no INDEXES entry on the batch path.

### M6 — `reconcile_running` treats inspect `InvalidInput` as workload-missing, then starts
`correctness` · `tenant-node-operator` · `crates/nimbus-node/src/reconciler.rs:179-187`

The arm `Ok(_) | Err(Error::InvalidInput(_))` merges not-running with *all*
`InvalidInput` then calls `backend.start`. Missing-workload is `InvalidInput` only in
the in-memory backend; the live (zbus) backend maps missing units to
inactive-dead/`NotFound`, so a live `InvalidInput` is a genuine systemd `.Failed`
inspect fault. Now production-reachable via `node_workload_executor.rs:189-198`. A
real fault is silently treated as workload-absent and triggers a redundant
`StartTransientUnit`, masking the error in the converge loop. Bounded (requires a
live-Linux `.Failed` inspect fault; single standalone caller today).

**Fix:** return `Error::NotFound` for missing-workload in the in-memory backends and
match `Ok(_) | Err(NotFound)`, so `InvalidInput` propagates via the final `Err` arm.
Test both branches.

### M7 — Firestore value encoder cannot write Date/Timestamp/Bytes/GeoPoint/Reference
`correctness` · `js-firebase` · `packages/firebase/src/internal/document-data.ts:154-194` (encode) & `:239-248` (decode)

`encodeFirestoreValue` handles null/boolean/number/string/array/plain-object then
throws `Unsupported Firestore value type: ...`. No branch for Date, Uint8Array
(bytesValue), GeoPoint, or DocumentReference — yet `decodeFirestoreValue` returns
those wire kinds. So `setDoc(ref, { createdAt: new Date() })` — one of the most
common Firestore field types — throws on write while reads pass them back, breaking
read-modify-write round-tripping. In the real Firebase Web SDK these are first-class
writable types.

**Fix:** add encoder branches for Date (→ timestampValue ISO-8601), Uint8Array
(→ bytesValue base64), and the Timestamp/GeoPoint/Bytes/DocumentReference sentinel
shapes; OR, if intentionally out of scope, make decode *reject* (not passthrough)
those kinds and document the narrowing symmetrically.

### M8 — `convex/internal/shared.ts` forks the nimbus original and has already drifted
`compat-shim` · `js-sdk-ui` · `packages/convex/src/internal/shared.ts:1-218` (vs `packages/nimbus/src/internal/shared.ts:1-228`); dead helpers `:176-217`

Normalizing away the `Convex*`/`Nimbus*` prefixes, the two files are the same shapes
(identical QueryShape/.../ActionShape, define*/make* factories, validate/strip/
normalize bodies). The fork has **already drifted**: convex `websocketUrlFromBase`
builds `${url.pathname}/ws` while nimbus wraps it in `stripTrailingSlash()` — a
trailing-slash base yields a double-slash `//ws` only in the convex copy. Five
helpers (`validateDeploymentUrl`, `stripTrailingSlash`, `websocketUrlFromBase`,
`normalizeArgs`, `createConvexError`) have **zero importers** in `packages/convex` —
dead code carrying the pre-fix bug. The THIN-wrapper import-and-alias pattern is
already the norm in sibling files (server.ts/browser.ts/react.ts); this forked file
is the inconsistent exception, and the exact failure the rule exists to prevent.

**Fix:** re-export the shared shapes/factories from `@nimbus/nimbus/internal/shared`
and alias the Convex-branded names; delete the five dead drifted helpers (`:176-217`).

### M9 — MongoDB: one tenant-agnostic credential reaches every tenant via the wire `$db` name
`security` *(critic extra — cross-adapter)* · `crates/nimbus-mongodb/src/lib.rs:14-34`, `crates/nimbus-mongodb/src/commands/tenant.rs:10-15`, `crates/nimbus-server/src/adapters/mongodb/listener.rs:24`

`AuthConfig` holds exactly one SCRAM credential. `dispatch()` authenticates the
connection, but the tenant is then selected from the wire `$db` name via
`resolve_tenant_id(db_name)` — **not** from the authenticated principal. The
adapter's own (uncommitted) doc comment states it plainly: "A caller who knows this
one username and password can therefore reach every tenant by varying the database
name on the wire." Contrast DynamoDB, which binds each access key to a tenant.

**Resolution status (the question this audit was asked to settle):**
- **Auth gate = FIXED.** The prior 2026-06-09 H1 ("mongodb adapter has no auth
  gate") is resolved — `dispatch` requires authentication for all data commands
  (test `dispatch_rejects_data_command_before_authentication`).
- **Per-tenant credential binding = OPEN.** Mitigated *only* by
  `guard_listener_is_loopback_only` refusing non-loopback binds. It is a documented
  design constraint, not a fix.

**Fix:** track as an explicit launch-readiness item — before the MongoDB adapter may
bind any non-loopback address, bind each SCRAM credential to a specific tenant
(mirror DynamoDB's `AccessKeyRegistry`). The loopback guard must remain load-bearing
until then.

---

## Low / Info findings

| # | Sev | Cat | Slice | Title | Location |
| --- | --- | --- | --- | --- | --- |
| L1 | low | docs-accuracy | engine | Encryption test comments claim libsql replica is "the only fully-wired path", contradicting the module (all four families supported) | `engine/encryption/mod.rs:237,255` |
| L2 | low | docs-accuracy | storage-core | TenantStore docstring still frames redb as a transitional "migration window" awaiting SQLite; both are live peer backends | `store.rs:76-88` |
| L3 | low | compat-shim | storage-core | Dual `durable_record` terminology: pass-through wrappers + type alias duplicate the tenant-event-record API | `commit_log.rs:16-26`; alias `nimbus-core/mutation.rs:365` |
| L4 | low | naming | sandbox | Partial de-Launch rename: `launch_*` idents (incl. persisted serde field `launch_mode`) survived `*LaunchMode→*StartMode` | `krun/vm.rs:93,244,253,281`; `container/runtime.rs:85,439,951,982,988` |
| L5 | low | docs-accuracy | js-firebase | Nested field-path rejection says "not supported yet" — contradicts the "intentionally narrow" docs | `firestore-helpers.ts:376-380` |
| L6 | low | compat-shim | js-sdk-ui | `convex/values.ts` is a byte-identical copy of nimbus `values.ts`, not a thin re-export | `packages/convex/src/values.ts:1-66` |
| L7 | low | correctness | js-sdk-ui | `filterFunctionTree` returns `count:0` with a false "recomputed below" comment | `nimbus-ui/src/shell/function-tree.ts:137-138` |
| L8 | low | naming | js-sdk-ui | Hardcoded "convex" branding in the canonical `@nimbus/codegen` runtime-bundle emit | `codegen/src/emit/runtime_bundle_*.mjs` (multiple) |
| L9 | low | modularity | engine | `execution_units/tests.rs` is 2001 lines — over the 2000-line decompose-or-document threshold, no exception recorded | `engine/execution_units/tests.rs:1-2001` |
| L10 | low | modularity | storage-providers | Three ≥2000-line provider test files lack the required documented ownership exception | `tests/{libsql,mysql,postgres}_provider.rs` (2406/2286/2235) |
| L11 | low | consistency | adapters | DynamoDB (per-key tenant binding) vs MongoDB (db-name-derived tenant) — divergent credential→tenant models, no shared contract | `dynamodb/tenant.rs:71-186` vs `mongodb/commands/tenant.rs:10-26` |
| L12 | low | maintainability | sandbox/services | In-flight `Launch→Start` rename is half-done: public enums/fns renamed but internal `launch_*` surface + serde wire field `launch_mode` remain | `container/runtime.rs` (many); `krun/vm/start.rs` |
| I1 | info | modularity | sandbox | `krun/vm/tests.rs` (1526 lines) over the 1500-line soft threshold | `krun/vm/tests.rs` |

> L4 and L12 are the same root (the de-Launch rename). L4 was found against committed
> state; L12 confirms it persists in the uncommitted refactor and adds the detail that
> `launch_mode` is a serde wire field — renaming it is a deliberate (acceptable
> pre-launch) wire-format change, not a silent edit.

---

## In-flight refactor review (the 51-file uncommitted diff)

The completeness critic flagged the uncommitted working tree as the highest-churn
*unreviewed* surface. A focused 5-cluster follow-up reviewed it. **Result: behavior
preserved, zero correctness/security/regression findings, one low naming-drift
(L12).** Three intentional behavior changes — all improvements, all with real tests:

1. **`disk_limit_bytes` fail-closed parity** across both sandbox backends
   (`krun/bundle.rs`, `container/bundle.rs`) — now rejects any `Some(_)` (was only
   `Some(0)`), matching the documented SBR-C3 posture that `disk_limit_bytes` is
   unenforceable (host bind-mount, no OCI total-disk knob). Paired negative/positive
   tests.
2. **`LocalBuildAdmission` new fail-closed gate** — `ServiceManager` now carries an
   admission posture defaulting to `#[default] Denied`, **genuinely enforced**
   through `manager_image_policy() → admit_sandbox_root → admit_local_build`, and
   production-wired so only `ComposeAdmissionMode::LocalDevelopment` maps to
   `Allowed`. (Closes critic gap #4 — the new security type *is* reviewed and is
   correctly fail-closed + enforced.)
3. **DynamoDB TTL sweep atomicity** — the delete + its REMOVE stream event now commit
   in one `AtomicWriteBatch` (was two separate engine calls), closing a crash window
   and tightening the storage-atomicity invariant.

Everything else — the `krun vm/launch.rs → vm/start.rs` rebirth, the Container/Krun
`*Launch* → *Start*` enum/fn renames, the services-manager `launch.rs →
service_start.rs` rename, and the MongoDB/DynamoDB doc + dead-helper cleanups — is a
pure rename or pure-doc change with no logic/ordering/error-handling/serde-wire/
egress-wiring delta, verified clean with no dangling old-name references. **No banned
durable names** (`start_from_image`, `SandboxImageLaunchSpec`,
`ServiceImplementation`, `Backing*`) anywhere in prod code.

---

## Completeness-critic gaps (and their resolution)

The main audit's critic raised five gaps. Status after the follow-up:

1. **Uncommitted refactor under-reviewed** → **CLOSED** by the in-flight workflow
   (clean; only L12).
2. **Cross-adapter tenant-isolation not compared side-by-side** → **ADDRESSED** as M9
   + L11 (DynamoDB binds credential→tenant; MongoDB derives tenant from `$db`).
3. **Bundle SHA-256 enforcement is provenance-gated, not unconditional** —
   `verify_integrity()` (`nimbus-runtime/src/runtime/bundle.rs:187`) returns early
   when `expected_sha256` is `None`. Not a defect (path-backed bundles legitimately
   carry no recorded provenance hash, and re-hashing has nothing to compare against),
   but the ARCHITECTURE.md "integrity-checked before every invocation" wording was
   stronger than the code. **RESOLVED** by reconciling the doc wording with the
   provenance-gated reality (the gate is correct as written; the overclaim was the
   defect). Four surfaces tightened to "a bundle that carries a recorded provenance
   hash is verified before every invocation": `ARCHITECTURE.md` (invariant 5),
   `AGENTS.md` (Runtime bundles gotcha), `docs/concepts/how-nimbus-works.md`, and
   `docs/source-map.md`.
4. **`LocalBuildAdmission` unreviewed** → **CLOSED** (item 2 in the in-flight section;
   fail-closed + enforced + tested).
5. **Too-clean crates not deep-read** (`nimbus-provenance` 87 LOC, `nimbus-license`,
   `nimbus-artifacts`) — all have ≥1 test file; sizes/test-presence spot-checked, no
   findings, but riskiest provenance-policy logic not deep-read. **Residual low-risk
   gap** — candidate for a future focused pass if provenance enforcement becomes
   load-bearing.

---

## Recommended order of attack

1. **Silent false-greens / data-integrity first** (each is a silent false-green or
   corruption path): M1 (encrypted-redb RMW), M4 (consistency verifier), M3 (egress
   body truncation), M6 (reconciler InvalidInput).
2. **Cross-backend/cross-path asymmetry before the gating preconditions go live:**
   M2 (SQL-escape), M5 (index-state).
3. **Compat surface:** M7 (Firestore encoder), M8 (convex shared.ts fork), then the
   low compat-shim/naming cleanup (L3, L4/L12, L6, L8).
4. **Launch-readiness:** M9 — bind MongoDB SCRAM credentials to tenants before any
   non-loopback bind; keep the loopback guard load-bearing until then. Document the
   canonical "authentication decides tenant, never a wire-supplied name" rule (L11).
5. **Modularity bookkeeping:** record ownership exceptions or split L9/L10/I1.
6. **Posture decision:** reconcile the bundle SHA-256 doc wording with the
   provenance-gated implementation (critic gap #3).

---

## Remediation log

Every confirmed finding was re-verified by a workflow pass (22/22 confirmed, 0
refuted) and then remediated with the canonical fix. State below reflects the
live tree. **No commits made** (pre-launch / commit-only-when-asked); changes
sit in the working tree for review.

### Medium

- **M1 — encrypted-redb RMW** ✅ *fixed.* Both partial-write RMW sites
  (`encrypted_redb.rs:441` memory, `:727` file) now propagate the read with `?`
  instead of `unwrap_or([0u8; …])`, so an AES-GCM-SIV `InvalidData` integrity
  failure surfaces out of `write` rather than zero-filling over tampered
  ciphertext. Regression tests
  `partial_write_over_corrupted_page_propagates_integrity_error` (memory, `:964`)
  and `…_file_backend` (`:1001`).
- **M2 — SQLite/MySQL index DDL escaping** ✅ *fixed.* `FieldSchema.name` now
  routes through `validate_logical_name`, and the SQLite/MySQL JSON-path emitters
  double `'` like PostgreSQL (defense in depth). Negative tests on all three
  backends.
- **M3 — egress ForwardHttp body truncation** ✅ *fixed.* The plain-HTTP forward
  path now relays the full client body upstream before reading the response.
  Regression test with a body larger than the header buffer.
- **M4 — consistency verifier table_identities** ✅ *fixed.*
  `compare_materialized_journal_snapshots` gained a `table_identities` comparison
  branch (`verification.rs:140-177`): a count mismatch emits `table_identities`,
  per-table drift emits `table_identities.<key>`, so the integrity flag can no
  longer disagree with the fingerprint digest. Identity-only divergence test.
- **M5 — index-state maintenance asymmetry** ✅ *fixed.* Batch, schema-rewrite,
  and journal-replay paths now route live-index writes through the same
  `is_maintained()`/`maintained_indexes()` filter the interactive and history
  paths use. Test that a staged Pending index writes no INDEXES entry on the
  batch path.
- **M6 — reconciler InvalidInput** ✅ *fixed.* In-memory backends return
  `Error::NotFound` for missing-workload; `reconcile_running` matches
  `Ok(_) | Err(NotFound)` so a live `InvalidInput` (genuine systemd `.Failed`
  inspect fault) propagates via the final `Err` arm instead of triggering a
  redundant start. Both branches tested.
- **M7 — Firestore value encoder** ✅ *fixed.* The encoder/decoder now round-trip
  Date/Timestamp, Bytes, GeoPoint, and Reference values.
- **M8 — convex shared.ts fork** ✅ *fixed* (with **L6**). `convex/values.ts` and
  the shared surface collapse to thin re-exports of the canonical nimbus
  implementation rather than byte-identical copies.
- **M9 — MongoDB tenant-agnostic credential** ✅ *documented + tracked; full bind
  deferred.* The canonical cross-adapter contract ("authentication decides the
  tenant; a wire-supplied name never does") and MongoDB's current deviation are
  now documented at both deviation sites (`adapters/mongodb/listener.rs` guard,
  `mongodb/commands/tenant.rs` resolver) and in the L11 module doc. The
  load-bearing mitigation — `guard_listener_is_loopback_only` — is enforced
  before bind (`adapters/mongodb/mod.rs:46`) and tested
  (`listener_rejects_non_loopback_bind_address`, `listener.rs:186`). Binding each
  SCRAM credential to a tenant (mirroring DynamoDB's `AccessKeyRegistry`) is
  feature-sized launch-readiness work and is correctly deferred to its own wave;
  the loopback guard must stay load-bearing until it lands.

### Low / Info

- **L1** ✅ encryption test comments corrected (all four backend families wired,
  not just the libsql replica).
- **L2** ✅ `TenantStore` docstring reframes redb as a live peer backend, not a
  transitional migration window.
- **L3** ✅ dual `durable_record` terminology collapsed onto the canonical
  `TenantEventRecord` API (pass-through wrappers + alias removed).
- **L4 / L12** ✅ de-Launch rename completed: zero `launch_mode`/`LaunchMode`
  residue in `nimbus-sandbox`/`nimbus-services`; the persisted serde field is now
  `start_mode` (`KrunStartMode`) — a deliberate, acceptable pre-launch wire-format
  change.
- **L5** ✅ Firestore nested field-path message aligned with the "intentionally
  narrow" docs (no "not supported yet").
- **L7** ✅ `filterFunctionTree` returns the real count; the false "recomputed
  below" comment is gone.
- **L8** ✅ zero "convex" branding across the ten `@nimbus/codegen`
  `runtime_bundle_*.mjs` emit files.
- **L9** ✅ `execution_units/tests.rs` decomposed (parent now 25 lines + concept
  children `atomic_write_batch.rs`, `mutation_execution_unit.rs`, …).
- **L10** ✅ the three external-provider test files decomposed into the same
  `<provider>/{support,foundation,journal,schema,versions,execution_units}.rs`
  shape the SQLite provider tests already use; every child under 1500 lines;
  `cargo check --tests -p nimbus-storage` green.
- **L11** ✅ cross-adapter "authentication decides the tenant" contract +
  MongoDB deviation documented in `mongodb/commands/tenant.rs` (module doc); see
  M9.
- **I1** ✅ `krun/vm/tests.rs` decomposed into `tests.rs` + `tests/support.rs`
  (fixtures/harness extracted); 35 tests pass.

### Posture decision

- **Bundle SHA-256 doc reconcile** ✅ — the provenance-gated `verify_integrity`
  is correct; the overclaim was the defect. Reconciled across `ARCHITECTURE.md`
  (invariant 5), `AGENTS.md`, `docs/concepts/how-nimbus-works.md`, and
  `docs/source-map.md` (critic gap #3).

### Final gates

- `cargo fmt --all --check` — **pass** (exit 0).
- `make clippy` — **pass** (exit 0, full workspace clean).

*No commits made (pre-launch / commit-only-when-asked).*
