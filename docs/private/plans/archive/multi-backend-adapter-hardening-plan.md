# Multi-Backend Multi-Adapter Hardening Plan (MBA)

Nimbus serves multiple storage backends (`redb`, `sqlite`, `postgres`,
`mysql`, `libsql_replica`, retained redb control-plane storage, and local
encryption providers) behind multiple compatibility adapters
(`cloud_functions`, `convex`, `firebase`, `mongodb`, DynamoDB next).
This plan hardens the cross-cutting architecture so the N×M product of
backends and adapters stays trustworthy as both axes grow.

The patterns adopted here are taken largely from ExtendDB
(Apache-2.0, AWS-maintained DynamoDB adapter on PostgreSQL,
`~/src/github.com/ExtendDB/extenddb`). The [[apache-license-posture]]
memory confirms direct adoption is permitted; this plan favors
copy-with-attribution over reimplementation when the upstream shape fits.

This plan is independent of `dynamodb-adapter-plan.md`. Every roadmap item
applies to backends and adapters that already exist today; the DynamoDB
adapter is a downstream beneficiary, not a prerequisite.

## Why this plan exists

Enterprise trust requires that every adapter and every backend exhibit
the same hygiene: predictable error shapes, parameterized SQL,
attributable latency, durable events, dual-target test coverage, and an
honest debt ledger. Today these properties are uneven across the
existing surfaces:

- Storage traits are still close to one large surface; adding a backend
  forces stubbing operations the backend does not support.
- Adapter dispatch is hand-rolled `match` statements that grow on every
  adapter addition.
- There is no single document where known gaps live — they are scattered
  across plan archives, code TODOs, and conversation memory.
- Auth cache policy is implicit, not stated as an ADR.
- SQL-backed stores have safety patterns but no named ADR to make them
  greppable.
- Adapters lack dual-target tests against the real services they emulate.
- Request handlers lack per-segment latency budgets, so regressions in
  one layer hide inside the wall-clock total.

ExtendDB has solved each of these problems in production for a single
adapter on a single backend. Generalizing the patterns across Nimbus's
multiple-of-each shape is the work this plan owns.

## Scope

In scope:

