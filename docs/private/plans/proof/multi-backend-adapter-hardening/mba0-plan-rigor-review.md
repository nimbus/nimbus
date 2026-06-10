# MBA0 Plan Rigor Review

Date: 2026-05-27

This review covers the MBA items beyond the two already narrowed in detail:
registration (`MBA3`) and table identity (`MBA10`). The purpose is to separate
architecture that earns its cost from defensive or copied complexity.

## Source Anchors

Nimbus anchors:

- `crates/nimbus-storage/src/async_storage/traits.rs` already has a small async
  executor seam; it is not a failed monolith, but it still leaves concrete
  store and transaction surfaces doing too much cross-backend work.
- `crates/nimbus-engine/src/persistence/provider.rs` and
  `crates/nimbus-engine/src/service/provider_hints.rs` contain typed provider
  worker dispatch for Postgres, MySQL, and libSQL replica.
- `crates/nimbus-server/src/application_auth.rs`,
  `crates/nimbus-server/src/adapters/convex/auth/`, and
  `crates/nimbus-server/src/adapters/mongodb/auth.rs` are the actual auth
  audit roots; there is no `crates/nimbus-server/src/auth/` tree.
- SQL backends already use bound values plus helper-built identifiers such as
  `qualified_table`, `quote_identifier`, `sqlite_index_name`, and generated
  column helpers. The plan should document and harden this pattern, not ban all
  dynamic SQL construction.
- `crates/nimbus-engine/src/service/queries/query_api.rs` and
  `crates/nimbus-engine/src/service/tenants.rs` already record useful timing
  slices. MBA8 should standardize this into a hot-path schema.
- Trigger and subscription paths are engine-owned today:
  `crates/nimbus-engine/src/triggers/`,
  `crates/nimbus-engine/src/tenant/trigger_candidates.rs`,
  `crates/nimbus-engine/src/tenant/subscription_delivery.rs`, and storage
  trigger tables. MBA13 should preserve this ownership.

ExtendDB anchors:

- `docs/manuals/01-architecture-guide.md` and `crates/storage/src/lib.rs`
  provide evidence for focused storage traits and composite stores.
- `docs/manuals/02-design-guide.md` provides the auth/cache posture, typed sort
  key storage, BoxFuture/object-safety rationale, and stream-capture split.
- `docs/adr/0002-sql-injection-defense.md` provides the SQL defense model:
  validated identifiers, bound values, and named helper funnels.
- `crates/storage/src/hooks.rs` provides the `ServerRuntimeHooks` precedent.
- `tests/conftest.py` and auth fidelity tests provide the dual-target pattern.

## Decision Matrix

| MBA | Decision | Why it earns complexity | Tightening |
|-----|----------|-------------------------|------------|
| MBA1 | Keep. | A single debt ledger improves handoff quality and prevents plan comments from becoming invisible risk. | Seed only Nimbus-owned, actionable debt. Exclude generated code, vendored/upstream code, fixture corpora, and noisy compatibility snapshots. Record the seed scope in proof. |
| MBA2 | Keep, but make it audit-first. | More backends will otherwise keep accumulating stub pressure and accidental shared surfaces. | Do not copy ExtendDB's trait taxonomy or force every method into a new trait. Split only around real Nimbus ownership boundaries and call sites. Static-dispatch async traits are allowed. |
| MBA4 | Keep. | Provider-specific poll/listen workers already leak backend names into engine code. A hook seam will age better as backends grow. | Move only backend-coupled workers. Engine-generic workers such as journal batching, scheduler, trigger execution, and subscription delivery stay engine-owned. |
| MBA5 | Keep, staged. | Adapter trust requires comparing exact protocol behavior against the service being emulated. | Start with narrow auth/error-fidelity cases. PR lanes may run Nimbus/local targets; real-service targets belong in nightly/weekly or credentialed lanes. |
| MBA6 | Keep, but classify caches. | Auth and policy cache bugs are security bugs, so policy must be explicit. | The ADR applies to security-sensitive auth, credential, and policy decisions. Operational caches, immutable keys, schema/document caches, and update-check caches need classification, not automatic removal. |
| MBA7 | Keep. | SQL safety is enterprise trust table stakes across every SQL backend. | ADRs must name helper allowlists and validation points. The grep gate should catch unreviewed user-input interpolation, not every `format!` that builds fixed internal SQL. |
| MBA8 | Keep, evidence-first. | Latency regressions must be attributable to parse/auth/dispatch/storage/runtime/serialize segments, not buried in wall-clock totals. | Measure and document baseline values before setting WARN thresholds. Do not copy ExtendDB budget numbers. Reuse existing timing/metrics surfaces where possible. |
| MBA9 | Keep, scoped. | Rust trait object mistakes are subtle and become expensive once plugins/backends multiply. | Audit traits that are actually object-erased with `dyn`. Do not convert static-dispatch async traits to `BoxFuture` purely for style. |
| MBA11 | Keep for SQL range correctness. | String-encoding user-typed sort keys breaks numeric and binary ordering. | Apply where a backend performs ordered scans over user-typed keys. redb can document native/key-encoding behavior instead of mimicking SQL columns. |
| MBA12 | Keep as a contract, not as fake infrastructure. | Some backend/adapters will have real read consistency choices, especially replicas and future distributed storage. | Route only where a backend actually exposes separate consistency surfaces. Do not invent read replicas just to satisfy the plan. |
| MBA13 | Keep, preserving current ownership. | Durable events must be atomic with committed writes, while protocol event shapes belong to adapters. | Preserve the existing engine/storage trigger path. Storage records generic committed-event metadata; adapters construct wire-protocol shapes outside storage. |
| MBA14 | Keep. | A broad cross-cutting wave needs closeout evidence and a canonical operating contract. | Close only after the verifier, CI evidence, and archived operating summary agree. |

## Review Outcome

The rest of the plan is welcome for Nimbus, but only in a bounded form:

1. Architecture changes must remove concrete pressure already visible in
   Nimbus, not import every ExtendDB shape.
2. Documentation items must produce greppable contracts and code audits, not
   standalone essays.
3. Verification must fail on missing proof or real invariants, not noisy scans
   that would punish existing safe helper patterns.
4. Backend-owned work and adapter-owned work stay distinct: storage owns
   atomicity, adapters own protocol shape, and the engine owns generic workers.