- `crates/nimbus-storage/` trait surface and every backend under it
- `crates/nimbus-server/src/adapters/` registration surface
- `crates/nimbus-engine/` worker boundaries
- Request-handler instrumentation in `crates/nimbus-server/`
- Cross-cutting documentation: ADRs and `docs/technical-debt.md`
- `scripts/verify-multi-backend-adapter-hardening.sh` (this plan's verifier)
- Routing entries in `docs/plans/README.md` and `AGENTS.md`
- ExtendDB-vendored or git-revved code under
  `crates/nimbus-storage/vendor/extenddb/` and any equivalent paths,
  with `NOTICE` updates

Out of scope:

- DynamoDB adapter feature work — owned by `dynamodb-adapter-plan.md`
- Firecracker snapshot backend — owned by
  `firecracker-snapshot-invocation-backend-plan.md`
- Runtime backend selection (Deno/V8/Bun) — owned by its own plans
- Tenant isolation enforcement — already complete in archived
  `tenant-isolation-enterprise-hardening-plan.md`
- Distribution and release pipeline — owned by `distribution-plan.md`

## Ledger

| MBA  | Description | Status |
|------|-------------|--------|
| MBA0 | Scaffold this plan + the verifier at `scripts/verify-multi-backend-adapter-hardening.sh` with 15 conditions. Routing entries are present in `docs/plans/README.md` and `AGENTS.md`. Baseline proof at `docs/plans/proof/multi-backend-adapter-hardening/mba0-baseline.md` captures: current `nimbus-storage` trait surface and which providers still require typed enum dispatch; current adapter dispatch sites; current request-handler instrumentation depth; current auth-cache behavior; current technical-debt surfacing. Inspiration audit at `docs/plans/proof/multi-backend-adapter-hardening/mba0-extenddb-pattern-map.md` lists each ExtendDB pattern, where it lives in their repo, which Nimbus surface inherits it, and whether it is direct adoption or a Nimbus-specific generalization. Rigor review at `docs/plans/proof/multi-backend-adapter-hardening/mba0-plan-rigor-review.md` records where the remaining MBA items are welcome, where they must be narrowed, and which existing Nimbus surfaces prove the scope. | done |
| MBA1 | Create `docs/technical-debt.md` modeled on ExtendDB's `docs/technical-debt.md`. Categories: F (fidelity), C (cleanup), S (security), T (testing), A (architecture), P (performance), O (observability). Each entry has ID, title, severity (low/medium/high), owner, status, description, motivation. Seed ≥ 20 entries from Nimbus-owned, actionable debt signals: scoped `rg -n 'TODO\|FIXME\|XXX\|HACK'` results plus the known gaps surfaced during MBA0. Exclude generated code, vendored/upstream copies, compatibility fixture corpora, protobuf/SDK outputs, and other comments whose purpose is to preserve third-party behavior. Record the seed scope and exclusions in `mba1-technical-debt-seed.md`. `AGENTS.md` links it; `docs/README.md` indexes it. Verifier asserts: file exists; ≥ 20 entries; every entry has all seven fields; categories present; seed proof records scope and exclusions. | done |
| MBA2 | Storage trait segregation, audit first. Use ExtendDB's focused-trait model as evidence, not a taxonomy to copy. First produce `mba2-storage-trait-split.md` with current call sites, ownership groups, and a per-backend implementation matrix. Then split only where it removes real stub pressure or clarifies a Nimbus ownership boundary: tenant lifecycle/discovery, point document read/write, query/range scan, durable journal/stream, scheduler/worker, backup/export, control-plane management/settings/metrics/rate-limit, and credential/key-provider surfaces where applicable. Static-dispatch async traits may stay `async fn`; object-safe wrappers are only needed where the trait is actually used behind `dyn`. Composite traits are allowed only at boundaries that truly require the full surface. Migrate redb, sqlite, postgres, mysql, libsql replica, retained redb control-plane storage, and local encryption providers one family at a time; each implements only the traits it supports. Document the split in `docs/architecture/storage/trait-segregation.md`. Verifier asserts: proof declares final focused traits and dispatch posture; code contains those traits plus any justified composite; no backend implements stub-`unimplemented!()` methods on a focused trait; doc exists. | done |
| MBA3 | Adapter/backend registration seam decision. ExtendDB's `inventory::submit!` pattern is a source-backed option for cross-crate plugin-style registration, but Nimbus's current built-in storage providers are typed product modes, not out-of-tree plugins. Record the provider factory/opened-tenant/background-task matrix and keep `explicit_typed_registry` for built-in providers with documented enum/list boundaries. Adapter route/capability listing may use local declarative helpers or explicit typed registries; do not add linker-section registration merely to avoid editing a central file. Direct `inventory` use is future-only and requires a later ADR/proof that names a real out-of-tree plugin boundary. Verifier asserts: `docs/plans/proof/multi-backend-adapter-hardening/mba3-registration-seam.md` records `posture: explicit_typed_registry`; direct `inventory` dependencies are absent; the proof names the allowed enum/list boundaries; no duplicate hand-maintained backend availability lists remain outside them. | done |
| MBA4 | `RuntimeHooks` for backend-coupled workers. Define object-safe `trait RuntimeHooks: Send + Sync` with `fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()>` and optional `fn backend_info(&self) -> Option<String>`, modeled on ExtendDB's `ServerRuntimeHooks` while preserving Rust object safety at the actual `dyn` boundary. `WorkerContext` carries shared resources needed by backend workers. Move provider-specific hint/poll/listener workers (Postgres LISTEN/NOTIFY catch-up, MySQL polling, libSQL replica polling, future provider-coupled maintenance, and any real embedded maintenance) into per-backend `RuntimeHooks` implementations. Redb/sqlite/control-plane/local-encryption families may explicitly opt out when they have no backend-coupled workers. Engine-generic workers such as journal batching, scheduler, trigger execution, and subscription delivery remain engine-owned. Engine consumes `Option<Box<dyn RuntimeHooks>>` and never knows about backend-specific worker types. Verifier asserts: trait exists; each backend implements or explicitly opts out; engine code has no backend-specific provider worker functions. | done |
| MBA5 | Dual-target test pattern per adapter. For each of `convex`, `firebase`, `cloud_functions`, `mongodb`: add at least one integration test file that runs against either the real service or Nimbus, selected by env var (`NIMBUS_TEST_TARGET=convex_cloud\|nimbus`, etc.). Same test code, different endpoint. Start with narrow auth-error fidelity tests because they assert exact error shapes the wire protocol depends on. PR lanes may run the Nimbus/local target only; real-service targets run in a credentialed weekly/nightly workflow, with emulator/local-cloud targets allowed where they preserve the same protocol shape. Verifier asserts: at least one dual-target test file per adapter; CI workflow `dual-target-nightly.yml` exists; each adapter's test file references `NIMBUS_TEST_TARGET`; workflow defines Nimbus plus external/emulator targets. | done |
| MBA6 | Auth caching ADR. Write `docs/decisions/00X-auth-caching-policy.md` documenting whether security-sensitive credentials, JWT metadata, identity, and policy lookups cache. ExtendDB's choice: no caching for credential and catalog state because cache-invalidation bugs in auth are security bugs. Nimbus's policy needs an explicit decision: match ExtendDB for security-sensitive auth/policy decisions, or document why we diverge (e.g., embedded mode, immutable keys, bounded JWKS TTL). Classify existing non-auth caches separately: tenant/runtime/document/schema caches, update-check caches, operational knobs, and immutable encryption-key material are not automatically in scope, but any cache that affects authorization must name the ADR and its TTL/invalidation semantics. Verifier asserts: ADR exists; proof audits the actual auth roots (`application_auth.rs`, Convex auth, Firebase auth paths, MongoDB auth, tenant-isolation policy); auth/policy cache references either are absent or annotate the ADR. | done |
| MBA7 | SQL-safety ADRs per SQL-backed store. ExtendDB's `docs/adr/0002-sql-injection-defense.md` documents the two-tier defense (engine-layer identifier validation + parameterized queries + validated-identifier interpolation through named helpers like `data_table_name()`). For each SQL backend in `nimbus-storage/`: write a parallel ADR naming the exact helper functions, the validation rule for each user-supplied identifier, and the parameterization invariant. Make the helper allowlist greppable; CI grep gate forbids unreviewed user-input interpolation into SQL, while allowing fixed internal SQL and helper-mediated identifier construction. Verifier asserts: one ADR per SQL backend; named identifier-validation helpers exist; grep gate is clean against the helper allowlist and documented exemptions. | done |
| MBA8 | Per-segment latency budgets across hot paths. ExtendDB is source evidence for per-segment timing, not for Nimbus's final budget values. Identify the request-handler hot path in `crates/nimbus-server/` and the runtime-invocation/query hot path in `crates/nimbus-engine/`. Reuse existing timing slices where possible and add missing per-segment timers (parse / auth / dispatch / storage / runtime / serialize). First record Nimbus baseline measurements in `mba8-latency-budgets.md`; choose documented budgets from that evidence; then emit structured WARN events for over-budget segments. Metrics include p50/p99/p999 per segment per adapter. Verifier asserts: handler files contain per-segment timer blocks; metrics names match a documented schema in `docs/operating/latency-budgets.md`; proof records baseline evidence; at least 5 segments are budgeted. | done |
| MBA9 | Trait object-safety + RPITIT audit. Find every `dyn TraitName` in the workspace. Verify each trait that is actually object-erased is object-safe (uses `BoxFuture<'_, T>` rather than `async fn`, or wraps via blanket impl). Do not convert static-dispatch async traits purely for style; those are acceptable when no `dyn` boundary exists. Add `Box<dyn Trait>` ergonomic impls only where they simplify call sites. Document the pattern in `docs/architecture/trait-conventions.md`. Verifier asserts: doc exists; proof lists every object-erased trait and its posture; clippy with the relevant lints enabled is clean; greppable invariant - every async trait used with `dyn` returns `BoxFuture` or has a wrapper. | done |
| MBA10 | Stable logical table identity and physical-layout decision. Convex has an internal table mapping from user table names to stable tablet/table-number identities, and ExtendDB maps user-facing tables to UUID-backed physical names. Nimbus should adopt the Convex-shaped logical identity lesson without forcing ExtendDB's per-table SQL DDL shape. Introduce a per-tenant table catalog that maps active `(namespace, table_name)` to stable `table_id`; storage, indexes, schemas, journals, resource-path bindings, and subscription/event records resolve public `TableName` to `TableId` at the transaction boundary. Keep public API and adapter protocol shape name-based. Physical layout stays backend-owned: redb uses `table_id` key prefixes; SQLite/Postgres/MySQL/libSQL keep shared `documents` storage keyed by `(table_id, document_id)` plus a catalog table. Per-table UUID physical tables are reserved for a later measured backend-specific optimization. Verifier asserts: the design doc records `logical_identity: table_id_catalog` and the chosen physical posture for redb, SQLite, Postgres, MySQL, and libSQL; a `TableId` type/catalog exists; SQL backends use `table_id` in document storage; no user-supplied identifiers appear in SQL `CREATE TABLE`/`ALTER TABLE` strings without a documented helper. | done |
| MBA11 | Typed-column storage for user-typed keys. Where SQL backends support ordered scans over user-typed keys (string / number / binary), store each as its own typed column or generated expression (e.g., `sort_s TEXT`, `sort_n NUMERIC`, `sort_b BLOB`) and pick the right column from the schema. String-encoding everything breaks numeric ordering for range scans. redb and other non-SQL/native-key backends document their typed key encoding instead of mimicking SQL columns. Document at `docs/architecture/storage/typed-key-columns.md`. Verifier asserts: doc exists; SQL backends that support range scans use typed storage; non-SQL exceptions are documented; tests cover correct ordering for each key type. | done |
| MBA12 | Read-consistency routing. Where backends actually expose separate read/write consistency surfaces (e.g., libSQL replica reads, future PostgreSQL replicas, future distributed followers), route eventually-consistent reads to the read path. Adapters that promise eventual consistency exercise this path only when the backend has a real read surface; adapters that promise strong consistency stay on the write/authoritative path. Do not add artificial replicas or fake consistency layers to satisfy the plan. Tests that exercise real read paths catch consistency bugs that single-pool tests miss. Verifier asserts: routing exists for backends that support it; unsupported backends document `not_applicable`; doc at `docs/architecture/storage/consistency-routing.md` lists each adapter's per-operation consistency contract. | done |
| MBA13 | Hybrid event-capture pattern for adapter subscriptions. Preserve Nimbus's existing engine-owned trigger/subscription path while standardizing the shape across Convex subscriptions, Firebase listeners, and any future Firestore snapshots: adapter/application layer constructs the protocol event shape; engine/storage persists generic committed-event metadata in the same transaction as the data write. Storage owns atomicity, not adapter wire formats. Adapters own protocol shape, not transaction handling. Document at `docs/architecture/adapters/event-capture.md`. Apply to existing subscription paths. Verifier asserts: doc exists; each adapter that supports subscriptions follows the hybrid pattern; greppable invariant - no adapter wire-format event construction in `crates/nimbus-storage/`; no transaction boundary handling in adapter event-emission code. | done |
| MBA14 | Closeout. Flip every ledger row to `done`. Append Execution Log with actual SHAs. Move plan to `docs/plans/archive/multi-backend-adapter-hardening-plan.md`. Promote canonical contracts: `docs/operating/multi-backend-adapter-hardening.md` (synthesis of MBA1-MBA13 contracts). Update routing in `docs/plans/README.md` and `AGENTS.md` to the archived path. Verifier's plan-file regex accepts both active and archived paths. | done |

## Completion Gate

`bash scripts/verify-multi-backend-adapter-hardening.sh` exits 0 with
summary line `15 passed, 0 failed`. The 15 conditions:

1. Plan file exists at `docs/plans/multi-backend-adapter-hardening-plan.md`
   or `docs/plans/archive/multi-backend-adapter-hardening-plan.md`, and MBA0
   baseline, pattern-map, and rigor-review proof files exist.
2. `docs/technical-debt.md` exists with ≥ 20 entries, all seven fields
   per entry, and ≥ 5 distinct category prefixes used. MBA1 seed proof records
   source scope and exclusions so generated/vendor/fixture noise is not counted
   as actionable debt.
3. Storage trait segregation: MBA2 proof declares the final focused traits and
   dispatch posture; focused traits exist in `crates/nimbus-storage/src/traits/`;
   any composite trait is justified; backends compile without
   `unimplemented!()` stubs on focused traits.
4. Registration seam proof exists at
    `docs/plans/proof/multi-backend-adapter-hardening/mba3-registration-seam.md`.
   It records `posture: explicit_typed_registry`; direct `inventory`
   dependencies are absent. No duplicate hand-maintained backend availability
   lists remain outside the documented boundary.
5. `RuntimeHooks` trait exists; each backend either implements it or has
   an explicit `// no backend-coupled workers` annotation in its crate
   root or MBA4 proof. Engine code has no backend-specific provider worker
   spawn calls; engine-generic workers remain engine-owned.
6. Dual-target tests: at least one test file per adapter under
   `tests/dual-target/<adapter>/` references `NIMBUS_TEST_TARGET`.
7. Dual-target nightly CI workflow exists at
   `.github/workflows/dual-target-nightly.yml`.
8. Auth caching ADR exists at `docs/decisions/00X-auth-caching-policy.md`
   (X assigned at write time).
9. Auth code matches the ADR: grep for cache references in
   `crates/nimbus-server/src/application_auth.rs`, adapter auth modules,
   Firebase auth paths, MongoDB auth, and tenant-isolation policy modules;
   either zero security-sensitive hits or every hit annotates the ADR ID.
10. SQL-safety ADR exists for each SQL backend (SQLite, Postgres, MySQL,
    libSQL) in `docs/decisions/`; named identifier-validation helpers exist
    in each SQL backend crate; the helper allowlist and documented exemptions
    are clean.
11. Latency budget instrumentation present: handler files contain per-
    segment timer blocks; metrics schema documented at
    `docs/operating/latency-budgets.md`; MBA8 proof records baseline evidence;
    ≥ 5 budgeted segments.
12. Trait conventions doc exists at
    `docs/architecture/trait-conventions.md`; clippy run clean for the
    object-safety lints; MBA9 proof audits object-erased traits without forcing
    static-dispatch async traits through `BoxFuture`.
13. Late storage/adapter contracts are implemented and proven:
    logical table identity proof exists at
    `docs/plans/proof/multi-backend-adapter-hardening/mba10-table-identity-and-layout.md`;
    it records `logical_identity: table_id_catalog` plus physical posture for
    redb, SQLite, Postgres, MySQL, and libSQL; `TableId` and a per-tenant table
    catalog exist; SQL document storage is keyed by `table_id`; no user
    identifiers enter SQL DDL strings without a documented helper. Typed-key
    storage doc/proof exist and SQL range-scan tests cover string, numeric, and
    binary ordering. Consistency-routing doc/proof exist and list each
    backend/adapter as supported or `not_applicable`. Event-capture doc/proof
    exist and preserve the storage-atomicity/adapter-wire-shape split.
14. Routing entries naming this plan exist in
    `docs/plans/README.md` and `AGENTS.md`.
15. Every ledger row marked `done`; latest CI run on main is green
    (status=completed, conclusion=success) and recorded in
    `docs/plans/proof/multi-backend-adapter-hardening/mba14-closeout.md`.

## Proof directory

`docs/plans/proof/multi-backend-adapter-hardening/`:

- `mba0-baseline.md` — current state of every surface this plan touches
- `mba0-extenddb-pattern-map.md` — per-pattern source pointer + Nimbus
  target surface
- `mba0-plan-rigor-review.md` — item-by-item review of MBA1-MBA14
  complexity posture and scope guardrails
- `mba1-technical-debt-seed.md` — initial debt-tracker seeding rationale
- `mba2-storage-trait-split.md` — before/after trait surface diagram +
  per-backend implementation matrix
- `mba3-registration-seam.md` — provider/adapter registration posture,
  allowed typed boundaries, and optional `inventory` crate rationale if chosen
- `mba4-runtime-hooks.md` — per-backend worker inventory + migration
  plan
- `mba5-dual-target-tests.md` — per-adapter test strategy + endpoint
  selection
- `mba6-auth-caching-adr.md` — decision rationale + audit findings
- `mba7-sql-safety-adrs.md` — per-backend defense matrix
- `mba8-latency-budgets.md` — segment list + budgets + escalation rules
- `mba9-trait-conventions.md` — object-safety pattern catalog
- `mba10-table-identity-and-layout.md` — logical identity posture plus
  per-backend physical layout matrix
- `mba11-typed-key-columns.md` — schema diagrams per backend
- `mba12-consistency-routing.md` — per-adapter consistency contracts
- `mba13-event-capture.md` — hybrid pattern with adapter walkthroughs
- `mba14-closeout.md` — final state + retro + cross-cutting follow-ups

## Apache-2.0 reuse posture

ExtendDB code adopted under this plan ships under its original
Apache-2.0 license inside Nimbus. Mechanical compliance only:

- Preserve `// Copyright YYYY ExtendDB contributors` and
  `// SPDX-License-Identifier: Apache-2.0` headers on copied files.
- Add `// Modified from <upstream path> by Nimbus contributors,
  YYYY-MM-DD` when changed.
- Maintain one `NOTICE` entry covering all ExtendDB-derived files (single
  entry per upstream project, no per-file fan-out).
- Apache-2.0 §3 patent grant flows through; no further legal sign-off.

The combined binary ships under the Nimbus Community License 1.0; the
Apache-2.0 portions retain their original license. This is the standard
multi-licensed-project pattern. See [[apache-license-posture]] memory for
the full rationale.

## Notes on staging order

MBA1 first because it costs nothing and immediately surfaces work the
other items uncover. Track every gap discovered while doing MBA2-MBA13
in the new debt tracker.

MBA2 (trait segregation) before MBA3 (registration) because cleaner
trait surfaces make the registration seam decision more obvious. MBA3 should
not force `inventory`; it should preserve explicit typed provider dispatch
where that remains simpler and more auditable.

MBA3 + MBA4 together because adapter/backend registration and runtime
hooks share the design vocabulary (factories, registrations, hooks).
They are separate ledger items because the verification surfaces differ.

MBA5 (dual-target tests) is independent and can run in parallel with
MBA2-MBA4. Auth-error fidelity tests are the recommended first set
because they assert wire-protocol invariants.

MBA6-MBA7 (ADRs) can run in parallel with MBA8 (latency budgets) and
MBA9 (object-safety audit). All four are largely documentation +
greppable invariants.

MBA10-MBA13 are storage-layer changes that depend on MBA2's clean trait
surface. MBA10 starts with the table-identity and physical-layout matrix before
implementation because Nimbus should adopt a Convex-style stable logical table
identity while preserving the current shared-table SQL physical shape. Do the
`table_id` catalog rewrite before MBA11 (typed columns) because that schema
rewrite is the natural place to introduce typed sort columns.

Within the wave, each MBA is its own commit so the Execution Log SHAs
are individually auditable. The same autonomous-commit posture used in
the CI Modernization, CI Caching, CI Wall Acceleration, and Coverage
Acceleration waves applies here.

## Execution Log

| MBA  | Commit(s) | Subject |
|------|-----------|---------|
| MBA0 | local | scaffold Multi-Backend Multi-Adapter Hardening plan + verifier + baseline proof |
| MBA1 | local | add technical debt ledger, seed proof, and docs index |
| MBA2 | local | add focused storage capability traits and segregation proof |
| MBA3 | local | record explicit typed registration seam decision |
| MBA4 | local | add object-safe RuntimeHooks seam for backend-coupled workers |
| MBA5 | local | add env-selected dual-target auth-error probes and nightly matrix |
| MBA6 | local | add auth caching ADR and auth-root cache audit |
| MBA7 | local | add per-SQL-backend injection-safety ADRs and helper matrix |
| MBA8 | local | add latency budget docs and segment WARN timers for Convex query/engine query paths |
| MBA9 | local | document trait object-safety conventions and audit object-erased traits |
| MBA10 | local | add TableId, tenant table catalog, and shared SQL table_catalog contract |
| MBA11 | local | document typed key ordering contract across SQL and native backends |
| MBA12 | local | document read-consistency routing by real backend read surfaces |
| MBA13 | local | document hybrid event-capture contract for subscriptions and adapter events |
| MBA14 | local | archive plan, promote operating contract, and record local green verifier evidence |
